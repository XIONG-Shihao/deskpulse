# deskpulse 技术栈与架构

[English](ARCHITECTURE.md) | 中文

本文档记录当前实现的技术选型、架构与关键取舍。功能和使用说明见 [README.zh.md](README.zh.md)。

## 1. 技术栈

| 方面 | 选择 | 说明 |
| --- | --- | --- |
| 语言 | Rust，edition 2024 | 只做 Windows（`x86_64-pc-windows-msvc`） |
| GUI | `eframe` / `egui` `0.36`（wgpu、D3D12 WARP 后端） | 立即模式 GUI；CPU 软件渲染、无边框、透明、置顶窗口 |
| 网络 / CPU / 内存 | `sysinfo` `0.39` | 网卡累计字节、CPU 占用、内存 |
| GPU | `nvml-wrapper` `0.13` | NVIDIA NVML：占用 / 显存 / 温度（运行时动态加载 `nvml.dll`） |
| CPU 温度（主） | PawnIO 内核驱动 + 手写 FFI | 无第三方 Rust 封装，直接用 `CreateFile` / `DeviceIoControl` |
| CPU 温度（退路） | `serde_json` + 标准库 `TcpStream` | 轮询 LibreHardwareMonitor 的 `http://127.0.0.1:8085/data.json` |
| 托盘 / 菜单 | `tray-icon` `0.25`（内部用 muda） | 托盘图标与原生菜单 |
| 配置 | `serde` + `toml` | `%APPDATA%\deskpulse\config.toml` |
| 开机自启 | `schtasks.exe` | 计划任务，`RunLevel Highest` |
| 打包 | `winresource`、`+crt-static`、LTO | 图标/版本信息、免 VC++ 运行时、单文件 exe |
| 中文显示 | 运行时加载系统字体 | egui 不自带 CJK 字形 |

设计原则：尽量少依赖；可能拿不到的指标一律用 `Option`，UI 显示 `--`，**绝不用 `0` 冒充未知**。

## 2. 架构

### 2.1 线程模型

```
主线程 (eframe/winit 事件循环)
├── 根视口：悬浮窗（数据展示）
├── Win32 分层窗口：不透明数据文字（鼠标穿透）
├── 延迟视口：右键设置菜单（独立小窗口）
└── App::logic 每帧：回收菜单动作、处理托盘事件、请求重绘

采集线程 (1 个)
└── 周期性采样 → 写入 Arc<Mutex<Snapshot>>

菜单事件线程 (1 个)
└── 阻塞在 MenuEvent::recv()，收到即入队并 ctx.request_repaint()
```

- **采集与 UI 解耦**：系统调用只在采集线程里做；UI 每帧只读取一份快照，避免每帧查询。
- **菜单事件即时唤醒**：托盘菜单事件若只在 `logic` 里轮询，最多要等一个重绘周期（~400ms）；监听线程用 `request_repaint()` 立刻唤醒，退出/切换操作即时生效。
- **UI 在 CPU 上栅格化**：`main.rs` 将 wgpu 限定为 Direct3D 12，并只选择标记为 `DeviceType::Cpu` 的适配器（Windows WARP）。三个窗口共用该渲染器。没有可用的软件适配器时启动失败，不会回退到硬件 GPU。

### 2.2 数据流

```
各 Collector ──sample()──► Snapshot ──(Arc<Mutex>)──► App::ui 每帧 clone 渲染
```

`Snapshot` 汇总 8 个指标，每个都是 `Option`（拿不到即 `None`）。百分比、显存占比等在 UI 侧由原始值计算。

### 2.3 视口模型

- **根视口**：无边框 + 透明 + 置顶 + 不进任务栏的悬浮窗。每帧测量内容尺寸并用 `ViewportCommand::InnerSize` **贴合内容**，所以没有多余透明区域。
- **文字窗口**：可穿透鼠标的 Win32 分层窗口跟随根视口的位置与大小。GDI 通过逐像素 alpha 绘制数据文字；根窗口绘制半透明面板，并用不可见文字保留相同布局。
- **菜单视口**：右键时创建 `show_viewport_deferred` 独立窗口（`decorations=false`、透明、置顶），紧贴菜单内容；主窗口大小不受影响。菜单是一棵二级结构。

### 2.4 模块职责

| 模块 | 职责 |
| --- | --- |
| `main.rs` | 解析 `--dump`；自提权；创建窗口并启动 eframe |
| `app.rs` | eframe `App`（`logic`/`ui`）；布局渲染；窗口自适应；菜单视口与动作回收 |
| `config.rs` | `Config` 读写、缺省值、旧目录迁移 |
| `i18n.rs` | `Language`、文案表、按系统语言选择 |
| `format.rs` | 速率 / 百分比 / 温度格式化（含进位规则） |
| `tray.rs` | 托盘图标与菜单，暴露 `set_autostart_checked` |
| `autostart.rs` | 用 `schtasks` 管理登录计划任务 |
| `elevate.rs` | `TokenElevation` 检测 + `ShellExecuteW("runas")` 自提权 |
| `diag.rs` | 带时间戳的诊断日志 |
| `window.rs` | Win32 面板/菜单透明度、圆角区域和原生窗口样式 |
| `text_window.rs` | 用逐像素 alpha 的 GDI 绘制数据文字的鼠标穿透分层窗口 |
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

## 6. 窗口与布局

- **贴合内容**：每帧取内容矩形，与上次尺寸比较，差异超过阈值才发 `InnerSize`，避免抖动。
- **WARP 下的透明效果**：测试机器的 D3D12 表面只支持不透明 alpha 模式。使用 Win32 分层窗口不透明度使面板半透明；另一个可穿透鼠标的 Win32 窗口用 GDI 将数据文字绘制到逐像素 alpha 位图上，保持文字不透明。圆角窗口区域裁剪面板和菜单的边角。
- **固定单元格**：标签/数值用固定宽度的框（紧凑 46 / 74，宽松 46 / 92），数字位数变化不会改变窗口宽度。
- **对齐**：紧凑模式下第一列标签右对齐、数值左对齐；竖两排的第二列标签左对齐（各自左边缘对齐）。对齐布局用 `set_min_width` 强制框宽，否则会缩到文字宽度导致列漂移。
- **间距预设**：`item_spacing.x = 0`，标签↔数值的 4pt 在单元格内显式添加；竖两排两列之间无间隔。
- **菜单**：延迟视口，深色 `Visuals` + 近黑底近白字（不依赖系统主题），二级结构；动作通过 `Arc<Mutex<MenuState>>` 队列回传，父级每帧回收。

## 7. 配置与迁移

- 路径 `%APPDATA%\deskpulse\config.toml`；`#[serde(default)]` 保证字段缺失时用默认值。
- 首次运行若新路径不存在，会尝试读取旧路径 `%APPDATA%\desk-stats\config.toml` 并迁移（项目曾用名 `desk-stats`）。
- `language` 为 `Option`：未设置时按系统 UI 语言（`GetUserDefaultUILanguage`）选择。

## 8. 打包

- `build.rs` 用 `winresource` 嵌入 `assets/icon.ico` 与版本信息。
- `.cargo/config.toml` 对 msvc 目标开启 `+crt-static`，免 VC++ 运行时。
- release：`lto` + `codegen-units=1` + `strip` + `panic="abort"`。

## 9. 关键取舍记录

1. **CPU 温度从 WMI 改为 PawnIO 直读。** 新版 LibreHardwareMonitor（0.9.x）移除了 WMI provider，原 `root\LibreHardwareMonitor` 方案已失效；进一步地，为了摆脱对常驻 app 的依赖，改为直连 PawnIO 驱动。LHM HTTP 仅作退路。
2. **GPU 只走 NVML。** 未实现 PDH 通用退路，非 NVIDIA 显卡相关项显示 `--`。
3. **不做自研驱动、不内嵌隐藏检测程序。** 前者成本/维护过高，后者会被杀软视为恶意行为。
4. **未知即 `--`。** 不用 `0` 冒充。

## 10. 已知限制 / 未做

- 直读温度需管理员；非管理员时回退 LHM（未安装则 `--`）。
- Intel 温度路径已实现但未在 Intel 机器上验证（开发机为 AMD）。
- 多 GPU 只取 `device_by_index(0)`。
- 未做历史曲线、日志持久化、多语言（仅中/英）。
