# deskpulse

[English](../README.md) | 中文

一个由纯 Rust 实现的常驻桌面的 Windows 悬浮窗，实时显示系统状态：

- 上传 / 下载网速
- CPU 占用率 + CPU 温度
- 内存占用率
- 显卡占用率 + GPU 温度（NVIDIA，走 NVML）
- 显存（专用 **加上** 从内存借用的部分，因此可能超过 100 %）

附加功能：三种排列（竖排 / 横排 / 竖两排）、两种间距、左/居中/右对齐、六档透明度、在系统缩放之上再叠加的缩放百分比、把面板锁在显示器内的开关、可选显示项、超过阈值的数值变红、中英文、系统托盘、开机自启、配置持久化。

技术栈：纯 Rust，用原生 Win32 分层窗口 + GDI 绘制。**不依赖任何 GPU API**（OpenGL / Direct3D / Vulkan 都不用），也不依赖 C++ 运行时，单文件 exe。

> 技术栈、架构与设计取舍见 [ARCHITECTURE.zh.md](ARCHITECTURE.zh.md)。

## 界面

所有设置都在右键菜单里（托盘图标打开的是同一个菜单，同一份代码构建）：

![设置菜单](menu.png)

| 菜单项 | 作用 |
| --- | --- |
| **显示数据** ▸ | 勾选要显示的指标；隐藏的项不占空间，面板随之缩小 |
| **布局** ▸ | 竖排 / 横排 / 竖两排 |
| **间距** ▸ | 紧凑 / 宽松 —— 行与行之间的垂直间距 |
| **对齐** ▸ | 单元格内名称与数值的左 / 居中 / 右对齐 |
| **透明度** ▸ | 30 / 45 / 60 / 72 / 85 / 100 % 背景不透明度 |
| **缩放** ▸ | 50 / 75 / 100 / 125 / 150 / 175 %，**在显示器缩放比例之上**再叠加 |
| **语言** ▸ | 中文 / English |
| **保持在屏内** | 面板拖不出所属显示器；放大后或显示器变化后会被自动拉回 |
| **开机自启** | 登录计划任务，因此 CPU 温度在无 UAC 提示的情况下也能工作 |
| **显示 / 隐藏** | 暂时隐藏面板，不退出 |
| **退出** | 退出程序 |

还有几点值得知道：

- **宽度由指标集合决定，不由数据决定。** 数值列按每个可见指标**可能出现的最宽值**预留（`999.9 MB/s`、`100%`、`100°C`），所以数值变化时面板既不增长也不抖动；只有隐藏指标才会让它变窄。
- **数值越过阈值变红**：温度 > 80 °C、内存 > 80 %、显存 > 100 %。名称列保持灰色，让数值继续承担视觉主位。
- **速率格式**：整数部分最多 3 位、小数 1 位（`999.9 KB/s`）；不足一个单位时 2 位小数（`0.98 KB/s`）；需要 4 位整数时进位到下一单位（`1023.9 KB/s` → `1.00 MB/s`）。
- **有两项设置没有菜单入口**，只存在于 `%APPDATA%\deskpulse\config.toml`：`refresh_secs`（采集间隔，默认 1 秒）和 `lhm_port`（LibreHardwareMonitor 的 HTTP 端口，仅在 PawnIO 驱动不可用时使用）。这个文件可以手工编辑，菜单每次改动都会重写它；其中的 `visible` 表以指标名为键：`net_up`、`net_down`、`cpu`、`cpu_temp`、`mem`、`gpu`、`vram`、`gpu_temp`。

### 透明度

只有面板背景是半透明的，文字始终以完全不透明绘制，因此每一档都清晰可读。下面依次是 30 %、45 %、72 %（默认）和 100 %：

| 30 % | 45 % | 72 %（默认） | 100 % |
| --- | --- | --- | --- |
| ![30 % 透明度](opacity-30.png) | ![45 % 透明度](opacity-45.png) | ![72 % 透明度](opacity-72.png) | ![100 % 透明度](opacity-100.png) |

### 缩放

面板按物理像素排布，比例是 `显示器 DPI / 96 × scale_percent / 100`。因此 `100 %` 就是完全跟随 Windows 的缩放比例，其余档位在此基础上相乘：在 150 % 的桌面上选 `150 %`，等于 100 % 桌面的 1.5 × 1.5 = 2.25 倍，选 `50 %` 则是它的四分之三。面板的所有部分一起缩放——字体、行高、边距、圆角半径，以及预留出来的列宽。

## 构建与运行

```powershell
cargo run --release
```

运行后在桌面出现悬浮窗，鼠标拖动可移动。**右键**打开设置菜单（见[界面](#界面)）：显示哪些指标、布局、间距、对齐、透明度、缩放、语言、保持在屏内与开机自启两个开关、显示/隐藏、退出。

诊断模式（不打开窗口，打印 5 次采样后退出）：

```powershell
cargo run -- --dump
```

## 性能

在开发机上实测（除悬浮窗外桌面处于空闲）：

| | |
| --- | --- |
| 处理器 | AMD Ryzen 5 9600X（6 核 / 12 线程） |
| CPU 占用 | **约 0.29% 单核**（约占整机 CPU 的 0.02%） |
| 私有内存 | **约 27 MB** |
| 工作集 | 约 43 MB |
| `deskpulse.exe` | **1.05 MB** |

显示 6 项指标、每秒采样一次、Windows 11 且显示缩放 150%。换算过来大约**每秒 3 毫秒 CPU 时间**——在任务管理器里基本看不到，远低于一个常见动画界面渲染一帧的开销。

之所以这么低：

- **数据不变就不重绘**：采集线程每采到一份新快照才发消息唤醒窗口（默认每秒一次），窗口只在此时重绘。没有持续渲染循环，也没有动画。
- **整个面板就是一张小位图**：背景和文字都合成到内存里约 160×180 像素的 32 位 DIB 上，再用一次 `UpdateLayeredWindow` 交给系统。
- **完全不用 GPU API**：界面不碰 OpenGL / Direct3D / Vulkan，因此只有 Microsoft 基本显示适配器的机器、虚拟机和远程桌面也能正常跑。

对比：同一悬浮窗此前的 `egui` / `wgpu` 构建工作集约 **125 MB**（OpenGL 后端）和 **420 MB**（WARP 后端），CPU 也高一个数量级。
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
├── docs/                    # 截图与中文文档
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
    │   └── dpi.rs           # DPI 感知、布局缩放、保持在屏内
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

[MIT](../LICENSE) © 2026 XIONG Shihao
