# EBC 适配笔记 / EBC Adaptation Notes

> 目标设备:HANVON N10 Pro III · Rockchip RK3576 · Android 14 (SDK 34) · kernel 6.1.75
>
> 本文记录我们在该设备上确认的 EBC(E-Book Controller)e-ink 驱动 ABI、像素格式、
> 坐标几何、刷新控制面与笔输入,作为 `hweink` 库(Rust)的事实底座。
> 措辞为"我们实测/确认"——陈述公开的驱动接口事实。

---

## 0. 一句话结论

本机 e-ink 屏由 Rockchip **EBC** 驱动控制,设备节点 `/dev/ebc`(char,0666,group system,shell 可 open)。
本机跑的是 Rockchip **"y8" fork** 的 ebc-dev 驱动:它响应 `0x7010/0x7013/0x7014`,而 stock `rockchip-linux/kernel develop-6.1` 头文件只定义到 `0x7007`——所以本机 fork 扩展了 ioctl 集。

两条显示通路:
- **OSD overlay**(默认低延迟渲染/笔迹,screencap 截不到)—— 用户态直接写 + 刷,`hweink` 已验证上屏。
- **主 buffer**(Android UI,screencap 可见)—— 归 SurfaceFlinger→DRM→EBC;用户态可用 `SET_FB_BLANK` 独占接管。

刷新频率/波形**全靠系统属性 + ioctl**,无用户态 Hz 旋钮。坐标几何:原生 buffer 2480×1860 横屏 → 物理屏 1860×2480 竖屏,**90° CW**。

> **★ `struct ebc_buf_info` 是 68 字节(y8 fork),不是 44** —— stock 6.1 是 44 字节(11 int),y8 fork 加了 `dropable`+`tid_name[16]`+`dma_buf_fd` = 68 字节。`EBC_GET_BUFFER_INFO`/`EBC_GET_OSD_BUFFER` 的 `copy_to_user` 写满 68 字节,传 44/64 字节 buffer 会 overrun 踩栈。详见 §1.1。

---

## 1. ioctl ABI(裸整数,非 _IO* 宏,y8 fork)

`/dev/ebc` 用**原始整数**ioctl 码 `0x7000–0x7018`(y8 fork 完整集)。本机 probe 全部 `rc=0`:

| 码 | 名 | 通路 | 说明 |
|---|---|---|---|
| 0x7000 | `EBC_GET_BUFFER` | 主 | 返回一个主 display buffer 的 offset(本机 0xa00000=10MB) |
| 0x7001 | `EBC_SEND_BUFFER` | **主** | 提交主 buffer 一帧刷新(设 `epd_mode` + `win_*`) |
| 0x7002 | `EBC_GET_BUFFER_INFO` | 两 | 返回面板几何(w/h/panel_color/mm),68 字节 struct |
| 0x7003 | `EBC_SET_FULL_MODE_NUM` | 主 | 每 N 次局部刷后强制全刷清残影 |
| 0x7004 | `EBC_ENABLE_OVERLAY` | overlay | 开 OSD overlay 层 |
| 0x7005 | `EBC_DISABLE_OVERLAY` | overlay | 关 OSD overlay 层 |
| 0x7006 | `EBC_GET_OSD_BUFFER` | overlay | 返回 OSD buffer offset(本机 0x1400000=20MB) |
| 0x7007 | `EBC_SEND_OSD_BUFFER` | overlay | 提交 OSD 一帧刷新(`epd_mode=EPD_OVERLAY`) |
| 0x7008 | `EBC_NEW_BUF_PREPARE` | 主 | 准备新 buffer |
| 0x7009 | `EBC_SET_DIFF_PERCENT` | 主 | 差异超 N% 强制全刷(默认 50) |
| 0x700a | `EBC_WAIT_NEW_BUF_TIME` | 主 | 新帧等待节流(ms) |
| 0x700b | `EBC_GET_OVERLAY_STATUS` | overlay | 读 overlay 状态 |
| 0x700c | `EBC_ENABLE_BG_CONTROL` | overlay | overlay 模式下控背景刷新(**非冻 HWC**) |
| 0x700d | `EBC_DISABLE_BG_CONTROL` | overlay | 同上关 |
| 0x700e | `EBC_ENABLE_RESUME_COUNT` | — | y8 扩展 |
| 0x700f | `EBC_DISABLE_RESUME_COUNT` | — | y8 扩展 |
| 0x7010 | `EBC_GET_BUF_FORMAT` | 两 | 返回 `EBC_Y4(0)`/`EBC_Y8(1)`;**本机返回 1(Y8)** |
| 0x7011 | `EBC_DROP_PREV_BUFFER` | — | y8 扩展 |
| 0x7012 | `EBC_GET_STATUS` | — | y8 扩展 |
| 0x7013 | `EBC_SET_FB_BLANK` | 主 | **关 HWC 主通路**(独占主 buffer 用) |
| 0x7014 | `EBC_SET_FB_UNBLANK` | 主 | 恢复 HWC 主通路 |
| 0x7015 | `EBC_ENABLE_REPAIR` | — | y8 扩展 |
| 0x7016 | `EBC_ENABLE_HIGH_FPS` | — | y8 扩展 |
| 0x7017 | `EBC_GET_NORMAL_REPAIR` | — | y8 扩展 |
| 0x7018 | `EBC_SET_FULL_REFRESH_WIDTH` | — | y8 扩展 |

> stock `rockchip-linux/kernel develop-6.1` 只定义 0x7000–0x7007;develop-4.19/5.10 到 0x700d;y8 fork 到 0x7018。

### 1.1 `struct ebc_buf_info` — 68 字节(y8 fork,踩坑核心)

本机真实定义(与公开的 y8 fork BSP 头文件及用户态 ABI 一致):
```c
struct ebc_buf_info {
    int  offset;        //  0
    int  epd_mode;      //  4
    int  height;        //  8
    int  width;         // 12
    int  panel_color;   // 16
    int  win_x1;        // 20
    int  win_y1;        // 24
    int  win_x2;        // 28
    int  win_y2;        // 32
    int  width_mm;      // 36
    int  height_mm;     // 40
    int  dropable;      // 44   (4.19 叫 needpic, 同 offset 改名)
    char tid_name[16];  // 48
    int  dma_buf_fd;    // 64   (y8 新增; 4.19/5.10 没有 → 那些是 64 字节)
};                      // = 68, 无 padding (全 4 对齐, 68%4==0)
```

各版本尺寸对照(关键):
| 内核版本 | struct size | 尾部字段 | ioctl 上限 |
|---|---|---|---|
| stock develop-6.1 | **44 字节** | 无(11 int) | 0x7007 |
| develop-4.19 / 5.10 | 64 字节 | +`needpic`+`tid_name[16]` | 0x700d |
| **y8 fork(本机)** | **68 字节** | +`dropable`+`tid_name[16]`+`dma_buf_fd` | 0x7018 |

**为什么必须 68**:`GET_BUFFER_INFO`/`GET_OSD_BUFFER` 的 `copy_to_user` 写满整个 struct。传 44/64 字节 buffer 会 overrun,多写的字节踩栈上保存的返回地址 → 函数 `ret` 跳垃圾地址 → SIGSEGV(且是个海森堡 bug:加 `eprintln!` 改栈布局能掩盖)。`hweink` 用 68 字节 struct + 编译期 guard `const _: () = assert!(size_of::<EbcBufInfo>() == 68);` 防回退。

> ⚠️ **EPD_DU 跨版本歧义**:stock develop-6.1 是 `EPD_DU=13`;y8 fork(本机)和平台 framework 常量都是 `DU=14`、`A2=12`。`hweink::Mode` 按本机取值(DU=14)。传错 `epd_mode` 不会崩但刷新波形会错。

EPD 模式(`epd_mode` 字段 / `sys.eink.mode` 属性值):AUTO=0, OVERLAY=1, FULL_GC16=2,
FULL_GL16=3, FULL_GLR16=4, FULL_GLD16=5, FULL_GCC16=6, PART_GC16=7, PART_GL16=8,
PART_GLR16=9, PART_GLD16=10, PART_GCC16=11, A2=12, A2_DITHER=13, DU=14, DU4=15,
A2_ENTER=16, RESET=17, AUTO_DU=22, AUTO_DU4=23。

---

## 2. 像素格式与几何

### 2.1 两条通路同位深:8bpp Y8

`EBC_GET_BUF_FORMAT`(0x7010)返回 `EBC_Y8(1)`。y8 驱动主/OSD buffer **同位深**。
- stride = width = 2480 bytes/row
- 1px/byte,0x00=黑,0xFF=白
- 主 buffer @ mmap offset 0xa00000(10MB,`GET_BUFFER` 返回);OSD buffer @ 0x1400000(20MB,`GET_OSD_BUFFER` 返回)
- mmap 32MB 覆盖两者

### 2.2 ⚠️ `panel_color=0` ≠ 4bpp

`panel_color=0` 是**面板类型**(灰度 vs 彩色),不是 buffer 位深。若按 4bpp(stride=W/2)写 → 硬件按 8bpp(stride=W)读 → **stride 不匹配折叠**:奇数行落左半、偶数行落右半、图高折半、只占屏幕上半。必须按 8bpp 写。

### 2.3 坐标映射:90° CW

原生 buffer 2480×1860 横屏 → 物理屏 1860×2480 竖屏,**90° 顺时针**:
```
buffer(x,y)  →  screen(1859 - y, x)      // H_buf = 1860
screen(sx,sy) →  buffer(sy, 1859 - sx)
```
边映射:buffer 顶(y=0)→屏右,底(y=1859)→屏左,左(x=0)→屏上,右(x=2479)→屏下。
Android `installOrientation=ROTATION_270` 是系统枚举值,物理等效 90° CW。

### 2.4 面板物理

- `GET_BUFFER_INFO` mm=216×173,2480×1860 → ~292/273 dpi
- `wm size` 1860×2480 @ 40fps,density 440,`FLAG_SECURE` 开
- DT 属性 `panel,vir_width/vir_height` 是虚拟尺寸(含 porch padding),本机返回 2480/1860

---

## 3. 两条显示通路(已验证)

| | OSD overlay | 主 buffer |
|---|---|---|
| mmap offset | 0x1400000 (20MB) | 0xa00000 (10MB) |
| 位深 | 8bpp Y8 | 8bpp Y8 |
| 刷新 ioctl | `SEND_OSD_BUFFER`(0x7007) | `SEND_BUFFER`(0x7001) |
| 前置 | `ENABLE_OVERLAY`(0x7004) | `SET_FB_BLANK`(0x7013) |
| epd_mode | `EPD_OVERLAY`(1) | FULL_GC16(2) 等 |
| screencap 可见 | **否** | 是 |
| 与 Android 共存 | 独立叠层 | 冲突(同 buffer,需 blank HWC) |
| 已验证上屏 | ✅ `hweink` osd 命令 | ✅ C 验证(Rust main 待跑) |

### 3.1 OSD overlay 流程

```
open("/dev/ebc", O_RDWR)
ioctl(0x7002, &info)              // 几何
ioctl(0x7010, &fmt)               // 确认 Y8
mmap(0, 25MB, PROT_RW, MAP_SHARED, fd, 0)
ioctl(0x7006, &osd)               // OSD offset = 0x1400000
ioctl(0x7004, NULL)               // ENABLE_OVERLAY
// 写 8bpp Y8 到 map + osd_offset(注意 90° CW 旋转)
ioctl(0x7007, &buf_info)          // epd_mode=EPD_OVERLAY(1), win_*=脏矩形
ioctl(0x7005, NULL)               // DISABLE_OVERLAY(收尾)
```

### 3.2 主 buffer 独占流程

```
ioctl(0x7010, &fmt)               // 确认 Y8
ioctl(0x7000, &gb)                // GET_BUFFER → main offset
ioctl(0x7013, NULL)               // SET_FB_BLANK 关 HWC  ← 关键
// 写 8bpp Y8 到 map + main_off(90° CW 旋转)
ioctl(0x7001, &s)                 // SEND_BUFFER, epd_mode=FULL_GC16(2)
// ...hold...
ioctl(0x7014, NULL)               // SET_FB_UNBLANK 恢复
```

### 3.3 ⚠️ `BG_CONTROL` 不是冻 HWC

`EBC_ENABLE_BG_CONTROL`(0x700c)/`DISABLE_BG_CONTROL`(0x700d)只设 `overlay_bg_control` 标志,**仅在 overlay 模式下控制要不要顺便刷背景**,不是"冻 HWC"开关。真正独占主 buffer 用 `SET_FB_BLANK`(0x7013)。

---

## 4. 刷新频率 / 波形控制

**无用户态 Hz ioctl/sysfs。** 频率 = waveform 帧数(GC16=16帧、DU=2-4帧、A2=2帧)× `panel,sdck` 时钟。
控制面分两层:

### 4.1 ioctl 直接控制

- `EBC_SET_FULL_MODE_NUM`(0x7003):每 N 次局部刷后强制全刷
- `EBC_SET_DIFF_PERCENT`(0x7009):差异超 N% 强制全刷(默认 50)
- `EBC_WAIT_NEW_BUF_TIME`(0x700a):新帧等待节流(ms)
- 每次 `SEND_BUFFER`/`SEND_OSD_BUFFER` 的 `epd_mode` 字段:per-frame waveform LUT

### 4.2 系统属性(平台 `EinkManager` → `SystemProperties.set`,由 HWC HAL / 内核消费)

| 属性 | 默认 | 含义 |
|---|---|---|
| `sys.eink.mode` | `9` | **波形选择**(见 §1 EPD 模式表) |
| `persist.vendor.ebook.fullmode_cnt` | `0` | 全刷节奏(每 N 次局部刷强制全刷,经 0x7003 进内核) |
| `sys.ebook.one_full_mode_timeline` | — | 递增计数,写一次触发一次全刷(`sendOneFullFrame`,800ms 节流) |
| `persist.vendor.ebook.hwc_vrefresh` | `40` | 静态 HWC 刷新率 40fps(boot 时读) |
| `persist.sys.clr_invert` | `0` | 夜间反色 |
| `debug.sys.clr_adj_contrast` | `-101` | 对比度(-101=关) |
| `debug.sys.clr_adj_dark` | `0` | 加深 |
| `persist.ebook.gray256_enable` | `0` | 256 灰阶抖动 |
| `persist.ebook.lumgain/contgain/satugain/colordep` | `64` | 色彩增益 [0,128](本机无前光,lumgain 无硬件效果) |
| `persist.ebook.regaltype` | `4` | Regal 全刷类型(黑白 4 / 彩色 5) |
| `persist.ebook.diff_percent` | — | Regal 差分(→ 0x7009) |

### 4.3 谁读 `sys.eink.mode`

内核 `buf_mode` 由 `0x7001/0x7007` 的 `epd_mode` 字段 per-buffer 设。`sys.eink.mode` 由 **HWC HAL** 监听并应用到主通路 auto-refresh。`hweink::sysprop::set_system_mode` 写这个属性,影响的是 HWC 主通路波形;OSD overlay 的波形由 `SEND_OSD_BUFFER` 的 `epd_mode` 决定(固定 `EPD_OVERLAY`)。

### 4.4 全屏刷新:广播是正路,counter 是退化

平台"全刷"的正规触发方式是发广播 `hanvon.intent.fullrefrsh.user`——SystemUI 下拉"刷新"磁贴(`RefreshTile`)、物理自定义键(`PhoneWindowManager`)、各 app 自刷新都发这个。它由系统多个组件响应,效果等同按一下官方刷新键。任何 app 或 `adb shell` 都能发(`am broadcast -a hanvon.intent.fullrefrsh.user`,无权限);`.system` 变体带权限,仅系统可发。

`hweink` 把 action 字符串暴露为 `sysprop::ACTION_FULL_REFRESH_USER` 常量,**不内置发送函数**——因为发广播是 Java 操作(`Context.sendBroadcast`),Java 调用方直接写 `context.sendBroadcast(new Intent(ACTION_FULL_REFRESH_USER))` 即可,不需要 Rust 库代劳。`hweink-demo` 是裸 native 进程无 Java Context,演示时 fork `am`(一次性 CLI 可接受;**app 内嵌千万别这么干**,fork+ART 启动几十 ms 且可能被 SELinux 拦)。

`sys.ebook.one_full_mode_timeline` 是另一条路(平台 `EinkManager.sendOneFullFrame()` 写的 counter),但它只是"标记下次刷新用全波形",得配合一次实际刷新才生效,且有 800ms 节流——不如广播直接。`hweink` 不再封装它。

---

## 5. 笔 / 触控输入

### 5.1 `/dev/input/event2` = "pen_touch"(电磁笔)✅ `hweink` 已验证

```
name: pen_touch
INPUT_PROP_DIRECT
ABS_X    : 0..2480   (EBC 原生横屏 X)
ABS_Y    : 0..1860   (EBC 原生横屏 Y)
ABS_PRESSURE: 0..1024
KEY: BTN_DIGI, BTN_TOOL_RUBBER, BTN_TOOL_FINGER,
     BTN_STYLUS, BTN_STYLUS2, BTN_STYLUS3, BTN_TOUCH
```

`hweink::pen::Pen` 已实测可用:open `/dev/input/event2`,`poll`+`read` evdev 事件,按 `EV_SYN` 帧聚合,输出屏幕坐标(已做 90° CW 旋转)+ 压力 + 工具类型 + 侧键。evdev `input_event` 在 aarch64 是 24 字节(`sec:i64, usec:i64, type:u16, code:u16, value:i32`)。

- `BTN_TOOL_PEN`/`BTN_TOOL_RUBBER`:笔/橡皮靠近
- `BTN_TOUCH`:接触表面
- `BTN_STYLUS`/`BTN_STYLUS2`/`BTN_STYLUS3`:侧键

### 5.1.1 压感级别:宣传 8192 级 vs 实测 1024 级

设备宣传 8192 级压感,但 evdev 实测 `ABS_PRESSURE` 范围 `0..1024`(10-bit)。用 `EVIOCGABS` ioctl 直接问内核,`getevent -p` 印证一致。这是内核 `input_set_abs_params` 设的上限,用户态改不了。平台自带的笔引擎也按 1024 处理。三种可能(硬件 ADC 就是 10-bit / 硬件更高被驱动截 / 8192 是标称等效值)无法从用户态完全区分,但**对外可验证的压感就是 1024 级**。`hweink::PenEvent::pressure` 给原始 0..1023,`pressure_norm()` 规整到 0..1000。想要更细只能软件插值(EMA/中值滤波 + 速度耦合),变不出更多物理档位。

### 5.2 其他输入设备

- `event6` = "gpio-keys":N10 物理按键(0x254-0x257, 0x2e8, 0x2f3, 0x2f4)
- `event1` = "rk805 pwrkey":电源键
- `event0` = "hall wake key":霍尔唤醒
- `event3` = "sc7a20"、`event4` = "gsensor":加速度计

### 5.3 系统笔服务

平台有个 `hvpen` Binder 服务(`IHvPenDrawService`),native 笔引擎直接 open `/dev/ebc` + `/dev/input`,native 线程读笔 → 画笔迹 → `SEND_OSD_BUFFER` 刷。它只画笔迹、不给自定义渲染,且绑死本平台。`hweink` 不依赖它,自己读 event2 + 自己画,可移植。

---

## 6. `hweink` 库设计(Rust)

库封装 `/dev/ebc` ioctl + 8bpp Y8 打包 + 90° CW 旋转,对外暴露竖屏坐标 API。

### 6.1 架构

```
hweink (Rust crate, no_std-able core + std io)
├── ioctl.rs     — 裸整数 ioctl 封装 (0x7000-0x7018), EbcBufInfo (68B)
├── ebc.rs       — /dev/ebc open/mmap, open_info (只读不 mmap)
├── path.rs      — Osd path / Main path 两条渲染通路
├── draw.rs      — 批量写 Draw scope (shadow buffer + 脏矩形合并)
├── geom.rs      — 90° CW 坐标变换 (screen↔buffer)
├── mode.rs      — EPD waveform 模式枚举
├── refresh.rs   — full_mode_num / diff_percent / wait_new_buf_time 调谐
├── sysprop.rs   — sys.eink.mode / one_full_mode_timeline 系统级
├── pen.rs       — /dev/input/event2 笔事件解析 (同步 + 异步迭代器 + raw fd)
└── lib.rs       — 统一 Eink 表面
```

### 6.2 核心 API

- `Ebc::open()` / `Ebc::open_info()`(只读不 mmap,probe 安全)/ `Surface::new(&ebc, Path::Osd|Main)`
- `surf.put_pixel(x, y, gray)` / `fill_rect` / `clear` — 竖屏坐标,内部 90° CW
- `surf.draw()` → `Draw` guard(shadow buffer + 批量写 + 脏矩形合并,drop 时一次 flush)
- `surf.refresh(ScreenRect, Option<Mode>)` / `refresh_full(Mode)` — 推帧,支持局部脏矩形 + 任选 waveform
- `ebc.set_full_mode_num(n)` / `set_diff_percent(pct)` / `set_wait_new_buf_time(ms)`
- `sysprop::set_system_mode(Mode)` / `get_system_mode()` / `ACTION_FULL_REFRESH_USER`(全刷广播 action 常量,Java 侧 `sendBroadcast` 用)
- `Pen::open()` → `pen.read(timeout)` / `pen.events()` 迭代器 / `pen.fd()` + `set_nonblocking()` + `poll_once()`(异步层3)

### 6.3 设备验证状态(`hweink-demo`)

| 子命令 | 通路 | 设备验证 |
|---|---|---|
| `probe` | 只读 ioctl(open_info) | ✅ 5/5 稳定,输出全对(2480×1860, Y8, osd=0x1400000) |
| `osd` | OSD overlay 直绘 | ✅ compass 上屏,用户确认看到 |
| `mode`/`mode?` | sysprop `sys.eink.mode` | ✅ 符号名/数字两种入参,设/读一致 |
| `pen` | `/dev/input/event2` | ✅ 用户实测可用 |
| `full` | `am broadcast hanvon.intent.fullrefrsh.user` | ✅ 广播触发全刷(官方刷新键同款) |
| `main` | 主 buffer + SET_FB_BLANK | ⏳ C 验证上屏;Rust 实现同逻辑,待跑(需愿重启时验) |
| `draw_batch` | 批量写 Draw scope | ✅ 贝塞尔 + 点阵一次刷出,已验证 |
| `pen_iter`/`pen_async` | 笔异步(迭代器 / raw fd) | ✅ 已验证,坐标 + 压感流正常 |

### 6.4 设计决策

1. **默认 OSD overlay**:低延迟、与 Android 共存、8bpp、笔迹友好。库主通路。
2. **主 buffer 作为"全屏接管"能力**:`SET_FB_BLANK` 关 HWC + 8bpp + `SEND_BUFFER`。用完 `SET_FB_UNBLANK` 恢复。
3. **统一 90° CW 旋转**:所有 API 用竖屏 1860×2480 坐标,库内部转原生 2480×1860。
4. **笔输入**:直接读 event2,可移植 Rust,不依赖平台 `hvpen` 服务。
5. **刷新模式双控**:per-frame `epd_mode`(OSD 固定 OVERLAY,主可选)+ 系统级 `sys.eink.mode` 属性。
6. **不碰 BG_CONTROL**:它不是冻 HWC,库用 `SET_FB_BLANK` 代替。
7. **`EbcBufInfo` 编译期 68 字节 guard**:防 struct 尺寸回退重蹈栈踩覆辙。

### 6.5 构建

- Rust workspace,依赖 `libc`(用 `libc::ioctl`/`open`/`mmap`/`poll`;`__system_property_set/get` 在 `sysprop.rs` 内联 `extern "C"` 声明,因 libc crate 不暴露)
- target: `aarch64-linux-android`(`rustup target add aarch64-linux-android`)
- linker: NDK r25+ `aarch64-linux-android31-clang`,在 `.cargo/config.toml` 或环境变量 `CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER` 配
- demo: `hweink-demo/`,`cargo build --release --target aarch64-linux-android`,push 到 `/data/local/tmp/`

---

## 7. 外部参考(公开资料)

| 来源 | 用途 |
|---|---|
| `github.com/rockchip-linux/kernel`(develop-6.1 / 4.19 / 5.10) | stock ebc-dev 头文件,struct/ioctl 各版本对照 |
| Rockchip 6.1 BSP "y8" fork(公开 mirror) | 本机同款 struct(68B)、ioctl 0x7000–0x7018、EBC_Y4/Y8 |
| Khadas `android_hardware_rockchip_libebook` / `NoteDemo` | 用户态 EBC 用法参考(主通路 SET_FB_BLANK、dma_buf_fd) |
| `github.com/canselcik/libremarkable` | libremarkable,rM 的 mxcfb ioctl 对照、API 设计参考 |
| `pine64.org/documentation/General/RK3566_EBC_reverse-engineering/` | Pine64 EBC 资料枢纽,waveform 格式 |
