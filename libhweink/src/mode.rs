//! EPD refresh waveforms (the `epd_mode` field / `sys.eink.mode` property values).
//!
//! These mirror the `EinkManager.EinkMode` constants exposed by the platform
//! framework and the Rockchip `panel_refresh_mode` enum. Values are the integer
//! codes used as `epd_mode` in `EbcBufInfo` and as the string value of
//! `sys.eink.mode`.

#![allow(dead_code)]

/// Refresh waveform mode. The integer value is used directly as `epd_mode` in
/// `EbcBufInfo`, and (as a decimal string) as the value of `sys.eink.mode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(i32)]
pub enum Mode {
    /// Driver auto-select. (`AUTO`)
    #[default]
    Auto = 0,
    /// OSD overlay partial waveform (GLD16). Used for the OSD path's `SEND_OSD_BUFFER`.
    Overlay = 1,
    /// Full 16-gray refresh; clears ghosting (flashes). High fidelity, slow.
    FullGc16 = 2,
    FullGl16 = 3,
    /// Regal full (no flash). High fidelity for clean images.
    FullGlr16 = 4,
    /// Regal full, color-panel variant.
    FullGld16 = 5,
    FullGcc16 = 6,
    /// Partial GC16 — local 16-gray refresh.
    PartGc16 = 7,
    PartGl16 = 8,
    /// Partial Regal — the device default (`sys.eink.mode=9`).
    PartGlr16 = 9,
    PartGld16 = 10,
    PartGcc16 = 11,
    /// A2 — fast 1bpp monochrome, for animation / rapid page-flip. No grays.
    A2 = 12,
    /// A2 + dither (for dithered grayscale images at A2 speed).
    A2Dither = 13,
    /// DU — direct update, fast 1bit.
    Du = 14,
    /// DU4 — direct update, 4-gray, fast.
    Du4 = 15,
    A2Enter = 16,
    /// Full panel reset.
    Reset = 17,
    AutoDu = 22,
    AutoDu4 = 23,
}

impl Mode {
    /// Decimal string value, as used for `sys.eink.mode`.
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Auto => "0",
            Mode::Overlay => "1",
            Mode::FullGc16 => "2",
            Mode::FullGl16 => "3",
            Mode::FullGlr16 => "4",
            Mode::FullGld16 => "5",
            Mode::FullGcc16 => "6",
            Mode::PartGc16 => "7",
            Mode::PartGl16 => "8",
            Mode::PartGlr16 => "9",
            Mode::PartGld16 => "10",
            Mode::PartGcc16 => "11",
            Mode::A2 => "12",
            Mode::A2Dither => "13",
            Mode::Du => "14",
            Mode::Du4 => "15",
            Mode::A2Enter => "16",
            Mode::Reset => "17",
            Mode::AutoDu => "22",
            Mode::AutoDu4 => "23",
        }
    }

    /// Parse a `sys.eink.mode` decimal string. Returns `None` for unknown codes.
    pub fn from_str(s: &str) -> Option<Mode> {
        Some(match s {
            "0" => Mode::Auto,
            "1" => Mode::Overlay,
            "2" => Mode::FullGc16,
            "3" => Mode::FullGl16,
            "4" => Mode::FullGlr16,
            "5" => Mode::FullGld16,
            "6" => Mode::FullGcc16,
            "7" => Mode::PartGc16,
            "8" => Mode::PartGl16,
            "9" => Mode::PartGlr16,
            "10" => Mode::PartGld16,
            "11" => Mode::PartGcc16,
            "12" => Mode::A2,
            "13" => Mode::A2Dither,
            "14" => Mode::Du,
            "15" => Mode::Du4,
            "16" => Mode::A2Enter,
            "17" => Mode::Reset,
            "22" => Mode::AutoDu,
            "23" => Mode::AutoDu4,
            _ => return None,
        })
    }
}
