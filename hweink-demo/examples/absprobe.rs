//! 读 evdev ABS_PRESSURE 真实范围 (EVIOCGABS),绕过 SELinux 挡的 sysfs。
//! 顺便读 EVIOCGBIT 确认压感轴存在, EVIOCGNAME/ID 确认设备身份。
use std::ffi::CString;
use std::os::unix::io::AsRawFd;

const EVIOCGABS: u64 = 0x80144540; // _IOR('E', 0x40 + abs, struct input_absinfo) on aarch64
const EVIOCGBIT: u64 = 0x80144520; // _IOC_READ, size=20, 'E', 0x20+ev_type
const EVIOCGNAME: u64 = 0x81004506; // _IOR('E', 0x06, char[128]) size 128
const EVIOCGID: u64 = 0x80144502;   // _IOR('E', 0x02, struct input_id) size 16
const EV_ABS: u16 = 0x03;
const ABS_PRESSURE: u16 = 0x18;

#[repr(C)]
#[derive(Default)]
struct InputId { bustype: u16, vendor: u16, product: u16, version: u16 }

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct AbsInfo { value: i32, minimum: i32, maximum: i32, fuzz: i32, flat: i32, resolution: i32 }

extern "C" {
    fn ioctl(fd: i32, request: u64, ...) -> i32;
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).map(|s| s.as_str()).unwrap_or("/dev/input/event2");
    let p = CString::new(path).unwrap();
    let fd = unsafe { libc::open(p.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if fd < 0 { eprintln!("open {} failed: {}", path, std::io::Error::last_os_error()); return; }

    // name
    let mut name = [0u8; 128];
    let _ = unsafe { ioctl(fd, EVIOCGNAME, name.as_mut_ptr()) };
    let namelen = name.iter().position(|&b| b == 0).unwrap_or(name.len());
    eprintln!("name: {}", String::from_utf8_lossy(&name[..namelen]));

    // id
    let mut id = InputId::default();
    let _ = unsafe { ioctl(fd, EVIOCGID, &mut id as *mut _ as *mut libc::c_void) };
    eprintln!("id: bustype=0x{:04x} vendor=0x{:04x} product=0x{:04x} version=0x{:04x}",
        id.bustype, id.vendor, id.product, id.version);

    // EV_ABS 位图 (确认 ABS_PRESSURE bit 存在)
    let mut absbits = [0u8; 32];
    let _ = unsafe { ioctl(fd, EVIOCGBIT + (EV_ABS as u64), absbits.as_mut_ptr() as *mut libc::c_void) };
    let has_pressure = (absbits[(ABS_PRESSURE/8) as usize] >> (ABS_PRESSURE%8)) & 1 != 0;
    eprintln!("ABS_PRESSURE present in EV_ABS bits: {}", has_pressure);

    // ABS_PRESSURE 范围
    let mut ai = AbsInfo::default();
    let r = unsafe { ioctl(fd, EVIOCGABS + (ABS_PRESSURE as u64), &mut ai as *mut _ as *mut libc::c_void) };
    eprintln!("EVIOCGABS(ABS_PRESSURE) rc={}", r);
    eprintln!("  value={}  minimum={}  maximum={}  fuzz={}  flat={}  resolution={}",
        ai.value, ai.minimum, ai.maximum, ai.fuzz, ai.flat, ai.resolution);
    if ai.maximum > 0 {
        let bits = (ai.maximum as f64).log2().ceil() as i32 + 1;
        eprintln!("  => max {} = ~{}-bit ({} 级)", ai.maximum, bits, ai.maximum + 1);
        if ai.maximum == 1023 || ai.maximum == 1024 {
            eprintln!("  => 10-bit 压感 (1024 级), NOT 8192");
        } else if ai.maximum == 8191 || ai.maximum == 8192 {
            eprintln!("  => 13-bit 压感 (8192 级) — 客服说的对!");
        }
    }

    // 顺便读其他可能的压感轴: ABS_MT_PRESSURE (0x3a)
    let abs_mt_pressure: u16 = 0x3a;
    let has_mt = (absbits[(abs_mt_pressure/8) as usize] >> (abs_mt_pressure%8)) & 1 != 0;
    eprintln!("ABS_MT_PRESSURE present: {}", has_mt);
    if has_mt {
        let mut ai2 = AbsInfo::default();
        let _ = unsafe { ioctl(fd, EVIOCGABS + (abs_mt_pressure as u64), &mut ai2 as *mut _ as *mut libc::c_void) };
        eprintln!("  ABS_MT_PRESSURE min={} max={}", ai2.minimum, ai2.maximum);
    }

    // X/Y 范围也读出来对照
    for (ax, nm) in [(0u16, "ABS_X"), (1u16, "ABS_Y"), (0x18, "ABS_PRESSURE"), (0x35, "ABS_TILT_X"), (0x36, "ABS_TILT_Y")] {
        let present = (absbits[(ax/8) as usize] >> (ax%8)) & 1 != 0;
        if present {
            let mut a = AbsInfo::default();
            let _ = unsafe { ioctl(fd, EVIOCGABS + (ax as u64), &mut a as *mut _ as *mut libc::c_void) };
            eprintln!("{} : min={} max={} res={}", nm, a.minimum, a.maximum, a.resolution);
        }
    }

    unsafe { libc::close(fd) };
}
