# at-pc 双屏与多显示器客户端适配优化计划：从“单屏主视”迈向“全拓扑全屏无缝操控”

> **版本**：v1.0  
> **制定日期**：2026-09-08  
> **适用模块**：`crates/protocol`、`crates/agent`、`crates/server`、`crates/server/frontend`  
> **关联规范**：[`docs/plans/2026-09-07-computer-use-optimization-milestones.md`](file:///Users/clkj/项目/at/at-pc/docs/plans/2026-09-07-computer-use-optimization-milestones.md)

---

## 一、 优化背景与核心瓶颈

在对 `at-pc` 代码库的审查中发现，当前系统在面对双屏或多显示器受控端时，存在三大核心阻断点：

```
                                  [ 双屏/多屏客户端拓扑 ]
                                             │
      ┌──────────────────────────────────────┼──────────────────────────────────────┐
      ▼                                      ▼                                      ▼
【1. 拓扑感知缺失】                 【2. 虚拟桌面坐标映射 Bug】               【3. 控制台流与操作受限】
- 缺少 `list_monitors` 工具         - `input.rs` 除以主屏宽 `SM_CXSCREEN`     - `router.rs` 硬编码 `display_index: 0`
- AI 无法预知屏幕数量与排布         - 副屏点击产生百倍偏移/越界截断           - Dashboard 缺少多屏切换 UI
- 无法按屏幕名称/主副屏调度         - `CGDisplay::main()` 锁死主屏            - 无法在副屏实时推流与控屏
```

1. **显示器枚举能力缺失（No Discovery）**：
   - 整个 MCP 工具集缺少 `list_monitors` 工具，LLM 和外部自动化客户端完全无法获知当前被控 PC 是否接了双屏、各个屏幕的分辨率、DPI 缩放比、在虚拟桌面中的相对偏移坐标（`[x, y, width, height]`）以及主副屏标识。
2. **底层操作系统虚拟桌面坐标映射严重失真（Coordinate Calculation Bug）**：
   - **Windows**：[`crates/agent/src/input.rs`](file:///Users/clkj/项目/at/at-pc/crates/agent/src/input.rs) 中使用 `GetSystemMetrics(SM_CXSCREEN)` 换算归一化坐标。`SM_CXSCREEN` 仅代表主屏幕宽度，而非多屏虚拟桌面总宽 `SM_CXVIRTUALSCREEN`。当光标移动至副屏（如 $X \ge 1920$）时，除以主屏宽导致 `norm_x > 65535`，Windows API 直接截断或漂移；若副屏位于主屏左侧或上方（原点为负），更会直接发生坐标翻转。
   - **macOS**：在 `input.rs` 中直接调用 `CGDisplay::main().bounds()`，硬编码绑定主屏幕，导致所有归一化鼠标事件无法移出主显示器。
3. **MCP 鼠标控制类工具缺少屏幕上下文（Missing Display Parameter）**：
   - `mouse_click`、`mouse_move`、`mouse_drag`、`mouse_scroll` 参数仅有 `x, y, coord_mode`，没有 `display_index` 参数。当调用方基于副屏截图得到局部坐标（如 $(200, 300)$）发起点击时，系统因不知屏幕偏移，直接点击到主屏对应的 $(200, 300)$。
4. **Web 远程桌面控制台流控与交互受限（Single-Display Hardcoded）**：
   - 服务端路由 [`server/src/router.rs`](file:///Users/clkj/项目/at/at-pc/crates/server/src/router.rs) 的 `start_desktop_stream` 将 `display_index` 硬编码为 `0`；
   - API 请求体 `StartStreamRequest` 未暴露 `display_index` 字段；
   - Web 页面缺少“显示器选择切换”交互按钮，运维人员在浏览器端无法查看和操控副屏。

---

## 二、 里程碑阶段规划概览

| 里程碑 | 版本 | 核心目标 | 涉及模块 | 预计交付物 |
| :--- | :--- | :--- | :--- | :--- |
| **Milestone 1 (P0)** | `v0.7.0` | **多屏拓扑感知与底层坐标系重构** | `protocol`, `agent`, `server` | 修复 `input.rs` 虚拟多屏映射 Bug；新增 `list_monitors` MCP 工具；支持 `click_mark` 副屏精准点击。 |
| **Milestone 2 (P1)** | `v0.7.5` | **MCP 交互工具多屏语义增强** | `agent`, `server` | `mouse_click` 等工具支持 `display_index` 局部坐标自适应；`list_windows` 与 `get_ui_tree` 附带屏幕归属索引。 |
| **Milestone 3 (P2)** | `v0.8.0` | **Web 远程桌面双屏平滑切换与协同** | `server`, `frontend` | Web Dashboard 增加屏幕选择切换工具栏；推流 API 开放多屏参数；前端鼠标坐标与活动屏幕实时换算。 |
| **Milestone 4 (P3)** | `v1.0.0` | **双屏混合场景全链路端到端验收** | `tests` | 跨屏拖拽、不同 DPI 双屏混贴、负坐标副屏等边界条件全自动化测试覆盖。 |

---

## 三、 详细任务分解与技术方案

### Milestone 1 (P0): 多屏拓扑感知与底层坐标系重构 (v0.7.0)

> **目标**：彻底消灭多显示器物理坐标换算 Bug，新增显示器枚举协议与工具，使系统具备“多屏感知与全桌面精准落点”基础。

#### [P0-1] 协议层增加显示器模型定义 (`crates/protocol/src/models.rs`)
* **修改文件**：`crates/protocol/src/models.rs`
* **具体改动**：
  定义结构体 `MonitorInfo`：
  ```rust
  /// Display monitor specification and virtual desktop placement
  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  pub struct MonitorInfo {
      pub display_index: usize,
      pub name: String,
      pub is_primary: bool,
      pub x: i32,
      pub y: i32,
      pub width: u32,
      pub height: u32,
      pub scale_factor: f64,
  }
  ```
* **验收标准**：
  - `MonitorInfo` 结构体支持 JSON 序列化与反序列化，集成测试编译通过。

---

#### [P0-2] 修复 Windows 与 macOS 底层虚拟桌面输入模拟 (`crates/agent/src/input.rs`)
* **修改文件**：`crates/agent/src/input.rs`
* **问题现状**：
  - Windows: `GetSystemMetrics(SM_CXSCREEN)` 仅代表主屏幕宽高，导致副屏绝对像素在 `SendInput` 中计算出的 `norm_x` 严重超限；
  - macOS: `CGDisplay::main().bounds()` 将坐标强行锁定在主屏幕。
* **具体改动**：
  1. **Windows 平台**：
     引入虚拟屏幕常量：
     - `SM_XVIRTUALSCREEN` (虚拟桌面左上角 X，可为负数)
     - `SM_YVIRTUALSCREEN` (虚拟桌面左上角 Y，可为负数)
     - `SM_CXVIRTUALSCREEN` (虚拟桌面总宽度)
     - `SM_CYVIRTUALSCREEN` (虚拟桌面总高度)
     
     更新 `MouseMovePixel` 与 `MouseMove` 换算逻辑：
     ```rust
     #[cfg(windows)]
     {
         let vx = GetSystemMetrics(windows_sys::Win32::UI::WindowsAndMessaging::SM_XVIRTUALSCREEN);
         let vy = GetSystemMetrics(windows_sys::Win32::UI::WindowsAndMessaging::SM_YVIRTUALSCREEN);
         let vw = GetSystemMetrics(windows_sys::Win32::UI::WindowsAndMessaging::SM_CXVIRTUALSCREEN);
         let vh = GetSystemMetrics(windows_sys::Win32::UI::WindowsAndMessaging::SM_CYVIRTUALSCREEN);
         
         // 1. Direct hardware cursor position via SetCursorPos (receives virtual coordinates)
         SetCursorPos(px, py);
         
         // 2. Standard SendInput with MOUSEEVENTF_VIRTUALDESK normalization:
         // Normalized coordinates range 0..65535 mapped across the virtual desktop rectangle:
         let norm_x = if vw > 0 { (((px - vx) as i64 * 65535) / vw as i64) as i32 } else { px };
         let norm_y = if vh > 0 { (((py - vy) as i64 * 65535) / vh as i64) as i32 } else { py };
     }
     ```
  2. **macOS 平台**：
     - 使用 CoreGraphics 全局多屏坐标系，支持坐标跨越主屏边界并传递到副屏。
* **验收标准**：
  - 在主屏 1920x1080、副屏 1920x1080（水平排列于右侧）环境下，执行 `MouseMovePixel { x: 2500, y: 500 }`，光标精准落在副屏的对应位置，无抖动或跳回主屏现象。

---

#### [P0-3] 实现并暴露 `list_monitors` MCP 工具
* **修改文件**：
  - `crates/agent/src/tools/screen.rs`：新增 `list_monitors() -> Result<Vec<MonitorInfo>, String>`。
  - `crates/agent/src/tools/mod.rs`：注册 `list_monitors` 分发分支与元数据定义。
  - `crates/server/src/mcp/tools.rs`：添加 `list_monitors` MCP 工具定义。
  - `crates/server/src/router.rs`：权限分级为 `ToolPermission::ReadOnly`。
* **MCP 工具 Schema**：
  ```json
  {
    "name": "list_monitors",
    "description": "Lists all connected physical and virtual display monitors on the target terminal with their display index, name, primary flag, screen bounds (x, y, width, height), and DPI scale factor.",
    "inputSchema": {
      "type": "object",
      "properties": {
        "terminal_id": {
          "type": "string",
          "description": "Optional target terminal ID (defaults to active terminal)."
        }
      },
      "required": []
    }
  }
  ```
* **验收标准**：
  - 双屏机器上调用 `list_monitors` 返回 2 个屏幕对象，清晰包含各自的 `display_index: 0` 和 `display_index: 1`，且 `x, y, width, height` 与系统设置一致。

---

### Milestone 2 (P1): MCP 交互工具多屏语义增强 (v0.7.5)

> **目标**：使 AI Agent 能够结合 `display_index` 直接操作副屏，自动完成屏幕局部坐标向系统全局坐标的变换。

#### [P1-1] 鼠标类工具支持 `display_index` 相对定位
* **修改文件**：
  - `crates/agent/src/tools/computer_use.rs`
  - `crates/agent/src/tools/mod.rs`
  - `crates/server/src/mcp/tools.rs`
* **具体改动**：
  1. 为 `mouse_click`、`mouse_move`、`mouse_drag`、`mouse_scroll` 增加可选参数：
     `"display_index": { "type": "integer", "description": "Optional zero-based display index. If provided, coordinates (x, y) are treated as local pixels relative to that monitor." }`
  2. 坐标自适应转换：
     若提供了 `display_index`，查询对应显示器的原点 `[mon_x, mon_y]`：
     $$\text{global\_x} = \text{mon\_x} + x, \quad \text{global\_y} = \text{mon\_y} + y$$
     自动将其转为全局虚拟桌面物理像素，彻底免去大模型手动累加屏幕偏移的心智负担。
* **验收标准**：
  - 调用 `mouse_click(display_index=1, x=100, y=100)` 时，光标准确点击在副屏左上角往内 100px 处。

---

#### [P1-2] 窗口管理与 UIA 树增加屏幕归属标记
* **修改文件**：
  - `crates/protocol/src/models.rs`：
    - `WindowInfo` 结构体新增 `pub display_index: Option<usize>`。
    - `UiTreeResponse` 结构体新增 `pub display_index: Option<usize>`。
  - `crates/agent/src/tools/window.rs`：
    - 在 `list_windows` 收集窗口时，获取当前显示器列表，根据窗口几何中心点计算落在哪个显示器的矩形范围内，填充 `display_index`。
  - `crates/agent/src/tools/uia.rs`：
    - 填充活动窗口对应的 `display_index`。
    - 修复非 Windows 平台的兜底代码，避免写死 `monitors.first()`。
* **验收标准**：
  - 调用 `list_windows` 返回的数据中，位于副屏的记事本窗口明确标记为 `"display_index": 1`。

---

### Milestone 3 (P2): Web 远程桌面双屏平滑切换与协同 (v0.8.0)

> **目标**：运维人员在 Web 浏览器控制台能够一键切换主/副屏流，且鼠标在所选屏幕上的点击 100% 精准映射。

#### [P2-1] 推流 API 与服务端路由打通 `display_index`
* **修改文件**：
  - `crates/server/src/mcp/dashboard.rs`：
    ```rust
    #[derive(Deserialize)]
    struct StartStreamRequest {
        fps: Option<u32>,
        quality: Option<u8>,
        display_index: Option<u32>, // <-- 新增字段
    }
    ```
  - `crates/server/src/router.rs`：
    `start_desktop_stream(terminal_id, fps, quality, display_index)` 透传 `display_index`，移除写死的 `0`。
  - `GET /api/terminals/:id/desktop/frame` 返回头部与 JSON 中增加 `display_index`。

---

#### [P2-2] Web Dashboard 前端界面增加多屏切换 UI
* **修改文件**：`crates/server/frontend/index.html`
* **具体改动**：
  1. 在 Remote Desktop HUD 工具栏增加“显示器切换”按键组：
     ```html
     <div class="rd-hud-section" id="rdMonitorsSection">
       <span style="color: var(--text-dim); font-size: 11px;">显示器:</span>
       <div id="rdMonitorButtons" class="btn-group"></div>
     </div>
     ```
  2. 建立远程桌面连接时，通过 `/api/terminals/:id/invoke` 静默拉取 `list_monitors`；
  3. 若检测到多屏幕，动态渲染 `[🖥️ 屏幕 0 (主屏)]`、`[🖥️ 屏幕 1]` 标签按钮；
  4. 点击切换屏幕时：
     - 发送带新 `display_index` 的 POST 流请求；
     - 平滑重置 Canvas 渲染宽高。
* **验收标准**：
  - 点击“屏幕 1”按钮后，网页控制台在 200ms 内平滑切换为副屏画面。

---

#### [P2-3] Web 前端鼠标坐标自适应注入
* **修改文件**：`crates/server/frontend/index.html` 与 `crates/agent/src/input.rs`
* **具体改动**：
  - 前端捕获鼠标事件时，附带当前正在推流的 `currentDisplayIndex`；
  - Agent 收到后，结合当前屏幕的位置与分辨率，将 Canvas 相对百分比换算到该显示器的虚拟桌面坐标，杜绝跨屏偏位。
* **验收标准**：
  - 在副屏画面中点击任意按钮，受控端副屏上的按钮被正确触发。

---

### Milestone 4 (P3): 双屏全链路端到端自动化测试与全景诊断 (v1.0.0)

> **目标**：构建完备的多屏自动化测试用例，保证单屏、双屏横排、双屏竖排、高分缩放等场景长期稳定。

#### [P3-1] Agent 虚拟多屏单元测试套件
* **新增测试文件**：`crates/agent/tests/multi_monitor_test.rs`
* **覆盖场景**：
  - 模拟双屏拓扑：
    - Case 1: 主屏 1920x1080 (0,0)，副屏 1920x1080 (1920, 0)；
    - Case 2: 主屏 2560x1440 (0,0)，副屏 1080x1920 竖屏 (-1080, 0)（负坐标原点）；
    - Case 3: 4K 200% 缩放 + 1080P 100% 缩放双屏混贴。
  - 验证：
    - 针对各屏幕执行局部 `(x, y)` 转换到全局坐标的数学正确性；
    - 坐标归一化反算不超过 `65535` 且不为负数。

#### [P3-2] Server 端到端 MCP 流程回归
* **测试文件**：`crates/server/tests/e2e_multi_monitor_test.rs`
* **验证流程**：
  `list_monitors` $\rightarrow$ `capture_screen(display_index=1)` $\rightarrow$ `get_marked_screen(display_index=1)` $\rightarrow$ `click_mark` 全流程闭环测试。

---

## 四、 风险评估与应对策略

| 风险项 | 严重级 | 应对策略 |
| :--- | :---: | :--- |
| **Windows DPI 缩放不一致（如主屏 150%，副屏 100%）** | 高 | 在 Agent 启动时调用 `SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)`，确保跨屏捕获与光标注入物理像素点对齐。 |
| **单屏旧客户端兼容性** | 中 | 所有新增参数（`display_index` 等）全部设为 `Option`，缺失时默认 fallback 到 `0`（主屏），对现有单屏客户端行为 100% 向后兼容。 |
| **Web 切换屏幕流延迟** | 低 | 采用 generation 计数器重置后台推流 task，复用既有 WebSocket 连接，切换过程无黑屏卡顿。 |
