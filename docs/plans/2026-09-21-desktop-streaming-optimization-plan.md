# at-pc 远程桌面全链路低延迟与流畅度优化实施计划 (2026-09-21)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 基于 `2026-09-21-desktop-ws-latency-attribution.md` 真实局域网与端到端探针归因结论，彻底破除“全屏 GDI 截图 + 全量哈希 + 2Hz 静止退避 + 输入不唤醒”的帧源水泵瓶颈；分三个阶段（P0/P1/P2）落地采集与编码分辨率对齐、事件驱动唤醒、光标合成、服务去冗余、协议通道隔离、前端高清自适应与状态感知重塑，将交互延迟从 p50 614ms 降至 <80ms，帧率稳定在 15~30 FPS，Agent CPU 降至 <15%，消除画面模糊与卡死虚假体感。

**Architecture:**
1. **帧源层（Agent / Desktop-Core）**：在 Windows 端重构抓帧链路，复用 DC 与位图缓存，直接在 GDI 采样阶段对齐目标编码分辨率（2880×1800 → 1280×800），直接降低 5 倍像素处理量；通过 `GetCursorInfo` + `DrawIconEx` 原生合成鼠标光标；建立跨线程 `tokio::sync::Notify` 机制，输入事件到达时毫秒级唤醒采集循环并跳过 500ms 退避，在交互窗口期内执行敏捷脏检。
2. **传输与调度层（Server / Protocol）**：重构 `router.rs` 推流路径，废弃无用的逐帧 Base64 计算与深拷贝，仅在有订阅者时编码二进制；修复 `ws/handler.rs` 漏合并 `MouseMovePixel` 导致 28 msg/s 洪峰的缺陷；消除 `agent/src/input.rs` 中 `SetCursorPos` 与 `SendInput` 的双重注入；为 WebSocket 发送端引入公平调度，防止大 JSON/大文件 MCP 工具响应顶死视频帧。
3. **展示与会话层（Frontend / Dashboard）**：解除 `dashboard.rs` 硬编码 `(15, 60, 0, 1.0)` 的参数抹平限制，支持客户端按物理像素与 DPR 协商 `scale` 消除 1.62× 模糊拉伸；修复静止期 Keepalive 帧导致 HUD 误显 `0 FPS` 的假死体感；重构 WS 多观众引用计数，避免单人离线踢出全局流；清理 HTTP 遗留队列死代码并引入自动退避重连。

**Tech Stack:** Rust (Edition 2021, Tokio, Axum, xcap, windows-sys, image), JavaScript (WebSockets, Canvas 2D, DataView, requestAnimationFrame), Playwright / Node.js (Probe testing)

**Spec:** [2026-09-21-desktop-ws-latency-attribution.md](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/docs/plans/2026-09-21-desktop-ws-latency-attribution.md)

---

## Global Constraints

- **协议向后兼容**：不得破坏 `BinaryDesktopFrame`（`[4B Magic 'DFRM'][4B Display][4B Width][4B Height][8B Timestamp][Raw JPEG bytes]`）和现有 `DesktopInputEvent` 消息契约，保持与老版本 Agent/Server 的双向兼容。
- **跨平台隔离**：Windows 专属的 GDI/Cursor 优化必须严格封装于 `#[cfg(windows)]` 下，macOS/Linux 保留 `xcap` 回退通道。
- **零破坏性依赖**：优先复用工程中已引入的 `windows-sys` 基础 API，不引入重型第三方未审计库。
- **度量指标真实性**：优化验收必须采用与归因报告一致的 `/tmp/atpc-ws-probe` 实测脚本（`probe2.mjs` / `probe3.mjs` / `probe4.mjs` / `web-probe-A.mjs`），以客观数据作为阶段交付依据。

---

## 里程碑总览与量化验收指标

| 里程碑 | 包含优化项 | 核心改进点 | 可量化验证目标 |
|:---|:---|:---|:---|
| **Milestone 1: P0 帧源采集与输入唤醒（根治源头）** | P0-1, P0-2, P1-1 | 采集分辨率对齐编码分辨率、DC缓存复用、输入事件即时唤醒、光标合成入帧 | ① 满屏变化实际帧率从 11.2 fps 提升至 **≥15 fps**<br>② 每帧 CPU 耗时从 46~52ms 降至 **<15ms**，Agent CPU 单核从 55% 降至 **<15%**<br>③ 静止后首次交互时延从 p50 614ms 降至 **<80ms**（无超时无丢帧）<br>④ 移动鼠标在远端画面中立即可见光标位移（消除 8/8 零帧） |
| **Milestone 2: P1 服务去冗余与通道公平调度** | P1-3, P1-4, R4, R5 | 懒计算 Base64、消除帧深拷贝、合并 `MouseMovePixel`、消除双跳、WS 公平调度 | ① 服务端广播内存深拷贝归零，消除每帧 270KB Base64 冗余开销<br>② `MouseMovePixel` 28msg/s 缩减至有效节流帧率（降低 80% 上行消息数）<br>③ 远端光标移动平滑无顿挫<br>④ 执行 MCP 截图/大工具期间，推流帧率不出现断崖式阻塞 |
| **Milestone 3: P2 前端体验重塑与会话生命周期治理** | 7.4(a), 7.4(b), R1, R2, R3, R7 | 客户端分辨率动态协商、消除 1.62× 模糊拉伸、HUD Keepalive 状态区分、多观众引用计数 | ① 高分屏画面清晰度达到原生 1:1，无模糊上采样<br>② 桌面静止时 HUD 显示“待机/静止”状态，不再误显示 `0 FPS`<br>③ 多人连入时一人断开不影响其他观众；观众归零时正常停流<br>④ 清除遗留 HTTP 输入死代码，网络抖动断线后支持自动重连 |

---

## 阶段一：Milestone 1 - P0 帧源采集与输入唤醒（根治卡顿源头）

### Task 1: 【P0-1】Windows Agent 采集分辨率对齐编码与 DC/位图内存复用

**Files:**
- Create: `crates/agent/src/capture/windows_gdi.rs`
- Modify: `crates/agent/src/capture/mod.rs` (或 `crates/agent/src/stream.rs:40-52, 270-327`)
- Test: `crates/desktop-core/tests/resample_test.rs`

**Interfaces:**
- Consumes: `display_index: u32`, `target_w: u32`, `target_h: u32`
- Produces: `pub struct GdiScreenCapturer`: `pub fn capture_into(&mut self, target_w: u32, target_h: u32) -> Result<&[u8], CaptureError>` 直接输出 `target_w * target_h * 4` 字节的 RGBA/BGRA 缓冲。

- [ ] **Step 1: 编写单元测试验证直接下采样输出尺寸与哈希一致性**

创建 `crates/desktop-core/tests/resample_test.rs`：
```rust
use at_pc_desktop_core::{compute_block_hashes, stream_output_size};

#[test]
fn test_aligned_resolution_hashing() {
    let (tw, th) = stream_output_size(2880, 1800, 1.0);
    assert_eq!((tw, th), (1280, 800), "默认 scale 1.0 必须精确映射至 1280x800");

    let buffer = vec![0x7fu8; (tw * th * 4) as usize];
    let hashes = compute_block_hashes(&buffer, tw, th);
    assert_eq!(hashes.len(), (1280u32.div_ceil(64) * 800u32.div_ceil(64)) as usize);
}
```

- [ ] **Step 2: 运行测试确保基础分辨率映射正确**

运行：`cargo test -p at-pc-desktop-core --test resample_test`
预期：PASS

- [ ] **Step 3: 实现 `windows_gdi.rs`：复用 DC/HBITMAP 并直接采样至目标尺寸**

在 `crates/agent/src/capture/windows_gdi.rs`（使用 `#[cfg(windows)]`）实现：
1. 持久化保存 `desktop_dc: HDC`, `mem_dc: HDC`, `hbitmap: HBITMAP`, `bitmap_w: u32`, `bitmap_h: u32`, `buffer: Vec<u8>`。
2. 捕获时使用 `SetStretchBltMode(mem_dc, HALFTONE)`，通过一次 `StretchBlt` 直接从屏幕物理 DC 抓取并缩放到 `target_w × target_h`。
3. 调用 `GetDIBits` 一次性将像素读入内部预分配的内存块，直接在局部将 BGR 调整为 RGB，避免全屏 20.7MB 逐像素标量循环。
4. 遇到显示分辨率变化时，才重新创建内存位图。

- [ ] **Step 4: 集成到 `stream.rs` 捕获循环中**

在 `crates/agent/src/stream.rs` 中：
1. 若运行于 Windows，优先使用持久化的 `GdiScreenCapturer`（而非每轮重新调用 `xcap::Monitor::capture_image()`）；
2. 捕获尺寸在进入循环时即由 `stream_output_size(orig_w, orig_h, scale)` 决定，直接以目标宽高采集；
3. `compute_block_hashes` 与 `encode_jpeg` 直接作用于已处于编码分辨率的像素缓冲上。

- [ ] **Step 5: 验证编译与性能测试**

运行：`cargo check -p at-pc-agent`
预期：PASS

- [ ] **Step 6: Git Commit**

```bash
git add crates/agent/src/ crates/desktop-core/tests/
git commit -m "perf(agent): align capture resolution to target stream size and reuse GDI DC buffers"
```

---

### Task 2: 【P0-2】输入事件即时唤醒采集循环与交互期脏检策略

**Files:**
- Modify: `crates/agent/src/stream.rs:219-280, 340-420`
- Modify: `crates/agent/src/ws_client.rs:90-115, 755-768`
- Test: `crates/agent/tests/stream_wake_test.rs`

**Interfaces:**
- Consumes: `DesktopStreamController::notify_interaction(&self)`
- Produces: 允许在收到 `DesktopInputEvent` 时通过 `tokio::sync::Notify` 立即打断 `sleep(current_interval)`，重置退避计数器并立即采集新一帧。

- [ ] **Step 1: 编写测试验证唤醒机制能重置退避计时器并立刻产生响应**

创建 `crates/agent/tests/stream_wake_test.rs`：
```rust
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

#[tokio::test]
async fn test_interaction_notifier_interrupts_idle_sleep() {
    let notify = Arc::new(Notify::new());
    let notify_clone = notify.clone();

    let start = tokio::time::Instant::now();
    let worker = tokio::spawn(async move {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(500)) => false,
            _ = notify_clone.notified() => true,
        }
    });

    tokio::time::sleep(Duration::from_millis(20)).await;
    notify.notify_one();

    let woke_by_notify = worker.await.unwrap();
    assert!(woke_by_notify, "Notify 必须在 500ms 睡眠中立即唤醒循环");
    assert!(start.elapsed() < Duration::from_millis(100), "唤醒耗时必须远小于 500ms");
}
```

- [ ] **Step 2: 运行测试验证 Notify 机制有效**

运行：`cargo test -p at-pc-agent --test stream_wake_test`
预期：PASS

- [ ] **Step 3: 在 `DesktopStreamController` 中接入交互通知与窗口期**

在 `crates/agent/src/stream.rs` 中：
1. 在 `DesktopStreamController` 中增加 `interaction_notify: Arc<tokio::sync::Notify>` 与 `last_interaction_time: Arc<AtomicU64>`；
2. 暴露 `pub fn notify_interaction(&self)` 方法，更新最后交互毫秒时间戳并调用 `interaction_notify.notify_one()`；
3. 在推流 `while` 循环内部等待节拍时，将 `tokio::time::sleep` 改为：
   ```rust
   tokio::select! {
       _ = tokio::time::sleep(remaining) => {},
       _ = interaction_notify.notified() => {
           consecutive_unchanged = 0; // 重置静止计数
       }
   }
   ```
4. 若距离上次交互在 500ms 窗口期内，只要画面发生任意变化（`ratio > 0.0`），即视为脏帧立即编码发出，保证微小移动或悬停高亮绝不被吞。

- [ ] **Step 4: 在 `AgentWsClient` 收到输入消息时调用通知**

在 `crates/agent/src/ws_client.rs`：
在处理 `ServerToAgentMessage::DesktopInput { event }` 时，除了投递给 `input_tx`，同步调用 `stream_controller.notify_interaction()`。

- [ ] **Step 5: 验证编译与测试**

运行：`cargo test -p at-pc-agent`
预期：PASS

- [ ] **Step 6: Git Commit**

```bash
git add crates/agent/src/stream.rs crates/agent/src/ws_client.rs crates/agent/tests/
git commit -m "feat(agent): wake up desktop capture loop instantly upon user input events"
```

---

### Task 3: 【P1-1】Windows 远端光标绘制合成 (Cursor Synthesis)

**Files:**
- Modify: `crates/agent/src/capture/windows_gdi.rs` (或 `crates/agent/src/stream.rs`)
- Test: `crates/desktop-core/tests/cursor_blend_test.rs`

**Interfaces:**
- Consumes: Win32 API `GetCursorInfo`, `GetIconInfo`, `DrawIconEx`
- Produces: 捕获出的内存 DC 画面在对应位置精确印上系统当前真实鼠标指针。

- [ ] **Step 1: 编写测试验证光标合成后的块哈希变化**

创建 `crates/desktop-core/tests/cursor_blend_test.rs`：
```rust
use at_pc_desktop_core::{compute_block_hashes, dirty_block_ratio};

#[test]
fn test_cursor_drawing_causes_dirty_block() {
    let w = 1280;
    let h = 800;
    let mut bg = vec![200u8; (w * h * 4) as usize];
    let baseline = compute_block_hashes(&bg, w, h);

    // 模拟在 (100, 100) 绘制一个 32x32 箭头光标
    for y in 100..132 {
        for x in 100..132 {
            let idx = (y * w + x) * 4;
            bg[idx] = 0;
            bg[idx + 1] = 0;
            bg[idx + 2] = 0;
        }
    }
    let with_cursor = compute_block_hashes(&bg, w, h);
    assert_ne!(baseline, with_cursor, "合成光标必须改变对应块的哈希");
    let ratio = dirty_block_ratio(&baseline, &with_cursor);
    assert!(ratio > 0.0, "脏块率必须大于 0");
}
```

- [ ] **Step 2: 运行测试**

运行：`cargo test -p at-pc-desktop-core --test cursor_blend_test`
预期：PASS

- [ ] **Step 3: 在 `windows_gdi.rs` 的捕获流程末尾添加原生光标绘制**

在 Windows GDI 捕获中：
```rust
unsafe {
    let mut ci: CURSORINFO = std::mem::zeroed();
    ci.cbSize = std::mem::size_of::<CURSORINFO>() as u32;
    if GetCursorInfo(&mut ci) != 0 && (ci.flags & CURSOR_SHOWING) != 0 {
        let mut ii: ICONINFO = std::mem::zeroed();
        if GetIconInfo(ci.hCursor, &mut ii) != 0 {
            let cursor_x = ci.ptScreenPos.x - ii.xHotspot as i32;
            let cursor_y = ci.ptScreenPos.y - ii.yHotspot as i32;
            
            // 按 target_w / orig_w 映射到下采样后的坐标
            let draw_x = (cursor_x as f64 * target_w as f64 / orig_w as f64).round() as i32;
            let draw_y = (cursor_y as f64 * target_h as f64 / orig_h as f64).round() as i32;
            let draw_w = (32.0 * target_w as f64 / orig_w as f64).round().max(16.0) as i32;
            let draw_h = (32.0 * target_h as f64 / orig_h as f64).round().max(16.0) as i32;

            DrawIconEx(mem_dc, draw_x, draw_y, ci.hCursor, draw_w, draw_h, 0, 0, DI_NORMAL);

            if ii.hbmColor != 0 { DeleteObject(ii.hbmColor); }
            if ii.hbmMask != 0 { DeleteObject(ii.hbmMask); }
        }
    }
}
```

- [ ] **Step 4: 验证编译通过**

运行：`cargo check -p at-pc-agent`
预期：PASS

- [ ] **Step 5: Git Commit**

```bash
git add crates/agent/src/ crates/desktop-core/tests/
git commit -m "feat(agent): synthesize hardware mouse cursor into captured desktop frames on Windows"
```

---

## 阶段二：Milestone 2 - P1 服务去冗余与通道公平调度

### Task 4: 【P1-3】Server 推流路径去冗余拷贝与按需 Base64 优化

**Files:**
- Modify: `crates/server/src/router.rs:840-880`
- Modify: `crates/server/src/mcp/dashboard.rs:980-1005`
- Test: `crates/server/tests/stream_routing_test.rs`

**Interfaces:**
- Consumes: `BinaryDesktopFrame`
- Produces: 仅在有订阅者时编码二进制；取消帧缓存中每帧必做 Base64 编码，改为在 REST 读取时懒生成；消除 WS 推送每订阅者 `clone()` 堆分配。

- [ ] **Step 1: 编写测试验证没有订阅者时不浪费编码，有订阅者时正常分发**

在 `crates/server/tests/stream_routing_test.rs` 补充用例，断言 `handle_desktop_frame_binary` 的正确分发。

- [ ] **Step 2: 改造 `crates/server/src/router.rs`**

1. 修改 `handle_desktop_frame_binary`：
   - 检查 `self.desktop_frame_senders` 是否存在且 `tx.receiver_count() > 0`。若有，调用 `Arc::new(frame.encode())` 并广播；若无订阅者，完全跳过 `frame.encode()`；
   - 写入 `self.desktop_frames` 缓存时：**移除** `base64::prelude::BASE64_STANDARD.encode(&frame.data)`，`data` 字段保持为空或标记为懒加载；仅保留 `raw_bytes: frame.data`。
2. 在 `api_get_desktop_frame`（JSON 端点）中：当用户显式请求 JSON 格式桌面帧时，按需对 `raw_bytes` 执行 Base64 序列化。

- [ ] **Step 3: 改造 `crates/server/src/mcp/dashboard.rs` 中的二进制下发**

在 `handle_desktop_ws_session`：
使用 `axum::extract::ws::Message::Binary(frame_bytes.to_vec())` 或直接支持引用型字节传输，消除冗余中间副本。

- [ ] **Step 4: 验证测试与编译**

运行：`cargo test -p at-pc-server`
预期：PASS

- [ ] **Step 5: Git Commit**

```bash
git add crates/server/src/router.rs crates/server/src/mcp/dashboard.rs
git commit -m "perf(server): eliminate unconditional base64 encoding and redundant frame heap copies"
```

---

### Task 5: 【P1-4】WebSocket 发送端优先级公平调度与通道隔离

**Files:**
- Modify: `crates/server/src/ws/handler.rs:180-236`
- Modify: `crates/agent/src/ws_client.rs:512-555`

**Interfaces:**
- Consumes: `control_rx`, `binary_rx`
- Produces: 非极度偏置的平衡轮询调度机制，保证单次大控制消息传输不会无限期饿死视频帧。

- [ ] **Step 1: 优化 `ws/handler.rs` 与 `agent/src/ws_client.rs` 中的 `tokio::select!` 调度**

1. 保持 Pong 帧具备最高绝对优先权（保证连接存活）；
2. 去除控制通道与视频帧通道之间的绝对 `biased`；
3. 为控制通道增加单次突发上限（例如连续处理最多 3 条控制消息后，必须让步检查一次视频帧通道）；
4. 确保视频二进制帧在满屏运动时拥有稳定的交付节拍。

- [ ] **Step 2: 验证编译与长任务不阻塞推流**

运行：`cargo check --workspace`
预期：PASS

- [ ] **Step 3: Git Commit**

```bash
git add crates/server/src/ws/handler.rs crates/agent/src/ws_client.rs
git commit -m "perf(ws): implement fair scheduling between control messages and desktop video frames"
```

---

### Task 6: 【R4 & R5】修复鼠标事件透传：合并 `MouseMovePixel` 与消除双重注入

**Files:**
- Modify: `crates/server/src/ws/handler.rs:195-218`
- Modify: `crates/agent/src/input.rs:17-71`
- Test: `crates/server/tests/input_coalesce_test.rs`

**Interfaces:**
- Consumes: `DesktopInputEvent::MouseMovePixel`
- Produces: 服务端连续像素鼠标移动合并为最后一次移动；Agent 注入时消除重复系统调用。

- [ ] **Step 1: 编写测试验证 `MouseMovePixel` 能够被队列合并**

创建 `crates/server/tests/input_coalesce_test.rs`：
测试连续发送 10 个 `MouseMovePixel` 消息，在转发通道排队时仅保留最后一个坐标。

- [ ] **Step 2: 在 `server/src/ws/handler.rs` 中支持 `MouseMovePixel` 聚合**

修改匹配分支：
```rust
let is_coalesceable_mouse = |m: &ServerToAgentMessage| matches!(
    m,
    ServerToAgentMessage::DesktopInput {
        event: at_pc_protocol::models::DesktopInputEvent::MouseMove { .. }
            | at_pc_protocol::models::DesktopInputEvent::MouseMovePixel { .. }
    }
);
```
遇到连续的鼠标移动消息时，持续 `try_recv()` 合并为最新坐标。

- [ ] **Step 3: 在 `agent/src/input.rs` 中消除重复注入**

在 Windows 分支下：
删除第 27 行的 `SetCursorPos(px, py);`，全权由 `SendInput(1, &input, ...)` 的 `MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_MOVE | MOUSEEVENTF_VIRTUALDESK` 负责光标定位与窗口消息生成，消除“双跳”顿挫感。

- [ ] **Step 4: 运行测试**

运行：`cargo test -p at-pc-server --test input_coalesce_test`
预期：PASS

- [ ] **Step 5: Git Commit**

```bash
git add crates/server/src/ws/handler.rs crates/agent/src/input.rs crates/server/tests/
git commit -m "fix(input): coalesce MouseMovePixel events and eliminate duplicate cursor injection"
```

---

## 阶段三：Milestone 3 - P2 Web 前端体验重塑与会话生命周期治理

### Task 7: 【7.4(a) & R1】画面分辨率协商与 1.62× 模糊消除

**Files:**
- Modify: `crates/server/src/mcp/dashboard.rs:70-85, 963-982`
- Modify: `crates/server/frontend/src/desktop/session.js:510-549`
- Modify: `crates/server/frontend/src/desktop/index.js:20-50`

**Interfaces:**
- Consumes: 前端 Canvas 的设备像素宽 `clientWidth * window.devicePixelRatio`
- Produces: 客户端在请求桌面 WebSocket 时带上 `?scale=` 或 `?target_w=` 参数，服务端透传给 Agent，避免强制降采样到 1280 宽后在 2067 宽屏幕上上采样模糊。

- [ ] **Step 1: 前端计算实际物理分辨率并在 WS 握手 URL 中携带**

在 `crates/server/frontend/src/desktop/session.js`：
根据 `wrapper.clientWidth * window.devicePixelRatio` 计算出期望的目标显示宽，在请求 WS URL 时附加查询参数（如 `?target_width=...&dpr=...` 或根据比例计算出的 `scale`）。

- [ ] **Step 2: 服务端 `dashboard.rs` 支持提取查询参数并透传**

在 `api_desktop_ws_stream` / `handle_desktop_ws_session` 中：
提取 Query 参数 `DesktopStreamParams { fps, quality, scale }`；若前端未传则使用安全默认值；调用 `start_desktop_stream(&terminal_id, fps, quality, display_index, scale)`，不再强行抹为 `(15, 60, 0, 1.0)`。

- [ ] **Step 3: 验证编译与前端语法检查**

运行：`cargo check -p at-pc-server`
预期：PASS

- [ ] **Step 4: Git Commit**

```bash
git add crates/server/src/mcp/dashboard.rs crates/server/frontend/src/desktop/
git commit -m "feat(desktop): enable dynamic scale/resolution negotiation to eliminate blur on HiDPI displays"
```

---

### Task 8: 【7.4(b)】静止状态 HUD 反馈修正与 Keepalive 状态感知

**Files:**
- Modify: `crates/server/frontend/src/desktop/session.js:550-600, 648-658`

**Interfaces:**
- Consumes: `jpegBytes.length === 0` (Keepalive 帧)
- Produces: 更新上次活跃时间戳，HUD 区分显示“15 FPS (推流中)”与“待机 (画面静止)”，消除 `0 FPS` 假死体感。

- [ ] **Step 1: 在 `session.js` 中增加 `lastKeepaliveTime` 并在收到 Keepalive 帧时打点**

在 `ws.onmessage` 中：
```javascript
if (!jpegBytes || jpegBytes.length === 0) {
    desktopState.lastActiveTime = Date.now();
    desktopState.isIdleStandby = true;
    return;
}
desktopState.isIdleStandby = false;
desktopState.lastActiveTime = Date.now();
```

- [ ] **Step 2: 改造 HUD FPS 刷新定时器**

在 `desktopState.fpsTimer` 定时器中：
```javascript
const now = Date.now();
const elapsed = (now - desktopState.lastFrameTime) / 1000;
if (elapsed > 0) {
    const currentFps = Math.round(desktopState.frameCount / elapsed);
    if (desktopState.frameCount > 0) {
        if (fpsEl) fpsEl.innerText = `${currentFps} FPS`;
    } else if (desktopState.isIdleStandby || (now - desktopState.lastActiveTime < 2000)) {
        if (fpsEl) fpsEl.innerText = `静止 (待机)`;
    } else {
        if (fpsEl) fpsEl.innerText = `0 FPS`;
    }
}
```

- [ ] **Step 3: Git Commit**

```bash
git add crates/server/frontend/src/desktop/session.js
git commit -m "fix(frontend): update HUD to show standby state on keepalive instead of misleading 0 FPS"
```

---

### Task 9: 【7.4(e) & 7.4(f) & R2 & R7】清理 HTTP 死代码、WS 断线重连与多观众引用计数

**Files:**
- Modify: `crates/server/frontend/src/desktop/session.js:105-175, 630-645`
- Modify: `crates/server/src/router.rs:798-830`
- Modify: `crates/server/src/mcp/dashboard.rs:1040-1050`
- Test: `crates/server/tests/multi_viewer_test.rs`

**Interfaces:**
- Consumes: WebSocket 连接打开/断开
- Produces: 多观众连接时不互踢；订阅者完全归零时安全 stop；WS 断开后自动退避重连；清理前端无用 HTTP 队列。

- [ ] **Step 1: 编写测试验证多观众订阅引用计数**

创建 `crates/server/tests/multi_viewer_test.rs`：
验证当有两个 WS 连接订阅同一终端的画面流时，关闭其中一个连接，后台推流不会被立即停止，只有当第二个连接也断开时才触发 `StopDesktopStream`。

- [ ] **Step 2: 在 `router.rs` 中为推流实现多观众引用计数管理**

1. 记录每个 `terminal_id` 的活动订阅连接数（通过 `desktop_frame_senders` 内部 `tx.receiver_count()` 或独立计数器）；
2. 在 `dashboard.rs` 的 `handle_desktop_ws_session` 退出时，检查剩余订阅者：
   ```rust
   let remaining = state.router.get_desktop_stream_subscribers(&terminal_id);
   if remaining == 0 {
       let _ = state.router.stop_desktop_stream(&terminal_id).await;
   }
   ```
3. 当订阅者为 0 时，清理 `desktop_frame_senders` 中过期的 broadcast sender，避免内存泄露。

- [ ] **Step 3: 前端清理 HTTP 死代码并引入断线重连**

1. 删除 `session.js` 中 `inputQueue` / `pendingMouseMove` / `isFlushingMove` / `flushMouseMove` 等旧 HTTP 轮询死代码；
2. 在 `ws.onclose` 中，若当前弹窗仍处于打开状态（`desktopState.currentStreamTid === tid`），启动指数退避定时器（1s、2s、4s...）自动重新获取 ticket 并重连 WebSocket。

- [ ] **Step 4: 运行所有测试**

运行：`cargo test --workspace`
预期：PASS

- [ ] **Step 5: Git Commit**

```bash
git add crates/server/ crates/server/frontend/
git commit -m "refactor(desktop): cleanup legacy HTTP dead code, add auto-reconnect and multi-viewer refcounting"
```

---

## 阶段四：端到端量化验收方案

优化全部落地后，通过原归因探针脚本逐项运行验证：

```bash
cd /tmp/atpc-ws-probe

# 1. 验证吞吐与帧率（需远端运行 anim.ps1 制造真实变化）：
ATPC_TOK=<token> ATPC_TID=<terminal> node probe3.mjs
# 验收目标：effective_fps >= 15（基线 11.2 fps），每帧 CPU < 15 ms

# 2. 验证首次交互时延：
ATPC_TOK=<token> ATPC_TID=<terminal> ATPC_IDLE_MS=1200 node probe2.mjs
# 验收目标：p50 首次交互响应时延 < 80ms（基线 614ms），且 0 次超时未响应

# 3. 验证端到端浏览器表现与鼠标移动即时刷新：
node web-probe.mjs
# 验收目标：鼠标移动后 80ms 内产生新帧（基线 8/8 零帧）；HUD 静止时不显示 0 FPS

# 4. 验证高负载与画质：
node web-probe-A.mjs
# 验收目标：画布无模糊拉伸，MCP 工具并发调用期间视频帧间隔 p90 < 100ms
```
