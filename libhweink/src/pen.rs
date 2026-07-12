//! Pen / stylus input from `/dev/input/event2` ("pen_touch").
//!
//! The pen reports native EBC coordinates: ABS_X [0..2480], ABS_Y [0..1860],
//! ABS_PRESSURE [0..1024]. We translate them to screen (portrait) coords via the
//! same 90°-CW transform as the buffer, so pen coordinates and drawn pixels line up.
//!
//! Tool/buttons come from evdev key events:
//!   BTN_TOOL_PEN / BTN_TOOL_RUBBER / BTN_TOOL_FINGER — proximity + tool type
//!   BTN_TOUCH                                      — surface contact
//!   BTN_STYLUS / BTN_STYLUS2 / BTN_STYLUS3         — side buttons
//!
//! `Pen::read` blocks up to `timeout_ms` collecting events until an `EV_SYN` frame
//! boundary, then returns a merged snapshot. This matches how evdev batches events.

use crate::geom::buf_to_screen;
use core::mem::size_of;

const EV_SYN: u16 = 0x0000;
const EV_KEY: u16 = 0x0001;
const EV_ABS: u16 = 0x0003;

const ABS_X: u16 = 0x0000;
const ABS_Y: u16 = 0x0001;
const ABS_PRESSURE: u16 = 0x0018;

const BTN_TOUCH: u16 = 0x014a; // 330
const BTN_TOOL_PEN: u16 = 0x0140; // 320
const BTN_TOOL_RUBBER: u16 = 0x0141; // 321
const BTN_TOOL_FINGER: u16 = 0x0145; // 325
const BTN_STYLUS: u16 = 0x014b; // 331
const BTN_STYLUS2: u16 = 0x014c; // 332
const BTN_STYLUS3: u16 = 0x0149; // 329

const SYN_REPORT: u16 = 0x0000;

/// Pen tool type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    None,
    Pen,
    Eraser,
    Finger,
}

/// A high-level, frame-synthesized pen event (one per `EV_SYN/SYN_REPORT`).
///
/// Coordinates are **screen (portrait)**, already 90°-CW rotated from the native
/// buffer coords the kernel reports. `pressure` is normalized to `0..1000` from the
/// raw `0..1024`. `x`/`y` are `-1` until the first ABS_X/ABS_Y frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct PenEvent {
    /// Screen (portrait) X, or `-1` if no ABS_X seen yet.
    pub x: i32,
    /// Screen (portrait) Y, or `-1` if no ABS_Y seen yet.
    pub y: i32,
    /// Pressure 0..1024.
    pub pressure: i32,
    pub tool: Tool,
    /// Tool is in proximity of the surface.
    pub in_proximity: bool,
    /// Tool is touching the surface.
    pub touch: bool,
    /// Side button 1.
    pub stylus_btn: bool,
    /// Side button 2.
    pub stylus_btn2: bool,
    /// Side button 3 (extra).
    pub stylus_btn3: bool,
}

impl PenEvent {
    /// Pressure normalized to `0..1000` (from raw `0..1024`).
    pub fn pressure_norm(&self) -> u32 {
        (self.pressure.clamp(0, 1024) as u32 * 1000) / 1025
    }
}

/// `input_event` layout on aarch64 Android: 16-byte (sec, usec, type, code, value),
/// where sec/usec are `long` (8 bytes each). Total 24 bytes. We use the kernel's
/// `struct input_event` exactly.
#[repr(C)]
#[derive(Clone, Copy)]
struct InputEvent {
    tv_sec: i64,
    tv_usec: i64,
    typ: u16,
    code: u16,
    value: i32,
}

const INPUT_EVENT_SIZE: usize = size_of::<InputEvent>();

/// Pen input handle. Opens `/dev/input/event2` by default.
pub struct Pen {
    fd: i32,
    /// Last known absolute X (native buffer coords), for frames that don't include it.
    last_bx: i32,
    last_by: i32,
    last_pressure: i32,
    tool: Tool,
    in_proximity: bool,
    touch: bool,
    s1: bool,
    s2: bool,
    s3: bool,
    have_xy: bool,
}

impl Pen {
    /// Open the default pen device `/dev/input/event2`.
    pub fn open() -> std::io::Result<Self> {
        Self::open_path("/dev/input/event2")
    }

    pub fn open_path(path: &str) -> std::io::Result<Self> {
        let c = std::ffi::CString::new(path).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "path contained NUL")
        })?;
        let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Pen {
            fd,
            last_bx: -1,
            last_by: -1,
            last_pressure: 0,
            tool: Tool::None,
            in_proximity: false,
            touch: false,
            s1: false,
            s2: false,
            s3: false,
            have_xy: false,
        })
    }

    /// Read one synthesized frame. Blocks up to `timeout_ms`.
    /// Returns `Ok(Some(event))` on a sync'd frame, `Ok(None)` on timeout, `Err` on error.
    pub fn read(&mut self, timeout_ms: i32) -> std::io::Result<Option<PenEvent>> {
        let mut pfd = libc::pollfd {
            fd: self.fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let r = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        if r < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if r == 0 {
            return Ok(None);
        }
        // Drain available events until we hit a SYN_REPORT.
        let mut buf = [0u8; INPUT_EVENT_SIZE * 32];
        loop {
            let n = unsafe {
                libc::read(self.fd, buf.as_mut_ptr() as *mut _, buf.len())
            };
            if n < 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                // In nonblocking mode, EAGAIN means we drained the kernel queue
                // without a SYN — treat as "no full frame yet".
                if e.kind() == std::io::ErrorKind::WouldBlock {
                    return Ok(None);
                }
                return Err(e);
            }
            let n = n as usize;
            if n == 0 {
                return Ok(None);
            }
            let count = n / INPUT_EVENT_SIZE;
            for i in 0..count {
                let ev: InputEvent = unsafe {
                    core::ptr::read_unaligned(
                        buf.as_ptr().add(i * INPUT_EVENT_SIZE) as *const InputEvent
                    )
                };
                self.apply(&ev);
                if ev.typ == EV_SYN && ev.code == SYN_REPORT {
                    return Ok(Some(self.snapshot()));
                }
            }
            // If we consumed all read events without a SYN, loop and poll again.
            let mut pfd2 = libc::pollfd {
                fd: self.fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let r2 = unsafe { libc::poll(&mut pfd2, 1, timeout_ms) };
            if r2 <= 0 {
                return Ok(None);
            }
        }
    }

    // ---- async layer 3: raw fd + nonblocking ----

    /// The raw evdev file descriptor. Register it in your own event loop (mio/tokio,
    /// g_main_context_add_poll, epoll directly) and call [`poll_once`] when readable.
    pub fn fd(&self) -> i32 {
        self.fd
    }

    /// Set/clear `O_NONBLOCK` on the fd. With nonblocking set, [`poll_once`] with a
    /// zero timeout will return `Ok(None)` instead of blocking when no full frame is
    /// available, and `read` will no longer block on an empty kernel queue.
    pub fn set_nonblocking(&mut self, nonblock: bool) -> std::io::Result<()> {
        let cur = unsafe { libc::fcntl(self.fd, libc::F_GETFL) };
        if cur < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let new = if nonblock { cur | libc::O_NONBLOCK } else { cur & !libc::O_NONBLOCK };
        if unsafe { libc::fcntl(self.fd, libc::F_SETFL, new) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    /// Try to read one complete frame without blocking longer than `timeout_ms`.
    ///
    /// This is the building block for event-loop integration: when your selector says
    /// the fd is readable, call `poll_once(Duration::ZERO)` (or a small timeout) to
    /// drain frames. It may return `Ok(None)` if the kernel has only partial-frame
    /// events buffered or the queue was already drained — keep the fd registered and
    /// wait for the next readable notification.
    ///
    /// Unlike [`read`], this returns after the first `SYN_REPORT` or first timeout,
    /// without an inner poll loop — so it composes cleanly with an external selector.
    pub fn poll_once(&mut self, timeout: std::time::Duration) -> std::io::Result<Option<PenEvent>> {
        let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
        // One poll, then drain whatever is readable; no inner re-poll loop.
        let mut pfd = libc::pollfd { fd: self.fd, events: libc::POLLIN, revents: 0 };
        let r = unsafe { libc::poll(&mut pfd, 1, ms) };
        if r < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if r == 0 {
            return Ok(None);
        }
        let mut buf = [0u8; INPUT_EVENT_SIZE * 64];
        loop {
            let n = unsafe { libc::read(self.fd, buf.as_mut_ptr() as *mut _, buf.len()) };
            if n < 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                if e.kind() == std::io::ErrorKind::WouldBlock {
                    // Drained the queue; if we haven't hit a SYN this call, return None.
                    return Ok(None);
                }
                return Err(e);
            }
            let n = n as usize;
            if n == 0 {
                return Ok(None);
            }
            let count = n / INPUT_EVENT_SIZE;
            for i in 0..count {
                let ev: InputEvent = unsafe {
                    core::ptr::read_unaligned(
                        buf.as_ptr().add(i * INPUT_EVENT_SIZE) as *const InputEvent
                    )
                };
                self.apply(&ev);
                if ev.typ == EV_SYN && ev.code == SYN_REPORT {
                    return Ok(Some(self.snapshot()));
                }
            }
            // Loop to read more without re-polling (fd was readable; keep draining
            // until SYN or EAGAIN). This bounds the call to one external event.
        }
    }

    // ---- async layer 1: iterator ----

    /// Blocking iterator over pen frames. Each `next()` blocks until a complete frame
    /// (or an error). Intended for a dedicated pen thread:
    ///
    /// ```no_run
    /// # use hweink::pen::Pen;
    /// # let mut pen = Pen::open().unwrap();
    /// for ev in pen.events() {
    ///     println!("{:?}", ev);
    /// }
    /// ```
    ///
    /// The iterator yields `PenEvent` forever; it only ends on read error (which
    /// panics, matching the `Iterator` contract — wrap `read` yourself if you want
    /// error handling).
    pub fn events(&mut self) -> PenIter<'_> {
        PenIter { pen: self }
    }

    fn apply(&mut self, ev: &InputEvent) {
        match ev.typ {
            EV_ABS => match ev.code {
                ABS_X => {
                    self.last_bx = ev.value;
                    self.have_xy = true;
                }
                ABS_Y => {
                    self.last_by = ev.value;
                    self.have_xy = true;
                }
                ABS_PRESSURE => self.last_pressure = ev.value,
                _ => {}
            },
            EV_KEY => {
                let down = ev.value != 0;
                match ev.code {
                    BTN_TOOL_PEN => {
                        self.in_proximity = down;
                        if down {
                            self.tool = Tool::Pen;
                        } else if self.tool == Tool::Pen {
                            self.tool = Tool::None;
                        }
                    }
                    BTN_TOOL_RUBBER => {
                        self.in_proximity = down;
                        if down {
                            self.tool = Tool::Eraser;
                        } else if self.tool == Tool::Eraser {
                            self.tool = Tool::None;
                        }
                    }
                    BTN_TOOL_FINGER => {
                        self.in_proximity = down;
                        if down {
                            self.tool = Tool::Finger;
                        } else if self.tool == Tool::Finger {
                            self.tool = Tool::None;
                        }
                    }
                    BTN_TOUCH => self.touch = down,
                    BTN_STYLUS => self.s1 = down,
                    BTN_STYLUS2 => self.s2 = down,
                    BTN_STYLUS3 => self.s3 = down,
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn snapshot(&self) -> PenEvent {
        let (sx, sy) = if self.have_xy && self.last_bx >= 0 && self.last_by >= 0 {
            let (x, y) = buf_to_screen(self.last_bx, self.last_by);
            (x, y)
        } else {
            (-1, -1)
        };
        PenEvent {
            x: sx,
            y: sy,
            pressure: self.last_pressure,
            tool: self.tool,
            in_proximity: self.in_proximity,
            touch: self.touch,
            stylus_btn: self.s1,
            stylus_btn2: self.s2,
            stylus_btn3: self.s3,
        }
    }
}

/// Blocking iterator over pen frames. See [`Pen::events`].
pub struct PenIter<'a> {
    pen: &'a mut Pen,
}

impl<'a> Iterator for PenIter<'a> {
    type Item = PenEvent;

    fn next(&mut self) -> Option<PenEvent> {
        // Block indefinitely (-1 timeout) for the next frame. On error, end the
        // iterator (None) — callers wanting error handling should use `read` directly.
        match self.pen.read(-1) {
            Ok(ev) => ev,
            Err(_) => None,
        }
    }
}

impl Drop for Pen {
    fn drop(&mut self) {
        if self.fd >= 0 {
            unsafe {
                libc::close(self.fd);
            }
        }
    }
}

// Re-export screen dims for callers.
pub use crate::geom::{SCREEN_H as PEN_SCREEN_H, SCREEN_W as PEN_SCREEN_W};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_event_size_is_24() {
        // aarch64: 8 + 8 + 2 + 2 + 4 = 24
        assert_eq!(INPUT_EVENT_SIZE, 24);
    }
}
