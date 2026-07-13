//! hweink — a libremarkable-like e-ink library for the Hanwang N10 Pro III
//! (Rockchip RK3576, Android 14, kernel 6.1.75, `/dev/ebc`).
//!
//! The library hides the raw EBC ioctls, the 8bpp Y8 packing, and the 90°-clockwise
//! buffer→screen rotation. Callers draw in **portrait screen coordinates**
//! (1860 × 2480 — what the user sees) and the library translates to the native
//! landscape buffer (2480 × 1860).
//!
//! # Two ways to use this
//!
//! ## 1. Lightweight adaptation (ordinary apps: scroll / page-flip refresh modes)
//!
//! For apps that render through the normal Android `View`→`SurfaceFlinger`→`DRM`
//! pipeline and just want to switch the system's refresh waveform: use
//! [`sysprop::set_system_mode`]. This writes `sys.eink.mode`, which the HWC applies to
//! the main display path. No overlay, no direct buffer writes.
//!
//! ```no_run
//! use hweink::{sysprop, mode::Mode};
//! // user started scrolling a list → fast mode
//! sysprop::set_system_mode(Mode::A2Dither);
//! // scroll ended → clear ghosting and return to high-fidelity.
//! // Full-refresh is a broadcast (see sysprop::ACTION_FULL_REFRESH_USER): from Java
//! // call context.sendBroadcast(new Intent(ACTION_FULL_REFRESH_USER)); from a bare
//! // native process, `am broadcast -a hanvon.intent.fullrefrsh.user`.
//! sysprop::set_system_mode(Mode::PartGlr16);
//! ```
//!
//! ## 2. Direct rendering (note-taking / drawing / kiosk apps)
//!
//! For apps that want pixel-level control, open a [`Surface`] on one of two paths:
//!
//! - [`path::Path::Osd`] — overlay layer, coexists with Android UI, low-latency (pen).
//!   Screencap cannot capture it. This is the path the system pen engine uses.
//! - [`path::Path::Main`] — exclusive full-screen takeover (blanks the HWC).
//!   Screencap-visible. Use for kiosk/full-screen readers.
//!
//! ```no_run
//! use hweink::{Ebc, path::{Surface, Path}, geom::ScreenRect, mode::Mode};
//! let ebc = Ebc::open().unwrap();
//! let surf = Surface::new(&ebc, Path::Osd).unwrap();
//! surf.clear(255);                               // white
//! surf.fill_rect(100, 100, 400, 400, 0);         // black square, screen coords
//! surf.refresh(ScreenRect::full(), None).unwrap(); // push to panel
//! // surf dropped → DISABLE_OVERLAY
//! ```
//!
//! # Pen input
//!
//! [`pen::Pen`] reads `/dev/input/event2` and returns screen-coordinate events:
//!
//! ```no_run
//! use hweink::pen::Pen;
//! let mut p = Pen::open().unwrap();
//! while let Ok(Some(ev)) = p.read(1000) {
//!     if ev.touch {
//!         println!("pen at ({}, {}) pressure {}", ev.x, ev.y, ev.pressure);
//!     }
//! }
//! ```

#![cfg_attr(not(feature = "std"), no_std)]

pub mod draw;
pub mod geom;
pub mod ioctl;
pub mod mode;
#[cfg(feature = "std")]
pub mod ebc;
#[cfg(feature = "std")]
pub mod path;
#[cfg(feature = "std")]
pub mod pen;
#[cfg(feature = "std")]
pub mod refresh;
#[cfg(feature = "std")]
pub mod sysprop;

#[cfg(feature = "std")]
pub use draw::Draw;
#[cfg(feature = "std")]
pub use ebc::Ebc;
#[cfg(feature = "std")]
pub use geom::ScreenRect;
#[cfg(feature = "std")]
pub use mode::Mode;
#[cfg(feature = "std")]
pub use path::{Path, Surface};
