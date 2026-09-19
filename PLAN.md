# deskpulse 实施计划

本文档是工程设计与落地计划，偏技术细节。产品概览见 [README.md](README.md)。

---

## 1. 目标与非目标

### 目标
- 一个无边框、可拖动、置顶、半透明的小悬浮窗，常驻桌面。
- 实时（默认 1s 刷新）显示：网络上下行速率、CPU 占用率与温度、内存占用率、GPU 占用率。
- 纯 Rust 实现，编译产物为单个 exe，不要求用户装 .NET / VC++ 运行时。
- 资源占用低（CPU < 1%，内存 < 50MB），长时间挂机稳定。

### 非目标（第一版不做）
- 历史曲线 / 图表 / 日志持久化。
- 跨平台（只做 Windows）。
- 多语言 / 主题市场。
- 内核驱动级硬件监控。

---

## 2. 技术选型

### 2.1 GUI 框架：`eframe` + `egui`（推荐）

| 候选 | 纯 Rust | 无边框/透明/置顶 | 开发成本 | 结论 |
| --- | --- | --- | --- | --- |
| **eframe/egui** | ✅ | ✅ 原生支持 `decorations(false)`、`transparent`、`always_on_top`、`with_drag_window` | 低 | **采用** |
| iced | ✅ | 透明窗口支持弱、API 频繁变动 | 中 | 否 |
| slint | ❌（自有 DSL，非纯 Rust） | ✅ | 中 | 与"纯 Rust"要求冲突，否 |
| 裸 Win32 (`windows` crate) | ✅ | ✅ 完全控制 | 高 | 备选，v2 若需极致轻量再用 |
| Tauri | ❌（含 WebView2 依赖 + JS） | ✅ | 低 | 不是纯 Rust，否 |

**理由**：egui 是立即模式 GUI，写一个只读仪表盘非常快；窗口标志位天然满足悬浮窗需求；纯 Rust 无额外运行时。

**代价**：立即模式每帧重绘，若刷新间隔与重绘频率不匹配会浪费 CPU。解决办法是用 `request_repaint_after(interval)` 按刷新间隔重绘，而不是无脑 60fps。

### 2.2 指标采集库

- `sysinfo`：CPU 占用、内存、网卡累计字节 → 涵盖 3 项。
- `windows` crate：调用 Win32 PDH API 读 GPU 占用；少量 WMI 调用读温度。
- `nvml-wrapper`（可选）：NVIDIA 显卡的占用/显存/温度，最省事。
- 不引入 `wgpu`：它不做硬件占用统计。

### 2.3 配置与序列化

- `serde` + `toml`：读写 `config.toml`。

### 2.4 托盘与开机自启（已实现）

- 托盘：`tray-icon` crate（纯 Rust 封装 Win32 Shell_NotifyIcon）。菜单项：显示/隐藏、竖排/横排、开机自启、退出。
- 开机自启：`auto-launch` crate（写 `HKCU\...\Run`，无需管理员）。
- **集成方式**：托盘在主线程创建（eframe 的消息循环同时泵托盘消息）；菜单事件用 `MenuEvent::receiver().try_recv()` 轮询。
- **关键点**：窗口隐藏后 eframe 不再调用 `App::ui`，但会调用 `App::logic`。因此托盘事件处理放在 `logic` 里，并在其中持续 `request_repaint_after`，才能保证隐藏后还能再次显示。

### 2.5 依赖清单（实现时实际解析版本）

```toml
[dependencies]
eframe = { version = "0.36", default-features = false, features = ["glow", "default_fonts"] }
egui = "0.36"
sysinfo = "0.39"
windows = "0.62"          # 由 wmi 间接引入；未直接调用 Win32
serde = { version = "1", features = ["derive"] }
serde_json = "1"          # 解析 LibreHardwareMonitor 的 /data.json
toml = "1.1"
nvml-wrapper = "0.13"     # NVIDIA：占用 / 显存 / 温度（运行时动态加载 nvml.dll）
tray-icon = "0.25"        # 托盘图标 + 菜单（内部用 muda）
auto-launch = "0.6"       # 开机自启（写 HKCU Run 注册表）
```

Rust edition 为 2024。**未使用 `windows` / `wmi` crate**：新版 LHM 移除了 WMI provider，改为轮询其 HTTP `/data.json`，用标准库 `TcpStream` 发一个极简 HTTP GET 即可，无额外 HTTP 依赖。

---

## 3. 架构设计

### 3.1 进程模型

```
┌───────────────────────────────────────────────┐
│                    main thread                 │
│  eframe 事件循环 / 渲染 egui UI                │
│  ── 每帧读取共享快照 ──► 绘制                    │
└────────────▲──────────────────────────────────┘
             │ Arc<Mutex<Snapshot>>
             │
┌────────────┴──────────────────────────────────┐
│              采集线程 (1 个)                    │
│  loop {                                        │
│     net.sample();   // 记录字节数，差分算速率    │
│     cpu.sample();   // 占用率                   │
│     mem.sample();                              │
│     gpu.sample();   // PDH 查询                 │
│     temp.sample();  // WMI 查询（低频/异步）     │
│     更新 Snapshot;                             │
│     sleep(interval);                           │
│  }                                             │
└───────────────────────────────────────────────┘
```

- **单一采集线程 + 共享快照**：避免每帧都做系统调用，也避免跨线程生命周期问题。
- 温度 WMI 查询较慢（几十 ms～百 ms），单独放到更低频率（如 5s 一次）的子任务，避免拖慢主采集节拍。

### 3.2 数据模型

```rust
struct Snapshot {
    net_up_bps: u64,        // 上传 字节/秒
    net_down_bps: u64,      // 下载 字节/秒
    cpu_usage: f32,         // 0.0 .. 1.0
    cpu_temp_c: Option<f32>,// None = 不可用
    mem_used: u64,
    mem_total: u64,
    gpu_usage: Option<f32>,
    gpu_mem_used: Option<u64>,
    gpu_mem_total: Option<u64>,
    gpu_temp_c: Option<f32>,
    timestamp: Instant,
}
```

所有"可能拿不到"的指标一律用 `Option`，UI 显示 `--`。**不要用 0 冒充未知**，那会误导用户。

### 3.3 建议目录结构

```
deskpulse/
├── Cargo.toml
├── README.md
├── PLAN.md
└── src/
    ├── main.rs            # 入口：加载配置、启动采集线程、拉起 eframe
    ├── app.rs             # egui App 实现：布局、绘制、拖动、右键菜单
    ├── config.rs          # Config 结构 + 默认值 + 读写
    ├── format.rs          # 速率/容量/百分比的显示格式化
    └── metrics/
        ├── mod.rs         # Snapshot 定义 + Collector 聚合 + 线程
        ├── net.rs         # 基于 sysinfo 的网卡差分
        ├── cpu.rs         # sysinfo CPU 占用
        ├── mem.rs         # sysinfo 内存
        ├── gpu.rs         # PDH GPU Engine 计数器
        └── temp.rs        # WMI 温度查询
```

---

## 4. 各指标实现细节

### 4.1 网络速率（可靠）

`sysinfo::Networks` 提供每个网卡的 `total_received()` / `total_transmitted()`（累计字节）。

```rust
let now = networks_total();          // 所有网卡求和
let dt = now_instant - last_instant;
let speed = (now - last) as f64 / dt.as_secs_f64();
```

注意：
- 只统计物理网卡，排除 `Loopback`、`vEthernet`、`Tunnel`、已断开的适配器，否则 VPN/虚拟机网卡会污染数据。
- 用单调时钟 `Instant` 求差分，不用墙上时钟。
- Windows 上 `sysinfo` 的网卡枚举偶尔会漏掉刚插拔的设备，需容错（拿不到就沿用上次）。

### 4.2 CPU / 内存（可靠）

- CPU：`System::refresh_cpu_usage()` 后取 `global_cpu_usage()`。
- 内存：`used_memory() / total_memory()`。
- `System` 实例要复用，不能每帧新建（否则 CPU 占用永远是 0——这是常见坑）。

### 4.3 GPU 占用率（可靠，与任务管理器同源）

用 PDH 查询计数器：

```
\GPU Engine(*)\Utilization Percentage
```

- 该计数器对每个 `pid_xxx_luid_xxx_engtype_3D` 实例给一个百分比。
- **把所有实例的值直接相加**即为总 GPU 占用（任务管理器就是这么算的，所以总和可能 > 100%，多引擎时正常）。
- 中文系统计数器路径为本地化字符串，需用 `PdhAddEnglishCounterW` 强制走英文路径，否则查询失败。
- 初始化时枚举一次实例掩码，之后 `PdhCollectQueryData` + 遍历。
- 无独显 / 纯核显机器上该计数器依然存在，通常可用。

### 4.4 GPU 显存 / 温度（已实现：NVIDIA）

直接走 `nvml-wrapper`：
- `utilization_rates()` → GPU 占用
- `memory_info()` → 显存已用 / 总量
- `temperature(TemperatureSensor::Gpu)` → GPU 温度

NVML 在运行时动态加载 `nvml.dll`，因此没有 N 卡的机器不会崩溃，相关项显示 `--`。

**与原计划的差异**：计划里的「PDH 通用退路」**未实现**。当前非 NVIDIA 显卡只能显示 `--`。若以后要支持 AMD/Intel，再补 PDH 的 `\GPU Engine(*)\Utilization Percentage`。

### 4.5 CPU 温度（已实现：HTTP，非 WMI）

**现实变化**：新版 LibreHardwareMonitor（0.9.x）已经**移除了 WMI provider**，`root\LibreHardwareMonitor` 命名空间不再存在（这也是原先计划的方案失效的原因）。当前有效做法是 LHM 内置的 HTTP 服务：

1. LHM 选项里勾选 **Run web server**（设置键 `runWebServerMenuItem`，默认关闭，端口 `listenerPort` 默认 8085）。
2. `GET http://127.0.0.1:8085/data.json` 返回完整传感器树 JSON。
3. 遍历树，取 `Type == "Temperature"` 且 `SensorId` 以 `/intelcpu` / `/amdcpu` / `/cpu` 开头的节点；优先 `Text` 含 `Tctl` / `Tdie` / `Package` 的（AMD 的 `Core (Tctl/Tdie)`、Intel 的 `CPU Package`）。
4. **注意**：LHM 的数值字段（`RawValue` 等）是**带单位的字符串**，如 `"56.9 °C"`，需要取首个空白分隔的 token 再解析成数字。

采集实现：`TcpStream` 连 `127.0.0.1:lhm_port`，发 `GET /data.json HTTP/1.1 ... Connection: close`，读到 EOF，按 `\r\n\r\n` 切出 body，`serde_json` 解析。每 3 个采样周期查询一次；连接失败后退避 10 个周期再重试。端口可由 `config.lhm_port` 配置。

---

## 5. 技术风险与取舍

### 5.1 CPU 温度：Windows 上纯用户态拿不到"真实核心温度"（重点）

必须直说：**Windows 没有官方公开的 CPU 核心温度 API**。可选的都不是理想方案：

| 方案 | 能拿到什么 | 问题 |
| --- | --- | --- |
| WMI `root\WMI: MSAcpi_ThermalZoneTemperature` | ACPI 热区温度 | 很多台式机主板根本不实现，返回的常是主板/机箱温度，甚至直接报错。**不可靠** |
| LibreHardwareMonitor / OpenHardwareMonitor 的 WMI 命名空间 | 真实核心温度（含 Intel/AMD） | **用户必须先安装并运行 LHM/OHM**，且以管理员启动。否则没有数据 |
| Intel Power Gadget / 厂商 SDK | 部分 Intel 平台 | 已停止维护、仅部分 CPU |
| `WinRing0` 内核驱动直接读 MSR | 真实核心温度 | 需要装内核驱动、管理员权限，有安全风险，杀软可能拦截。**不建议** |

**已确认采用（按优先级）**：
1. **MVP 默认走 LHM/OHM WMI**：检测 `root\LibreHardwareMonitor`（以及 `root\OpenHardwareMonitor`）是否存在，存在就读，不存在就在 UI 显示 `CPU 温度: --`。
2. 不内嵌内核驱动。这会把一个"小挂件"变成一个需要签名、可能被杀软报毒的系统级软件，性价比极低。
3. 文档明确写清前置条件，而不是假装能开箱即得。


### 5.2 其它风险

- **首次采样无速率**：网络速率需要两次采样，第一帧显示 `--`。
- **多网卡/VPN 重复计数**：需按接口名过滤。
- **PDH 计数器本地化**：必须用英文计数器路径 API。
- **性能**：WMI 查询慢，必须降频 + 移出主采集循环。
- **透明窗口在部分显卡驱动下渲染异常**：提供"关闭透明"选项作为兜底。
- **DPI 缩放**：高分屏下字体模糊或尺寸错乱，需处理 `egui` 的 `pixels_per_point`。

---

## 6. 里程碑

### M0：骨架（0.5 天）
- `cargo init`，加依赖，跑通一个空白置顶透明无边框窗口。
- 实现拖动、右键菜单（切换横排/竖排、退出）。
- 验收：窗口能拖动、能切换布局、能关闭。

### M1：可靠指标（1 天）
- 接入 sysinfo：网速、CPU 占用、内存。
- 实现采集线程 + 快照 + `request_repaint_after`。
- 验收：数据与任务管理器误差 < 5%。

### M2：GPU（0.5 天，已实现）
- NVML 接入 GPU 占用、显存、温度。
- ~~PDH 作为 NVML 失败时的占用率退路~~ → 未实现（见 4.4）。
- 验收：跑一个 3D 程序时数值明显上升，与任务管理器趋势一致。

### M3：CPU 温度（0.5 天，风险项，已实现）
- 实现 LHM/OHM WMI 探测与读取，失败优雅降级为 `--`。
- 验收：装了 LHM 的机器上能读数；没装的机器显示 `--` 且不崩溃。

### M4：托盘 / 自启 / 打磨（已实现）
- `tray-icon` 托盘菜单、`auto-launch` 开机自启。
- 配置文件读写、格式化（B/s、KB/s、MB/s）、字体加载、异常自恢复。
- 验收：托盘能显示/隐藏窗口；重启后能自启；连续运行 8 小时无内存增长、无误报。

---

## 7. 已确认的决策

| 议题 | 决定 | 实现状态 |
| --- | --- | --- |
| CPU 温度 | 读 LibreHardwareMonitor 的数据；无则显示 `--` | 已实现（HTTP `/data.json`，非 WMI） |
| 显卡 | NVIDIA，走 NVML | 已实现；PDH 通用退路未做 |
| 显示样式 | 无边框半透明置顶；运行时可切换横排 / 竖排，选择持久化 | 已实现 |
| 托盘 / 自启 | 都要 | 已实现 |
| 许可证 | MIT | 已采用（见 LICENSE） |

## 8. 运行前须知（给用户）

- **CPU 温度**需要本机安装并运行 [LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor)（以管理员启动），并勾选 **Options → Run web server**。否则该项显示 `--`。
- LHM 的 web server 默认关闭；本机已通过编辑 `LibreHardwareMonitor.config` 写入 `runWebServerMenuItem=true` 开启。
- 本机已创建计划任务 `LibreHardwareMonitor`（登录触发、`RunLevel Highest`、`LogonType Interactive`），实现免 UAC 的登录自启。
- GPU 显存 / 温度需要 NVIDIA 驱动正常。

## 9. 实现状态与文件结构

已实现全部里程碑，`cargo check` / `cargo clippy` / `cargo test` 全绿。

```
src/
├── main.rs              # 入口；--dump 诊断模式；按配置建窗口
├── app.rs               # egui App（eframe 0.36 的 logic/ui）；布局、拖动、右键菜单
├── config.rs            # Config 读写 + 单测
├── autostart.rs         # auto-launch 封装
├── tray.rs              # tray-icon 托盘 + 程序生成的图标
├── format.rs            # 速率/百分比/温度的显示格式化
└── metrics/
    ├── mod.rs           # Snapshot + 采集线程 + 百分比计算
    ├── net.rs           # sysinfo 网卡差分（过滤虚拟网卡）
    ├── cpu.rs           # sysinfo CPU 占用
    ├── mem.rs           # sysinfo 内存
    ├── gpu.rs           # NVML：占用 / 显存 / 温度
    └── temp.rs          # LHM HTTP /data.json：CPU 温度，带退避
```

### 与原计划的技术差异

1. **eframe 0.36 API 变了**：`App` trait 用 `logic(&mut self, &Context, &mut Frame)` + `ui(&mut self, &mut Ui, &mut Frame)` 取代旧的 `update`。托盘事件因此放在 `logic`（窗口隐藏时仍调用）。
2. **CPU 温度从 WMI 改为 HTTP**：新版 LHM 移除 WMI provider，检测到其 HTTP `/data.json` 才是当前唯一可行路径。这一点被原计划完全误判。
3. **不再使用 `windows` crate 直接调 Win32/PDH**：GPU 走 NVML，温度走 LHM HTTP，代码量大幅下降。
4. **中文字体**：egui 不含 CJK 字形，启动时从 `C:\Windows\Fonts` 依次尝试 `msyh.ttc` / `simhei.ttf` 等加载，失败则回退到内置拉丁字体。
5. **新增 `--dump`**：无 GUI 打印 5 次采样，用于确认本机可用指标。
6. **`config.lhm_port`**：LHM 的端口可配置，deskpulse 同步支持。


