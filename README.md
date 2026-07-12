# hwn10pro-eink

> **⚠️ AI-generated.** The overwhelming majority of the code and documentation in
> this repository was written by an LLM (Claude). Human involvement was minimal —
> limited to on-device testing, direction-setting, and review. Treat it accordingly:
> verify before relying on it.

A libremarkable-style e-ink library for the **Hanwang N10 Pro III** (Rockchip RK3576, Android 14, kernel 6.1.75), implemented in Rust.

`hweink` talks to the on-device Rockchip **EBC** (E-Book Controller) driver at `/dev/ebc` and gives you a portrait-coordinate drawing API, refresh-waveform selection, and pen/stylus input — so you can build note-taking, reading, and kiosk apps that fit the e-ink screen instead of fighting it.

> **Status:** experimental. The OSD overlay, system-mode, and pen paths are verified on-device; the main-buffer takeover and batch-draw paths compile but are still being validated. See `OVERVIEW.md` for the full ABI notes and verification matrix.

## Features

- **Two render paths**
  - `Path::Osd` — overlay layer that coexists with the Android UI, low-latency, ideal for pen input. (Screencap can't see it.)
  - `Path::Main` — exclusive full-screen takeover (blanks the HWC), screencap-visible, for kiosk/full-screen readers.
- **Portrait-coordinate API** — draw in 1860×2480 (what the user sees); the library handles the native 2480×1860 landscape buffer and the 90°-CW rotation internally.
- **Refresh-waveform control** — per-frame `epd_mode` (GC16 / GLR16 / A2 / DU / …) plus system-level `sys.eink.mode` switching.
- **Batch drawing** — `Surface::draw()` scope with a shadow buffer and dirty-rect union, flushing one refresh per batch.
- **Pen/stylus input** — reads `/dev/input/event2`, synthesizes frame-aligned events, exposes both blocking and non-blocking (raw-fd) APIs.
- **Refresh tuning** — `set_full_mode_num`, `set_diff_percent`, `set_wait_new_buf_time` ioctls, plus `request_one_full_frame`.

## Quick start

### Prerequisites

- Rust (stable) + the Android aarch64 target:
  ```sh
  rustup target add aarch64-linux-android
  ```
- Android NDK r25+ (provides `aarch64-linux-android31-clang`).
- A Hanwang N10 Pro III connected over ADB with `adb` on your PATH.

### Configure the linker

The workspace `.cargo/config.toml` is intentionally machine-agnostic. Set the linker via environment variable before building:

```sh
# Windows PowerShell
$env:CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER = "C:\Users\you\AppData\Local\Android\Sdk\ndk\<ver>\toolchains\llvm\prebuilt\windows-x86_64\bin\aarch64-linux-android31-clang.cmd"

# Linux / macOS
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=/path/to/ndk/toolchains/llvm/prebuilt/<host>/bin/aarch64-linux-android31-clang
```

(Or edit `.cargo/config.toml` to hardcode your path.)

### Build & run the demo

```sh
cargo build --release --target aarch64-linux-android
adb push target/aarch64-linux-android/release/hweink-demo /data/local/tmp/
adb shell chmod 755 /data/local/tmp/hweink-demo

# Probe the EBC driver (read-only, safe)
adb shell /data/local/tmp/hweink-demo probe

# Draw a compass pattern on the OSD overlay (look at the screen)
adb shell /data/local/tmp/hweink-demo osd

# Read pen events (pick up the stylus)
adb shell /data/local/tmp/hweink-demo pen

# Set / query the system refresh mode
adb shell /data/local/tmp/hweink-demo mode a2
adb shell /data/local/tmp/hweink-demo mode?
```

### Use the library

```toml
# Cargo.toml
[dependencies]
hweink = { path = "../libhweink" }   # or a git/published version
```

```rust
use hweink::{mode::Mode, path::Path, Ebc, Surface};

let ebc = Ebc::open()?;                         // opens /dev/ebc, mmaps the buffers
let surf = Surface::new(&ebc, Path::Osd)?;      // enable the OSD overlay path

surf.clear(255);                                // white
{
    let mut d = surf.draw();                    // batch scope: shadow buffer + dirty union
    d.fill_rect(100, 100, 400, 400, 0);         // black square, portrait coords
    d.set_mode(Some(Mode::A2));                 // this batch flushes with A2
}                                               // drop → one refresh of the dirty rect

// Pen input
let mut pen = hweink::pen::Pen::open()?;
while let Some(ev) = pen.read(1000)? {
    if ev.touch {
        println!("pen at ({}, {}) pressure {}", ev.x, ev.y, ev.pressure);
    }
}
```

## Repository layout

```
.
├── Cargo.toml          # workspace root (members: libhweink, hweink-demo)
├── .cargo/config.toml  # aarch64-linux-android linker config
├── OVERVIEW.md         # EBC ABI notes — the technical reference
├── README.md           # this file
├── libhweink/          # the library crate
│   ├── Cargo.toml
│   └── src/            # ioctl, ebc, path, draw, geom, mode, refresh, sysprop, pen
└── hweink-demo/        # the demo binary
    ├── Cargo.toml
    ├── src/main.rs     # probe / osd / main / mode / pen / draw_batch …
    └── examples/       # small probes (absprobe, smoke)
```

## Documentation

- **`OVERVIEW.md`** — the full EBC adaptation notes: ioctl ABI, the 68-byte `ebc_buf_info` struct (and why it's not 44), 8bpp Y8 packing, the 90°-CW rotation, refresh-control surface, and pen input. Read this before doing anything non-trivial.

## License

MIT. See `LICENSE`.

## Disclaimer

This is an independent, community project. It is **not** affiliated with, endorsed by, or officially supported by Hanwang or Rockchip. The EBC driver ABI implemented here was determined from the public driver headers and on-device observation; it may differ on other firmware revisions or other Rockchip-based devices. Use at your own risk.
