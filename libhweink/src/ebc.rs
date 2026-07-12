//! `/dev/ebc` device handle: open, mmap, geometry, format.

use crate::ioctl::{
    EbcBufInfo, EBC_GET_BUFFER, EBC_GET_BUF_FORMAT, EBC_GET_BUFFER_INFO, EBC_GET_OSD_BUFFER,
    EBC_Y8,
};
use crate::ioctl::{ioctl_null, ioctl_ptr};
use core::ptr;

/// Default mmap span. The OSD buffer lives at offset 0x1400000 (20MB) and the main
/// buffer at 0xa00000 (10MB); 32MB covers both with margin. (The system pen engine
/// maps 25MB; we use 32MB to be safe.)
pub const MMAP_SIZE: usize = 0x20_0000_0; // 32 MB

/// Handle on `/dev/ebc`. Owns the fd and (optionally) the mmap.
pub struct Ebc {
    fd: i32,
    map: *mut u8,
    map_len: usize,
    /// Native buffer dimensions (landscape). width=2480, height=1860 on this device.
    pub width: i32,
    pub height: i32,
    pub panel_color: i32,
    pub width_mm: i32,
    pub height_mm: i32,
    /// Buffer format from `EBC_GET_BUF_FORMAT`: `EBC_Y4` or `EBC_Y8`.
    /// On this device it is `EBC_Y8` — both main and OSD buffers are 8bpp.
    pub buf_format: i32,
    /// Main display-buffer offset, from `EBC_GET_BUFFER` (0xa00000 on this device).
    /// 0 if opened via `open_info` (which skips the buffer-acquiring GET_BUFFER call).
    pub main_offset: i32,
    /// OSD buffer offset, from `EBC_GET_OSD_BUFFER` (0x1400000 on this device).
    pub osd_offset: i32,
}

impl Ebc {
    /// Open `/dev/ebc` read/write, query geometry + offsets, mmap 32MB.
    ///
    /// WARNING: `mmap` of the full CMA region can block in the kernel
    /// (`ebc_empty_buf_get`) when the HWC owns the buffers. For read-only diagnostics
    /// use `open_info` instead.
    pub fn open() -> std::io::Result<Self> {
        Self::open_path("/dev/ebc")
    }

    /// Probe-only: open the device, query geometry/format/offsets, but do NOT mmap
    /// and do NOT acquire a display buffer. Safe for diagnostics — never hangs.
    pub fn open_info() -> std::io::Result<Self> {
        Self::open_info_path("/dev/ebc")
    }

    pub fn open_info_path(path: &str) -> std::io::Result<Self> {
        let c_path = std::ffi::CString::new(path).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "path contained NUL")
        })?;
        let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut info = EbcBufInfo::default();
        ioctl_ptr(
            fd,
            EBC_GET_BUFFER_INFO,
            &mut info as *mut EbcBufInfo as *mut _,
        )?;
        // GET_BUF_FORMAT — read-only, safe.
        let mut fmt: i32 = -1;
        let _ = ioctl_ptr(
            fd,
            EBC_GET_BUF_FORMAT,
            &mut fmt as *mut i32 as *mut _,
        );
        // GET_OSD_BUFFER — returns the OSD offset without acquiring a display buffer.
        let mut osd = EbcBufInfo::default();
        let mut osd_offset = info.offset;
        if ioctl_ptr(fd, EBC_GET_OSD_BUFFER, &mut osd as *mut EbcBufInfo as *mut _).is_ok()
            && osd.offset >= 0
        {
            osd_offset = osd.offset;
        }
        // Deliberately do NOT call EBC_GET_BUFFER (0x7000) — it acquires a buffer that
        // we cannot release without the mmap+send, and would leak/hang the driver.
        Ok(Ebc {
            fd,
            map: ptr::null_mut(),
            map_len: 0,
            width: info.width,
            height: info.height,
            panel_color: info.panel_color,
            width_mm: info.width_mm,
            height_mm: info.height_mm,
            buf_format: fmt,
            main_offset: 0,
            osd_offset,
        })
    }

    pub fn open_path(path: &str) -> std::io::Result<Self> {
        let c_path = std::ffi::CString::new(path).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "path contained NUL")
        })?;
        let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }

        let mut info = EbcBufInfo::default();
        if ioctl_ptr(
            fd,
            EBC_GET_BUFFER_INFO,
            &mut info as *mut EbcBufInfo as *mut _,
        )
        .is_err()
        {
            let e = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(e);
        }

        let mut fmt: i32 = -1;
        let _ = ioctl_ptr(
            fd,
            EBC_GET_BUF_FORMAT,
            &mut fmt as *mut i32 as *mut _,
        );

        // main buffer offset — acquires a display buffer (released by SEND_BUFFER).
        let mut gb = EbcBufInfo::default();
        let mut main_offset = 0;
        if ioctl_ptr(fd, EBC_GET_BUFFER, &mut gb as *mut EbcBufInfo as *mut _).is_ok()
            && gb.offset >= 0
        {
            main_offset = gb.offset;
        }

        let mut osd = EbcBufInfo::default();
        let mut osd_offset = info.offset;
        if ioctl_ptr(fd, EBC_GET_OSD_BUFFER, &mut osd as *mut EbcBufInfo as *mut _).is_ok()
            && osd.offset >= 0
        {
            osd_offset = osd.offset;
        }

        let map_len = MMAP_SIZE;
        let map = unsafe {
            libc::mmap(
                ptr::null_mut(),
                map_len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if map == libc::MAP_FAILED {
            let e = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(e);
        }

        Ok(Ebc {
            fd,
            map: map as *mut u8,
            map_len,
            width: info.width,
            height: info.height,
            panel_color: info.panel_color,
            width_mm: info.width_mm,
            height_mm: info.height_mm,
            buf_format: fmt,
            main_offset,
            osd_offset,
        })
    }

    /// Raw fd.
    pub fn fd(&self) -> i32 {
        self.fd
    }

    /// Mapped base pointer (null if opened via `open_info`).
    pub fn map_base(&self) -> *mut u8 {
        self.map
    }

    pub fn map_len(&self) -> usize {
        self.map_len
    }

    /// Bytes per pixel. 8 on this device (Y8), 4 if the driver reports Y4.
    pub fn bpp(&self) -> i32 {
        if self.buf_format == EBC_Y8 {
            8
        } else {
            4
        }
    }

    /// Stride in bytes for an 8bpp Y8 buffer = width.
    pub fn stride(&self) -> usize {
        self.width as usize
    }

    /// Pointer to the OSD buffer region (8bpp Y8). Null if no mmap.
    pub fn osd_ptr(&self) -> *mut u8 {
        if self.map.is_null() {
            return ptr::null_mut();
        }
        unsafe { self.map.add(self.osd_offset as usize) }
    }

    /// Pointer to the main display-buffer region (8bpp Y8 on this device). Null if no mmap.
    pub fn main_ptr(&self) -> *mut u8 {
        if self.map.is_null() {
            return ptr::null_mut();
        }
        unsafe { self.map.add(self.main_offset as usize) }
    }

    /// Size in bytes of one full 8bpp frame (width × height).
    pub fn frame_bytes(&self) -> usize {
        self.stride() * self.height as usize
    }

    /// Issue a no-argument ioctl (e.g. ENABLE_OVERLAY, SET_FB_BLANK).
    pub fn ioctl0(&self, cmd: u32) -> std::io::Result<()> {
        ioctl_null(self.fd, cmd)
    }

    /// Issue an ioctl with a pointer to an `EbcBufInfo`.
    pub fn ioctl_info(&self, cmd: u32, info: &mut EbcBufInfo) -> std::io::Result<()> {
        ioctl_ptr(self.fd, cmd, info as *mut EbcBufInfo as *mut _)
    }
}

impl Drop for Ebc {
    fn drop(&mut self) {
        unsafe {
            if !self.map.is_null() && self.map as *mut _ != libc::MAP_FAILED {
                libc::munmap(self.map as *mut _, self.map_len);
            }
            if self.fd >= 0 {
                libc::close(self.fd);
            }
        }
    }
}

// SAFETY: the mmap is shared with the kernel; no Rust aliasing within the process.
// Callers must coordinate access to buffer regions themselves.
unsafe impl Send for Ebc {}
unsafe impl Sync for Ebc {}
