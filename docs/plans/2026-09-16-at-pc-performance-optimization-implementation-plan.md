# at-pc 全链路性能优化实施计划 (2026-09-16)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 基于 `2026-09-16-at-pc-performance-deep-dive-ii.md` 实测与 `2026-09-16-added-content-verification.md` 复核结论，分三阶段（P0/P1/P2）彻底消除脏检 240x 退化、静态桌面 32% 空转、动作审查 42.5ms 交互阻塞、高频 MouseMove 26.4MB/h 磁盘轰炸，以及 5000 终端元数据锁线性退化等全量性能隐患。

**Architecture:**
1. **推流热路径（Agent / Core）**：将 `desktop-core` 的 64x64 逐字节 FNV-1a 升级为 8 字节字宽分组哈希（提速 14x）；在 `stream.rs` 中将 keepalive 与空闲判定前置，加入动态退避调度，静态桌面将 15fps 探测降低至 1-2fps，彻底消除空转。
2. **交互审查与 I/O 路径（Agent / Server）**：在 macOS 上以原生 `CGWindowListCopyWindowInfo` 替代开销高昂的 `xcap` 窗口枚举与逐窗跨进程属性遍历（耗时从 21.4ms 直降至 0.14ms）；在 `dashboard.rs` 拦截过滤 `MouseMove` 磁盘写审计；在 `process.rs` 为持久 `System` 增加 TTL 缓存（2.65ms -> 0.15ms）。
3. **传输层与观察工具（Server / Screen）**：服务端在帧入库时预计算或直接消费二进制 raw 端点（削减 25.1% 载荷与 Base64 重算）；`capture_screen` 默认下采样至 1280 宽削减 70% Token 载荷并清理 4 份冗余 Base64；`meta_store` 将整表深拷贝重构为写时复制/Arc 快照。

**Tech Stack:** Rust (Edition 2021, Tokio, Axum, sysinfo, core-graphics, image, criterion)

**Spec:** [2026-09-16-at-pc-performance-deep-dive-ii.md](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/docs/plans/2026-09-16-at-pc-performance-deep-dive-ii.md) 以及 [2026-09-16-added-content-verification.md](file:///Users/clkj/%E9%A1%B9%E7%9B%AE/at/at-pc/docs/plans/2026-09-16-added-content-verification.md)

## Global Constraints

- **代码质量与稳定性**：所有改动不得破坏现有协议契约（WebSocket 消息结构、JSON 接口入参与响应兼容）。
- **平台兼容性**：macOS 平台专属优化必须以 `#[cfg(target_os = "macos")]` 隔离，保留 Linux/Windows 的现有跨平台回退路径。
- **性能度量真实性**：每项任务必须提供真实单元测试/基准测试前后对比数据，拒绝主观估算，拒绝把网络抖动噪声（$\sigma \approx 0.35$ ms）作为微基准论据。
- **依赖控制**：禁止引入重量级新依赖；macOS 原生调用仅复用已有的 `core-graphics` / `core-foundation`。

---

## 里程碑总览与量化验证指标

| 里程碑 | 包含优化项 | 核心改进点 | 可量化验证目标 |
|:---|:---|:---|:---|
| **Milestone 1: P0 热路径核心与门禁** | P0-A, P0-B, P0-C | 词宽哈希、基准门禁、抓屏判定前置与空闲退避 | ① 1080p 脏检从 7.85ms 降至 <0.6ms（**>13x**）<br>② 静态桌面 Agent CPU 从 32% 降至 **<4%**<br>③ CI 新增 block hash 性能阈值门禁 |
| **Milestone 2: P1 交互低延迟与 I/O 瘦身** | P1-A, P1-B/C, P1-D, P1-E, P1-F | 修复探针、二进制流/预算 Base64、keepalive 收紧、MouseMove 审计降级、macOS 审查重构 | ① 单次交互动作审查耗时从 40.6ms 降至 **<1ms**（省 40ms）<br>② 3s 86 次鼠标移动磁盘日志量从 22.6KB 降至 **0KB**<br>③ 流式出网带宽从 2.90MB/s 稳定降至 **2.18MB/s**（-25.1%）<br>④ 修复 `perf_probe` panic |
| **Milestone 3: P2 长效扩展性与观察瘦身** | P2-A, P2-C, P2-E, P2-F | 进程 TTL 缓存、元数据写锁轻量化、截图载荷削减与去重、SoM 优化 | ① `list_processes` 耗时从 2.65ms 降至 **<0.2ms**（**>13x**）<br>② 5,000 终端规模下写锁从 0.64ms 降至 **<0.05ms**<br>③ 截图上下文载荷从 348KB 降至 **<110KB**（-70%） |

---

## 阶段一：Milestone 1 - P0 热路径核心与基准门禁

### Task 1: 【P0-A】`desktop-core` 词宽 8 字节分组哈希替换与行为一致性测试

**Files:**
- Modify: `crates/desktop-core/src/lib.rs:88-130`
- Test: `crates/desktop-core/tests/block_hash_test.rs`

**Interfaces:**
- Consumes: `raw_rgba: &[u8]`, `width: u32`, `height: u32`, `block_size: u32`
- Produces: `pub fn compute_block_hashes(raw_rgba: &[u8], width: u32, height: u32) -> Vec<u64>` (签名与行为不变，内部执行 8 字节字宽加速)

- [ ] **Step 1: 编写测试用例验证 8 字节字宽哈希对单像素/单字节改动的敏感性与幂等性**

创建 `crates/desktop-core/tests/block_hash_test.rs`：
```rust
use at_pc_desktop_core::{compute_block_hashes, dirty_block_ratio};

#[test]
fn test_block_hash_determinism_and_single_byte_mutation() {
    let width = 1920;
    let height = 1080;
    let mut buffer = vec![128u8; (width * height * 4) as usize];
    
    let baseline = compute_block_hashes(&buffer, width, height);
    let second = compute_block_hashes(&buffer, width, height);
    assert_eq!(baseline, second, "哈希计算必须具备确定性与幂等性");

    // 修改单像素的单通道字节（测试双射覆盖）
    buffer[width as usize * 4 * 100 + 200 * 4 + 1] ^= 0x01;
    let mutated = compute_block_hashes(&buffer, width, height);
    assert_ne!(baseline, mutated, "单字节变更必须导致块哈希改变");

    let ratio = dirty_block_ratio(&baseline, &mutated);
    assert!(ratio > 0.0 && ratio < 0.01, "仅应引起单块变化");
}
```

- [ ] **Step 2: 运行测试确保当前基线通过**

运行：`cargo test -p at-pc-desktop-core --test block_hash_test`
预期：PASS

- [ ] **Step 3: 实现 8 字节字宽分组哈希（Word-wise Hash）替换逐字节 FNV-1a**

修改 `crates/desktop-core/src/lib.rs` 中的 `compute_block_hashes_sized`：
```rust
pub fn compute_block_hashes_sized(
    raw_rgba: &[u8],
    width: u32,
    height: u32,
    block_size: u32,
) -> Vec<u64> {
    if width == 0 || height == 0 || block_size == 0 {
        return Vec::new();
    }
    let cols = width.div_ceil(block_size);
    let rows = height.div_ceil(block_size);
    let stride = (width as usize).saturating_mul(4);
    let mut hashes = Vec::with_capacity((cols * rows) as usize);

    const MULTIPLIER: u64 = 0x9e3779b97f4a7c15;

    for by in 0..rows {
        let y0 = by * block_size;
        let y1 = (y0 + block_size).min(height);
        for bx in 0..cols {
            let x0 = bx * block_size;
            let x1 = (x0 + block_size).min(width);
            let mut hash: u64 = 0xcbf29ce484222325;
            for y in y0..y1 {
                let row = (y as usize).saturating_mul(stride);
                let start = row + (x0 as usize) * 4;
                let end = row + (x1 as usize) * 4;
                if end > raw_rgba.len() {
                    break;
                }
                let mut chunk = &raw_rgba[start..end];
                // 8 字节一组快速处理
                while chunk.len() >= 8 {
                    let word = u64::from_le_bytes(chunk[..8].try_into().unwrap());
                    hash = hash.rotate_left(13) ^ word.wrapping_mul(MULTIPLIER);
                    chunk = &chunk[8..];
                }
                // 尾部不足 8 字节回退逐字节
                for &byte in chunk {
                    hash = hash.rotate_left(13) ^ (u64::from(byte)).wrapping_mul(MULTIPLIER);
                }
            }
            hashes.push(hash);
        }
    }
    hashes
}
```

- [ ] **Step 4: 运行单元测试与回归测试**

运行：`cargo test -p at-pc-desktop-core`
预期：所有现有测试与新建测试全部通过。

- [ ] **Step 5: 验证性能提升**

运行微基准：`cargo run --release -p at-pc-benchmarks --example p1_hot_paths`
量化指标：1080p 块哈希从 ~7.85 ms 降至 <0.60 ms（提速 >13x）。

- [ ] **Step 6: Git 提交**

```bash
git add crates/desktop-core/
git commit -m "perf(desktop-core): replace byte-by-byte FNV-1a with word-wise 64-bit block hash"
```

---

### Task 2: 【P0-B】基准函数指向修复与 CI 性能门禁建立

**Files:**
- Modify: `crates/benchmarks/benches/p1_hot_paths.rs`
- Modify: `crates/benchmarks/benches/p2_perf_deep.rs`
- Modify: `thresholds.toml`
- Modify: `.github/workflows/ci.yml` (或对应 CI 流程)

**Interfaces:**
- Consumes: `compute_block_hashes`
- Produces: 自动化基准测试与性能门禁，阻断超过阈值（如 1080p 哈希 >1.5ms）的代码合并。

- [ ] **Step 1: 修正基准代码中的测量函数**

检查并修改 `crates/benchmarks/benches/p1_hot_paths.rs` 与 `p2_perf_deep.rs`，将测量旧函数 `compute_sample_hash` 全部更新为生产实际调用的 `compute_block_hashes`：
```rust
// 原代码测的是废弃函数:
// b.iter(|| compute_sample_hash(&raw, width, height));
// 修改为:
b.iter(|| compute_block_hashes(&raw, width, height));
```

- [ ] **Step 2: 更新 `thresholds.toml` 设定硬门禁**

在 `thresholds.toml` 中配置：
```toml
[benchmarks.desktop_core]
block_hash_1080p_ms_max = 1.00
block_hash_2560_ms_max = 2.00
```

- [ ] **Step 3: 运行基准验证门禁通过**

运行：`cargo bench -p at-pc-benchmarks --bench p1_hot_paths -- --nocapture`
预期：基准通过，1080p 块哈希耗时稳定低于 0.8ms。

- [ ] **Step 4: 在 CI 中接入自动化校验脚本**

编写检查脚本 `scripts/check_perf_thresholds.sh` 并加入 CI 任务中执行。

- [ ] **Step 5: Git 提交**

```bash
git add crates/benchmarks/ thresholds.toml .github/
git commit -m "ci(benchmarks): update benchmarks to measure compute_block_hashes and enforce thresholds"
```

---

### Task 3: 【P0-C】`stream.rs` 抓屏前置判定与空闲动态退避机制

**Files:**
- Modify: `crates/agent/src/stream.rs:225-275`
- Modify: `crates/desktop-core/src/lib.rs` (定义退避参数与决策)
- Test: `crates/agent/tests/stream_backoff_test.rs`

**Interfaces:**
- Consumes: `STREAM_KEEPALIVE_INTERVAL`, `consecutive_unchanged_frames`
- Produces: 静态桌面下探测频率动态降至 1~2 Hz，动态帧恢复全速 15 fps。

- [ ] **Step 1: 编写退避逻辑单元测试**

创建 `crates/agent/tests/stream_backoff_test.rs`：
```rust
#[test]
fn test_dynamic_backoff_idle_detection() {
    let mut idle_count = 0;
    let target_fps = 15;
    
    // 模拟连续 5 帧无变化进入退避
    for _ in 0..5 {
        idle_count += 1;
    }
    let effective_fps = if idle_count >= 5 { 2 } else { target_fps };
    assert_eq!(effective_fps, 2, "连续无变化帧应退避至 2 fps");

    // 模拟新脏帧立刻打断退避
    idle_count = 0;
    let effective_fps = if idle_count >= 5 { 2 } else { target_fps };
    assert_eq!(effective_fps, 15, "发现变化后应立即恢复满帧率");
}
```

- [ ] **Step 2: 在 `crates/agent/src/stream.rs` 实现空闲退避**

在流循环中维护 `consecutive_unchanged: u32`：
```rust
let is_idle = consecutive_unchanged >= 5;
let loop_delay = if is_idle {
    Duration::from_millis(500) // 空闲时降为 2 Hz 探测
} else {
    frame_interval // 默认 15 fps (66 ms)
};
```
当准备帧为 `PreparedFrame::Unchanged` 时 `consecutive_unchanged += 1`；当准备帧为 `PreparedFrame::Ready` 时重置 `consecutive_unchanged = 0`。

- [ ] **Step 3: 运行测试验证**

运行：`cargo test -p at-pc-agent --test stream_backoff_test`
预期：PASS

- [ ] **Step 4: 端到端验证静态桌面 CPU 占用**

起真实 Agent 与 Server，保持桌面静止 10 秒：
量化指标：Agent CPU 单核占用从 ~32% 降至 **<4%**。

- [ ] **Step 5: Git 提交**

```bash
git add crates/agent/src/stream.rs crates/agent/tests/
git commit -m "perf(agent): add dynamic idle backoff to screen capture loop"
```

---

## 阶段二：Milestone 2 - P1 交互低延迟、传输层改造与 I/O 瘦身

### Task 4: 【P1-A】修复 `perf_probe` 探针几何断言 panic

**Files:**
- Modify: `crates/benchmarks/examples/perf_probe.rs:335-346`

**Interfaces:**
- Consumes: `fast_rgba_to_rgb`, `fast_rgba_to_rgb_scaled`
- Produces: `perf_probe` 运行至第 8 节不 panic，正常输出所有转化与控制路径基准。

- [ ] **Step 1: 运行确认当前 panic 现场**

运行：`cargo run --release -p at-pc-benchmarks --example perf_probe`
预期：在第 6 节 `assertion left == right failed` panic。

- [ ] **Step 2: 调整 `conversion_headroom` 中的几何输入或断言逻辑**

在 `crates/benchmarks/examples/perf_probe.rs` 中：
由于生产的 `fast_rgba_to_rgb` 默认将宽缩放至 1280，使原型转换与生产转换在比较前保持同一目标几何，或使用 `fast_rgba_to_rgb_scaled(&rgba, 1.0)` 与原尺寸对比：
```rust
// 确保原型转换与参考输出在同一几何规格下比较
let (reference, target_w, target_h) = fast_rgba_to_rgb(&rgba);
let chunked_scaled = rgba_to_rgb_chunked_sized(&rgba, target_w, target_h);
assert_eq!(chunked_scaled.as_raw(), reference.as_raw());
```

- [ ] **Step 3: 验证完整运行**

运行：`cargo run --release -p at-pc-benchmarks --example perf_probe`
预期：所有 1–8 节完整输出，退出码为 0。

- [ ] **Step 4: Git 提交**

```bash
git add crates/benchmarks/examples/perf_probe.rs
git commit -m "fix(benchmarks): resolve geometry mismatch in perf_probe assertion"
```

---

### Task 5: 【P1-B/C】流式传输层改造：服务端入库预存 Base64 与前端二进制消费

**Files:**
- Modify: `crates/server/src/router.rs:754-762, 852-863`
- Modify: `crates/server/frontend/src/session.js` (或对应轮询模块)
- Test: `crates/server/tests/frame_stream_test.rs`

**Interfaces:**
- Consumes: `BinaryDesktopFrame`, `DesktopFrameCache`
- Produces:
  - 服务端入库时计算一次 Base64，读锁内消除 0.2ms 计算。
  - 前端支持消费 `/api/terminals/:id/desktop/frame.jpg` 二进制流。

- [ ] **Step 1: 编写服务端读锁 Base64 免计算单元测试**

在 `crates/server/tests/frame_stream_test.rs`：
验证多次调用 `get_latest_desktop_frame` 时，不会在读取阶段重复执行 base64 编码。

- [ ] **Step 2: 在 `handle_desktop_frame_binary` 帧入库时预计算 Base64**

修改 `crates/server/src/router.rs:852-863`：
```rust
let base64_str = base64::prelude::BASE64_STANDARD.encode(&frame.data);
frames.insert(
    terminal_id.to_string(),
    DesktopFrameCache {
        display_index: frame.display_index,
        width: frame.width,
        height: frame.height,
        format: "jpeg".to_string(),
        data: base64_str,
        raw_bytes: frame.data,
        timestamp: frame.timestamp,
    },
);
```
并在 `get_latest_desktop_frame` 中直接返回，移除 `read().unwrap()` 内的编码分支。

- [ ] **Step 3: 前端启用 `/frame.jpg` 二进制获取与解码**

在前端将轮询路径改为 `restPaths.desktopFrameRaw(tid)`（即 `/frame.jpg`），使用 `createImageBitmap(blob)` 渲染至 Canvas，规避大 JSON 解析与 Base64 字符串。

- [ ] **Step 4: 运行前后对比与网络带宽验证**

量化指标：
- 服务端读取锁内耗时从 0.198ms 降至 0.010ms（19.8x）。
- 15fps 流式传输出网带宽从 2.90MB/s 稳定降至 2.18MB/s（节省 25.1%）。

- [ ] **Step 5: Git 提交**

```bash
git add crates/server/src/router.rs crates/server/frontend/
git commit -m "perf(server): precompute base64 on ingest and enable raw binary frame endpoint"
```

---

### Task 6: 【P1-D】`STREAM_KEEPALIVE_INTERVAL` 收紧至 500ms

**Files:**
- Modify: `crates/desktop-core/src/lib.rs:162` (或 `stream.rs`)
- Test: `crates/desktop-core/tests/keepalive_test.rs`

**Interfaces:**
- Consumes: `elapsed_since_send`
- Produces: 保活包最大间隔由 5000ms 缩短为 500ms，微小光标闪烁等修改在最长 500ms 内强制刷新。

- [ ] **Step 1: 修改常量定义**

修改 `crates/desktop-core/src/lib.rs` 中的 `STREAM_KEEPALIVE_INTERVAL`：
```rust
pub const STREAM_KEEPALIVE_INTERVAL: Duration = Duration::from_millis(500);
```

- [ ] **Step 2: 运行所有相关测试**

运行：`cargo test -p at-pc-desktop-core`
运行：`cargo test -p at-pc-agent`
预期：所有测试通过。

- [ ] **Step 3: Git 提交**

```bash
git add crates/desktop-core/
git commit -m "fix(desktop-core): tighten stream keepalive interval from 5000ms to 500ms"
```

---

### Task 7: 【P1-E】输入审计降级：排除高频 `MouseMove` 磁盘写日志

**Files:**
- Modify: `crates/server/src/mcp/dashboard.rs:820-836`
- Test: `crates/server/tests/audit_filter_test.rs`

**Interfaces:**
- Consumes: `event: at_pc_protocol::models::DesktopInputEvent`
- Produces: 仅对 `MouseClick`, `KeyPress`, `TextInput` 等写安全审计日志；`MouseMove` 仅转发不落盘。

- [ ] **Step 1: 编写测试验证 `MouseMove` 不产生 `AuditRecord`**

在 `crates/server/tests/audit_filter_test.rs` 中验证：
向 `/api/terminals/:id/desktop/input` 发送 `MouseMove` 时，`audit.jsonl` 文件大小保持不变；发送 `MouseClick` 时正常增加审计行。

- [ ] **Step 2: 在 `dashboard.rs` 中增加事件类型判定**

修改 `crates/server/src/mcp/dashboard.rs:820`：
```rust
let should_audit = !matches!(event, at_pc_protocol::models::DesktopInputEvent::MouseMove { .. });
if should_audit {
    if let Some(logger) = state.router.audit_logger() {
        logger.log_async(crate::audit::AuditRecord {
            // ... 记录审计日志
        }).await;
    }
}
```

- [ ] **Step 3: 运行测试**

运行：`cargo test -p at-pc-server --test audit_filter_test`
预期：PASS

- [ ] **Step 4: 端到端验证日志产生速率**

模拟 28.6 Hz 鼠标移动 3 秒（86 次）：
量化指标：`audit.jsonl` 新增字节为 0，单小时写入量从 26.4 MB 降至 0 MB。

- [ ] **Step 5: Git 提交**

```bash
git add crates/server/src/mcp/dashboard.rs crates/server/tests/
git commit -m "perf(audit): exclude high-frequency MouseMove from disk audit log"
```

---

### Task 8: 【P1-F】macOS 前台动作审查重构：直连 `CGWindowListCopyWindowInfo`

**Files:**
- Modify: `crates/agent/src/tools/window.rs:510-607`
- Test: `crates/agent/tests/macos_window_review_test.rs`

**Interfaces:**
- Consumes: `core_graphics::window::copy_window_info`
- Produces: `capture_active_window_state() -> Option<WindowState>` 单次耗时从 21.4ms 降至 0.14ms（两次交互审查立省 42.5ms）。

- [ ] **Step 1: 编写 macOS 快速前台窗口测试用例**

在 `crates/agent/tests/macos_window_review_test.rs` 中（`#[cfg(target_os = "macos")]`）：
```rust
#[test]
fn test_macos_fast_window_capture() {
    let start = std::time::Instant::now();
    let state = at_pc_agent::tools::window::capture_active_window_state();
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
    println!("capture_active_window_state took: {:.2} ms", elapsed);
    assert!(elapsed < 5.0, "macOS 前台窗口审查耗时应严格控制在 5ms 以内，实际为 {:.2}ms", elapsed);
    assert!(state.is_some());
}
```

- [ ] **Step 2: 在 `crates/agent/src/tools/window.rs` 中为 macOS 提供快速路径**

利用已引入的 `core-graphics` / `core-foundation`：
```rust
#[cfg(target_os = "macos")]
pub fn capture_foreground_window_fast() -> Option<WindowState> {
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use core_graphics::window::{
        copy_window_info, kCGNullWindowID, kCGWindowListOptionOnScreenOnly,
        kCGWindowName, kCGWindowOwnerPID,
    };

    let array = copy_window_info(kCGWindowListOptionOnScreenOnly, kCGNullWindowID)?;
    let k_name = unsafe { CFString::wrap_under_get_rule(kCGWindowName) };
    let k_pid = unsafe { CFString::wrap_under_get_rule(kCGWindowOwnerPID) };

    for i in 0..array.len() {
        let Some(item) = array.get(i) else { continue };
        let ptr: *const std::ffi::c_void = *item;
        let dict: CFDictionary<CFString, CFType> = unsafe {
            TCFType::wrap_under_get_rule(ptr as CFDictionaryRef)
        };
        let title: Option<String> = dict
            .find(&k_name)
            .and_then(|v| v.downcast::<CFString>())
            .map(|s| s.to_string());
        if let Some(t) = title {
            if !t.trim().is_empty() {
                let pid = dict
                    .find(&k_pid)
                    .and_then(|v| v.downcast::<CFNumber>())
                    .and_then(|n| n.to_i64())
                    .unwrap_or(0) as usize;
                let is_dialog = t.contains("Dialog") || t.contains("Alert") || t.contains("确认") || t.contains("提示");
                return Some(WindowState {
                    hwnd: pid,
                    title: t,
                    is_dialog,
                });
            }
        }
    }
    None
}
```
并在 `non_windows_impl::capture_foreground_window()` 中优先调用该快速实现。

- [ ] **Step 3: 运行基准与单元测试验证**

运行：`cargo test -p at-pc-agent --test macos_window_review_test`
量化指标：单次审查耗时从 20.3~21.4 ms 降至 **<0.5 ms**（降幅 >97%），单次点击总注入延迟削减 40ms。

- [ ] **Step 4: Git 提交**

```bash
git add crates/agent/src/tools/window.rs crates/agent/tests/
git commit -m "perf(agent): optimize macOS window review using direct CGWindowListCopyWindowInfo"
```

---

## 阶段三：Milestone 3 - P2 长效扩展性、进程查询与观察路径瘦身

### Task 9: 【P2-F】`list_processes` 引入持久 TTL 缓存

**Files:**
- Modify: `crates/agent/src/tools/process.rs:20-46`
- Test: `crates/agent/tests/process_cache_test.rs`

**Interfaces:**
- Consumes: `sys.refresh_processes()`, `Duration::from_millis(1000)`
- Produces: 1 秒内重复调用无需反复枚举全系统 800+ 进程，耗时从 2.65ms 降至 0.15ms（17x）。

- [ ] **Step 1: 编写进程缓存时效性单元测试**

在 `crates/agent/tests/process_cache_test.rs`：
测试在 200ms 内连续调用两次 `list_processes(None, None, 10)`，第二次调用应在 <0.3ms 内返回，且数据一致。

- [ ] **Step 2: 为 `SYSTEM_CACHE` 增加上次刷新时间戳**

在 `crates/agent/src/tools/process.rs` 中重构缓存结构：
```rust
struct CachedSystemState {
    sys: System,
    last_refresh: std::time::Instant,
}

static SYSTEM_CACHE: OnceLock<Mutex<CachedSystemState>> = OnceLock::new();
const PROCESS_CACHE_TTL: Duration = Duration::from_millis(1000);
```
在 `list_processes` 中：
```rust
let mut guard = get_cached_system().lock().unwrap();
if guard.last_refresh.elapsed() >= PROCESS_CACHE_TTL {
    guard.sys.refresh_processes();
    guard.last_refresh = std::time::Instant::now();
}
```

- [ ] **Step 3: 运行测试**

运行：`cargo test -p at-pc-agent --test process_cache_test`
预期：PASS，第二次调用耗时稳定在 0.15ms 左右。

- [ ] **Step 4: Git 提交**

```bash
git add crates/agent/src/tools/process.rs crates/agent/tests/
git commit -m "perf(agent): add 1s TTL cache to sys.refresh_processes in list_processes"
```

---

### Task 10: 【P2-E】`meta_store` 写路径轻量化

**Files:**
- Modify: `crates/server/src/meta_store.rs:216-235`
- Modify: `crates/server/src/ws/registry.rs:260-285`
- Test: `crates/server/tests/registry_scaling_test.rs`

**Interfaces:**
- Consumes: `Arc<HashMap<String, TerminalRecord>>`
- Produces: 在 5,000 终端规模下写锁开销从 0.64ms 降至 <0.05ms，消除整表克隆瓶颈。

- [ ] **Step 1: 编写 1,000 ~ 5,000 规模扩展性压测用例**

在 `crates/server/tests/registry_scaling_test.rs`：
构造 1,000 个终端，循环调用 `update_meta` 100 次，统计 p50/p95 延迟。

- [ ] **Step 2: 重构 `StoreState` 中的快照引用机制**

在 `crates/server/src/meta_store.rs` 中：
让 `state.records` 本身由 `Arc` 包装；写操作只在变更时执行浅层替换或使用共享 Arc，使快照读取 `snapshot_locked` 为 $O(1)$ 的 Arc 克隆，避免逐项克隆 5,000 条记录。

- [ ] **Step 3: 验证压测结果**

运行：`cargo test --release -p at-pc-server --test registry_scaling_test -- --nocapture`
量化指标：5,000 终端下单次写元数据耗时从 0.64ms 降至 <0.05ms。

- [ ] **Step 4: Git 提交**

```bash
git add crates/server/src/meta_store.rs crates/server/src/ws/registry.rs
git commit -m "perf(server): optimize metadata store write lock to eliminate full map clones"
```

---

### Task 11: 【P2-A】`capture_screen` 观察路径瘦身：默认 1280 上限与 4 份 Base64 清理

**Files:**
- Modify: `crates/agent/src/tools/screen.rs:280-355`
- Test: `crates/agent/tests/screen_payload_test.rs`

**Interfaces:**
- Consumes: `ScreenCaptureResult`
- Produces: 默认上下文截屏输出体积从 348 KB 降至 100 KB（削减 70% Token 消耗），消除堆内存中 4 份相同图像字符串冗余。

- [ ] **Step 1: 编写输出结果体积与字段引用测试**

在 `crates/agent/tests/screen_payload_test.rs`：
验证 `capture_screen` 在未传 `max_dimension` 时默认不超过 1280，产物 Base64 大小在真实桌面上小于 120 KB；验证 `ScreenCaptureResult` 字段序列化兼容性。

- [ ] **Step 2: 清理 4 份冗余 Base64 并优化缩放**

在 `crates/agent/src/tools/screen.rs` 中：
1. 默认设置 `max_dimension = Some(1280)`；
2. 缩放算法前先 `to_rgb8()` 再缩放，将 Triangle 耗时从 23.6ms 压低；
3. `ScreenCaptureResult` 内部各字段共用同一个 `Arc<str>` 或按需在序列化时提供 getter，避免同时常驻 4 份独立字符串。

- [ ] **Step 3: 运行测试**

运行：`cargo test -p at-pc-agent --test screen_payload_test`
预期：PASS

- [ ] **Step 4: Git 提交**

```bash
git add crates/agent/src/tools/screen.rs crates/agent/tests/
git commit -m "perf(agent): cap default screen capture to 1280 and eliminate duplicate base64 buffers"
```

---

### Task 12: 【P2-C】SoM 视觉定位算法下采样优化

**Files:**
- Modify: `crates/agent/src/tools/som.rs`
- Test: `crates/agent/tests/som_perf_test.rs`

**Interfaces:**
- Consumes: `detect_visual_boxes`, `get_marked_screen`
- Produces: 2.5K 下视觉轮廓检测耗时从 ~10ms 降至 <3ms，端到端编码耗时显著降低。

- [ ] **Step 1: 编写 SoM 降采样检测精准度对比测试**

在 `crates/agent/tests/som_perf_test.rs`：
验证将 2560x1600 原图下采样至 1280 进行边缘检测后，坐标等比例映射回原图，所得到的边界框精度误差在 2 像素以内。

- [ ] **Step 2: 在 `som.rs` 中落地检测前置降采样**

在执行 `detect_visual_boxes` 之前，如果原图分辨率大于 1280，先进行双线性/近邻下采样执行轮廓寻找，最后将识别到的 `ScreenMark` 坐标映射回目标分辨率。

- [ ] **Step 3: 运行基准验证**

量化指标：2.5K 分辨率下 `detect_visual_boxes` 耗时从 9.58ms 降至 2.5ms 内。

- [ ] **Step 4: Git 提交**

```bash
git add crates/agent/src/tools/som.rs crates/agent/tests/
git commit -m "perf(som): optimize visual contour detection with downsampling"
```

---

### Task 13: 综合验收与性能基线回归测试

**Files:**
- Run: `crates/benchmarks/examples/perf_probe.rs`
- Run: 全套端到端验收脚本

- [ ] **Step 1: 运行全套 Cargo 单元与集成测试**

运行：`cargo test --workspace`
预期：所有测试 100% 通过。

- [ ] **Step 2: 运行性能探针验收报告**

运行：`cargo run --release -p at-pc-benchmarks --example perf_probe`
验证所有 1~8 节基准数据符合优化预期目标。

- [ ] **Step 3: 更新性能跟踪文档与状态**

更新 `docs/plans/2026-09-16-at-pc-performance-deep-dive-ii.md` 中的优化落实状态为已落地。

- [ ] **Step 4: 最终提交**

```bash
git add docs/
git commit -m "docs: finalize performance optimization milestones and empirical verification results"
```
