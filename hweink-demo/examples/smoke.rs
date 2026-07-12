fn main() {
    eprintln!("ALIVE 1");
    println!("ALIVE 2");
    use std::ffi::CString;
    let p = CString::new("/dev/ebc").unwrap();
    eprintln!("opening...");
    extern "C" {
        fn open(path: *const core::ffi::c_char, flags: core::ffi::c_int, ...) -> core::ffi::c_int;
        fn ioctl(fd: core::ffi::c_int, cmd: core::ffi::c_ulong, ...) -> core::ffi::c_int;
        fn close(fd: core::ffi::c_int) -> core::ffi::c_int;
    }
    let fd = unsafe { open(p.as_ptr(), 0o2 | 0o2000000) };
    eprintln!("fd = {}", fd);
    if fd >= 0 {
        // GET_BUFFER_FORMAT = 0x7010 (returns i32: 0=Y4, 1=Y8)
        let mut fmt: i32 = -1;
        let r = unsafe { ioctl(fd, 0x7010u64 as _, &mut fmt as *mut i32 as *mut core::ffi::c_void) };
        eprintln!("GET_BUF_FORMAT rc={} fmt={}", r, fmt);

        // GET_OSD_BUFFER = 0x7006 (fills ebc_buf_info, returns osd offset)
        #[repr(C)]
        #[derive(Default)]
        struct Info {
            o: i32, e: i32, h: i32, w: i32, pc: i32,
            x1: i32, y1: i32, x2: i32, y2: i32, wm: i32, hm: i32,
        }
        let mut osd = Info::default();
        let r2 = unsafe { ioctl(fd, 0x7006u64 as _, &mut osd as *mut _ as *mut core::ffi::c_void) };
        eprintln!("GET_OSD_BUFFER rc={} offset=0x{:x} w={} h={} pc={}", r2, osd.o, osd.w, osd.h, osd.pc);

        unsafe { close(fd) };
    }
    eprintln!("DONE");
}
