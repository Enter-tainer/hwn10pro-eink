//! Raw integer ioctl codes and `ebc_buf_info` struct for the Rockchip EBC driver.
//!
//! These are NOT `_IO*`-macro-encoded ioctls — the Rockchip `ebc-dev` driver uses
//! plain integers `0x7000..0x700d` (develop-4.19 / 6.1) plus the 6.1 "y8" extension
//! `0x7010/0x7013/0x7014`. Confirmed `rc=0` for all of these on the Hanwang N10 Pro III
//! (RK3576, kernel 6.1.75).
//!
//! `struct ebc_buf_info` is 44 bytes (11 × int32) on this device — confirmed by probe.
//! (The develop-4.19 variant is 64 bytes with `needpic` + `tid_name[16]`; this device
//! returns success with the 44-byte layout, so we use that.)

#![allow(dead_code)]

// ---- ioctl request codes (raw integers, not _IO* macros) ----
pub const EBC_GET_BUFFER: u32 = 0x7000;
pub const EBC_SEND_BUFFER: u32 = 0x7001;
pub const EBC_GET_BUFFER_INFO: u32 = 0x7002;
pub const EBC_SET_FULL_MODE_NUM: u32 = 0x7003;
pub const EBC_ENABLE_OVERLAY: u32 = 0x7004;
pub const EBC_DISABLE_OVERLAY: u32 = 0x7005;
pub const EBC_GET_OSD_BUFFER: u32 = 0x7006;
pub const EBC_SEND_OSD_BUFFER: u32 = 0x7007;
pub const EBC_NEW_BUF_PREPARE: u32 = 0x7008;
pub const EBC_SET_DIFF_PERCENT: u32 = 0x7009;
pub const EBC_WAIT_NEW_BUF_TIME: u32 = 0x700a;
pub const EBC_GET_OVERLAY_STATUS: u32 = 0x700b;
pub const EBC_ENABLE_BG_CONTROL: u32 = 0x700c;
pub const EBC_DISABLE_BG_CONTROL: u32 = 0x700d;
// 6.1 y8 extension
pub const EBC_GET_BUF_FORMAT: u32 = 0x7010;
pub const EBC_SET_FB_BLANK: u32 = 0x7013;
pub const EBC_SET_FB_UNBLANK: u32 = 0x7014;

// ---- buffer format (returned by EBC_GET_BUF_FORMAT) ----
pub const EBC_Y4: i32 = 0;
pub const EBC_Y8: i32 = 1;

/// `struct ebc_buf_info` — **68 bytes** on this device (Hanwang N10 Pro III, RK3576,
/// kernel 6.1.75 running a downstream Rockchip **"y8" fork** of the ebc-dev driver).
///
/// The stock `rockchip-linux/kernel` `develop-6.1` `ebc_dev.h` is only 44 bytes
/// (11 ints, ioctls 0x7000–0x7007), but this device answers `0x7010/0x7013/0x7014`
/// which stock 6.1 does NOT define — so it runs the y8 fork
/// (matches `yinbaiyuan/houzzkit-f1-opensource` kernel-6.1, and the Khadas
/// `libebook`/`NoteDemo` userspace ABI). That fork's struct adds three tail fields:
///
/// ```text
///  int  offset        //  0
///  int  epd_mode      //  4
///  int  height        //  8
///  int  width         // 12
///  int  panel_color   // 16
///  int  win_x1        // 20
///  int  win_y1        // 24
///  int  win_x2        // 28
///  int  win_y2        // 32
///  int  width_mm      // 36
///  int  height_mm     // 40
///  int  dropable      // 44   (4.19 called this `needpic`; same offset, renamed)
///  char tid_name[16]  // 48
///  int  dma_buf_fd    // 64   (y8-only; 4.19/5.10 stop at tid_name → 64 bytes)
/// ```                  // = 68, no padding (all 4-aligned, 68 % 4 == 0)
///
/// **WHY 68, NOT 44/64:** `EBC_GET_BUFFER_INFO`/`EBC_GET_OSD_BUFFER` do a
/// `copy_to_user` of the FULL 68-byte struct. If we hand a 44- or 64-byte buffer,
/// the overrun (24 or 4 bytes) smashes the caller's saved LR → `ret` jumps to a
/// garbage/0 address → SIGSEGV (a Heisenbug: an `eprintln!` can mask it by
/// reshuffling the stack). Mirroring the real 68-byte layout is the fix.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EbcBufInfo {
    pub offset: i32,
    pub epd_mode: i32,
    pub height: i32,
    pub width: i32,
    pub panel_color: i32,
    pub win_x1: i32,
    pub win_y1: i32,
    pub win_x2: i32,
    pub win_y2: i32,
    pub width_mm: i32,
    pub height_mm: i32,
    pub dropable: i32,
    pub tid_name: [u8; 16],
    pub dma_buf_fd: i32,
}

impl Default for EbcBufInfo {
    fn default() -> Self {
        EbcBufInfo {
            offset: 0,
            epd_mode: 0,
            height: 0,
            width: 0,
            panel_color: 0,
            win_x1: 0,
            win_y1: 0,
            win_x2: 0,
            win_y2: 0,
            width_mm: 0,
            height_mm: 0,
            dropable: 0,
            tid_name: [0u8; 16],
            dma_buf_fd: 0,
        }
    }
}

// Compile-time guard: if a field edit ever changes the size, fail the build rather
// than silently reintroduce the stack-smash.
const _: () = assert!(core::mem::size_of::<EbcBufInfo>() == 68);

impl EbcBufInfo {
    /// All-zero info with only `offset`/`epd_mode`/`win_*` set for a refresh call.
    /// `dropable=0` (droppable), `dma_buf_fd=0` (none) — sensible defaults for SEND.
    pub fn for_refresh(offset: i32, epd_mode: i32, w: i32, h: i32, rect: Rect) -> Self {
        let mut s = EbcBufInfo::default();
        s.offset = offset;
        s.epd_mode = epd_mode;
        s.height = h;
        s.width = w;
        s.win_x1 = rect.x1;
        s.win_y1 = rect.y1;
        s.win_x2 = rect.x2;
        s.win_y2 = rect.y2;
        s
    }
}

/// Half-open native-buffer rectangle `[x1, x2) × [y1, y2)`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Rect {
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
}

impl Rect {
    pub const fn new(x1: i32, y1: i32, x2: i32, y2: i32) -> Self {
        Rect { x1, y1, x2, y2 }
    }
    pub const fn full(w: i32, h: i32) -> Self {
        Rect { x1: 0, y1: 0, x2: w, y2: h }
    }
}

/// ioctl helpers: thin wrappers over `libc::ioctl` returning `io::Result<()>`.
/// `cmd` is a raw integer request code (0x7000..0x7014 for the EBC driver).
#[cfg(feature = "std")]
pub fn ioctl_ptr(fd: i32, cmd: u32, arg: *mut core::ffi::c_void) -> std::io::Result<()> {
    let r = unsafe { libc::ioctl(fd, cmd as i32, arg) };
    if r < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(feature = "std")]
pub fn ioctl_null(fd: i32, cmd: u32) -> std::io::Result<()> {
    let r = unsafe { libc::ioctl(fd, cmd as i32, core::ptr::null_mut::<core::ffi::c_void>()) };
    if r < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
