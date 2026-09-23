# deskpulse

[English](README.md) | 中文

一个由纯Rust实现的常驻桌面的 Windows 悬浮窗，实时显示系统状态：

- 上传 / 下载网速
- CPU 占用率 + CPU 温度
- 内存占用率
- 显卡占用率、显存占用、GPU 温度（NVIDIA）

附加功能：三种排列（竖排 / 横排 / 竖两排）、两种间距、可选显示项、中英文、系统托盘、开机自启、配置持久化。

技术栈：纯 Rust（GUI 用 `egui` / `eframe`），通过 Windows WARP 进行 CPU 软件渲染，不依赖 C++ 运行时，单文件 exe。

> 技术栈、架构与设计取舍见 [ARCHITECTURE.zh.md](ARCHITECTURE.zh.md)。

## 构建与运行

```powershell
cargo run --release
```

运行后在桌面出现悬浮窗，鼠标拖动可移动。**右键**打开设置菜单（一级菜单：显示数据 / 布局 / 间距 / 语言 / 开机自启 / 退出，前四项展开为子菜单）。托盘图标菜单同样可操作。

### 界面行为

- **窗口自动贴合内容**：每帧测量内容并按内容尺寸调整窗口，因此没有多余的透明空白，也不会占用不该占的鼠标区域。
- **单元格宽度固定**：数字位数变化（`9.9 KB/s` → `1.02 MB/s`）不会导致窗口宽度抖动。
- **右键弹出菜单**：菜单是**独立的小窗口**（二级结构），紧贴内容、没有多余透明区；主窗口始终保持数据尺寸。关闭方式：点菜单项、再按一次右键、点击菜单外、按 Esc，或菜单失焦。
- **深色主题**：强制使用高对比的深色配色（近黑底 + 近白字），不受系统浅色主题影响。
- **CPU 渲染**：面板和设置菜单通过 Direct3D 12 的 Windows WARP 软件适配器渲染；原生 Win32 分层窗口使用 GDI 绘制数据文字，使文字在半透明面板上保持不透明。界面不使用 NVIDIA 或 AMD GPU。GPU 指标可用时仍通过 NVML 读取。
- **指标字体**：Microsoft YaHei，14 逻辑像素，向 GDI 请求 600 字重，不额外启用粗体样式。名称和数值使用相同的不透明样式。
- **可选指标**：菜单「显示数据」里逐项勾选，隐藏的项不占空间，窗口会相应缩小。
- **中英文**：首次运行按 Windows 显示语言自动选择（英文系统→English，其余→中文），可随时在「语言」切换。
- **三种排列**：竖排（每行一项）、横排（一行所有项）、竖两排（每行两项，默认依次为 上传/下载、CPU/CPU温、内存/GPU、显存/GPU温）。
- **两种间距**：紧凑（默认）用较窄单元格、标签右对齐 + 数值左对齐，名称↔数值间距约 4pt、两列之间无间隔；宽松为居中宽格（约 47pt）。
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
    ├── app.rs               # eframe App：UI、布局、菜单、窗口自适应
    ├── config.rs            # 配置读写 + 旧目录迁移
    ├── i18n.rs              # 中英文文案 + 系统语言检测
    ├── format.rs            # 速率 / 百分比 / 温度格式化
    ├── tray.rs              # 托盘图标与菜单
    ├── autostart.rs         # 计划任务自启（schtasks）
    ├── elevate.rs           # 未提权时用 UAC 重启自己
    ├── diag.rs              # %APPDATA%\deskpulse\diag.log
    ├── window.rs            # 原生面板透明度、圆角和窗口样式
    ├── text_window.rs       # 用可穿透鼠标的 GDI 分层窗口绘制不透明数据文字
    └── metrics/
        ├── mod.rs           # Snapshot + 采集线程 + 百分比
        ├── net.rs           # sysinfo 网卡差分（过滤虚拟网卡）
        ├── cpu.rs           # sysinfo CPU 占用
        ├── mem.rs           # sysinfo 内存
        ├── gpu.rs           # NVIDIA NVML：占用 / 显存 / 温度
        ├── pawnio.rs        # PawnIO 内核驱动直读 CPU 温度
        └── temp.rs          # 温度：PawnIO 优先，LHM HTTP 退路
```

## 配置

路径：`%APPDATA%\deskpulse\config.toml`，字段：

| 字段 | 说明 |
| --- | --- |
| `layout` | `vertical`（竖排）、`horizontal`（横排）或 `grid`（竖两排） |
| `spacing` | `tight`（紧凑，默认）或 `loose`（宽松） |
| `position` | 窗口左上角坐标，拖动后自动保存 |
| `refresh_secs` | 采集间隔（秒） |
| `opacity` | 面板不透明度 0.0–1.0；默认 `0.72`。文字保持不透明。 |
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

### 悬浮窗上的 `NORMAL` 字样

旧版 OpenGL 构建中，异常的 `NORMAL` 字样同时出现在悬浮窗和右键菜单上；它不是 deskpulse 的指标，也不是背景透出来的文字。关闭 G-SYNC 后仍会出现，确切来源尚未查明。

新版使用 **Microsoft Basic Render Driver (WARP)** 绘制面板和菜单，并使用 GDI 绘制不透明的数据文字。用户已确认，在其机器上新版不再出现 `NORMAL`。这说明切换渲染路径能解决问题，但不能据此确定究竟是哪个组件绘制了该字样。运行新版前请彻底退出旧进程。

## 指标与数据来源

| 指标 | 数据来源 | 本机实测 |
| --- | --- | --- |
| 上传/下载网速 | `sysinfo` 网卡累计字节差分 | 正常 |
| CPU 占用率 | `sysinfo` | 正常 |
| CPU 温度 | 优先 PawnIO 内核驱动直读；否则 LHM HTTP | 正常（Tctl/Tdie） |
| 内存占用率 | `sysinfo` | 正常 |
| GPU 占用 / 显存 / 温度 | NVIDIA NVML | 正常 |

CPU 温度的细节（为什么必须内核驱动、PawnIO 协议、提权要求）见 [ARCHITECTURE.zh.md](ARCHITECTURE.zh.md)。

## 已知限制

- **直读 CPU 温度需要管理员权限**（PawnIO 设备的限制）；非管理员运行时回退到 LHM（HTTP）。
- GPU 指标依赖 NVIDIA 驱动（NVML）。非 NVIDIA 显卡时相关项显示 `--`。
- 未知指标一律显示 `--`，绝不用 `0` 冒充，以免误导。

## 许可证

[MIT](LICENSE) © 2026 XIONG Shihao
