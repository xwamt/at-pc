# at-pc Computer-Use 进阶演进与落地计划：从“视觉盲打”迈向“双模态控件树 + 终端优先”架构

> **版本**：v1.0  
> **制定日期**：2026-09-07  
> **适用模块**：`crates/agent`、`crates/server`、`crates/protocol`  
> **设计思想**：借鉴 OpenAI GPT-6 Astra 与微软 UFO 体系，将桌面端操作从“全屏截图 + VLM 猜坐标”的粗放模式，升级为“系统可访问性控件树（类似浏览器 DOM）为主、终端 API 优先、视觉 ROI 为辅”的现代化 OS-Agent 架构。

---

## 一、 演进愿景与核心痛点对齐

当前 `at-pc` 的 `computer_use` 工具链路采用典型的“全量截图 -> 坐标预测 -> 硬件模拟”流派，在实际落地中存在四大核心痛点：

1. **Token 消耗惊人**：每次交互需调用 `capture_screen` 抓取全分辨率（1080P/2K/4K）大图，单张图消耗 1500~3000+ 视觉 Token，多轮交互极易击穿上下文窗口并带来高昂 API 成本。
2. **坐标系统存在严重歧义 Bug**：协议定义为 `0-65535` 归一化坐标，而主流大模型（如 Claude 3.5 / GPT-6）天然习惯输出物理像素坐标。在 `input.rs` 中按 `65535` 折算后直接产生百倍偏移，导致点击严重漂移至屏幕边缘。
3. **缺乏状态闭环（Blind Fire）**：`mouse_click` 点击后不管界面是否响应、窗口是否失去焦点，大模型只能盲目反复截图确认，产生冗余轮次。
4. **忽视系统原生能力**：大模型倾向于用鼠标去“双击控制面板、点选菜单”，未充分发挥 `at-pc` 已经具备的 `exec_powershell`、`manage_service` 等秒级精准工具的威力。

### 架构演进全景图

```
                           [ AI Agent 用户指令 ]
                                     │
      ┌──────────────────────────────┴──────────────────────────────┐
      ▼                                                             ▼
【Tier 1: 终端/API 优先】                                    【Tier 2: 原生 UI 控件树 (类似 DOM)】
- exec_powershell / exec_cmd                                - get_ui_tree (获取窗口结构化控件)
- manage_service / kill_process                              - click_element (通过 ID 语义点击,无需算像素)
- read_text_file / write_text_file                           - set_element_text (直接设置文本,防漏字)
(Token 消耗: < 150, 准确率: 100%)                            - focus_window (激活目标窗口)
                                                            (Token 消耗: 200~500, 准确率: 100%)
                                                                    │
                                                              遇到自绘/Canvas/特殊界面
                                                                    ▼
                                                            【Tier 3: 视觉 ROI 与 SoM 打标兜底】
                                                            - capture_screen (支持裁剪/缩放/局部ROI)
                                                            - Set-of-Mark (画框编号让模型选号)
                                                            (Token 消耗比原先降低 80%)
```

---

## 二、 里程碑阶段规划概览

| 阶段 | 版本 | 核心目标 | 预期成果 | 预计周期 |
| :--- | :--- | :--- | :--- | :--- |
| **Milestone 1 (P0)** | `v0.4.1` | **坐标系统修复与视觉截流防爆** | 消除点偏 Bug，截图支持自适应缩放与 ROI 裁剪，树立终端优先 Prompt。 | 3 天 |
| **Milestone 2 (P1)** | `v0.5.0` | **引入桌面“DOM 树”：UIA 控件树操作** | 实现 `get_ui_tree`、`click_element`、`set_element_text`，实现标准应用无视觉操作。 | 5 ~ 7 天 |
| **Milestone 3 (P2)** | `v0.6.0` | **窗口生命周期与闭环自校验** | 实现窗口焦点控制与动作前后 State Review，杜绝盲点。 | 4 ~ 5 天 |
| **Milestone 4 (P3)** | `v1.0.0` | **混合标注 (Set-of-Mark) 与非无障碍自绘兜底** | 针对 Canvas/游戏/自绘 UI，实现本地边缘检测与角标图生成。 | 5 天 |

---

## 三、 详细任务分解与技术方案

### Milestone 1 (P0): 坐标系统修复与视觉截流防爆 (v0.4.1)

> **目标**：以最小代价修复当前代码中导致“点偏”的核心 Bug，并大幅降低单次截图的 Token 消耗，立即见效。

#### [P0-1] 坐标输入体系重构与像素/归一化双模自适应
* **问题现状**：
  * [`crates/server/src/mcp/tools.rs`](file:///Users/clkj/项目/at/at-pc/crates/server/src/mcp/tools.rs#L518) 强行要求 `0-65535` 归一化。
  * [`crates/agent/src/input.rs`](file:///Users/clkj/项目/at/at-pc/crates/agent/src/input.rs#L17-L28) 按 `px = (norm_x * screen_w) / 65535` 计算。当模型传入物理像素（如 `1280`）时，算出的像素仅为 `37px`。
* **改动位置**：`crates/server/src/mcp/tools.rs`、`crates/agent/src/input.rs`、`crates/protocol/src/models.rs`
* **具体实施**：
  1. 在 `mouse_click`、`mouse_move`、`mouse_drag` 中增加 `coord_mode` 参数，枚举支持：`"pixel"`（默认物理像素）与 `"normalized"`（0~1000 或 0~65535）。
  2. 智能探测启发式逻辑：在 `input.rs` 中，若未指定 `coord_mode`，检查传入的数值：若坐标大于 1 且未超过主屏宽高，直接作为真实像素坐标处理；
  3. 修复 Windows 下高分屏 DPI Scale 偏差：调用 `GetDpiForSystem()` / `SetProcessDpiAwarenessContext()`，确保 `SetCursorPos` 和截图的像素对齐。
* **验收标准**：
  - 发送 `{ "x": 1280, "y": 720, "coord_mode": "pixel" }` 时，鼠标光标精准落于 1080P 屏幕的对应物理像素点，误差为 0。

#### [P0-2] `capture_screen` 分辨率智能降采样与 ROI 局部裁剪
* **问题现状**：全屏捕获未经处理直接 Base64 编码，2K/4K 截屏单次吃掉 3000+ tokens。
* **改动位置**：`crates/agent/src/tools/screen.rs`、`crates/server/src/mcp/tools.rs`
* **具体实施**：
  1. 在 `capture_screen` 参数中增加：
     - `max_dimension: Option<u32>`（默认限制最长边不超过 1280px，维持宽高比降采样）；
     - `crop: Option<{ x: u32, y: u32, width: u32, height: u32 }>`（支持仅截取弹窗或局部区域）；
  2. 使用 `image::imageops::resize` 进行快速双线性插值缩放；
  3. 在返回的 JSON 中补充原始分辨率 `original_width/height` 与缩放比例 `scale_factor`，便于大模型计算相对坐标。
* **验收标准**：
  - 4K 分辨率截图经过 `max_dimension=1280` 处理后，Base64 字符量下降 75% 以上，模型单次视觉 Token 消耗从 ~3200 降低至 ~800。

#### [P0-3] MCP System Prompt 引导规范：确立“终端优先”分层准则
* **改动位置**：`crates/server/src/mcp/mod.rs`（MCP `prompts/list` 与 Tools Descriptions）
* **具体实施**：
  1. 优化各工具的描述文字，明确告知大模型：
     - 查询状态/查进程/改配置/启停服务，**严禁使用鼠标截图操作**，必须优先使用 `get_system_overview`、`list_processes`、`manage_service`、`exec_powershell`；
     - 只有在必须与独有 GUI 软件交互时才调用图形工具。
* **验收标准**：
  - 常见 IT 运维场景（如“重启打印机服务”）下，大模型不再发起 `capture_screen`，直接命中 `manage_service(Spooler, restart)`。

---

### Milestone 2 (P1): 引入桌面“DOM 树”：UIA 控件树操作 (v0.5.0)

> **目标**：彻底告别纯视觉盲猜，在 Windows 系统上实现对标浏览器 DOM 的原生控件树操作体系。

#### [P1-1] 引入 Windows UI Automation (UIA) 采集器
* **涉及模块**：`crates/agent/src/tools/uia/`（新建模块）
* **依赖引入**：`windows` crate 启用 `Win32_UI_Accessibility` 与 `Win32_UI_WindowsAndMessaging`。
* **技术实现**：
  1. 初始化 `IUIAutomation` COM 实例；
  2. 获取当前前台活动窗口 `GetForegroundWindow()` 对应的 `IUIAutomationElement`；
  3. 树形遍历算法与**剪枝策略（Tree Pruning）**：
     - 过滤规则：剔除不可见（`IsOffscreen == true`）、空尺寸、无名称无文本的纯布局容器；
     - 捕获目标：`Button`, `Edit`, `MenuItem`, `CheckBox`, `RadioButton`, `ComboBox`, `TabItem`, `ListItem`, `Text`；
  4. 生成扁平化索引清单，每个控件赋予会话内唯一数字 ID（如 `#1`, `#2`）。

#### [P1-2] 新增 MCP 工具 `get_ui_tree`
* **Schema 定义**：
  ```json
  {
    "name": "get_ui_tree",
    "description": "Returns a structured, pruned interactive UI element tree (like a browser DOM) for the active window. Elements include unique IDs, names, control types, bounding rects, and current values.",
    "inputSchema": {
      "type": "object",
      "properties": {
        "depth": { "type": "integer", "description": "Maximum tree depth to traverse (default: 5)" },
        "window_title": { "type": "string", "description": "Optional title filter to target a specific window" }
      }
    }
  }
  ```
* **输出数据结构**：
  ```json
  {
    "active_window": "打印机和扫描仪 - 设置",
    "window_bounds": [200, 150, 960, 640],
    "elements": [
      { "id": 1, "type": "Button", "name": "添加设备", "rect": [250, 210, 120, 36], "enabled": true },
      { "id": 2, "type": "Edit", "name": "搜索设置", "value": "", "rect": [500, 180, 200, 32] },
      { "id": 3, "type": "ListItem", "name": "HP Color LaserJet Pro", "rect": [250, 280, 400, 48] }
    ]
  }
  ```
* **收益**：几百个文本字符完整描述界面，消耗仅 **100~200 Tokens**（对比截图省 95%）。

#### [P1-3] 新增语义操作工具 `click_element` 与 `set_element_text`
* **Schema 定义**：
  - `click_element(element_id: integer, action_type: "invoke" | "click")`
  - `set_element_text(element_id: integer, text: string)`
* **底层实现**：
  1. `click_element`：
     - 查找 ID 对应的 UIA 元素缓存；
     - 优先获取 `IUIAutomationInvokePattern` 并调用 `Invoke()`，**零鼠标位移、后台直接激活**；
     - 若不支持 Invoke，自动取其 `BoundingRectangle` 中心点，驱动 `SendInput` 触发物理点击。
  2. `set_element_text`：
     - 获取 `IUIAutomationValuePattern`，调用 `SetValue(text)`；
     - 解决传统打字工具输入法干扰、漏字符与焦点漂移问题。
* **验收标准**：
  - 在 Windows 记事本/计算器/系统设置中，模型通过 `get_ui_tree` 获取元素并调用 `click_element`，准确率 100%，全程不消耗任何图片 Token。

---

### Milestone 3 (P2): 窗口生命周期与闭环自校验 (v0.6.0)

> **目标**：赋予 Agent 完整的窗口焦点控制能力，并在动作执行后自动完成状态 Review，杜绝盲点。

#### [P2-1] 窗口管理套件 (`list_windows`, `focus_window`, `close_window`)
* **改动位置**：`crates/agent/src/tools/window.rs`
* **具体实施**：
  1. `list_windows`：使用 `EnumWindows` 遍历所有顶层可见窗口，输出 `{ hwnd, pid, title, process_name, is_minimized, is_foreground, rect }`；
  2. `focus_window`：入参支持 `process_name`、`pid` 或 `title`。通过 `ShowWindow(SW_RESTORE)` 与 `SetForegroundWindow()` 强制将窗口带到最前并激活；
  3. `close_window`：向窗口发送 `WM_CLOSE` 信号实现优雅退出。
* **验收标准**：
  - 目标应用被最小化或被其他应用完全遮挡时，调用 `focus_window` 可在 50ms 内将其弹至前台并置顶。

#### [P2-2] 动作执行闭环校验机制 (Review Loop)
* **改动位置**：`crates/agent/src/tools/computer_use.rs`
* **具体实施**：
  1. 在执行 `click_element`、`mouse_click`、`press_key` 之后，内部执行轻量级等待（默认 80ms）；
  2. 自动检查 UI 状态变动（State Diff）：
     - 焦点窗口是否切换？
     - 是否生成了新的模态弹窗或新子窗口？
     - 控件内容或选中状态是否变化？
  3. 在工具调用返回值中附加 `state_diff`：
     ```json
     {
       "success": true,
       "action": "click_element",
       "state_diff": {
         "foreground_window": "用户账户控制",
         "new_window_detected": true
       }
     }
     ```
* **收益**：大模型无需在每次点击后发起额外的截图请求，直接在返回结果中得知界面流转情况。

---

### Milestone 4 (P3): 混合标注 (Set-of-Mark) 与自绘应用兜底 (v1.0.0)

> **目标**：解决 Electron、自绘 Canvas、未暴露 UIA 接口的旧版桌面程序交互难题。

#### [P3-1] Agent 端轻量级 Set-of-Mark (SoM) 图像标注生成器
* **涉及技术**：纯 Rust 图像处理（`imageproc`）
* **具体实施**：
  1. 当 `get_ui_tree` 返回的元素较少（表明应用未完全暴露无障碍接口）或大模型显式请求视觉标注时；
  2. Agent 截取当前屏幕，并在每个可识别的控件/轮廓边界上绘制半透明矩形边框与醒目的高亮数字标号（`#1`, `#2`, `#3`）；
  3. 将打标后的轻量级图片（压缩至 ~500KB）返回给模型，同时在 Agent 内存维护 `{ "#1": (x1, y1), "#2": (x2, y2) }` 的坐标映射表。
* **验收标准**：
  - 模型直接下发指令 `click_mark(id=2)`，Agent 查找字典并点击，避免模型直接输出连续浮点像素坐标引起的计算误差。

---

## 四、 跨平台兼容性演进矩阵

虽然 `at-pc` 首要解决 Windows 办公机运维，但架构设计保持跨平台扩展性：

| 功能层 | Windows (首要目标) | macOS | Linux |
| :--- | :--- | :--- | :--- |
| **UI 控件树** | Windows UI Automation (COM) | Accessibility API (`AXUIElement`) | AT-SPI2 (D-Bus) |
| **输入模拟** | Win32 `SendInput` (已支持) | CoreGraphics `CGEventPost` (已支持) | X11 `XTest` / Wayland |
| **窗口管理** | `EnumWindows`, `SetForegroundWindow` | `NSWorkspace`, `CGWindowListCopyWindowInfo` | Xlib `_NET_ACTIVE_WINDOW` |
| **屏幕捕获** | `xcap` DXGI / GDI (已支持) | `xcap` ScreenCaptureKit (已支持) | `xcap` PipeWire / X11 (已支持) |

---

## 五、 优化前后关键指标对比 (ROI)

| 核心指标 | 当前基线 (v0.3.x 视觉模式) | Milestone 1 (P0 完成后) | Milestone 2-4 (全量完成后) |
| :--- | :--- | :--- | :--- |
| **单次交互 Token 消耗** | 2,500 ~ 3,500 tokens | 600 ~ 900 tokens (降采样/ROI) | **100 ~ 250 tokens (DOM树模式)** |
| **任务定位准确率** | ~65% - 75% (常因缩放/归一化点偏) | 85% (修复坐标与缩放) | **99%+ (原生 UIA / 语义点击)** |
| **多轮诊断平均耗时** | 30 ~ 60 秒 (传输/多模态推理慢) | 15 ~ 25 秒 | **3 ~ 8 秒 (秒级响应)** |
| **每百次操作 API 成本** | ~$1.50 - $2.50 | ~$0.40 - $0.60 | **~$0.05 - $0.10 (下降 95%)** |
| **操作黑盒度** | 纯盲点，无反馈 | 有日志输出 | **带 State Review 与窗口感知** |

---

## 六、 实施与推进建议

1. **第一周（即刻启动 P0）**：
   - 重点突击 `[P0-1] 坐标修复` 与 `[P0-2] capture_screen 降采样/裁剪`，并在本地测试套件中固化坐标回归用例；
   - 快速消除用户最直观的“点不准”与“刷 Token”痛点。
2. **第二至三周（攻坚 P1）**：
   - 搭建 `crates/agent/src/tools/uia/` 原型，完成 UIA 基础封装与剪枝算法；
   - 跑通 Windows 常用应用（记事本、服务管理器、系统设置）的纯文本 DOM 树提取与点击验证。
3. **第四周（P2 完善与发布）**：
   - 上线窗口管理与操作自校验（Review Loop），正式发布具备“双模态能力”的 `at-pc v0.6.0`。
