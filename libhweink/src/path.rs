//! Render paths: OSD overlay (coexists with Android UI) and Main buffer (exclusive
//! full-screen takeover). Both write 8bpp Y8 with a 90°-CW screen→buffer rotation.

use crate::ebc::Ebc;
use crate::geom::{screen_to_buf, ScreenRect, BUF_H, BUF_W};
use crate::ioctl::{ioctl_ptr, EbcBufInfo, Rect, EBC_SEND_BUFFER, EBC_SEND_OSD_BUFFER};
use crate::mode::Mode;

/// Which display path to drive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Path {
    /// OSD overlay layer. Coexists with the Android UI underneath; screencap cannot
    /// capture it. Low-latency, ideal for pen input and custom drawing.
    ///
    /// Lifecycle: `ENABLE_OVERLAY` on open, write 8bpp Y8 to the OSD buffer,
    /// `SEND_OSD_BUFFER` with `epd_mode=OVERLAY` to refresh, `DISABLE_OVERLAY` on close.
    Osd,
    /// Main display buffer. Owned by SurfaceFlinger/DRM/HWC normally; we blank the HWC
    /// via `SET_FB_BLANK` to take exclusive control, write 8bpp Y8, refresh with
    /// `SEND_BUFFER`, then `SET_FB_UNBLANK` to restore. Screencap-visible.
    Main,
}

impl Path {
    fn buffer_offset(self, ebc: &Ebc) -> i32 {
        match self {
            Path::Osd => ebc.osd_offset,
            Path::Main => ebc.main_offset,
        }
    }

    fn buffer_ptr<'a>(self, ebc: &'a Ebc) -> &'a mut [u8] {
        let base = match self {
            Path::Osd => ebc.osd_ptr(),
            Path::Main => ebc.main_ptr(),
        };
        let len = ebc.frame_bytes();
        unsafe { core::slice::from_raw_parts_mut(base, len) }
    }

    /// Native mmap buffer slice for this path (8bpp Y8, `frame_bytes()` long).
    /// Used by the `Draw` flush to blit the shadow into the real buffer.
    pub(crate) fn buffer_slice(self, ebc: &Ebc) -> &mut [u8] {
        self.buffer_ptr(ebc)
    }

    fn enable(self, ebc: &Ebc) -> std::io::Result<()> {
        match self {
            Path::Osd => ebc.ioctl0(crate::ioctl::EBC_ENABLE_OVERLAY),
            Path::Main => ebc.ioctl0(crate::ioctl::EBC_SET_FB_BLANK),
        }
    }

    fn disable(self, ebc: &Ebc) -> std::io::Result<()> {
        match self {
            Path::Osd => ebc.ioctl0(crate::ioctl::EBC_DISABLE_OVERLAY),
            Path::Main => ebc.ioctl0(crate::ioctl::EBC_SET_FB_UNBLANK),
        }
    }

    fn send_cmd(self) -> u32 {
        match self {
            Path::Osd => EBC_SEND_OSD_BUFFER,
            Path::Main => EBC_SEND_BUFFER,
        }
    }

    fn default_mode(self) -> Mode {
        match self {
            Path::Osd => Mode::Overlay,
            Path::Main => Mode::FullGc16,
        }
    }
}

/// A render surface bound to one path. Holds the EBC handle open and the path enabled.
///
/// All drawing uses **screen (portrait) coordinates** — 1860 wide × 2480 tall — and the
/// library applies the 90°-CW rotation to native buffer coords internally. Gray is 8-bit:
/// `0 = black`, `255 = white`.
pub struct Surface<'a> {
    ebc: &'a Ebc,
    path: Path,
    enabled: bool,
    /// Screen-space shadow buffer for batch-write `Draw` scopes. ~4.6 MiB, allocated
    /// lazily on first `draw()` and reused across scopes. Stays in sync with the last
    /// flushed frame (it's the "current canvas").
    shadow: std::cell::UnsafeCell<alloc::ShadowBuffer>,
}

mod alloc {
    /// Screen-space shadow: `SCREEN_W × SCREEN_H` 8bpp, lazily allocated.
    pub struct ShadowBuffer {
        buf: Option<Vec<u8>>,
    }
    impl ShadowBuffer {
        pub const fn new() -> Self {
            ShadowBuffer { buf: None }
        }
        pub fn get_mut(&mut self, len: usize) -> &mut [u8] {
            let b = self.buf.get_or_insert_with(|| vec![0xFFu8; len]);
            if b.len() != len {
                // size changed (shouldn't happen on a fixed device) — re-alloc
                *b = vec![0xFFu8; len];
            }
            b.as_mut_slice()
        }
    }
}

impl<'a> Surface<'a> {
    /// Open a surface on the given path. Enables the path (overlay / fb-blank).
    pub fn new(ebc: &'a Ebc, path: Path) -> std::io::Result<Self> {
        path.enable(ebc)?;
        Ok(Surface {
            ebc,
            path,
            enabled: true,
            shadow: std::cell::UnsafeCell::new(alloc::ShadowBuffer::new()),
        })
    }

    // ---- accessors used by `draw.rs` ----

    pub(crate) fn ebc(&self) -> &Ebc {
        self.ebc
    }
    pub(crate) fn path(&self) -> Path {
        self.path
    }
    /// Mutable borrow of the shadow buffer. Sound because `Draw<'_>` borrows `&self`
    /// for its whole lifetime — only one `Draw` can exist at a time per surface.
    pub(crate) fn shadow_mut(&self) -> &mut [u8] {
        let len = crate::geom::SCREEN_W as usize * crate::geom::SCREEN_H as usize;
        unsafe { (*self.shadow.get()).get_mut(len) }
    }

    /// Screen (portrait) width = 1860.
    pub fn screen_w(&self) -> i32 {
        crate::geom::SCREEN_W
    }
    /// Screen (portrait) height = 2480.
    pub fn screen_h(&self) -> i32 {
        crate::geom::SCREEN_H
    }

    /// Put a pixel at screen coords `(x, y)` with gray value `g` (0=black, 255=white).
    pub fn put_pixel(&self, x: i32, y: i32, g: u8) {
        if x < 0 || y < 0 || x >= crate::geom::SCREEN_W || y >= crate::geom::SCREEN_H {
            return;
        }
        let (bx, by) = screen_to_buf(x, y);
        let stride = self.ebc.stride();
        let bi = (by as usize) * stride + (bx as usize);
        let buf = self.path.buffer_ptr(self.ebc);
        if bi < buf.len() {
            buf[bi] = g;
        }
    }

    /// Fill a screen-space rectangle `[x1, x2) × [y1, y2)` with `g`.
    pub fn fill_rect(&self, x1: i32, y1: i32, x2: i32, y2: i32, g: u8) {
        let (x1, x2) = (x1.min(x2), x1.max(x2));
        let (y1, y2) = (y1.min(y2), y1.max(y2));
        for y in y1..y2 {
            for x in x1..x2 {
                self.put_pixel(x, y, g);
            }
        }
    }

    /// Clear the whole screen to `g`.
    pub fn clear(&self, g: u8) {
        self.fill_rect(0, 0, crate::geom::SCREEN_W, crate::geom::SCREEN_H, g);
    }

    /// Push a screen-space dirty region to the panel.
    /// `mode=None` uses the path default (OSD→OVERLAY, MAIN→FULL_GC16).
    pub fn refresh(&self, rect: ScreenRect, mode: Option<Mode>) -> std::io::Result<()> {
        let m = mode.unwrap_or_else(|| self.path.default_mode());
        let buf_rect = rect.to_buf();
        // Clamp to native buffer.
        let buf_rect = Rect::new(
            buf_rect.x1.max(0).min(BUF_W),
            buf_rect.y1.max(0).min(BUF_H),
            buf_rect.x2.max(0).min(BUF_W),
            buf_rect.y2.max(0).min(BUF_H),
        );
        let mut info = EbcBufInfo::for_refresh(
            self.path.buffer_offset(self.ebc),
            m as i32,
            self.ebc.width,
            self.ebc.height,
            buf_rect,
        );
        ioctl_ptr(
            self.ebc.fd(),
            self.path.send_cmd(),
            &mut info as *mut EbcBufInfo as *mut _,
        )
    }

    /// Refresh the whole screen.
    pub fn refresh_full(&self, mode: Option<Mode>) -> std::io::Result<()> {
        self.refresh(ScreenRect::full(), mode)
    }
}

impl<'a> Drop for Surface<'a> {
    fn drop(&mut self) {
        if self.enabled {
            let _ = self.path.disable(self.ebc);
            self.enabled = false;
        }
    }
}
