//! hweink-demo — on-device demonstration of the hweink library.
//!
//! Subcommands:
//!   probe                    — print EBC geometry, format, offsets
//!   osd                      — draw a compass pattern on the OSD overlay + refresh
//!   main                     — take over the main buffer (SET_FB_BLANK), draw, refresh
//!   modes                    — cycle through refresh modes (writes sys.eink.mode)
//!   full                     — force one full refresh (clear ghosting)
//!   pen                      — read pen events (blocking, sync) and print them
//!   pen_iter                 — pen events via the blocking iterator API (layer 1)
//!   pen_async                — pen events via raw fd + poll_once (layer 3, nonblocking)
//!   draw_batch               — batch-write Draw scope: bezier curve + dot grid in one refresh
//!
//! Build & push:
//!   cargo build --release --target aarch64-linux-android
//!   adb push target/aarch64-linux-android/release/hweink-demo /data/local/tmp/
//!   adb shell /data/local/tmp/hweink-demo <cmd>

use hweink::{mode::Mode, path::Path, Ebc, Surface};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("probe");

    match cmd {
        "probe" => probe(),
        "osd" => draw_compass(Path::Osd),
        "main" => draw_compass(Path::Main),
        "mode" => set_mode(args.get(2).map(|s| s.as_str())),
        "mode?" => print_mode(),
        "full" => {
            // The demo is a bare native process (no Java Context), so it can't call
            // Context.sendBroadcast. Fork `am` instead — fine for a one-shot CLI
            // tool, NOT for an embedded library (use Java sendBroadcast there).
            let _ = std::process::Command::new("am")
                .args(["broadcast", "-a", hweink::sysprop::ACTION_FULL_REFRESH_USER])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            println!("sent full-refresh broadcast ({})", hweink::sysprop::ACTION_FULL_REFRESH_USER);
        }
        "pen" => pen_loop(),
        "pen_iter" => pen_iter_loop(),
        "pen_async" => pen_async_loop(),
        "draw_batch" => draw_batch(Path::Osd),
        _ => {
            eprintln!("usage: hweink-demo <probe|osd|main|modes|full|pen|pen_iter|pen_async|draw_batch>");
            std::process::exit(1);
        }
    }
}

fn probe() {
    // Use open_info (no mmap) — probe is read-only diagnostics and must never hang.
    let ebc = match Ebc::open_info() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("open /dev/ebc failed: {}", e);
            std::process::exit(1);
        }
    };
    println!("EBC /dev/ebc opened (info-only, no mmap)");
    println!("  native buffer : {}x{} (landscape)", ebc.width, ebc.height);
    println!("  panel_color   : {} (0=grayscale)", ebc.panel_color);
    println!("  panel mm      : {}x{}", ebc.width_mm, ebc.height_mm);
    println!(
        "  buf_format    : {} ({})",
        ebc.buf_format,
        if ebc.buf_format == 1 { "Y8 8bpp" } else { "Y4 4bpp" }
    );
    println!(
        "  main offset   : 0x{:x} ({} bytes)",
        ebc.main_offset, ebc.main_offset
    );
    println!(
        "  osd  offset   : 0x{:x} ({} bytes)",
        ebc.osd_offset, ebc.osd_offset
    );
    println!(
        "  screen (portrait) : {}x{}",
        hweink::geom::SCREEN_W,
        hweink::geom::SCREEN_H
    );
    println!("  rotation      : 90 CW (buffer->screen)");
}

/// Draw a direction-distinguishing compass pattern so the user can verify the
/// buffer→screen rotation: 1 block on screen-top edge, 2 on bottom, 3 on left,
/// 4 on right, plus a centered black square with an off-center white dot.
fn draw_compass(path: Path) {
    let ebc = Ebc::open().expect("open /dev/ebc");
    println!("path: {:?}", path);
    let surf = Surface::new(&ebc, path).expect("enable path");

    // Clear to white, full refresh.
    surf.clear(255);
    surf.refresh_full(None).expect("refresh clear");
    std::thread::sleep(std::time::Duration::from_millis(800));

    let w = hweink::geom::SCREEN_W;
    let h = hweink::geom::SCREEN_H;
    let t = 10; // border thickness

    // Black border on all four edges (screen coords).
    surf.fill_rect(0, 0, w, t, 0);
    surf.fill_rect(0, h - t, w, h, 0);
    surf.fill_rect(0, 0, t, h, 0);
    surf.fill_rect(w - t, 0, w, h, 0);

    // Edge block counts: top=1, bottom=2, left=3, right=4.
    let bw = 150; // block half-size
    surf.fill_rect(w / 2 - bw, 30, w / 2 + bw, 30 + 2 * bw, 0); // top: 1
    surf.fill_rect(w / 3 - bw, h - 30 - 2 * bw, w / 3 + bw, h - 30, 0); // bottom: 2a
    surf.fill_rect(2 * w / 3 - bw, h - 30 - 2 * bw, 2 * w / 3 + bw, h - 30, 0); // bottom: 2b
    surf.fill_rect(30, h / 4 - bw, 30 + 2 * bw, h / 4 + bw, 0); // left: 3a
    surf.fill_rect(30, h / 2 - bw, 30 + 2 * bw, h / 2 + bw, 0); // left: 3b
    surf.fill_rect(30, 3 * h / 4 - bw, 30 + 2 * bw, 3 * h / 4 + bw, 0); // left: 3c
    surf.fill_rect(w - 30 - 2 * bw, h / 5 - bw, w - 30, h / 5 + bw, 0); // right: 4a
    surf.fill_rect(w - 30 - 2 * bw, 2 * h / 5 - bw, w - 30, 2 * h / 5 + bw, 0); // right: 4b
    surf.fill_rect(w - 30 - 2 * bw, 3 * h / 5 - bw, w - 30, 3 * h / 5 + bw, 0); // right: 4c
    surf.fill_rect(w - 30 - 2 * bw, 4 * h / 5 - bw, w - 30, 4 * h / 5 + bw, 0); // right: 4d

    // Centered black square with a white dot in its top-left (rotation tell).
    surf.fill_rect(w / 2 - 300, h / 2 - 300, w / 2 + 300, h / 2 + 300, 0);
    surf.fill_rect(w / 2 - 300 + 60, h / 2 - 300 + 60, w / 2 - 300 + 220, h / 2 - 300 + 220, 255);

    println!("refreshing...");
    surf.refresh_full(None).expect("refresh");
    println!("done — check screen. Expect: top=1 blk, bottom=2, left=3, right=4, white dot at screen top-left of center.");

    // Hold so the user (and screencap for the main path) can see it.
    std::thread::sleep(std::time::Duration::from_secs(10));
    // Surface drop disables the path (DISABLE_OVERLAY / SET_FB_UNBLANK).
}

/// `mode <NAME|NUM>` — set `sys.eink.mode` once and print the result. No cycling.
/// Accepts a symbolic name (case-insensitive, `full_gc16`, `part_glr16`, `a2`, ...)
/// or a raw integer/string code the driver knows. Run `mode?` with no arg to list.
fn set_mode(arg: Option<&str>) {
    let Some(s) = arg else {
        eprintln!("usage: hweink-demo mode <NAME|NUM>");
        eprintln!("  names: auto overlay full_gc16 full_gl16 full_glr16 full_gld16 full_gcc16");
        eprintln!("         part_gc16 part_gl16 part_glr16 part_gld16 part_gcc16");
        eprintln!("         a2 a2_dither du du4 a2_enter reset auto_du auto_du4");
        eprintln!("  or a raw integer/string code (0,1,2,...,23). `mode?` shows current.");
        std::process::exit(1);
    };
    let mode = match parse_mode(s) {
        Some(m) => m,
        None => {
            eprintln!("unknown mode {:?}. `mode?` lists valid names.", s);
            std::process::exit(1);
        }
    };
    let before = hweink::sysprop::get_system_mode();
    hweink::sysprop::set_system_mode(mode);
    // Read back to confirm the HWC actually accepted it (may differ if rejected).
    let after = hweink::sysprop::get_system_mode();
    println!(
        "sys.eink.mode: {} -> {}  (asked {:?}={})",
        before,
        after,
        mode,
        mode.as_str()
    );
}

/// `mode?` — print the current `sys.eink.mode` (decimal string) and its name.
fn print_mode() {
    let cur = hweink::sysprop::get_system_mode();
    let name = Mode::from_str(&cur)
        .map(|m| format!("{:?}", m))
        .unwrap_or_else(|| "?".into());
    println!("sys.eink.mode = {} ({})", cur, name);
}

fn parse_mode(s: &str) -> Option<Mode> {
    // Try numeric first (decimal string the driver uses).
    if let Some(m) = Mode::from_str(s) {
        return Some(m);
    }
    // Strip a leading "EPD_" if present (e.g. "EPD_A2").
    let s2 = s.strip_prefix("EPD_").unwrap_or(s);
    Some(match s2.to_ascii_lowercase().as_str() {
        "auto" => Mode::Auto,
        "overlay" => Mode::Overlay,
        "full_gc16" => Mode::FullGc16,
        "full_gl16" => Mode::FullGl16,
        "full_glr16" => Mode::FullGlr16,
        "full_gld16" => Mode::FullGld16,
        "full_gcc16" => Mode::FullGcc16,
        "part_gc16" => Mode::PartGc16,
        "part_gl16" => Mode::PartGl16,
        "part_glr16" => Mode::PartGlr16,
        "part_gld16" => Mode::PartGld16,
        "part_gcc16" => Mode::PartGcc16,
        "a2" => Mode::A2,
        "a2_dither" => Mode::A2Dither,
        "du" => Mode::Du,
        "du4" => Mode::Du4,
        "a2_enter" => Mode::A2Enter,
        "reset" => Mode::Reset,
        "auto_du" => Mode::AutoDu,
        "auto_du4" => Mode::AutoDu4,
        _ => return None,
    })
}

fn pen_loop() {
    let mut pen = hweink::pen::Pen::open().expect("open /dev/input/event2");
    println!("reading pen events (sync, blocking; Ctrl-C to stop)...");
    loop {
        match pen.read(1000) {
            Ok(Some(ev)) => {
                if ev.touch || ev.in_proximity {
                    println!(
                        "x={:4} y={:4} p={:4}({}) tool={:?} prox={} touch={} b1={} b2={} b3={}",
                        ev.x, ev.y, ev.pressure, ev.pressure_norm(), ev.tool, ev.in_proximity,
                        ev.touch, ev.stylus_btn, ev.stylus_btn2, ev.stylus_btn3
                    );
                }
            }
            Ok(None) => { /* timeout */ }
            Err(e) => {
                eprintln!("pen read error: {}", e);
                break;
            }
        }
    }
}

/// Layer 1: blocking iterator API. `for ev in pen.events()` in a dedicated thread.
fn pen_iter_loop() {
    let mut pen = hweink::pen::Pen::open().expect("open /dev/input/event2");
    println!("pen_iter: blocking iterator (layer 1). Ctrl-C to stop.");
    for ev in pen.events() {
        if ev.touch || ev.in_proximity {
            println!(
                "[iter] x={:4} y={:4} p={:4}({}) tool={:?} touch={}",
                ev.x, ev.y, ev.pressure, ev.pressure_norm(), ev.tool, ev.touch
            );
        }
    }
    eprintln!("pen_iter: iterator ended");
}

/// Layer 3: raw fd + nonblocking + poll_once. Demonstrates the building block for
/// integrating pen input into an external event loop (mio/tokio/GUI). We poll with a
/// 100ms timeout in a loop — a real integration would register `pen.fd()` with a
/// selector and call `poll_once(Duration::ZERO)` only when readable.
fn pen_async_loop() {
    let mut pen = hweink::pen::Pen::open().expect("open /dev/input/event2");
    pen.set_nonblocking(true).expect("set_nonblocking");
    let fd = pen.fd();
    println!("pen_async: raw fd={} + poll_once (layer 3, nonblocking). Ctrl-C to stop.", fd);
    // A tiny "event loop": poll once every 100ms. In a real app, the fd would be
    // registered with epoll/mio and poll_once(Duration::ZERO) called on readable.
    loop {
        match pen.poll_once(std::time::Duration::from_millis(100)) {
            Ok(Some(ev)) => {
                if ev.touch || ev.in_proximity {
                    println!(
                        "[async] x={:4} y={:4} p={:4}({}) tool={:?} touch={}",
                        ev.x, ev.y, ev.pressure, ev.pressure_norm(), ev.tool, ev.touch
                    );
                }
            }
            Ok(None) => { /* no full frame this 100ms tick; keep looping */ }
            Err(e) => {
                eprintln!("pen_async error: {}", e);
                break;
            }
        }
    }
}

/// Batch-write Draw scope: draws a cubic bezier + a 5x5 dot grid into the shadow
/// buffer, then flushes ONE refresh. Demonstrates the batch API + raw_shadow_mut for
/// custom point loops (bezier sampling).
fn draw_batch(path: Path) {
    let ebc = Ebc::open().expect("open /dev/ebc");
    println!("path: {:?}", path);
    let surf = Surface::new(&ebc, path).expect("enable path");

    let w = hweink::geom::SCREEN_W;
    let h = hweink::geom::SCREEN_H;

    // Clear to white, full refresh (outside the batch scope, one-shot).
    surf.draw_with(|d| d.fill_rect(0, 0, w, h, 255)).expect("clear");
    std::thread::sleep(std::time::Duration::from_millis(600));

    // Now ONE batch scope that draws a bezier + a dot grid, flushed as a single refresh.
    println!("drawing bezier + dot grid in one batch...");
    let t0 = std::time::Instant::now();
    surf.draw_with(|d| {
        // Cubic bezier: P0(200,200) P1(w-200,100) P2(100,h-200) P3(w-200,h-200).
        let p0 = (200.0_f32, 200.0);
        let p1 = ((w - 200) as f32, 100.0);
        let p2 = (100.0, (h - 200) as f32);
        let p3 = ((w - 200) as f32, (h - 200) as f32);
        // Sample 400 points along the curve; collect into a Vec for put_pixels.
        let mut pts = Vec::with_capacity(400);
        for i in 0..400u32 {
            let t = i as f32 / 399.0;
            let u = 1.0 - t;
            let x = u*u*u*p0.0 + 3.0*u*u*t*p1.0 + 3.0*u*t*t*p2.0 + t*t*t*p3.0;
            let y = u*u*u*p0.1 + 3.0*u*u*t*p1.1 + 3.0*u*t*t*p2.1 + t*t*t*p3.1;
            // 3px-thick cross around each sample point (cheap "brush").
            for &(dx, dy) in &[(0,0),(1,0),(-1,0),(0,1),(0,-1)] {
                pts.push(((x as i32)+dx, (y as i32)+dy, 0u8));
            }
        }
        d.put_pixels(&pts);

        // 5x5 dot grid via raw_shadow_mut (direct buffer write) + manual dirty_union.
        let (buf, stride) = d.raw_shadow_mut();
        let gx0 = w / 2 - 240;
        let gy0 = h / 2 - 240;
        for gy in 0..5 {
            for gx in 0..5 {
                let cx = gx0 + gx * 120;
                let cy = gy0 + gy * 120;
                for ry in -4..5 {
                    for rx in -4..5 {
                        if rx*rx + ry*ry <= 16 {
                            let px = cx + rx;
                            let py = cy + ry;
                            if px >= 0 && py >= 0 && (px as i32) < w && (py as i32) < h {
                                buf[(py as usize) * stride + (px as usize)] = 0;
                            }
                        }
                    }
                }
            }
        }
        d.dirty_union(hweink::ScreenRect::new(gx0 - 5, gy0 - 5, gx0 + 4*120 + 5, gy0 + 4*120 + 5));

        // Override the refresh mode to A2 (fast mono) — good for line art.
        d.set_mode(Some(Mode::A2));
    }).expect("flush");
    let dt = t0.elapsed();
    println!("done in {:?} — single refresh issued. Expect: one bezier curve + 5x5 dot grid.", dt);

    std::thread::sleep(std::time::Duration::from_secs(10));
}
