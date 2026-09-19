# deskpulse

一个常驻桌面的 Windows 悬浮窗，实时显示系统状态：

- 上传 / 下载网速
- CPU 占用率 + CPU 温度
- 内存占用率
- 显卡占用率、显存占用、GPU 温度（NVIDIA）

附加功能：运行时切换横排 / 竖排、系统托盘、开机自启、配置持久化。

技术栈：纯 Rust（GUI 用 `egui` / `eframe`），不依赖 C++ 运行时，单文件 exe。

> 设计取舍与决策记录见 [PLAN.md](PLAN.md)。

## 项目状态

M0–M4 已实现并在本机验证，五项指标全部有真实数据：

- 无边框、半透明、置顶窗口，可鼠标拖动，右键菜单切换布局 / 开机自启 / 退出。
- 网络、CPU、内存、GPU（NVML）、CPU 温度（LibreHardwareMonitor）均跑通。
- 托盘图标与开机自启已接入。
- 配置写入 `%APPDATA%\deskpulse\config.toml`。

本机 `--dump` 实测：`cpu-temp=72°C gpu=1% vram=20% gpu-temp=40°C`。

## 构建与运行

```powershell
cargo run --release
```

运行后在桌面出现悬浮窗，鼠标拖动可移动。**右键**弹出设置菜单（竖排 / 横排、开机自启、退出）。托盘图标菜单同样可操作。

### 界面行为

- **窗口自动贴合内容**：每帧测量内容并按内容尺寸调整窗口，因此没有多余的透明空白，也不会占用不该占的鼠标区域。
- **单元格宽度固定**：数字位数变化（`9.9 KB/s` → `1.02 MB/s`）不会导致窗口宽度抖动。
- **菜单内嵌而非弹窗**：右键菜单直接画在内容区里，窗口会临时撑高以容纳菜单；关闭后缩回。这样避免了小窗口把弹出菜单裁掉的问题。
- 实测尺寸：竖排 `166×194`（菜单展开 `166×269`），横排 `748×57`（菜单展开 `748×133`）。

诊断模式（不打开窗口，打印 5 次采样后退出，用于确认本机哪些指标可用）：

```powershell
cargo run -- --dump
```

## 打包成独立 exe

`cargo build --release` 产出的 `target\release\deskpulse.exe` 即为可分发的单文件程序：

- **带应用图标与版本信息**：由 `build.rs` + `winresource` 嵌入 `assets/icon.ico`。
- **不依赖 VC++ 运行时**：`.cargo\config.toml` 对 `x86_64-pc-windows-msvc` 开启 `+crt-static`，静态链接 C 运行时。
- **体积优化**：release 开启 `lto`、`codegen-units = 1`、`strip`、`panic = "abort"`，成品约 5.8 MB。
- **无控制台窗口**：release 构建带 `windows_subsystem = "windows"`。

```powershell
cargo build --release
Copy-Item .\target\release\deskpulse.exe .\dist\deskpulse.exe
```

`dist\deskpulse.exe` 可直接双击运行或拷给别人，无需安装。

重新生成图标（改配色/样式后）：

```powershell
pwsh -File .\assets\make-icon.ps1
```

## 配置

路径：`%APPDATA%\deskpulse\config.toml`，字段：

| 字段 | 说明 |
| --- | --- |
| `layout` | `vertical` 或 `horizontal` |
| `position` | 窗口左上角坐标，拖动后自动保存 |
| `refresh_secs` | 采集间隔（秒） |
| `opacity` | 背景透明度 0.0–1.0 |
| `autostart` | 是否开机自启（以注册表实际状态为准） |
| `lhm_port` | LibreHardwareMonitor HTTP 服务端口，默认 `8085` |

## 指标与数据来源一览

| 指标 | 数据来源 | 本机实测 |
| --- | --- | --- |
| 上传/下载网速 | `sysinfo` 网卡累计字节差分 | 正常 |
| CPU 占用率 | `sysinfo` | 正常 |
| CPU 温度 | LibreHardwareMonitor 的 HTTP `/data.json` | 正常（Tctl/Tdie） |
| 内存占用率 | `sysinfo` | 正常 |
| GPU 占用 / 显存 / 温度 | NVIDIA NVML | 正常（GPU 2%、显存 21%、40°C） |

## CPU 温度的前置条件（重要）

Windows 没有可靠的公开 CPU 温度 API，只能借第三方硬件监控。本项目的做法：

1. 安装 [LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor)（`winget install LibreHardwareMonitor.LibreHardwareMonitor`）。
2. 在 LHM 里勾选 **Options → Run web server**（默认端口 8085）。本机已通过修改 LHM 的 `LibreHardwareMonitor.config` 打开此项。
3. **保持 LHM 在后台运行**（需要管理员权限）。deskpulse 会轮询 `http://127.0.0.1:8085/data.json`，从中取 CPU 的 Tctl/Tdie 或 CPU Package 温度。

LHM 未运行或 web server 未开启时，温度显示 `--`，程序不会报错；连接失败后有约 10 秒退避再重试。

本机已创建一个计划任务 `LibreHardwareMonitor`，在登录时以**最高权限**启动 LHM，因此不会每次开机弹 UAC，温度开箱可用。手动管理：

```powershell
Start-ScheduledTask   -TaskName LibreHardwareMonitor   # 立即启动
Disable-ScheduledTask -TaskName LibreHardwareMonitor   # 取消自启
```

> 注：新版 LibreHardwareMonitor（0.9.x）已移除 WMI provider，网上大量 `root\LibreHardwareMonitor` 的教程已过时，本项目的 HTTP 方案才是当前有效的。

## 已知限制

- CPU 温度依赖 LHM 常驻运行（本机已用计划任务设为登录自启）。
- GPU 指标依赖 NVIDIA 驱动（NVML）。非 NVIDIA 显卡时相关项显示 `--`（暂未实现 PDH 通用退路）。
- 本机同时存在 AMD 核显与 NVIDIA 独显，当前只取 NVIDIA（`device_by_index(0)`）。
- 未知指标一律显示 `--`，绝不用 `0` 冒充，以免误导。

## 许可证

[MIT](LICENSE) © 2026 XIONG Shihao
