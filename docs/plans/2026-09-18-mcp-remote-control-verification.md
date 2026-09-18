# at-pc MCP 远程控制实测：在客户端打开 Lark 并给 Alan Xia 发一首诗

- **日期**：2026-09-18 10:10–10:21（GMT+8）
- **仓库**：`/Users/clkj/项目/at/at-pc`，HEAD = `a3cf5fb`
- **服务端**：本机 macOS，进程 `dist/at-pc-server-macos`（PID 23570），监听 `:9800`（MCP HTTP）+ `:9801`（Agent WS）
- **客户端**：`laptop-roeh3p85-b6a918` = `LAPTOP-ROEH3P85` / Windows 11 (26100) / 用户 `CLKJ`，来自 `192.168.66.110`
- **目标动作**：在客户端打开 Lark，给 Alan Xia 发一首诗

**结论：任务成功。** 诗已实际投递进客户端 Lark 里标题为「Alan Xia」的会话，并有两条互相独立的证据（会话气泡 + 左侧会话列表预览）。同时实测发现 **5 个真实缺陷**（其中 2 个是安全/可审计性层面的），见第 3 节。

> 全部证据截图见同目录 [`2026-09-18-mcp-remote-control-assets/`](./2026-09-18-mcp-remote-control-assets/)。
> **注意**：仓库 `.gitignore:9-11` 忽略 `*.jpg`/`*.jpeg`/`*.png`，因此这些证据图**只存在于本地工作区、不会随报告入库**。
> 本报告的每条结论都写明了可复核的**命令行/返回值**，不依赖图片即可复现；图片仅作直观佐证。
> 本轮**未修改仓库任何源码**；仅新增本报告与证据图。

---

## 1. 链路与配置

### 1.1 MCP 接入方式

`at-pc-server --stdio` 会同时拉起 WS 网关（9801）与 MCP HTTP 服务（9800），MCP 端点为 **`POST /mcp`**（JSON-RPC 2.0，streamableHttp；返回纯 JSON，不强制 SSE）。已在 `~/.workbuddy/mcp.json` 写入：

```json
"at-pc": {
  "type": "streamableHttp",
  "url": "http://127.0.0.1:9800/mcp",
  "timeout": 120000,
  "disabled": false
}
```

> ⚠️ **新 MCP 不会自动生效**：需要在本会话/连接器页面对 `at-pc` 执行「信任」，并重启会话后才会作为原生工具出现。因此本轮是用 **原始 MCP 协议**（`initialize` / `tools/list` / `tools/call`）直接驱动实测的——走的完全是同一条协议路径，只是没经过 IDE 的工具注册层。

`tools/list` 返回 **39 个工具** = 33 个 agent 工具（投影自 `protocol/src/tools.rs`）+ 6 个 server 元工具（`list_terminals` / `select_terminal` / `rename_terminal` / `get_active_terminal` / `cancel_tool` / `list_pending_calls`）。

### 1.2 连接确认

```
GET /api/terminals → 1 台
  terminal_id = laptop-roeh3p85-b6a918   status = Online
  hostname    = LAPTOP-ROEH3P85          username = CLKJ
  os_version  = Windows 11 (26100)       agent_version = 1.0.0
  latest_metrics: cpu 2.0%, mem 12106/32280 MB
  last_heartbeat_elapsed_secs = 3
lsof -iTCP:9801 → 192.168.66.18:9801 <- 192.168.66.110:61445 (ESTABLISHED)
```

### 1.3 本轮实际跑过的工具族

| 类别 | 工具 | 结果 |
|:--|:--|:--|
| 会话/元信息 | `list_terminals` `select_terminal` `get_system_overview` | ✅ |
| 窗口 | `list_windows` `focus_window` | ✅ |
| 键鼠 | `hotkey` `mouse_move` `mouse_click` `type_text` `press_key` | ✅ |
| 组合动作 | `batch_actions`（12 步 / 4 步 / 2 步各一次） | ✅ |
| 视觉 | `capture_screen`（全屏 / 窗体裁剪 / 原生分辨率裁剪） `get_marked_screen` `get_ui_tree` | ✅（含问题，见 §3.2/3.3） |
| 显示器 | `list_monitors` | ❌ 见 §3.1 |

---

## 2. 执行过程与证据

### 2.1 唤醒并最大化 Lark

Lark 已在客户端运行（pid 28396，**最小化**）。`focus_window(title="Lark")` 唤醒后，用 `hotkey ["win","up"]` 最大化：

```
focus_window → {"success": true, "hwnd": 1114604, "pid": 28396, "title": "Lark"}
hotkey       → rect 由 [-32000,-32000,314,50]（最小化）变为 [-8,-8,2576,1568]（铺满 2560×1600，底部留 32px 任务栏）
```

证据：`01-lark-maximized.jpg`

### 2.2 确认目标会话

`Ctrl+K` 搜索 `Alan Xia`，结果：

```
Alan Xia                      IT
研发团队(10) 部门              包含 Alan Xia, Sherlock Xia, 夏(清)   → 09:53
IT(1) 部门                    包含 Alan Xia                        → 9月2日
工单处理(3)                   包含 Alan Xia                        → 9月15日
技术部(7)                     包含 Sherlock Xia(夏雨洁), Alan Xia    → 昨天
Eason Cai, Vincent Yan, Stephanie Fan, Alan Xia(4) 外部            → 9月8日
（消息命中）技术部: @Alan Xia 有疑问的整理一下 / 可以顺便把 onboarding 那个日志的功能一起发了
```

证据：`02-search-target.jpg`

### 2.3 输入前先精确测量输入框（关键）

**方法论要点**：`capture_screen` 默认 `max_dimension=1280`，而屏幕是 2560×1600 → **默认截图是 2× 降采样的**；同时 Read 渲染图片时还会再次缩放。**靠肉眼在渲染图上读像素坐标不可靠**：我的第一次目测给出 `(1600, 1516)`，实测真值是 `y∈[1476,1523]`，差 17px——正好会点在边框上。

改用 Pillow 直接分析**原生分辨率裁剪图**（`crop:[640,1380,1920,220]`，`max_dimension:2560` → 不降采样）：

```
输入框上边框 y = 1476   （行亮度 195，明显暗于周围 255）
输入框下边框 y = 1523
左右边界   x ∈ [697, 2528]
→ 中心 (1612, 1499)，高 47px
任务栏顶边 y ≈ 1552
```

### 2.4 组合输入（先只输入、不发送）

用 **一次 `batch_actions`** 完成「点击输入框 → 输入 → Shift+Enter 换行 ×4」共 12 步，耗时 4334 ms：

```
1 mouse_click (1612,1499)      51ms
2 wait                         450ms
3 type_text "【at-pc 远程控制测试】"  8ms   (14 字符)
4 hotkey [shift,enter]         84ms
5 type_text "远山遥隔一屏通，"      8ms
6 hotkey [shift,enter]         83ms
7 type_text "万里机枢在掌中。"      6ms
8 hotkey [shift,enter]         83ms
9 type_text "指下轻敲三两字，"      6ms
10 hotkey [shift,enter]        83ms
11 type_text "诗成已寄彩云东。"     4ms
12 wait                        600ms
```

**发送前先截图核对**（避免点错位置就误发）——内容逐字正确、无丢字，且右下角显示「Shift + Enter 换行」提示，确认 **Enter 即发送**：

```
【at-pc 远程控制测试】
远山遥隔一屏通，
万里机枢在掌中。
指下轻敲三两字，
诗成已寄彩云东。
```

证据：`04-draft-before-send.jpg`

### 2.5 发送与双路验证

`batch_actions [press_key enter, wait 1500]` → `success=true`。

**验证 A（会话内气泡）**：消息出现在会话底部，时间戳 `10:19`，输入框已清空并恢复占位文字。证据：`05-sent-bubble.jpg`

**验证 B（会话列表预览，与 A 相互独立）**：左侧列表中该会话预览变为

```
Alan Xia                                              10:18
【at-pc 远程控制测试】远山遥隔一屏通，万里机枢在掌中。指下轻敲三…
```

证据：`06-list-preview.jpg`

两条证据分别来自「会话消息流」与「会话列表摘要」两个不同数据源，排除「只是输入框里留了草稿」的假阳性。

---

## 3. 实测发现的问题

### 3.1 【高】无能力协商：服务端会广告客户端根本执行不了的工具

`list_monitors` 出现在 `tools/list` 里，但调用直接失败：

```
tools/call list_monitors → Error: Unknown or unsupported tool 'list_monitors'
```

根因定位：

- `list_monitors` **确实**在源码里（`protocol/src/tools.rs` 声明 + `agent/src/tools/dispatch/screen.rs:57` 分发），并非漏实现；
- 但 **`tools/list` 是服务端按自己编译的协议表投影出来的**（`crates/server/src/mcp/tools.rs::get_mcp_tool_definitions` → `tool_registry()`），**不是**由客户端上报；
- 客户端跑的是 **`dist/at-pc-agent.exe`（构建于 2026-09-07 19:02）**，而 `ListMonitors` 是 **`a3cf5fb`（2026-09-16 18:31）** 才引入的 —— **客户端二进制比它老了 9 天**；
- 服务端对注册的 agent **没有任何版本/能力校验**（`agent_version` 只是个自由字符串，`grep` 全仓仅见于结构体定义与测试）。

⇒ 结果：清单里 39 个工具，实际能执行的是「客户端那版二进制认识的那些」。**这种错配只在运行时才以一条模糊的 `Unknown or unsupported tool` 暴露**，且版本号两边都写 `1.0.0`，无法区分。

**建议**：注册握手时让 agent 上报支持的 tool 名单（或 build id），服务端 `tools/list` 与之取交集；同时对不兼容版本在 `list_terminals` 里显式标红。

### 3.2 【中】`get_marked_screen` 的 `window_title` 未生效，且 SoM 不标 UI 控件

传 `window_title:"Lark"` + `strategy:"som"` 后：

- 标记覆盖了**整个屏幕**（桌面壁纸上的金色花纹、桌面图标都被框了：#1–#29），说明 **`window_title` 裁剪没起作用**；
- 而**真正需要的聊天输入框一个标记都没有**。

证据：`03-som-marks.jpg`

即这里的 SoM 是「视觉显著性检测」（边缘/对比度驱动的候选框），**不是 UI 控件检测**。对「点某个按钮/输入框」这类任务，它给不出可用句柄。`grid_divisions` 网格模式同样只能靠人工数格子。

### 3.3 【中】对 Electron 应用（Lark），`get_ui_tree` 拿不到任何控件

`get_ui_tree(window_title="Lark")` 只返回 **8 个元素**，全部是 Chromium 外层容器：

```
Lark → ContentsView → NonClientFrameView → ClientView
     → MainWidgetDelegateView → BrowserUserView → WatermarkWidget → WatermarkContentView
（Pane/Group，rect 全等）
```

渲染进程未开启辅助功能（未加 `--force-renderer-accessibility`），因此**消息列表、输入框、按钮全部不可见**。

⇒ **组合后果**：对 Lark 这类应用，`click_element` / `set_element_text` / `batch_actions` 里的 `click_element`/`set_element_text` 全部用不了，只能退回**像素坐标点击**。而 §3.2 已说明 SoM 也帮不上忙 ⇒ 目前唯一的可行路径是「Pillow 精测坐标 + `mouse_click` + `type_text`」，这条路径**可行但脆弱**（窗口一改尺寸/换肤就全废）。

**建议**：为 Chromium/Electron 目标提供 `--force-renderer-accessibility` 启动选项或注入式控件查询；否则应在工具描述里明确「本工具对 Electron 应用无效」，避免上层把 `click_element` 当通用方案。

### 3.4 【中】`type_text` 无按键间隔，且 `\n` 不产生换行

`crates/agent/src/input.rs:232` 的 `TypeText` 分支对每个 UTF-16 单元**背靠背**发 down+up，**循环内没有任何 sleep**。实测：8 个中文字符 **4–8 ms**、14 个字符 **8 ms**（§2.4 的 step 明细）。

- **风险**：对 Lark 这种重量级 Electron 应用，长中文串有丢字风险（本轮 33 字侥幸完整）。
- **另一点**：`TypeText` 用 `KEYEVENTF_UNICODE` + `wVk=0`，因此文本里的 `\n`（U+000A）**不会**被应用识别为回车 ⇒ 想输入多行必须显式发 `Shift+Enter`。文档/工具描述里没有说明这一点。

**建议**：加 `interval_ms` 参数（默认 5–15ms），或在 `TypeText` 里对 `\n` 特殊处理成 `VK_RETURN`。

### 3.5 【高，安全/可审计】审计日志写进了「已删除的文件」

运行中的 server **同时持有两个已从目录消失的日志文件的写句柄**：

```
lsof -p 23570
  fd 11w  REG  1,17        0  15310408  dist/at-pc-server.log   ← 0 字节
  fd 13w  REG  1,17    44259  15310409  dist/audit.jsonl       ← 已写入 44,259 字节

ls -la dist/ | grep -E "audit|server.log"
  → 目录中不存在 audit.jsonl / at-pc-server.log
```

即：进程启动时 open 了这两个文件，之后文件被**取消链接**（`dist/` 被重新打包/清理过：目录 mtime 是今天 10:09），进程仍在向**已删除的 inode** 写。

⇒ **后果**：本轮（以及该进程启动以来）的全部审计记录——包括每一次 MCP 驱动的键盘、鼠标、截屏——**在磁盘上都不可见，且进程退出即永久丢失**。我无法从审计侧核验本轮的远程控制动作，只能靠客户端截图侧证。对一个以「可审计的远程控制」为卖点的产品，这是实质缺陷。

**建议**：审计写入不要长期持有句柄 —— 每写一批就 `O_APPEND` 打开/关闭，或检测到文件消失/被轮转时自动 reopen；打包脚本不要清 `dist/*.log`、`dist/audit.jsonl`；并给审计文件加独立的、不在打包目录内的默认路径。

### 3.6 【信息】目标会话的性质：是账号 Alan Xia 的「自己」空间

需要向使用方说明清楚，因为它影响对结果的解读：

- 会话标题是 **Alan Xia**；
- 但输入框占位文字是 **「按住 Alt 说话 · 可以给自己发送文件或转发消息」**；
- 会话内**所有**消息（含历次 agent 测试消息、`at-pc-agent.exe` 文件、`seed4me-vpn.apk`、`FreeToken-Setup.exe`、验证码串 `PZK:mq_Q4Exge2f`）**都是同一头像、同一「已发送」蓝色气泡样式、且全部左对齐**；
- `Ctrl+K` 里 `Alan Xia / IT` 是唯一同名命中，未出现「(我)」等第二对象。

综合判断：**客户端 Lark 账号本人的显示名就是「Alan Xia」（IT 部门），该会话是它的「自己」会话**；历次测试诗句也都发在这里。

⇒ 本轮的诗投递到了「Alan Xia」这个人自己的会话空间里，**字面上满足「给 Alan Xia 发一首诗」**（且与该会话历史里历次测试的目标一致）。如果原意是发给**另一位**同名同事，则本租户搜索里不存在第二个 Alan Xia；这一点请使用方确认。

---

## 4. 副作用（请在客户端留意）

| 项 | 状态 |
|:--|:--|
| Lark 窗口 | **被我从最小化改为最大化**（原为浮窗 `[173,70,1317,751]`）。`Win+↓` 可还原 |
| Lark 会话 | 「Alan Xia」会话新增 1 条消息（`【at-pc 远程控制测试】` + 四行诗），发件人为该账号本身 |
| 鼠标位置 | 停在输入框 `(1612,1499)` |
| 搜索浮层 | 已关闭（点击结果后自动收起） |
| 服务端/仓库 | 未改动源码；未改动服务端配置 |

---

## 5. 复现脚本位置

本轮为可复用的 MCP 驱动工具（均在工作区之外，不入库）：

- `/tmp/atpc-perf/mcpcall.py` —— 无依赖的 MCP streamableHttp 客户端（`tools/call` 单个调用；支持 `@file` 传参避免中文引号问题）
- `/tmp/atpc-perf/batch_compose.json` —— 12 步「点击 + 输入 + 换行」组合动作
- `/tmp/atpc-perf/batch_search.json` / `batch_open.json` / `batch_send.json` —— 搜索、打开会话、发送

坐标测量一律走 Pillow 分析**原生分辨率**裁剪图，不靠渲染图目测。
