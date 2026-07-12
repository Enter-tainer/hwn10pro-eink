// Inline the open_info logic with explicit eprintln after every line, using libc
// directly (not hweink) to bypass any wrapper. This isolates where it dies.
fn main() {
    eprintln!("A");
    use std::ffi::CString;
    let p = CString::new("/dev/ebc").unwrap();
    eprintln!("B: cstring");
    let fd = unsafe { libc::open(p.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
    eprintln!("C: open fd={}", fd);
    if fd < 0 {
        eprintln!("C-err: {}", std::io::Error::last_os_error());
        return;
    }
    #[repr(C)]
    #[derive(Default)]
    struct Info {
        o: i32, e: i32, h: i32, w: i32, pc: i32,
        x1: i32, y1: i32, x2: i32, y2: i32, wm: i32, hm: i32,
    }
    let mut info = Info::default();
    eprintln!("D: sizeof Info = {}", std::mem::size_of::<Info>());
    let r = unsafe { libc::ioctl(fd, 0x7002u32 as libc::c_int, &mut info as *mut Info as *mut libc::c_void) };
    eprintln!("E: GET_BUFFER_INFO rc={} w={} h={}", r, info.w, info.h);
    let mut fmt: i32 = -1;
    let _ = unsafe { libc::ioctl(fd, 0x7010u32 as libc::c_int, &mut fmt as *mut i32 as *mut libc::c_void) };
    eprintln!("F: GET_BUF_FORMAT fmt={}", fmt);
    let mut osd = Info::default();
    let r2 = unsafe { libc::ioctl(fd, 0x7006u32 as libc::c_int, &mut osd as *mut Info as *mut libc::c_void) };
    eprintln!("G: GET_OSD_BUFFER rc={} off=0x{:x}", r2, osd.o);
    unsafe { libc::close(fd) };
    eprintln!("H: DONE");
}
