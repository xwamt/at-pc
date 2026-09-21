# 远程桌面「HTTP 轮询 → WS 长链」改造后的卡顿归因报告

- 日期：2026-09-21
- 对象：`at-pc` 远程桌面链路（前端 `crates/server/frontend/src/desktop/session.js` + server `crates/server/src/mcp/dashboard.rs` + agent `crates/agent/src/stream.rs`）
- 被测环境：**真实局域网**。Agent = Windows 11 `LAPTOP-ROEH3P85`（`\\.\DISPLAY1` 2880×1800 物理 / 200% DPI，18 逻辑核）；Server = 192.168.99.94:9800；客户端 = 本机 macOS（192.168.66.18），到 server **ping RTT 3.0 ms**。
- 方法：改进程探针（`/tmp/atpc-ws-probe/probe*.mjs`，纯 WS 客户端，不发浏览器），配合 agent 侧 PowerShell 采 `at-pc-agent` 的 CPU 时间。全部为只读观测，未改动任何源码。

---

## 0. 结论（先看这段）

**WS 长链不是瓶颈，换传输救不了卡。** 实测传输层开销：ticket 23 ms、WS 握手 9 ms、**中继附加延迟 p50 = 4 ms / p90 = 35 ms**（第 2 节 probe4）。带宽只用了 **5.9 Mbit/s**，在千兆局域网里连 1% 都不到。

卡的原因 100% 在**帧源（agent 的采集+编码）**和**交互触发模型**上：

| 实测项 | 数值 | 说明 |
|:---|---:|:---|
| 满屏变化时的**实际帧率** | **11.2 fps** | 配置是 15 fps，**帧源达不到**（probe3，7 s / 77 帧复核） |
| 帧间隔（agent 侧时间戳间距） | p50 **87 ms**，min 70，max 109 | 目标 66.7 ms |
| agent 进程 CPU（推流中） | **516–578 ms / 秒 ≈ 单核 52–58%** | 18 核机器；**每交付 1 帧 ≈ 46–52 ms CPU** |
| 静止后第一次交互 → 画面更新 | p50 **614 ms**，max **1114 ms** | 12 次试验，**6 次 3 s 内完全没有帧**（probe2） |
| 中继/网络附加延迟 | p50 4 ms / max 46 ms | **传输无罪** |
| **Web 控制台侧（浏览器）每帧成本** | **解码 3.9 ms + 绘制 ~0 ms** | 主线程 90 ms 里 86 ms 空闲（第 7 节补测） |
| **Web 控制台上行注入时延** | **p50 11 ms / max 37 ms** | 浏览器输入→远端光标真的动（第 7 节补测） |
| Web 控制台静止时 HUD | 显示 **0 FPS** | 静止只有 1 条/秒 24 B keepalive，前端不计数（第 7 节） |
| Web 控制台画面清晰度 | 帧 1280×800 → 实际显示 **2067×1292 设备像素** | **1.62× 上采样**（第 7 节） |
| 带宽 | 5.92 Mbit/s（64.4 KB/帧） | 局域网余量巨大 |
| 无人观看时 agent CPU | **0 ms/s** | 流循环随 WS 断开而停，这一点设计是对的 |

一句话：**HTTP 改成 WS 只换了水管，但水管从来不是瓶颈；真正的水泵是"全屏 GDI 截图 + 全屏哈希 + 定时器轮询"**，它每秒最多只能打出 11 帧，并且静止时会把帧率降成 2 Hz（500 ms 一次），而输入事件**不能唤醒它**。

> **补测（第 7 节）**：把真实浏览器 + 线上控制台页面也接上打点后，浏览器侧每帧只花 **3.9 ms 解码 + ~0 ms 绘制**（主线程几乎空闲），上行 **浏览器输入 → 远端光标移动只要 11 ms（p50）**。⇒ **"卡"既不在传输，也不在浏览器渲染，只能来自帧源**。Web 控制台这一侧真正的问题是**观感**：帧被 1.62× 上采样（糊）、静止时 HUD 显示 0 FPS（像死了）、以及"只动鼠标"浏览器实测 **8/8 次 4 秒内零帧**（你动了，画面不动）。

---

## 1. 抓帧链路实况（源码级）

`ws Client ──ticket/WS──> server ──(agent WS)──> agent stream loop`

agent 的循环在 `crates/agent/src/stream.rs:270-421`，每个 tick 做 4 件事，**全部在"编码分辨率"之上多做了 ~5 倍的无用功**：

```
loop {
  spawn_blocking {
    ① xcap Monitor::capture_image()            // ← 2880x1800 全屏
    ② compute_block_hashes(全屏 RGBA)          // ← 20.7 MB 全量哈希
    ③ decide_frame_send(脏?/keepalive?)
       ④ fast_rgba_to_rgb_scaled(scale=1.0)    // ← 缩到 1280x800 才编码
       ⑤ encode_jpeg(1280x800, q60)            // ← 真正要传的只有 1.02 Mpix
  }
  binary_tx.try_send(...)                      // cap=2，满了丢帧
  sleep(interval - elapsed)                    // 15fps -> 66ms
}
```

**关键事实 1：采集/哈希在 2880×1800，编码/传输只有 1280×800。**
- Agent 自报 `list_monitors` → `display_index 0 = \\.\DISPLAY1 2880x1800, scale_factor 2.0`；DPI-aware 下 `GetSystemMetrics(SM_CXVIRTUALSCREEN) = 2880x1800`，即**物理分辨率**。
- `crates/desktop-core/src/lib.rs:11` `DEFAULT_TARGET_WIDTH = 1280`，`stream_output_size(2880,1800,1.0) = (1280,800)`，与实测帧头 `1280x800` 完全吻合。
- ⇒ 每帧：**采集 5.18 Mpix / 20.7 MB RGBA，哈希 20.7 MB，最后只发送 1.02 Mpix**。约 5 倍像素被白干。
- ⇒ 顺带说明 `scale` 协议字段形同虚设：WS 处理器 `dashboard.rs` 里 `start_desktop_stream(&tid, 15, 60, 0, 1.0)` 把前端 POST 的 `fps/quality` 全盖掉、`scale` 恒定 1.0。

**关键事实 2：Windows 侧的采集路径每次调用都从头建对象。**
工作区 `xcap = "0.9"` 未开 `wgc` feature（`Cargo.toml:25`，xcap 无 `default` feature），所以走的是 **GDI 路径** `xcap-0.9.8/src/windows/gdi.rs:83-122`：
```
每次 capture_image():
  GetWindowDC(GetDesktopWindow)          // 每次
  CreateCompatibleDC                      // 每次
  CreateCompatibleBitmap(2880,1800)       // 每次新建 20.7 MB 位图
  BitBlt(SRCCOPY)                         // 每次全屏 blit
  GetDIBits -> Vec<u8> 20.7MB             // 每次全屏回读
  bgra_to_rgba(): 逐像素 swap(0,2)        // utils.rs:80，5.18M 次标量循环
```
没有持久化会话、没有脏区、没有 GPU 直通。这解释了 ~46–52 ms / 帧的 CPU 量与 11.2 fps 的天花板。

**关键事实 3：光标不在画面里。**
- GDI 路径是 `BitBlt` 桌面窗口 DC——**鼠标光标不属于该 GDI 表面**（是硬件/DWM 叠加层，需要 `GetCursorInfo + DrawIconEx` 才画得上）；
- 即便切到 WGC 路径，xcap 也显式关掉了光标：`xcap-0.9.8/src/windows/wgc.rs:157 session.SetIsCursorCaptureEnabled(false)`。
- 观测吻合：probe2 里 12 次真实 `MouseMovePixel` 有 **6 次 3 s 内不产生任何新帧**（剩下的也很可能是 hover/动画副作用）。即"只动鼠标"在远端**完全没有任何视觉反馈**。

**关键事实 4：脏块阈值把"光标级变化"主动过滤掉了。**
`desktop-core/src/lib.rs:86 DIRTY_BLOCK_RATIO_THRESHOLD = 0.005`（64px 块；2880×1800 = 45×29 = 1305 块，需 ≥7 块变化才编码）。仓库里甚至有专门断言"光标大小的变化不应触发重编码"的测试（`lib.rs:411`）。配合事实 3，等价于：**鼠标移动、悬停高亮、小提示框、输入光标闪烁，一律不会让远端画面刷新。**

**关键事实 5：静止后帧率退到 2 Hz，且输入不唤醒。**
`stream.rs:36-39` `IDLE_BACKOFF_THRESHOLD=5` / `IDLE_FRAME_INTERVAL=500ms`，连续 5 帧无变化后采集间隔变 500 ms；`compute_backoff_interval()` (`stream.rs:56`) 只依赖"连续静止帧数"，**与是否有输入到达无关**。输入走的是另一条独立通道（agent `input_tx` → 独立 OS 线程 `ws_client.rs:99-110`），两条路径之间没有任何通知。⇒ 静止后第一次点击/移动，最坏等满 500 ms 才被抓到。这正是 probe2 的 p50 614 ms。

---

## 2. 实测数据（可复现）

### 2.1 probe4：中继保真度（77 帧，7 s）
```
samples=77  clock_skew_estimate=-516ms (agent 与本机时钟差)
relay-added-delay above skew: min=0  p50=4  p90=35  p99=46  max=46 ms
frame ts spacing (agent 侧编码节拍): 80 81 83 84 85 87 99 80 88 97 103 83 97 84 103 79 104 97 81 84 ms
```
- 中继（server 转发 + 局域网 + 接收）**中位只加 4 ms**，最坏 46 ms。
- 帧间距恒在 79–104 ms ⇒ **agent 端就是 11 fps**，服务端没有任何"额外攒批"。
- 结论：**传输层不是瓶颈，优化 server/WS 无法让远程变快。**

### 2.2 probe3：满屏变化时的吞吐（远端跑一个 55 ms 变色的 WinForms 窗口制造真实脏屏）
```
frames=100  span=8920ms  keepalives=0  geometry=1280x800
effective_fps=11.21  (target 15)
inter-frame gap: min=70 p50=87 p90=102 max=109 ms
jpeg mean=64.4 KB  throughput=5.92 Mbit/s
```
### 2.3 远端 agent CPU 采样（`Get-Process at-pc-agent).TotalProcessorTime` 每秒差分）
```
1004ms:agent_cpu_ms=0      <- 无 WS 观众，流循环未运行
...
5053ms:agent_cpu_ms=0
6070ms:agent_cpu_ms=375    <- WS 连上、动画开始
7071ms:agent_cpu_ms=516
8071ms:agent_cpu_ms=547
9086ms:agent_cpu_ms=578
10088ms:agent_cpu_ms=562
...
15136ms:agent_cpu_ms=547
```
- 推流中稳定 **520–580 ms CPU/秒 ≈ 单核 55%**；11.2 fps ⇒ **每帧 46–52 ms CPU**。
- 空闲（无观众）0 ms/s：流循环确实随会话终止，这部分无需改。

### 2.4 probe2：静止后第一次交互的时延（12 次真实 MouseMovePixel）
```
trial  1 333ms | 4 557 | 5 614 | 6 1114 | 8 347 | 9 622   （其余 6 次 3 s 内无帧）
summary: answered=6/12  min=333 p50=614 p90=1114 max=1114 ms
```
333 ms 起跳 = 500 ms 退避睡眠的残余 + 采集编码；1114 ms ≈ 两个退避周期。**这就是"手感很卡"的直接来源。**

---

## 3. 为什么"以前 HTTP 感觉没那么卡"（客户端消费模型的退化）

前端的取帧方式在本次提交里从**拉**改成了**推**，这是第二个独立问题：

- 旧 HTTP 模式（`git show d568410 -- .../desktop/session.js` 中删除的 `pollFrameLoop`）：`await fetch(/desktop/frame/raw)` 循环——**每次拿到的是服务端缓存里"当前最新"的那一帧**，再用 `X-Frame-Timestamp` 去重。
  ⇒ 天然 **latest-wins**：客户端慢的时候，下一轮直接取更新的帧，**永远不会积压**。
- 新 WS 模式（`session.js:550-629`）：`ws.onmessage` 是 **async**，对**每一帧**都 `await createImageBitmap(blob)` 再 `drawImage`，**没有任何丢帧/取最新逻辑**。
  ⇒ 先进先出。一旦客户端某段时间跟不上（GC、后台标签页降频、控制消息抢占、解码抖动），队列就堆积，**显示的画面只会越来越旧，不会自愈**。
  正确写法是"只留最新一块 buffer + rAF 里渲染，in-flight ≤ 1"。

> 注：第 7.1 节已用真实浏览器实测：`createImageBitmap` p50 3.9 ms、到达间隔 p50 90 ms、并发峰值 1、乱序 0 ⇒ **当前工况下没有积压**。但它是**设计缺陷**，一旦帧率提高或标签页被降频就会先进先出越拖越旧；优先级 P2（不是当前卡顿的原因）。

另外，服务端 WS 推送路径上还有明显的每帧浪费（与卡顿有关但量级次之）：
- `router.rs:846` `frame.encode()` —— 每帧整帧拷贝一份；
- `router.rs:871` `BASE64_STANDARD.encode(&frame.data)` —— **每帧做一次 base64**（+270 KB 分配 + 常驻缓存双份内存），而 WS 路径发的是裸二进制，**base64 结果根本没人用**；
- `dashboard.rs handle_desktop_ws_session` `Message::Binary(frame_bytes.as_ref().clone())` —— 每订阅者再整帧拷一次。
三层冗余拷贝 + 一次 base64/帧，全都与"传得快不快"无关，但都在偷 CPU。

---

## 4. 其它确认到的问题（按影响排序）

| # | 位置 | 问题 | 影响 |
|:--|:---|:---|:---|
| R1 | `dashboard.rs handle_desktop_ws_session` | WS 建连时硬编码 `start_desktop_stream(15, 60, 0, 1.0)`，前端 POST 的 `fps/quality/scale` 被覆盖；`scale` 恒 1.0 | 参数协商失效，无法给弱网/弱机降载 |
| R2 | 同上（会话结束处）+ `router.rs:798 stop_desktop_stream` | WS 断开**无条件**停流 | 两个观众互踢；与 REST 启动的流互相打架 |
| R3 | `session.js:635 ws.onclose` | 只改文案，**不重连**（票据 30 s 过期即死） | 网络抖动后必须手动重开弹窗 |
| R4 | `ws/handler.rs:195-218` | 只合并 `DesktopInputEvent::MouseMove`，而前端实际发的是 **`MouseMovePixel`**（`desktop/index.js:70`） | 合并逻辑完全不生效，28 msg/s 全量透传 |
| R5 | `agent/src/input.rs:27 + 56` | 同一次移动既 `SetCursorPos` 又 `SendInput(MOUSEEVENTF_ABSOLUTE)` | 远端光标"双跳"，观感顿挫 |
| R6 | agent `ws_client.rs:513-555`（`biased`）+ server `ws/handler.rs:179-241`（`biased`） | 控制面**绝对优先**于视频；而工具结果是大 JSON/base64（截图、列目录动辄百 KB～MB） | 跑一次 MCP 工具（截图/列目录）期间画面被顶住 |
| R7 | `router.rs:819-829` | `desktop_frame_senders` 只增不删 | 长跑内存泄漏（小） |
| R8 | `stream.rs:284-290 / 355-360` | CaptureError 时 `find_monitor()` 重新 `Monitor::all()` 枚举 | 采集失败会退化成 150 ms 一轮的重复枚举（已知 `Window::all()`/`Monitor::all()` 昂贵） |

---

## 5. 修复路线（按性价比）

**P0-1 采集分辨率 = 编码分辨率（最大收益、改动最小）**
让 Windows 采集直接产出 1280×800，而不是 2880×1800 再缩。
- 方案 A（无新依赖）：自写 Windows capture —— `CreateCompatibleBitmap(1280,800)` + `SetStretchBltMode(HALFTONE)` + `StretchBlt(desktopDC 2880×1800 → 1280×800)`，再 `GetDIBits`。像素量 ÷5.06。
- 方案 B：开 `xcap` 的 `wgc` feature 并持久化会话，在 `frame_pool` 创建时就把输出尺寸设成 1280×800。
- 预期：每帧 CPU 46–52 ms → **~12–18 ms**，帧率回到并超过 15 fps；agent 单核占用 55% → ~15%。

**P0-2 持久化捕获会话 + 事件驱动（根治帧率与 500 ms 惩罚）**
- 捕获对象长生命周期（WGC: 常驻 `Direct3D11CaptureFramePool` + `FrameArrived` 回调；GDI: 复用 DC/位图），删除"每帧重建 + `recv_timeout` 等新帧"。
- 输入事件到达时用 `Notify`/`AtomicWaker` **立即唤醒采集**，并给首帧跳过退避；静止退避只在"确实无输入且无脏区"时生效。
- 预期：消除 p50 614 ms 的首次交互时延（→ 1 帧 ≈ 70 ms 内）。

**P1-1 让光标进画面**：`GetCursorInfo + DrawIconEx` 合成到帧上（或在客户端本地叠一层远端光标状态），否则"只动鼠标"永远无反馈。
**P1-2 客户端只渲染最新帧**：`ws.onmessage` 仅存最新 ArrayBuffer + `requestAnimationFrame` 渲染 + `in-flight ≤ 1`；把"推流"变成"latest-wins"。
**P1-3 服务端去冗余**：`frame.encode()` 仅在存在订阅者时做；base64 移到 `/desktop/frame` JSON 处理器里按需生成；去掉 `Message::Binary(..clone())`。
**P1-4 视频与控制隔离**：视频帧单独一条 WS/TCP 连接（或独立 task + 有界队列 + 公平调度），避免一次工具调用顶住画面。
**P2**：合并 `MouseMovePixel`；去掉重复注入；`fps/quality/scale` 由客户端协商；WS 自动重连；`desktop_frame_senders` 生命周期回收。

---

## 6. 验收方法（与本次同一套探针）
```bash
cd /tmp/atpc-ws-probe
ATPC_TOK=<admin token> ATPC_TID=<terminal> node probe3.mjs   # 吞吐：目标 effective_fps >= 15
ATPC_TOK=... ATPC_TID=... node probe4.mjs                    # 中继延迟：p90 应保持 < 50ms（本就不是瓶颈）
ATPC_TOK=... ATPC_TID=... ATPC_IDLE_MS=1200 node probe2.mjs  # 首次交互时延：目标 p50 < 120ms 且无"无帧"case
```
配合远端 `Get-Process at-pc-agent).TotalProcessorTime` 每秒差分，验收"每帧 CPU < 20 ms"。

**Web 控制台侧（第 7 节）**
```bash
cd /tmp/atpc-ws-probe
# 脏屏场景：帧到达 / 解码 / 绘制 / 长任务 / 上行注入（需远端先跑 atpc_anim27.ps1 与 atpc_cursorwatch.ps1）
node web-probe-A.mjs        # 看 createImageBitmap p50、乱序次数、事件循环延迟、上行坐标
# 静态场景：点开弹窗后"只动鼠标"是否产生新帧（验收"移动即有反馈"）
node web-probe.mjs
```
验收线：`drawImage/解码` 不是瓶颈（已达标，无需再压）；**静止 HUD 不得显示 0 FPS**；**鼠标移动后 300 ms 内必须有新帧**（当前 8/8 无帧）；**帧显示像素不再被上采样**（`canvas.width ≥ 显示设备像素宽`）。

## 7. Web 控制台侧（浏览器）链路实测 —— 补测

第 1–6 节的探针是**裸 WS 客户端**，量到的是"帧源 + 中继 + 网络"。它没有回答另一半问题：**在浏览器里点开控制台弹窗、推鼠标、看画面时，浏览器自己贡献了多少卡顿？** 本节把真实浏览器接上打点补齐。

- 方法：`playwright-core` 驱动 Chromium 1223（headless，viewport 1440×900，**deviceScaleFactor=2** 模拟 Retina），加载**线上控制台** `http://192.168.99.94:9800/?token=…`，点 `🖥️ 远程桌面直控`。在页面脚本执行前 `addInitScript` 注入打点：`WebSocket`（构造/到达/send）、`createImageBitmap`、`CanvasRenderingContext2D.drawImage`、`PerformanceObserver('longtask')`、事件循环延迟、rAF 节奏、`fetch`。
- 远端同时跑两个 PowerShell 侧助手：`atpc_cursorwatch.ps1`（每 3 ms 采 `[Windows.Forms.Cursor]::Position`，落 UTC 毫秒时间戳）与 `atpc_anim27.ps1`（55 ms 变色窗口，制造确定脏屏）。
- 脚本：`/tmp/atpc-ws-probe/web-probe.mjs`（静态场景：8 次交互试探 + 上行坐标）、`/tmp/atpc-ws-probe/web-probe-A.mjs`（脏屏场景：12 s 连续采样 + 16 次定位移动）。

### 7.1 浏览器侧每帧只花 ~4 ms（所以优化浏览器没用）

脏屏 12 s、131 帧实测：

| 环节 | 实测 |
|:---|---:|
| 点开弹窗 → **首帧出图** | **212 / 221 / 277 ms**（3 次运行） |
| 帧到达速率 | 131 帧 / 12 s = **10.9 fps**（和裸探针一致） |
| 到达 → 解码开始 | p50 **0.4 ms**（max 5.1） |
| `createImageBitmap` 解码 1280×800 JPEG | p50 **3.9 ms** / p90 4.9 / max 9.4，**并发峰值 1** |
| 解码完 → `drawImage` | p50 **0 ms** |
| `drawImage` 本身 | p50 **0 ms**（GPU 异步，不阻塞 JS） |
| 绘制间隔 | p50 **90 ms**（= 帧源节拍，不是浏览器在拖） |
| 事件循环延迟 | max **7.6 ms** |
| 长任务（>50 ms） | **0 个** |
| rAF 间隔 | p50 20.9 ms |
| 解码完成顺序被打乱 | **0 / 131** |
| HUD 自报 | `11 FPS` / `1280 × 800` / `实时推流中` |

**结论：浏览器主线程在 90 ms 的帧间隔里只用了 ~4 ms，剩下 86 ms 全空闲。** 把浏览器渲染优化到 0，也只能把 11 fps 变成 11 fps。**"卡"在感知上属于帧源，不属于浏览器。**

> 注意（顺手修正第 3 节的风险判断）：我原先担心的 `onmessage` async 无背压、FIFO 积压，**在当前 11 fps 工况下没有发生**（解码 3.9 ms ≪ 到达间隔 90 ms，并发峰值 1，乱序 0）。它仍是设计缺陷（帧率提高或标签页降频后会咬人），但**不是当前卡顿的原因**，优先级应从 P1 降到 P2。

### 7.2 上行注入：p50 11 ms（浏览器 → 远端光标真的动）

`web-probe-A.mjs` 通过真实 canvas 事件派发 16 次定位移动，记录**客户端 `ws.send()` 的 epoch**；远端光标助手记录**每次光标变化的 UTC 毫秒**。按目标像素坐标一一配对（10/16 唯一匹配成功）：

```
原始 Δ(远端时间戳 − 客户端发出时刻):  min 526  p50 530  max 556 ms
其中包含两侧时钟偏斜:                -519 ms  ← 由帧头 timestamp 独立测得（§2.1，两次运行 -519 / -516）
⇒ 真实上行注入时延:                  min  7   p50  11  p90  34  max  37 ms
```
即：浏览器事件 → WS → server → agent WS → `desktop_input-worker` 线程 `SetCursorPos`，**中位 11 毫秒**。**上行也是清白的**，没有任何"消息排队等 500 ms"的现象。

顺带确认坐标链路无 bug：客户端发出的像素坐标与远端落点逐点吻合（如发 (2187,270) → 落 (2186,270)）。远端 PS 采到的是逻辑坐标（DPI-unaware 进程读到 `/2`），这是**观测者**的偏差，不是协议 bug。

### 7.3 上行节流与合并：422 → 54，且服务端合并逻辑确实没生效

2 s 内往 canvas 派发 **422** 次 `mousemove` → 实际发出 **54** 条 WS 消息（**28.6/s**，`session.js:451` 的 35 ms 节流生效），action 全部是 **`MouseMovePixel`**。
⇒ 与第 4 节 R4 一致：server `ws/handler.rs:195` 只合并 `MouseMove`，**前端发的是 `MouseMovePixel`，合并逻辑完全不生效**，28 条/秒全部原样打到 `SetCursorPos + SendInput`。局域网下这只是白烧 CPU，弱网下会顶住控制通道。

### 7.4 Web 控制台特有的、真正影响"手感"的问题

**(a) 画面被 1.62× 上采样 —— "糊"来自这里，且前端无法调**
实测画布几何：`backing = 1280×800`，元素 box = `1116×646` CSS，`dpr = 2`，`object-fit: contain`。
⇒ 实际显示尺寸 1034×646 CSS = **2067×1292 设备像素**，而位图只有 1280×800 ⇒ **放大 1.62 倍**。再叠加链路本身 2880×1800 → 1280×800（0.44×）的降采样，最终清晰度只有原生的 ~0.72×。
⇒ 而且**没有任何途径改**：`start_desktop_stream(15, 60, 0, 1.0)` 把 `scale` 写死 1.0（R1），前端三个"显示模式"（自适应/铺满/1:1）**只改 CSS，不改采集分辨率**。

**(b) 静止时 HUD 显示 `0 FPS` —— 误导性反馈**
静止时服务端只推 24 字节 keepalive（实测 **1 条/秒**，间隔 1017 ms：退避期每轮 500 ms 采一次、而 keepalive 判定是 `elapsed_since_send >= 500 ms`，隔轮才发）。前端 `session.js:575` 对 `jpegBytes.length === 0` 直接 `return`，**`frameCount` 不计数** ⇒ 右上角稳定显示 **`0 FPS`**，同时状态栏还是"实时推流中"。
⇒ 用户看到"0 FPS"会以为链路死了 —— 这是纯粹的 UI 反馈缺陷，**与实际延迟无关但极影响体感**。

**(c) "只动鼠标"在浏览器路径下 8/8 次 4 秒内零帧（复核第 1 节关键事实 3/4/5）**
`web-probe.mjs` 静态场景 8 次试验：每次静置 2.5 s（必然进入 500 ms 退避）→ 派发一次真实鼠标移动 → 等 4 s。结果 **8 次全部无新帧**。上行是通的（7.2 节证明同一时刻发的事件在 11 ms 内落到远端光标），**但画面完全不更新**：光标不在采集内（GDI BitBlt 桌面 DC 无光标）+ 脏块阈值 0.5%（2880×1800 → 45×29 块，需 ≥7 块变化）+ 输入不唤醒采集。
⇒ **这就是"远程很卡"的体感来源**：你动了，远端也动了，但你看到的画面不动。

**(d) 控制台后台轮询：**实测不是瓶颈，但会抢同一条控制面**
`app.js:275/280` 每 2 s 打 `/api/terminals` + `/api/calls` + `/api/auth/me`。12 s 内 13 次请求，各 **4–9 ms**，无 >50 ms 请求 ⇒ 对画面无影响。但它与视频共享服务端/agent 的 `biased` 控制面优先级（R6）：**跑一次大 JSON 工具（截图/列目录）时，画面会被顶住**。

**(e) HTTP 时代死代码仍在跑**：WS 模式下 `sendDesktopInput` 在 `ws.readyState === OPEN` 分支直接 return，`flushMouseMove` / `inputQueue` / `pendingMouseMove` / `isSendingInput` 全部永不执行（`session.js:105-174`）。维护陷阱，建议删除。

**(f) 断线不自愈**：`ws.onclose` 只改文案（R3），票据 30 s 过期后必须手动重开弹窗。

### 7.5 修正后的修复优先级（在 §5 基础上）

| 顺位 | 项 | 依据 |
|:--|:--|:--|
| 0 | **§5 P0-1 帧源分辨率（采集=编码）** | 唯一能真正提速的一刀 |
| 0 | **§5 P0-2 持久会话 + 输入唤醒采集** | 消除 500 ms 退避与"移动无反馈" |
| 1 | **让帧分辨率匹配显示尺寸**（协商 `scale`：按 canvas 显示像素 ÷ DPR 请求；或前端按显示尺寸自适应） | 第 7.4(a)：直接消掉 1.62× 上采样，Retina 上"糊"立刻改善 |
| 2 | **静止时 HUD 不要显示 0 FPS**（改用 keepalive 计数或显示"画面静止"） | 第 7.4(b)：纯 UI，1 行代码，最便宜的体感提升 |
| 3 | **光标合成进画面**（原 P1-1） | 第 7.4(c)：浏览器路径 8/8 无反馈 |
| 4 | 客户端 latest-wins + rAF；删除 HTTP 死代码；WS 自动重连 | 风险与整洁性，非当前瓶颈 |
## 8. 未验证 / 边界

- 46–52 ms/帧 内部（blit / DIB 回读 / bgra 逐像素 / 哈希 / 缩放 / JPEG）的**细分拆解未做**：需要 agent 侧埋点或本地基准，本次未改源码。
- 客户端浏览器侧队列深度：第 7.1 节已用真实浏览器实测（解码 3.9 ms、并发峰值 1、乱序 0），**当前 11 fps 工况下无积压**；帧率提升后是否会积压仍未验证（属推演）。
- 第 7 节的上行时延依赖**时钟偏斜校正**（−519 ms，由帧头 `timestamp` 与客户端 `Date.now()` 的最小差独立测得，两次运行 −519/−516 一致）。若两侧时钟在测量期间跳变（NTP），该数字会失真；本次运行窗口内未见跳变。
- 第 7.4(c) 的 8/8 零帧结论基于**合成 `MouseEvent`**（与真实鼠标走同一 JS 处理路径，但浏览器合成事件不产生"真实指针"的 hover 副作用）；真实鼠标的实测在 §2.4 裸探针（6/12 有帧）。两者都指向"鼠标移动基本不产生刷新"。
- 本轮观测的显示器是单屏 2880×1800；多屏/4K 下 P0-1 与第 7.4(a) 的收益会更大（像素量线性）。
