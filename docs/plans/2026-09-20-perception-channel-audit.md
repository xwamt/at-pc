# at-pc 客户端感知通道审计 —— 为什么实际落地的是截图

- 日期：2026-09-20
- 版本：`at-pc-server` / `at-pc-agent` v1.0.0，HEAD `8a638be`
- 被测客户端：`laptop-roeh3p85-b6a918`（自定义名 **备用Windows**），Windows 11 26100，屏幕 2880×1800，`scale_factor = 2.0`
- 触发场景：远程"打开 Lark → 写祝福诗 → 发给 Jack Zhu（祝飞）"，实现过程中**全程依赖 `capture_screen` + Pillow 量像素**
- 结论口径：以下所有数字均为**本机实测**（`POST http://192.168.99.94:9800/mcp`），代码引用均指到 `文件:行`

---

## 0. 一句话结论

**不是"没实现更好的通道"，而是三条链路分别在三个不同的层被切断：**

1. **策略层**：3 层优先级策略写死在 `prompts/get`，**没有通过 MCP `initialize.instructions` 下发，也不在 `tools/list` 里** ⇒ 客户端（LLM）根本看不见"截图是 Tier 3 最后手段"这条规则。
2. **能力层**：Tier 2（`get_ui_tree` / `click_element`）对**本次目标应用类型（Electron/Chromium）结构性失效**——Lark 的 UIA 树只有 6 个 `Pane`，全是整窗同尺寸容器，`query="Jack"` 命中 0。
3. **投影层（最致命）**：唯一"为视觉交互设计"的通道 `get_marked_screen` 的**坐标映射在 server 侧被丢弃**——agent 明明算了 `marks`（含 `rect`/`center`/`label`），server 只回"一句文本摘要 + 一张图"。于是它比 `capture_screen` **只贵不省**：Token 一样、语义为零，仍然必须用眼睛读徽标数字。

叠加两个放大器：**① `capture_screen/get_marked_screen` 无法"只落盘不回传图片"**（即使传了 `save_path`，18.7 万字节 base64 照样进上下文）；**② 我的技能文件把"截图+Pillow 量坐标"写成了推荐做法，并残留了一条已失效的负面结论**（"`list_monitors` 不可用"），形成自我强化。

---

## 1. 通道清单（代码定位 + 实测返回）

"感知客户端"共 8 个只读 + 4 个动作通道，全部来自 `crates/protocol/src/tools.rs` 的 33 个 `AgentToolKind`（`:38-72`）。按"是否需要视觉"分类：

| # | 通道 | 工具 | 返回形态 | 需要读图 | 本次实测 |
|:--|:--|:--|:--|:--|:--|
| 1 | 系统数据 | `get_system_overview` | 纯文本 | ✗ | 1,445 B |
| 2 | 进程/服务/日志/网络 | `list_processes` / `manage_service` / `get_event_logs` / `list_network_connections` / `test_network` | 纯文本 | ✗ | — |
| 3 | 文件 | `list_directory` / `search_files` / `read_text_file` | 纯文本 | ✗ | — |
| 4 | **窗口元数据** | `list_windows` | 纯文本：title / process / pid / **rect** / minimized / **foreground** | ✗ | **3,262 B**，11 窗 |
| 5 | **显示器元数据** | `list_monitors` | 纯文本：width / height / x,y / **scale_factor** | ✗ | **312 B**，`2880×1800, scale 2.0` |
| 6 | **结构化 UI 树** | `get_ui_tree`（支持 `query` 语义检索 / `compact` 裁剪） | 纯文本：`type`/`name`/`value`/`rect`/`enabled` | ✗ | Lark **6 元素**；桌面 **19 元素**；`设置` **0 元素** |
| 7 | **语义动作** | `click_element` / `set_element_text` / `batch_actions`（支持 `element_id` 步骤，`crates/agent/src/tools/batch.rs:29-44,73-100`） | 文本 + 可选 UI 状态 diff | ✗ | 依赖 #6 |
| 8 | **画面** | `capture_screen`（`format`/`quality`/`crop`/`max_dimension`/`save_path`） | **文本摘要 + `image` 块** | ✓ | 1280 档 **187,711 B** |
| 9 | **画面+标注** | `get_marked_screen`（`strategy` ∈ auto/ui_tree/grid/contours/hybrid） | **文本摘要 + `image` 块**（`marks` 被丢） | ✓ | 147,710 ~ 204,726 B，31 个标记 |
| 10 | **标记点击** | `click_mark` / `mouse_click(mark_id=N)` | 文本 | 否* | `crates/agent/src/tools/som.rs:1194` |

\* 名义上不需要读图，但 `mark_id → 控件语义`的对照表不在返回体里，**不读图就不知道 #N 是什么**。

> 没有 OCR、没有 accessibility 文本抽取：全仓检索 `ocr|tesseract` 仅命中 `CoCreateInstance` 的误报。
> 也就是说，对"UIA 覆盖不到"的应用，**除截图外不存在任何非视觉通道**。

---

## 2. 实测原始数据

```
=== get_ui_tree {window_title:Lark, compact:true, depth:3} ===
  elapsed=89ms  raw_json=1,123B  content=[text(892)]
  total_elements = 4  window_bounds = [194, 78, 2634, 1502]
   id=1 type=? name='Lark'                rect=[208, 78, 2608, 1490]
   id=2 type=? name='ContentsView'        rect=[208, 78, 2608, 1490]
   id=3 type=? name='NonClientFrameView'  rect=[208, 78, 2608, 1490]
   id=4 type=? name='WatermarkWidget'     rect=[208, 78, 2610, 1490]

=== get_ui_tree {window_title:Lark, depth:5}  → 6 元素，全部 "type":"Pane"，rect 全等 ===
   Lark / ContentsView / NonClientFrameView / ClientView / MainWidgetDelegateView / WatermarkWidget
=== get_ui_tree {window_title:Lark, query:"Jack"} → total=8  query_matched=0  返回 0 ===
   （屏幕上明明写着 "Jack Zhu（祝飞）"，但文本从未进入 UIA 树）

=== get_ui_tree {window_title:Program Manager, depth:2} → 19 元素 ===
   '桌面'/[0,0,2880,1800]  '回收站'/[0,5,150,134]  'ToDesk'  'WPS Office'  'QQ音乐'  '年会视频' ...
   raw_json=4,343B   ← 原生 Win32 覆盖良好、且极省

=== get_ui_tree {window_title:设置, depth:8} → total_elements = 0 ===
=== get_ui_tree {window_title:Realtek Audio Console} → total_elements = 0 ===
=== get_ui_tree {window_title:下载-文件资源管理器(已最小化)} → 6 元素，rect 全在 -32000 ===
   id=23 name='系统' rect=[-31987,-31987,44,44]   ← 最小化窗口泄漏屏幕外坐标

=== list_monitors ===  312B  [{"width":2880,"height":1800,"scale_factor":2.0,"is_primary":true}]
=== list_windows {only_visible:true} ===  3,262B
=== get_marked_screen {strategy:ui_tree, max_dimension:1280} ===  147,710B  text(113) + image(147,440)
=== get_marked_screen {strategy:auto,    max_dimension:1280} ===  204,726B  text(113) + image(204,456)
      has 'marks' in payload?  False      has 'total_marks'?  False
=== capture_screen {max_dimension:1280, save_path:...} ===  187,711B  text(157) + image(187,396)
      客户端磁盘确有落盘：C:\Users\CLKJ\Downloads\probe.jpg = 140,545 bytes（16:03:39）
      ⇒ 落盘成功，但图片**仍然回传**，省 Token 的目的没达成
```

SoM `auto` 在 Lark 上实际标了 31 个框（见 `-assets/som-auto.jpg`）：会话列表**每一行**被整框标出（#5/#7/#14/#18/#22/#23/#26/#27/#29），聊天区段落被切成碎片框（#11/#13/#16/#20/#24），右侧工具栏图标 #9/#10，输入框所在行被 #29 覆盖。
—— 也就是说，**SoM 在本次任务里其实"够用"**（点 #7 就是点会话行），但它把结果**只画在图上**，所以用起来仍然等价于截图。

---

## 3. 根因分析：三条链路各自断在哪一层

### 根因 A ｜ 策略只存在于 `prompts/get`，客户端看不见 —— 代码定位 `crates/server/src/mcp/mod.rs:513-591`

`initialize` 的返回体是：

```rust
"initialize" => {
    let res = json!({
        "protocolVersion": "2024-11-05",
        "capabilities": { "tools": {...}, "prompts": {...} },
        "serverInfo": { "name": "at-pc-server", "version": ... }
    });   // ← 没有 "instructions" 字段
```

MCP 规范里"想无条件告诉客户端怎么用我"的标准位置就是 `initialize.instructions`。而这份仓库的 3 层策略被写成了 `prompts/get("desktop_automation_strategy")`：

> `mcp/mod.rs:557-571`
> ```
> 1. Tier 1 (Terminal / API First - 100% Accuracy, Zero Vision Tokens)
> 2. Tier 2 (Structured UI Tree / DOM - Token-Efficient, Deterministic Semantic Interaction)
> 3. Tier 3 (Visual ROI & Set-of-Mark Fallback - Use Only When Needed)
>    - Only call `capture_screen` or `get_marked_screen` when interacting with
>      custom-drawn Canvas, games, or legacy non-accessible windows.
>    - Always supply `crop` or `max_dimension` ...
> ```
> 这段写得非常准确——**问题在于它是"要你主动来问"的**。

实测确认：`initialize` 无 `instructions`；`prompts/list` 只回一个名字，客户端必须先知道要调 `prompts/get`。而工具注册层（WorkBuddy 连接器 / 原生 MCP 工具面）**只把 tools 暴露成工具**，prompt 不会自动注入上下文 ⇒ 我这个 LLM 从头到尾没看到过这段策略。

**这是"为什么一直用截图"的第一因：唯一的优先级声明是隐形的。**

### 根因 B ｜ 工具描述里没有任何优先级信号，而截图那条的描述最"诱人"

`tools/list` 是客户端唯一必然读取的东西，但 `crates/protocol/src/tools.rs` 里：

| 工具 | 描述（`:311 / :295 / :318`） | 是否提示"先用我 / 别乱用截图" |
|:--|:--|:--|
| `get_ui_tree` | "Returns a filtered interactive UI element tree for a window." | ❌ 无 |
| `capture_screen` | "Captures a monitor screenshot with JPEG/PNG output, **downsampling, cropping, and optional Agent-side saving**; results include raw/image Base64 and a browser-ready data URI." | ❌ 无，且**功能列表最长** |
| `get_marked_screen` | "…**numbered UI regions**, image Base64/data URI output, **and a mark-to-coordinate mapping**." | ⚠️ 反向误导：承诺了 `mark-to-coordinate mapping`，但（见根因 C）**根本没返回** |

`click_element` 是唯一有链路提示的（"by the ID returned from `get_ui_tree`"）。所以从纯 schema 角度看，**"截图 + 标注 + 坐标映射"看上去是能力最强、最省心的通道** —— 实际却相反。

### 根因 C ｜ `get_marked_screen` 的坐标映射在 server 投影层被丢弃（**核心缺陷**）

数据在 agent 侧是完整的。`crates/protocol/src/models.rs:163-184`：

```rust
pub struct MarkedScreenResponse {
    pub marks: Vec<ScreenMark>,   // id / rect[x,y,w,h] / center[x,y] / label / control_type
    pub total_marks: usize,
    pub source: String,           // "ui_tree" | "grid" | "contours" | "hybrid"
    pub scale_factor: Option<f32>,
    ...
}
```

agent 也确实填满了它（`crates/agent/src/tools/som.rs:1031-1041` 重排 1..N 号、`:1044` 写进 mark 缓存、`:1088-1105` 组装响应），并且 `dispatch` 原样返回整个 JSON（`crates/agent/src/tools/dispatch/screen.rs:87-97`）。

**但 server 把它扁平成一句话**（`crates/server/src/mcp/mod.rs:735-753`）：

```rust
let text_summary = format!(
    "Screenshot captured successfully: display {}, resolution {}x{}{}{}, format: {}.{}{}",
    display_idx, width, height, scale_note, crop_note, mime_type, file_path_note, server_saved_note
);   // ← marks / total_marks / source / scale_factor 一个都没进
Ok(json!({ "content": [ {"type":"text","text":text_summary},
                        {"type":"image","data":clean_b64,"mimeType":mime_type} ], "isError": false }))
```

实测印证：`get_marked_screen` 的返回体里搜不到 `marks` 也搜不到 `total_marks`。

**后果（这就是"最浪费 token 且最不准确"的机理）**：
- **不省 Token**：图片块照付（147–205 KB 线上体积 ≈ 1280×800 的视觉 Token），与 `capture_screen` 同价；
- **更不准确**：`capture_screen` 至少是"原图 + 我自己量"；`get_marked_screen` 是"覆盖了彩色边框和编号徽标的图"，徽标会**压住内容**（见 `som-auto.jpg`：`#29/#30` 的徽标就压在输入框和工具栏上），量像素之前还得先把标注当噪声排除；
- **`click_mark` 变成猜谜**：`som.rs:1194-1215` 用 `mark.center` 点击，坐标在 agent 内存里是准的，但**模型不知道 #7 是"Jack Zhu 那一行"还是"聊天区某段文字"**，只能回图上去读徽标 → 又回到读图。

顺带两个同层缺陷：
- **`window_title` 只用于取元素，不裁剪图片**（`som.rs:815-853`：`window_title` 仅参与 `ui_tree/hybrid/auto` 分支取 UIA 元素；图片只受 `crop` 参数影响）⇒ 想看某个窗口，代价仍是全屏图。
- **`scale_factor` 被丢**，导致模型不知道 2880×1800 是 200% 缩放下的物理像素（`list_monitors` 才回 2.0）⇒ "坐标到底该写哪个数"全靠试。

### 根因 D ｜ Tier 2 对本次目标应用（Electron）结构性失效

`crates/agent/src/tools/uia.rs` 的实现是**真 UIA**（`CoCreateInstance(&CUIAutomation)` `:392-394`、`ElementFromHandle` `:422-424`、递归 `FindAll(TreeScope_Children, TrueCondition)` `:509-524`），不是 naive 的 `EnumChildWindows`。所以问题不在实现手法，而在**覆盖面**：

| 应用类型 | 实测结果 | 判定 |
|:--|:--|:--|
| 原生 Win32（explorer 桌面） | 19 个带名字的元素 | ✅ 可用 |
| **Electron / Chromium（Lark）** | 6 个 `Pane`，rect 全等于窗口 | ❌ 无控件、无文本 |
| UWP / XAML（`设置`、Realtek Audio Console） | **0 个元素** | ❌ 完全空白 |

Lark 那 6 个 `Pane` 是 Chromium 的宿主视图类名（`ContentsView` / `NonClientFrameView` / `ClientView` / `MainWidgetDelegateView` / `WatermarkWidget`），**渲染进程的辅助功能树没有暴露给 UIA**。因此 `click_element` / `set_element_text` / `batch_actions` 的 element 步骤对 Lark 全部无效——**这是"只能截图"的硬约束**，不是选择问题。

而且由于该树的 6 个元素 rect 全等于整窗，即便强行 `click_element`，也是点窗口正中心——是危险动作而非可用动作。

两个可复现的实现侧问题：
- **最小化窗口泄漏屏幕外坐标**：`get_ui_tree("下载-文件资源管理器")` 返回 `name='系统' rect=[-31987,-31987,44,44]`（`:337` 只判 `IsWindowVisible`，`IsWindowVisible` 对最小化窗口仍为 true；`:345-347` 只过滤 `width/height > 0`，没有过滤 `-32000` 哨兵值）。
- **同标题窗口无法区分**：`enum_windows_proc`（`:406-409`）取 Z 序第一个标题匹配者；`设置` 有 **两个**同标题窗口（`SystemSettings.exe` pid 544 与 `ApplicationFrameHost.exe` pid 23168），而 `get_ui_tree` 的入参**只有 `depth/window_title/query/compact`**（`protocol/src/tools.rs:311`），**没有 pid/hwnd**——`list_windows` 能给出 hwnd，却没法传给 `get_ui_tree`。
- 附带发现：`focus_window(title="设置")` 返回 `success:true`，紧接着无 `window_title` 的 `get_ui_tree` 报 **`Failed to get foreground window`**（`GetForegroundWindow()` 返回 0）⇒ **"focus 成功"不等于真的到了前台**，也说明不带 `window_title` 的 UI 树调用不可靠。

### 根因 E ｜ Token 侧没有任何"只要文件不要图"的出口

`server_save_path` 是 server 侧加的（`crates/server/src/mcp/tools.rs:55-65`，仅给 `CaptureScreen | GetMarkedScreen` 两个工具），`save_path` 是 agent 侧原生的（`protocol/src/tools.rs:246`）。**但两条路径都不改变回传行为**：`mcp/mod.rs:645-753` 在保存后**照旧**返回 `image` 块。

实测：`capture_screen(max_dimension=1280, save_path=...)` → 客户端磁盘确实写出 140,545 B 的 JPEG，同时 MCP 回包仍带 **187,396 字符 base64**。

⇒ 我"想用 Pillow 精确量坐标"这个正确意图，在协议上**无法表达**：没有 `include_image: false` 之类的开关，只能把图收进上下文。这是"截图=浪费 Token"的直接制度原因。

### 根因 F ｜ 我自己的技能把它固化了（自省）

`~/.workbuddy/skills/atpc-mcp-remote-control/SKILL.md`：
- 第 2 步标题就是"**坐标必须『测』出来，不能『看』出来**"，正文把"`capture_screen` + `crop` + `max_dimension` + Pillow 数边框"写成了**推荐做法**；
- 第 1.5 步、第 3 步的所有操作配方都建立在"点像素坐标"之上；
- 记忆里还留着一条**已失效的负面结论**："`list_monitors` 会报 `Unknown or unsupported tool`"。本次实测它**完全正常**（312 B，135 ms，`scale_factor=2.0`）——而它正是"一开始就该问、能省掉整轮坐标猜测"的通道。

也就是说：技能把"最后手段"写成了"标准流程"，并且用一条过期结论关掉了更便宜的通道。这是**流程性自我强化**：用截图 → 挫折都在坐标精度上 → 于是继续把截图技巧写进技能。

---

## 4. 修复建议（按性价比排序）

| 优先级 | 改动 | 位置 | 收益 |
|:--|:--|:--|:--|
| **P0** | `get_marked_screen` 的返回体把 `marks` 作为**文本**回传（`#id label rect center control_type` + `source` + `total_marks` + `scale_factor`），或至少追加 `include_marks:true` | `crates/server/src/mcp/mod.rs:735-753` | 让 SoM 从"另一张截图"变成"结构化数据"；模型可直接 `click_mark(7)` 而无需读图，**这才是 Tier 3 被设计出来的意义** |
| **P0** | 新增 `include_image`（默认 `true`）或让 `save_path`/`server_save_path` 生效时默认 `false` | 同上 | "落盘 + 本地 Pillow 量坐标"可做到**近乎 0 视觉 Token**；这是把"截图"从昂贵手段降级为廉价手段的关键开关 |
| **P1** | 3 层策略搬进 `initialize.instructions`（`prompts/get` 保留作详情） | `crates/server/src/mcp/mod.rs:513-530` | 客户端**无需主动拉取**即可知道截图是最后手段。这是 P0 之外成本最低、影响面最大的一条 |
| **P1** | 把优先级写进工具描述本身：`capture_screen` 描述加 "Prefer `get_ui_tree`; use this only for non-accessible/Canvas apps"；`get_ui_tree` 描述加 "Try this first for native apps" | `crates/protocol/src/tools.rs:295,311,318` | 描述是客户端必然读取的唯一文本，**不依赖任何客户端实现差异** |
| **P1** | `get_marked_screen` 支持 `window_title` 裁剪（或明确文档化"不裁剪"） | `crates/agent/src/tools/som.rs:815-853` | 看一个窗口只付该窗口的 Token（本例 Lark 占屏约 66%，可省三分之一） |
| **P2** | `get_ui_tree` 增加 `pid` / `hwnd` 入参 | `crates/protocol/src/tools.rs:311`、`crates/agent/src/tools/uia.rs:396-420` | 解掉"同标题窗口选不准"（UWP frame vs content），这是 `设置` 返回 0 元素最可能的成因之一 |
| **P2** | `get_ui_tree` 过滤 `-32000` 哨兵坐标（最小化窗口） | `crates/agent/src/tools/uia.rs:345-347` | 避免把屏幕外坐标喂给鼠标工具 |
| **P2** | UWP/XAML 覆盖率：在 `traverse_node` 前尝试从顶层窗口向下找 `CoreWindow`/内容子窗口；或对 `FindAll(Children)` 为空时回退 `TreeScope_Subtree` | `crates/agent/src/tools/uia.rs:509-524` | 目前 `设置`/`Realtek Audio Console` 这类窗口 0 元素，等于逼着用户走截图 |
| **P3** | 给 `get_marked_screen` 的 `contours` 策略换成"控件级"检测，或至少输出每个 mark 的候选 `control_type` 供模型判断 | `crates/agent/src/tools/som.rs:533-683` | 当前 `detect_visual_candidate_boxes` 是边缘/对比度显著性，产出碎片框（见表证图 #20） |

> 验收口径建议：任何"省 Token"的改动都要落到**活链路可观测量**（线上回包字节数 / content 块类型与数量），不能只看代码常量。例如 P0 的验收 = `get_marked_screen` 回包中出现 `marks` 文本且 `include_image=false` 时 `content` 不再有 `image` 块。

---

## 5. 副作用与遗留

- 本次审计对客户端**只做只读调用**（`get_ui_tree` / `list_windows` / `list_monitors` / `get_marked_screen` / `capture_screen` / `exec_powershell` 查询），**没有点鼠标、没有输入**；
- `focus_window("设置")` → 探测 → `focus_window("Lark")` 已恢复，**Lark 当前为前台**，Jack Zhu 会话未受影响；
- 遗留文件：`C:\Users\CLKJ\Downloads\probe.jpg`（140,545 B，16:03:39，本审计为验证 `save_path` 落盘而产生）。删除命令被 CLI 安全护栏拦截（`remove-item` 受限），**未删除**，需要时请手动清理；
- 证据图：`docs/plans/2026-09-20-perception-channel-audit-assets/`（`som-auto.jpg` = SoM auto 31 框；`som-grid.jpg` = grid 对照；`07-sent-chat.jpg` = 上一任务发送结果）。
