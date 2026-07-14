//! hweink-note — a minimal native note-taking app for the Hanwang N10 Pro III.
//!
//! Built on the `hweink` library: pen input + OSD overlay drawing + e-ink refresh.
//! Bare native binary (adb push to /data/local/tmp), no APK / Java.
//!
//! # Usage
//!   adb push hweink-note /data/local/tmp/ && adb shell /data/local/tmp/hweink-note
//!
//! # Controls
//!   Pen (pen end)    → draw a fountain-pen stroke (smoothed, slight pressure width)
//!   Pen (eraser end) → erase (flip the pen; BTN_TOOL_RUBBER)
//!   Physical key     → printed to stdout when pressed (code shown), so you can tell
//!                       me which button is which; currently 0x2f4=clear, 0x2e8=full-refresh
//!   Ctrl-C           → quit
//!
//! # Refresh model
//!   Drawing uses incremental EPD_OVERLAY refresh on the OSD layer (low latency, no
//!   flashing). There is NO auto full-refresh on pen-up — that was clearing the overlay
//!   and caused flicker. Full refresh is manual only (physical key), to clear ghosting
//!   when the user wants it.

use hweink::{
    mode::Mode,
    path::{Path, Surface},
    pen::Pen,
    pen::Tool,
    Ebc,
};
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

const EV_KEY: u16 = 0x0001;
const KEY_F8: u16 = 0x02e8; // candidate: full refresh
const KEY_F24: u16 = 0x02f4; // candidate: clear screen

/// Set by the SIGINT/SIGTERM handler so the event loop can exit cleanly and run
/// `Surface`'s `Drop` (which calls `DISABLE_OVERLAY`). Without this, Ctrl-C / `adb
/// shell` kill would terminate the process without unwinding, leaving the OSD overlay
/// enabled and the last drawing stuck on screen over every app.
static EXIT: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_sig: i32) {
    EXIT.store(true, Ordering::SeqCst);
}

fn install_signal_handlers() {
    unsafe {
        for sig in [libc::SIGINT, libc::SIGTERM] {
            libc::signal(sig, on_signal as usize);
        }
    }
}

/// Exponential moving average over raw pressure, so stroke width changes slowly
/// instead of jittering on every pen sample. Damping 0.7 = fairly smooth.
struct PressureSmoother {
    val: f32,
}
impl PressureSmoother {
    fn new() -> Self {
        Self { val: 0.0 }
    }
    /// Start a new stroke from this raw pressure (no smoothing on the first sample).
    fn reset(&mut self, raw: i32) {
        self.val = raw as f32;
    }
    /// Feed one raw sample, return the smoothed value.
    fn feed(&mut self, raw: i32) -> f32 {
        self.val = self.val * 0.7 + (raw as f32) * 0.3;
        self.val
    }
}

/// Smoothed pressure → pen stroke radius (px). Narrow range 2..4 so there's a slight
/// fountain-pen width variation but not enough to feel jumpy.
fn pen_radius(smoothed: f32) -> i32 {
    let norm = (smoothed / 1024.0).clamp(0.0, 1.0);
    2 + (norm * 2.0).round() as i32
}

/// Eraser radius — fat and fixed, no pressure modulation.
const ERASER_RADIUS: i32 = 14;

fn stamp_disk(d: &mut hweink::Draw, cx: i32, cy: i32, r: i32, g: u8) {
    let r2 = r * r;
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy <= r2 {
                d.put_pixel(cx + dx, cy + dy, g);
            }
        }
    }
}

/// Interpolate a line between two points, stamping disks along the way so fast
/// strokes don't leave gaps.
fn stroke_segment(d: &mut hweink::Draw, x0: i32, y0: i32, x1: i32, y1: i32, r: i32, g: u8) {
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let steps = dx.max(dy).max(1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let x = (x0 as f32 + (x1 - x0) as f32 * t).round() as i32;
        let y = (y0 as f32 + (y1 - y0) as f32 * t).round() as i32;
        stamp_disk(d, x, y, r, g);
    }
}

fn main() {
    let ebc = Ebc::open().expect("open /dev/ebc");
    let surf = Surface::new(&ebc, Path::Osd).expect("enable OSD overlay");

    // Clear to white + one full refresh to start clean.
    {
        let mut d = surf.draw();
        d.fill_rect(0, 0, hweink::geom::SCREEN_W, hweink::geom::SCREEN_H, 255);
        d.set_mode(Some(Mode::FullGc16));
    }
    std::thread::sleep(std::time::Duration::from_millis(900));

    let mut pen = Pen::open().expect("open /dev/input/event2");
    let keys_fd = open_gpio_keys();
    install_signal_handlers();

    let mut smoother = PressureSmoother::new();
    let mut last: Option<(i32, i32)> = None;
    let mut drawing = false;
    let mut cur_tool = Tool::None;
    let stdout = io::stdout();

    println!("hweink-note ready. Pen-end draws, eraser-end (flip pen) erases.");
    println!("No auto full-refresh on pen-up (that cleared the overlay). Press 0x2e8 to manually full-refresh, 0x2f4 to clear.");
    println!("Any physical key press prints its code — tell me which button is which.");

    loop {
        if EXIT.load(Ordering::SeqCst) {
            break;
        }

        // 1. Physical keys (non-blocking).
        if keys_fd >= 0 {
            drain_keys(keys_fd, |code, pressed| {
                if pressed {
                    let _ = writeln!(stdout.lock(), "key 0x{:04x}", code);
                    match code {
                        KEY_F24 => {
                            let _ = writeln!(stdout.lock(), "clear");
                            let mut d = surf.draw();
                            d.fill_rect(0, 0, hweink::geom::SCREEN_W, hweink::geom::SCREEN_H, 255);
                            d.set_mode(Some(Mode::FullGc16));
                        }
                        KEY_F8 => {
                            let _ = writeln!(stdout.lock(), "full refresh");
                            force_full_refresh();
                        }
                        _ => {}
                    }
                }
            });
        }

        // 2. Pen (20 ms poll).
        match pen.read(20) {
            Ok(Some(ev)) => {
                // Track tool changes (pen-end ↔ eraser-end via BTN_TOOL_RUBBER).
                if ev.tool != cur_tool {
                    let _ = writeln!(stdout.lock(), "tool: {:?} -> {:?}", cur_tool, ev.tool);
                    cur_tool = ev.tool;
                    // End the current stroke so we don't interpolate across the tool change.
                    drawing = false;
                    last = None;
                }

                let erasing = matches!(cur_tool, Tool::Eraser);
                let g = if erasing { 255 } else { 0 };

                if ev.touch && ev.x >= 0 && ev.y >= 0 {
                    let r = if erasing {
                        ERASER_RADIUS
                    } else {
                        if !drawing {
                            smoother.reset(ev.pressure);
                        }
                        pen_radius(smoother.feed(ev.pressure))
                    };

                    if !drawing {
                        drawing = true;
                        let mut d = surf.draw();
                        stamp_disk(&mut d, ev.x, ev.y, r, g);
                        last = Some((ev.x, ev.y));
                    } else if let Some((lx, ly)) = last {
                        let mut d = surf.draw();
                        stroke_segment(&mut d, lx, ly, ev.x, ev.y, r, g);
                        last = Some((ev.x, ev.y));
                    }
                } else if drawing && !ev.touch {
                    // Pen-up: just end the stroke. NO full-refresh broadcast — that
                    // cleared the OSD overlay and made the stroke flicker/disappear.
                    drawing = false;
                    last = None;
                }
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("pen read error: {}", e);
                break;
            }
        }
    }

    // Explicitly drop the surface BEFORE process exit: Surface::Drop calls
    // DISABLE_OVERLAY, which tears down the OSD layer so the drawing stops showing
    // over every other app. (Rust's runtime already drops it, but being explicit +
    // flushing stdout ensures the teardown actually happens before the process dies.)
    let _ = writeln!(stdout.lock(), "exiting — disabling OSD overlay…");
    let _ = io::stdout().flush();
    drop(surf);
    let _ = writeln!(stdout.lock(), "OSD overlay disabled. Bye.");
}

fn open_gpio_keys() -> i32 {
    let c = match std::ffi::CString::new("/dev/input/event6") {
        Ok(c) => c,
        Err(_) => return -1,
    };
    unsafe { libc::open(c.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NONBLOCK) }
}

fn drain_keys<F: FnMut(u16, bool)>(fd: i32, mut f: F) {
    #[repr(C)]
    struct InputEvent {
        _sec: i64,
        _usec: i64,
        typ: u16,
        code: u16,
        value: i32,
    }
    const EVSZ: usize = std::mem::size_of::<InputEvent>();
    let mut buf = [0u8; EVSZ * 16];
    loop {
        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut _, buf.len()) };
        if n <= 0 {
            break;
        }
        let count = (n as usize) / EVSZ;
        for i in 0..count {
            let ev: InputEvent = unsafe {
                std::ptr::read_unaligned(buf.as_ptr().add(i * EVSZ) as *const InputEvent)
            };
            if ev.typ == EV_KEY {
                f(ev.code, ev.value != 0);
            }
        }
    }
}

fn force_full_refresh() {
    let _ = std::process::Command::new("am")
        .args(["broadcast", "-a", hweink::sysprop::ACTION_FULL_REFRESH_USER])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}
