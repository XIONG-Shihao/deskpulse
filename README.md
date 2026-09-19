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

运行后在桌面出现悬浮窗，鼠标拖动可移动。**右键**打开设置菜单：选择显示哪些数据、竖排 / 横排、切换中英文、开机自启、退出。托盘图标菜单同样可操作。

### 界面行为

- **窗口自动贴合内容**：每帧测量内容并按内容尺寸调整窗口，因此没有多余的透明空白，也不会占用不该占的鼠标区域。
- **单元格宽度固定**：数字位数变化（`9.9 KB/s` → `1.02 MB/s`）不会导致窗口宽度抖动。
- **右键弹出菜单**：菜单是**独立的小窗口**，紧贴内容、没有多余透明区；主窗口始终保持数据尺寸（既不放大也不留死区）。关闭方式：点菜单项、再按一次右键、点击菜单外、按 Esc，或菜单失焦。
- **深色主题**：强制使用高对比的深色配色（近黑底 + 近白字），不受系统浅色主题影响，菜单文字清晰可读。
- **可选指标**：菜单里逐项勾选，隐藏的项不占空间，窗口会相应缩小。
- **中英文**：首次运行按 Windows 显示语言自动选择（英文系统→English，其余→中文），可随时在菜单切换。
- **三种排列**：竖排（每行一项）、横排（一行所有项）、竖两排（每行两项，默认依次为 上传/下载、CPU/CPU温、内存/GPU、显存/GPU温）。
- **两种间距**：菜单里可选。紧凑（默认）用较窄的单元格并把标签右对齐、数值左对齐，名称↔数值间距约 4pt，**竖两排两列之间无间隔**；竖两排时第二列的名称改为左对齐。宽松为原样（居中，约 47pt）。
- **调试着色**：菜单里「显示框/间距」勾选后，数值框填半透明黄、列间距填半透明红，用来直观查看布局占位；关闭即恢复原样。
- **速度格式**：整数部分最多 3 位、小数 1 位（`5.9 KB/s`、`999.9 KB/s`）；整数部分为 0 时用 2 位小数（`0.98 KB/s`）；超过 3 位整数就进位到下一单位（`1023.9 KB/s` → `1.00 MB/s`）。

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
| `layout` | `vertical`（竖排）、`horizontal`（横排）或 `grid`（竖两排，每行两项） |
| `spacing` | `tight`（紧凑，标签右对齐 + 数值左对齐）或 `loose`（宽松，原样） |
| `show_boxes` | 调试用：`true` 时数值框填黄、列间距填红，便于观察布局 |
| `position` | 窗口左上角坐标，拖动后自动保存 |
| `refresh_secs` | 采集间隔（秒） |
| `opacity` | 背景透明度 0.0–1.0 |
| `autostart` | 是否开机自启（以注册表实际状态为准） |
| `lhm_port` | LibreHardwareMonitor HTTP 服务端口，默认 `8085` |
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

## 指标与数据来源一览

| 指标 | 数据来源 | 本机实测 |
| --- | --- | --- |
| 上传/下载网速 | `sysinfo` 网卡累计字节差分 | 正常 |
| CPU 占用率 | `sysinfo` | 正常 |
| CPU 温度 | 优先 PawnIO 内核驱动直读；否则 LibreHardwareMonitor HTTP | 正常（Tctl/Tdie） |
| 内存占用率 | `sysinfo` | 正常 |
| GPU 占用 / 显存 / 温度 | NVIDIA NVML | 正常（GPU 2%、显存 21%、40°C） |

## CPU 温度：两级数据源

Windows 没有可靠的公开 CPU 温度 API。核心温度只能通过 **MSR / SMN** 寄存器读取，而读寄存器需要内核驱动（`RDMSR` 是 ring-0 指令，用户态做不到）。因此：

1. **优先：PawnIO 驱动直读**（不需要任何常驻程序）。deskpulse 直接打开 `\\.\GLOBALROOT\Device\PawnIO`，加载 `AMDFamily17.bin` / `IntelMSR.bin` 模块，读寄存器自己换算：
   - AMD：SMN `0x00059800`，`(raw >> 21) * 0.125`，按标志位减 49°C。
   - Intel：MSR `0x1A2`（TjMax）、`0x1B1`/`0x19C`（DTS），`TjMax - delta`。
   
   ⚠️ **PawnIO 设备需要管理员权限**。所以直读模式要求 deskpulse 以管理员运行。
2. **退路：LibreHardwareMonitor HTTP**。没有管理员权限（或 PawnIO 未安装）时，回退到轮询 LHM 的 `http://127.0.0.1:8085/data.json`（需 LHM 正在运行）。

两者都不可用时温度显示 `--`，程序不会报错。

**本机部署**：已**卸载 LHM**，改用计划任务 `deskpulse`（登录时以**最高权限**启动，免 UAC），因此 deskpulse 常驻管理员、直读 PawnIO，不再需要任何其他程序。app 内的「开机自启」开关现在就是管理这个计划任务。

**手动启动**：直接双击 exe 时，若当前不是管理员，程序会用 UAC 请求提权重启自己（计划任务启动时已是管理员，不会弹 UAC）。诊断信息写在 `%APPDATA%\deskpulse\diag.log`（记录温度后端与失败原因）。

> 注：PawnIO 模块来自 [namazso/PawnIO.Modules](https://github.com/namazso/PawnIO.Modules)（**LGPL-2.1**），已随本项目放在 `assets/` 下；PawnIO 驱动本身由独立包 `namazso.PawnIO` 提供，仍需保留安装。

## 已知限制

- **直读 CPU 温度需要管理员权限**（PawnIO 设备的限制）；本机用计划任务满足，非管理员运行时回退到 LHM（已卸载，故会显示 `--`）。
- GPU 指标依赖 NVIDIA 驱动（NVML）。非 NVIDIA 显卡时相关项显示 `--`（暂未实现 PDH 通用退路）。
- 本机同时存在 AMD 核显与 NVIDIA 独显，当前只取 NVIDIA（`device_by_index(0)`）。
- 未知指标一律显示 `--`，绝不用 `0` 冒充，以免误导。

## 许可证

[MIT](LICENSE) © 2026 XIONG Shihao
