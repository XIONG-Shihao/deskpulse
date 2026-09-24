# deskpulse

[English](README.md) | 中文

一个由纯 Rust 实现的常驻桌面的 Windows 悬浮窗，实时显示系统状态：

- 上传 / 下载网速
- CPU 占用率 + CPU 温度
- 内存占用率
- 显卡占用率、显存占用、GPU 温度（NVIDIA）

附加功能：三种排列（竖排 / 横排 / 竖两排）、两种间距、左/居中/右对齐、可选显示项、中英文、系统托盘、开机自启、配置持久化。

技术栈：纯 Rust，用原生 Win32 分层窗口 + GDI 绘制。**不依赖任何 GPU API**（OpenGL / Direct3D / Vulkan 都不用），也不依赖 C++ 运行时，单文件 exe。

> 技术栈、架构与设计取舍见 [ARCHITECTURE.zh.md](ARCHITECTURE.zh.md)。

## 构建与运行

```powershell
cargo run --release
```

运行后在桌面出现悬浮窗，鼠标拖动可移动。**右键**打开设置菜单：显示/隐藏、显示数据、布局、间距、对齐、透明度、语言、开机自启、退出（其中「显示数据」和五个设置项为子菜单）。托盘图标打开的是同一个菜单，由同一份代码构建。

## 性能

在开发机上实测（除悬浮窗外桌面处于空闲）：

| 测试机 | |
| --- | --- |
| CPU | AMD Ryzen 5 9600X（6 核 / 12 线程） |
| 内存 | 31 GB |
| 系统 | Windows 11，显示缩放 **150%** |
| 悬浮窗 | 显示 6 项指标，1 秒刷新 |

| 悬浮窗开销 | 实测值 |
| --- | --- |
| CPU | **约 0.29% 单核**（约占整机 CPU 的 0.02%） |
| 私有内存 | **约 27 MB** |
| 工作集 | 约 43 MB |
| 线程 | 4–7 |
| 句柄 | 约 290 |
| `deskpulse.exe` | **0.97 MB** |

换算过来大约**每秒 3 毫秒 CPU 时间**——在任务管理器里基本看不到，远低于一个常见动画界面渲染一帧的开销。

之所以这么低：

- **数据不变就不重绘**：采集线程每采到一份新快照才发消息唤醒窗口（默认每秒一次），窗口只在此时重绘。没有持续渲染循环，也没有动画。
- **整个面板就是一张小位图**：背景和文字都合成到内存里约 160×180 像素的 32 位 DIB 上，再用一次 `UpdateLayeredWindow` 交给系统。
- **完全不用 GPU API**：界面不碰 OpenGL / Direct3D / Vulkan，因此只有 Microsoft 基本显示适配器的机器、虚拟机和远程桌面也能正常跑。

对比：同一悬浮窗此前的 `egui` / `wgpu` 构建工作集约 **125 MB**（OpenGL 后端）和 **420 MB**（WARP 后端），CPU 也高一个数量级。

## 界面行为

- **宽度固定，按最坏情况预留**：名称列按标签宽度、数值列按每个可见指标**可能出现的最宽值**（`999.9 MB/s`、`100%`、`100°C`）用 `GetTextExtentPoint32W` 实测确定。宽度只由「显示哪些指标」决定，因此数据变化时面板既不增长也不抖动，也不会宽于它能显示的最坏情况。
- **DPI 感知**：进程声明 per-monitor-v2 DPI 感知，面板和字体按显示器原生像素网格排布；窗口拖到不同缩放的显示器时会自动重新适配。
- **按「点」定义的间距**：名称↔数值间距是物理 2pt（向上取整到整像素），因此任何分辨率和缩放下物理尺寸恒定。
- **文字对齐**：左 / 居中 / 右，作用于每个数据项单元格内的名称和数值。
- **三种排列**：竖排（每行一项）、横排（一行所有项）、竖两排（每行两项，默认依次为 上传/下载、CPU/CPU温、内存/GPU、显存/GPU温）。
- **两种间距**：紧凑 / 宽松 —— 控制行与行之间的垂直间距。
- **深色半透明**：近黑面板，透明度可配置（默认 `0.72`）；数据文字始终保持不透明。
- **报警颜色**：数值越过阈值就变红——温度 > 80 °C、内存 > 80 %、显存 > 100 %；名称列保持灰色，让数值继续承担视觉主位。
- **可选指标**：菜单「显示数据」里逐项勾选，隐藏的项不占空间，面板会相应缩小。
- **中英文**：首次运行按 Windows 显示语言自动选择（英文系统→English，其余→中文），可随时在「语言」切换。
- **速度格式**：整数部分最多 3 位、小数 1 位（`5.9 KB/s`、`999.9 KB/s`）；整数部分为 0 时 2 位小数（`0.98 KB/s`）；超 3 位整数进位到下一单位（`1023.9 KB/s` → `1.00 MB/s`）。

诊断模式（不打开窗口，打印 5 次采样后退出）：

```powershell
cargo run -- --dump
```

## 打包成独立 exe

`cargo build --release` 产出的 `target\release\deskpulse.exe` 即为可分发的单文件程序：

- **带应用图标与版本信息**：由 `build.rs` + `winresource` 嵌入 `assets/icon.ico`。
- **不依赖 VC++ 运行时**：`.cargo\config.toml` 对 `x86_64-pc-windows-msvc` 开启 `+crt-static`。
- **体积优化**：release 开启 `lto`、`codegen-units = 1`、`strip`、`panic = "abort"`。
- **无控制台窗口**：release 构建带 `windows_subsystem = "windows"`。

```powershell
cargo build --release
Copy-Item .\target\release\deskpulse.exe .\dist\deskpulse.exe
```

`dist\deskpulse.exe` 可直接双击运行或拷给别人。重新生成图标：`pwsh -File .\assets\make-icon.ps1`。

## 文件结构

```
deskpulse/
├── Cargo.toml               # 依赖与 release 优化档
├── build.rs                 # 嵌图标 / 版本信息（winresource）
├── .cargo/config.toml       # x86_64-pc-windows-msvc: +crt-static
├── assets/
│   ├── icon.ico             # 应用图标
│   ├── make-icon.ps1        # 图标生成脚本
│   ├── AMDFamily17.bin      # PawnIO 模块（AMD SMN）
│   └── IntelMSR.bin         # PawnIO 模块（Intel MSR）
└── src/
    ├── main.rs              # 入口、--dump 诊断、自提权
    ├── overlay.rs           # 模块根：窗口生命周期、消息循环、拖动
    ├── overlay/             # 界面，按职责拆分
    │   ├── win32.rs         # 原生 Win32 声明（FFI 块与值类型）
    │   ├── metric.rs        # 可显示的指标
    │   ├── canvas.rs        # GDI 位图/字体与文字测量
    │   ├── layout.rs        # 名称/数值矩形的位置计算
    │   ├── paint.rs         # 单次重绘
    │   ├── menu.rs          # 控制菜单（右键 + 托盘）及其动作
    │   ├── topmost.rs       # 保持在其它置顶窗口之上
    │   └── dpi.rs           # DPI 感知与跨显示器重算
    ├── config.rs            # 配置读写 + 旧目录迁移
    ├── i18n.rs              # 中英文文案 + 系统语言检测
    ├── format.rs            # 速率 / 百分比 / 温度格式化
    ├── tray.rs              # 托盘图标（菜单由 `overlay` 提供）
    ├── autostart.rs         # 计划任务自启（schtasks）
    ├── elevate.rs           # 未提权时用 UAC 重启自己
    ├── single_instance.rs   # 每个登录会话只允许一个悬浮窗
    ├── diag.rs              # %APPDATA%\deskpulse\diag.log
    ├── wide.rs              # 补 NUL 的 UTF-16 字符串
    └── metrics/
        ├── mod.rs           # Snapshot + 采集线程 + 百分比
        ├── net.rs           # sysinfo 网卡差分（过滤虚拟网卡）
        ├── system.rs        # sysinfo CPU 占用 + 内存（共用一个 System）
        ├── gpu.rs           # NVIDIA NVML：占用 / 温度
        ├── gpu_mem.rs       # 显存：取 GPU Adapter Memory 性能计数器
        ├── pawnio.rs        # PawnIO 内核驱动直读 CPU 温度
        └── temp.rs          # 温度：PawnIO 优先，LHM HTTP 退路
```

## 配置

路径：`%APPDATA%\deskpulse\config.toml`，字段：

| 字段 | 说明 |
| --- | --- |
| `layout` | `vertical`（竖排）、`horizontal`（横排）或 `grid`（竖两排） |
| `spacing` | `tight`（紧凑，默认）或 `loose`（宽松）—— 行与行之间的垂直间距 |
| `align` | `left`（默认）、`center` 或 `right` —— 单元格内文字对齐 |
| `position` | 窗口左上角坐标，拖动后自动保存 |
| `refresh_secs` | 采集间隔（秒） |
| `opacity` | 面板不透明度 0.0–1.0；默认 `0.72`。菜单提供六档；该字段仍可填任意值。文字保持不透明。 |
| `autostart` | 是否登录自启（实际对应计划任务 `deskpulse`） |
| `lhm_port` | LibreHardwareMonitor HTTP 端口（退路用），默认 `8085` |
| `language` | `zh` 或 `en`；留空则首次运行按系统语言自动选择 |
| `visible` | 各指标显示开关的子表，键为 `net_up` / `net_down` / `cpu` / `cpu_temp` / `mem` / `gpu` / `vram` / `gpu_temp`；缺省为显示 |

`visible` 示例（只显示 CPU 与内存）：

```toml
[visible]
net_up = false
net_down = false
cpu = true
cpu_temp = false
mem = true
gpu = false
vram = false
gpu_temp = false
```

## 指标与数据来源

| 指标 | 数据来源 | 本机实测 |
| --- | --- | --- |
| 上传/下载网速 | `sysinfo` 网卡累计字节差分 | 正常 |
| CPU 占用率 | `sysinfo` | 正常 |
| CPU 温度 | 优先 PawnIO 内核驱动直读；否则 LHM HTTP | 正常（Tctl/Tdie） |
| 内存占用率 | `sysinfo` | 正常 |
| GPU 占用 / 温度 | NVIDIA NVML | 正常 |
| 显存 | `GPU Adapter Memory` 性能计数器：专用 + 借用的总和，因此可以超过 100 %（NVML 只报专用，作为回退） | 正常（同一帧 65 %，纯专用口径 55 %） |

CPU 温度的细节（为什么必须内核驱动、PawnIO 协议、提权要求）见 [ARCHITECTURE.zh.md](ARCHITECTURE.zh.md)。

## 已知限制

- **直读 CPU 温度需要管理员权限**（PawnIO 设备的限制）；非管理员运行时回退到 LHM（HTTP）。
- **独占全屏无法叠加显示，而且不是所有「全屏」都一样。** 面板是逐像素透明的分层窗口，只有经过 DWM 合成才能出现在屏幕上。真正把显示器从桌面手上拿走的独占全屏（Windows 会用 `QUNS_RUNNING_D3D_FULL_SCREEN` 标记它）会让 DWM 停止合成那块屏，此时**任何非注入窗口**都画不上去——任务管理器勾了「置于顶层」也一样，Windows 自己的音量条也不显示。所有能在真独占上显示数字的工具（RTSS / MSI Afterburner、Steam、Discord、WeGame 的浮窗）都是把 DLL 注入游戏进程、hook 它的 Present，在游戏自己那一帧里画；deskpulse 不做注入（反作弊会拦，也有封号风险）。
  - **能显示**：黑神话：悟空（UE5 / DX12）、Apex（`r5apex_dx12.exe`，DX12）。DX12 游戏的「全屏」实际上仍是 DWM 合成的无边框全屏，所以面板照常可见。
  - **不能显示**：英雄联盟（走老的 D3D9 真独占路径）、瓦洛兰特（配置为独占全屏）。这两种全屏模式下面板确实不在画面上，改成「无边框」或「窗口」即可。
- GPU 占用与温度依赖 NVIDIA 驱动（NVML），非 NVIDIA 显卡这两项显示 `--`；显存读 GPU 性能计数器，任何显卡都可用。
- 未知指标一律显示 `--`，绝不用 `0` 冒充，以免误导。

## 许可证

[MIT](LICENSE) © 2026 XIONG Shihao
