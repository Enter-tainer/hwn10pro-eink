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

/// Broadcast action the platform uses to request a full screen refresh — the same
/// event the SystemUI quick-settings "Refresh" tile, the physical custom key, and
/// apps' self-refresh emit. Sendable by any app or `adb shell` with no special
/// permission (unlike the `.system` variant, which is restricted to the system).
///
/// This is a plain string constant, NOT a function. Sending a broadcast is a Java
/// operation (`Context.sendBroadcast`), and a Java caller doesn't need a Rust library
/// to do it — they just write:
/// ```java
/// context.sendBroadcast(new Intent("hanvon.intent.fullrefrsh.user"));
/// ```
/// We expose the constant only so the canonical action string lives in one place.
///
/// From `adb shell` / a standalone native process with no Java `Context`:
/// `am broadcast -a hanvon.intent.fullrefrsh.user`.
pub const ACTION_FULL_REFRESH_USER: &str = "hanvon.intent.fullrefrsh.user";

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
