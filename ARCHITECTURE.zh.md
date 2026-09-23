# deskpulse 技术栈与架构

[English](ARCHITECTURE.md) | 中文

本文档记录当前实现的技术选型、架构与关键取舍。功能和使用说明见 [README.zh.md](README.zh.md)。

## 1. 技术栈

| 方面 | 选择 | 说明 |
| --- | --- | --- |
| 语言 | Rust，edition 2024 | 只做 Windows（`x86_64-pc-windows-msvc`） |
| 界面 | 原生 Win32 分层窗口 + GDI，手写 FFI | 逐像素 alpha 的 `UpdateLayeredWindow`；不用任何 GPU API，也不用 GUI 框架 |
| 网络 / CPU / 内存 | `sysinfo` `0.39` | 网卡累计字节、CPU 占用、内存 |
| GPU | `nvml-wrapper` `0.13` | NVIDIA NVML：占用 / 显存 / 温度（运行时动态加载 `nvml.dll`） |
| CPU 温度（主） | PawnIO 内核驱动 + 手写 FFI | 无第三方 Rust 封装，直接用 `CreateFile` / `DeviceIoControl` |
| CPU 温度（退路） | `serde_json` + 标准库 `TcpStream` | 轮询 LibreHardwareMonitor 的 `http://127.0.0.1:8085/data.json` |
| 托盘 / 菜单 | `tray-icon` `0.25`（内部用 muda） | 托盘图标与原生右键菜单 |
| 配置 | `serde` + `toml` | `%APPDATA%\deskpulse\config.toml` |
| 开机自启 | `schtasks.exe` | 计划任务，`RunLevel Highest` |
| 打包 | `winresource`、`+crt-static`、LTO | 图标/版本信息、免 VC++ 运行时、单文件 exe |
| 中文显示 | `CreateFontW` 加载 `Microsoft YaHei` | GDI 直接渲染 CJK，无需内置字形 |

设计原则：尽量少依赖；可能拿不到的指标一律用 `Option`，UI 显示 `--`，**绝不用 `0` 冒充未知**。

## 2. 架构

### 2.1 线程模型

```
主线程 —— Win32 消息循环（GetMessageW / DispatchMessageW）
├── 拥有分层窗口：负责布局、把内容画进位图、处理输入与菜单
└── 收到 WM_APP_DATA 时重绘面板；收到 WM_APP_MENU 时执行菜单动作

采集线程（1 个）
└── 每 refresh_secs 采样一次 → 写入 Arc<Mutex<Snapshot>> → PostMessageW(WM_APP_DATA)

muda / 托盘事件线程（由 tray-icon 持有）
└── MenuEvent 回调 → 把 id 入队 → PostMessageW(WM_APP_MENU)
```

- **采集与 UI 解耦**：系统调用只在采集线程里做；UI 被唤醒时只 clone 一份快照。
- **事件驱动，没有渲染循环**：采集线程每采到新数据就发 `WM_APP_DATA`，面板因此每个周期只重绘一次，其余时间什么都不做。
- **菜单事件只入队、不就地处理**：muda 在自己的线程回调，所以回调里只把 id 推入 `Arc<Mutex<Vec<String>>>` 并发 `WM_APP_MENU`；所有 UI 状态都由主线程独占。

### 2.2 数据流

```
各 Collector ──sample()──► Snapshot ──(Arc<Mutex>)──► refresh() 每周期 clone 一次
```

`Snapshot` 汇总 8 个指标，每个都是 `Option`（拿不到即 `None`）。百分比、显存占比等在 UI 侧由原始值计算。

### 2.3 窗口模型

- **一切都用一个窗口**：无边框 `WS_POPUP`，扩展样式 `WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE` —— 置顶、点击不抢焦点、不进任务栏和 Alt-Tab。
- **GDI 逐像素 alpha**：面板背景（用代码绘制的圆角矩形，填成配置的透明度）和文字（`DrawTextW` + `Microsoft YaHei`）都合成到一张 32 位从上到下的 DIB 上。任何非黑像素一律强制 alpha 255，保证文字在半透明面板上依然锐利。最后用一次 `UpdateLayeredWindow(..., ULW_ALPHA)` 把 DIB 交给合成器。
- **拖动**用手动 `SetCapture` + `SetCursorPos`；**菜单**用 muda 的原生菜单，通过 `show_context_menu_for_hwnd` 弹出（右键菜单与托盘菜单由同一份构建代码生成，不会各自漂移）；**重绘**由 `WM_APP_DATA` 触发，而不是定时器。

### 2.4 模块职责

| 模块 | 职责 |
| --- | --- |
| `main.rs` | 解析 `--dump`；自提权；声明 DPI 感知；持有 `Overlay` 实例并跑消息循环 |
| `overlay.rs` | 整个界面：创建窗口、处理 DPI、布局、GDI 绘制、原生菜单、拖动与消息处理 |
| `config.rs` | `Config`/`Layout`/`Spacing`/`Align` 读写、缺省值、旧目录迁移 |
| `i18n.rs` | `Language`、文案表、按系统语言选择 |
| `format.rs` | 速率 / 百分比 / 温度格式化（含进位规则） |
| `tray.rs` | 持有托盘图标，并换上由 `overlay` 构建的菜单 |
| `autostart.rs` | 用 `schtasks` 管理登录计划任务 |
| `elevate.rs` | `TokenElevation` 检测 + `ShellExecuteW("runas")` 自提权 |
| `diag.rs` | 带时间戳的诊断日志 |
| `metrics/mod.rs` | `Snapshot`、采集线程、百分比计算 |
| `metrics/*.rs` | 各指标采集器（net / cpu / mem / gpu / pawnio / temp） |

## 3. 指标数据源

| 指标 | 采集方式 |
| --- | --- |
| 网速 | `sysinfo::Networks` 累计字节做时间差分；过滤 loopback / 虚拟 / VPN 网卡 |
| CPU 占用 | `sysinfo` `global_cpu_usage()`（复用同一个 `System` 实例） |
| 内存 | `sysinfo` `used_memory / total_memory` |
| GPU | NVML：`utilization_rates` / `memory_info` / `temperature` |
| CPU 温度 | PawnIO 直读优先，LHM HTTP 退路（见下节） |

## 4. CPU 温度：为什么需要内核驱动

Windows 没有可靠的公开 CPU 温度 API。核心温度只能读 **MSR**（Intel）/ **SMN**（AMD）寄存器，而 `RDMSR` 是 **ring-0 特权指令**，用户态执行会触发 #GP。因此必须有内核驱动代读。可选路径：

| 方案 | 代价 |
| --- | --- |
| **PawnIO 驱动**（当前主路径） | 需要驱动已安装 + 管理员权限；无需任何常驻 app |
| LibreHardwareMonitor HTTP（退路） | 需要 LHM 常驻 |
| 自研签名驱动 | EV 证书 + 微软认证，成本高、易被拦截 —— 不采用 |
| ACPI 热区 WMI | 常拿不到核心温度，不可靠 —— 不采用 |

### 4.1 PawnIO 协议

```
打开 \\.\GLOBALROOT\Device\PawnIO
  → DeviceIoControl(IOCTL_PIO_LOAD_BINARY, 模块字节)     // 载入 .bin 模块
  → DeviceIoControl(IOCTL_PIO_EXECUTE_FN, [32字节函数名][i64 参数…])  // 执行并取回 i64 结果
```

`DEVICE_TYPE = 41394 << 16`；`LOAD_BINARY = DEVICE_TYPE | (0x821 << 2)`；`EXECUTE_FN = DEVICE_TYPE | (0x841 << 2)`。

### 4.2 寄存器与换算

- **AMD**：`ioctl_read_smn(0x00059800)` → `temp = ((raw >> 21) & 0x7FF) * 0.125`；若 `raw & 0x80000 != 0` 或 `raw & 0x30000 == 0x30000` 再减 49°C。
- **Intel**：`ioctl_read_msr(0x1A2)` 取 TjMax（bit 16..24）；`0x1B1`（Package）或 `0x19C`（Core）的 DTS 读数 `delta`（bit 16..22）；`temp = TjMax - delta`。

模块来自 [namazso/PawnIO.Modules](https://github.com/namazso/PawnIO.Modules)（LGPL-2.1），随应用嵌入。

## 5. 提权与自启

- 直读需要管理员权限（PawnIO 设备限制），这是硬约束。
- **自提权**：启动时用 `OpenProcessToken` + `TokenElevation` 判断；未提权则 `ShellExecuteW("runas")` 重启自己（弹一次 UAC）。计划任务启动时已是管理员，不会再弹。
- **自启**：用 `schtasks` 建登录计划任务 `deskpulse`（`/SC ONLOGON /RL HIGHEST`），免去每次登录的 UAC。app 内「开机自启」开关即管理该任务。

## 6. 窗口、DPI 与布局

- **先声明 DPI 感知**：在任何窗口创建之前，`main.rs` 调用 `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)`（失败回退 `SetProcessDPIAware`）。不做这一步 Windows 会把窗口位图整体拉伸，文字就会糊。之后 `GetDpiForWindow` 才能拿到真实 DPI（150% 时为 `144`），并由 `WM_DPICHANGED` 在窗口跨到不同显示器时重新缩放、移动到系统建议的矩形并重建字体与位图。
- **宽度由指标集合决定**：所有几何都由 `scale = dpi / 96` 推导。名称列取可见标签中最宽的，数值列取每个可见指标**可能出现的最宽值**，都用临时 DC 的 `GetTextExtentPoint32W` 量出来；面板宽度为 `边距 + 名称列 + 间距 + 数值列 + 边距`。因为预留的是格式上限（`999.9 MB/s`、`100%`、`100°C`）而不是实时数据，宽度**恒定**：只有显示/隐藏指标或切换语言时才会变，数值变化不会改变它。
- **按「点」定义的间距**：名称↔数值间距定义在印刷点（`PT = 96/72` 逻辑单位），并**向上取整**到整像素，所以在任何分辨率与缩放下都是物理 2pt。
- **全整数几何**：所有矩形和面板尺寸都用整数像素，保证文字矩形与面板尺寸一致（不会出现 1px 裁切），行也落在像素网格上，GDI 文字更锐利。
- **对齐与间距**：`align` 映射为 `DrawTextW` 的对齐标志（`left`/`center`/`right`），作用于单元格内的名称和数值；`spacing` 控制行与行之间的垂直间距（紧凑 1 / 宽松 2 逻辑单位）。
- **透明度**：面板填充时以 `opacity * 255` 作为 alpha，数据文字强制不透明，因此即使桌面是浅色，数字也保持高对比。

## 7. 配置与迁移

- 路径 `%APPDATA%\deskpulse\config.toml`；`#[serde(default)]` 保证字段缺失时用默认值，所以新增选项（如 `align`）不会破坏旧配置文件。
- 首次运行若新路径不存在，会尝试读取旧路径 `%APPDATA%\desk-stats\config.toml` 并迁移（项目曾用名 `desk-stats`）。
- `language` 为 `Option`：未设置时按系统 UI 语言（`GetUserDefaultUILanguage`）选择。

## 8. 打包

- `build.rs` 用 `winresource` 嵌入 `assets/icon.ico` 与版本信息。
- `.cargo/config.toml` 对 msvc 目标开启 `+crt-static`，免 VC++ 运行时。
- release：`lto` + `codegen-units=1` + `strip` + `panic="abort"`。

## 9. 关键取舍记录

1. **界面从 `egui` / `wgpu` 重写为原生 Win32/GDI 窗口。** 原框架会拖入 GPU API 并且每帧都渲染；同一悬浮窗在开发机上用 OpenGL 后端工作集约 125 MB、WARP 后端约 420 MB，CPU 高 3–40 倍。改成自己画一张小 DIB、再用 `UpdateLayeredWindow` 交给系统后，约 27 MB 私有内存 / 43 MB 工作集、约 0.29% 单核，而且在完全没有 GPU API 的环境（基本显示适配器、虚拟机、远程桌面）也能跑。
2. **CPU 温度从 WMI 改为 PawnIO 直读。** 新版 LibreHardwareMonitor（0.9.x）移除了 WMI provider，原 `root\LibreHardwareMonitor` 方案已失效；进一步地，为了摆脱对常驻 app 的依赖，改为直连 PawnIO 驱动。LHM HTTP 仅作退路。
3. **GPU 只走 NVML。** 未实现 PDH 通用退路，非 NVIDIA 显卡相关项显示 `--`。
4. **不做自研驱动、不内嵌隐藏检测程序。** 前者成本/维护过高，后者会被杀软视为恶意行为。
5. **未知即 `--`。** 不用 `0` 冒充。

## 10. 已知限制 / 未做

- 直读温度需管理员；非管理员时回退 LHM（未安装则 `--`）。
- Intel 温度路径已实现但未在 Intel 机器上验证（开发机为 AMD）。
- 多 GPU 只取 `device_by_index(0)`。
- 未做历史曲线、日志持久化、多语言（仅中/英）。
