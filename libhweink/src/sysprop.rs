//! System-property control surface — the "lightweight adaptation" API.
//!
//! These write Android system properties that the HWC HAL / EBC kernel consume. They
//! affect the **main display path** (Android UI), NOT the OSD overlay. This is the
//! right tool for ordinary apps that want to switch refresh modes on scroll/page-flip
//! without taking over rendering.
//!
//! From Java/Kotlin, the equivalent is `android.os.EinkManager.setMode(...)` /
//! `sendOneFullFrame()`. From native code, set the property directly with
//! `__system_property_set` (no Java/Binder round-trip, no `EinkManager` reflection).

use crate::mode::Mode;

/// Set `sys.eink.mode` — the waveform for the main path's next refresh.
///
/// This is **global**: it affects the whole system until changed back. Apps should
/// restore a sensible mode (e.g. `Mode::PartGlr16`) in `onStop` to avoid leaving the
/// device in A2.
pub fn set_system_mode(mode: Mode) {
    set_property("sys.eink.mode", mode.as_str());
}

/// Read the current `sys.eink.mode` (decimal string, e.g. `"9"`). Returns `"9"`
/// (the device default) if the property is unset.
pub fn get_system_mode() -> String {
    get_property("sys.eink.mode", "9")
}

/// Increment `sys.ebook.one_full_mode_timeline` to force one full refresh (clears
/// ghosting, e.g. after an A2 scroll burst). Mirrors `EinkManager.sendOneFullFrame()`.
pub fn request_one_full_frame() {
    // The property is a monotonic counter; the HWC triggers a full frame on each change.
    // We just need a value different from the current one — toggling between "1" and "2"
    // is sufficient (the framework itself uses an incrementing int with 800ms throttle).
    let cur = get_property("sys.ebook.one_full_mode_timeline", "0");
    let n: i64 = cur.trim().parse::<i64>().unwrap_or(0).wrapping_add(1);
    set_property("sys.ebook.one_full_mode_timeline", &n.to_string());
}

/// Full-refresh cadence: force one full GC16 every `n` partials (0 = disabled).
/// Maps to `persist.vendor.ebook.fullmode_cnt` (Java: `EinkManager.setFullModeCnt`).
pub fn set_full_mode_cnt(n: i32) {
    set_property("persist.vendor.ebook.fullmode_cnt", &n.to_string());
}

/// Night mode (global inversion). `true` = inverted.
pub fn set_night_mode(on: bool) {
    set_property("persist.sys.clr_invert", if on { "1" } else { "0" });
}

// ---- property get/set via __system_property_set (bionic) ----
// bionic's __system_property_set/_get are not exposed by the `libc` crate, so we
// declare them inline here.

extern "C" {
    fn __system_property_set(key: *const core::ffi::c_char, value: *const core::ffi::c_char) -> i32;
    fn __system_property_get(key: *const core::ffi::c_char, value: *mut core::ffi::c_char) -> i32;
}

fn set_property(key: &str, val: &str) {
    let k = match std::ffi::CString::new(key) {
        Ok(c) => c,
        Err(_) => return,
    };
    let v = match std::ffi::CString::new(val) {
        Ok(c) => c,
        Err(_) => return,
    };
    unsafe {
        __system_property_set(k.as_ptr(), v.as_ptr());
    }
}

fn get_property(key: &str, default: &str) -> String {
    let k = match std::ffi::CString::new(key) {
        Ok(c) => c,
        Err(_) => return default.to_string(),
    };
    // PROP_VALUE_MAX = 92 on Android. Use a 128-byte buffer to be safe.
    let mut buf = [0u8; 128];
    let n = unsafe { __system_property_get(k.as_ptr(), buf.as_mut_ptr() as *mut _) };
    if n <= 0 {
        return default.to_string();
    }
    String::from_utf8_lossy(&buf[..n as usize]).into_owned()
}
