//! Refresh-tuning ioctls: full-mode cadence, diff percent, new-buf wait time.
//!
//! These all live in the kernel auto-refresh loop (`ebc_global.info.full_mode_num`,
//! `diff_percent`, `waiting_new_buf_time`). They are global to the EBC device, not
//! per-path.

use crate::ebc::Ebc;
use crate::ioctl::{ioctl_ptr, EBC_SET_DIFF_PERCENT, EBC_SET_FULL_MODE_NUM, EBC_WAIT_NEW_BUF_TIME};

impl Ebc {
    /// `EBC_SET_FULL_MODE_NUM` (0x7003): after `n` partial refreshes, force one full
    /// GC16 to clear accumulated ghosting. `n=0` disables. Typical values: ~50 for
    /// reading (rare full clears), ~5 for clean display (frequent clears, more flashing).
    ///
    /// Mirrors the Java path `EinkManager.setFullModeCnt` →
    /// `persist.vendor.ebook.fullmode_cnt` (which the HWC then pushes to this ioctl).
    pub fn set_full_mode_num(&self, n: i32) -> std::io::Result<()> {
        let mut v = n;
        ioctl_ptr(
            self.fd(),
            EBC_SET_FULL_MODE_NUM,
            &mut v as *mut i32 as *mut _,
        )
    }

    /// `EBC_SET_DIFF_PERCENT` (0x7009): if the new frame differs from the old frame on
    /// more than `pct`% of pixels, force a full refresh instead of a partial. Default 50.
    pub fn set_diff_percent(&self, pct: i32) -> std::io::Result<()> {
        let mut v = pct;
        ioctl_ptr(self.fd(), EBC_SET_DIFF_PERCENT, &mut v as *mut i32 as *mut _)
    }

    /// `EBC_WAIT_NEW_BUF_TIME` (0x700a): time in ms the auto-refresh loop waits for a
    /// new buffer before refreshing with the current one. A throttle that prevents
    /// excessive background refreshes.
    pub fn set_wait_new_buf_time(&self, ms: i32) -> std::io::Result<()> {
        let mut v = ms;
        ioctl_ptr(self.fd(), EBC_WAIT_NEW_BUF_TIME, &mut v as *mut i32 as *mut _)
    }
}
