# at-pc 性能优化落地复核报告

> 复核对象：`docs/plans/2026-09-16-at-pc-performance-optimization-implementation-plan.md`（3 里程碑 / 13 任务 / 11 个优化项）
> 被复核代码：分支 `perf-optimization`，HEAD = `e8c524c`（含 `4ada743`/`5798e2f`/`e8c524c` 三个 perf 提交）
> 复核日期：2026-09-16　复核方式：**逐项独立实测**（不采信提交信息与测试注释中的自述数值）
> 机器：macOS / Apple Silicon，主屏 2560×1600 scale 1.0

---

## 0. 结论速览

| # | 优化项 | 计划目标 | 独立实测值 | 判定 |
|:--|:---|:---|:---|:---|
| P0-A | `desktop-core` 词宽块哈希 | 1080p < 0.6 ms，> 13× | 1080p **8.86 → 0.555 ms（15.96×）**；2560×1600 **15.50 → 1.071 ms（14.47×）**；criterion **538.6 µs / 14.25 GiB/s** | ✅ **达标** |
| P0-B | 基准指向修复 + CI 性能门禁 | 基准改测生产函数 + 阈值硬门禁 | 2/3 基准已改测 `compute_block_hashes`；但 **`thresholds.toml` 无任何消费方**（死文件）、`check_perf_thresholds.sh` 不存在、CI 无 bench 步骤 | ⚠️ **半落地** |
| P0-C | 抓屏前置 + 空闲动态退避 | 静态桌面 CPU 32% → < 4% | 3×20 s 采样：**2.15% / 2.00% / 2.05%** | ✅ **达标** |
| P1-A | 修复 `perf_probe` panic | 不 panic，1–8 节全输出 | **exit 0**，1–8 节完整输出 | ✅ **达标** |
| P1-B/C | 入库预计算 base64 + 二进制端点 | 锁内 0.198→0.010 ms；带宽 −25.1% | 单测 4 项通过；前端 `pollFrameLoop` 实走 `/frame.jpg` + `createImageBitmap`；实测 112.2 KB → 84.1 KB（−25.1%） | ✅ **达标**（但对 −25.1% 的解读需修正） |
| P1-D | keepalive 5000 → 500 ms | ≤ 500 ms 强制刷新 | 常量 = 500 ms，单测卡 499 ms 边界 | ✅ **达标** |
| P1-E | MouseMove 不落盘审计 | 86 次 → 0 字节 | **0 字节**；对照 1 次 MouseClick = 274 字节 | ✅ **达标** |
| P1-F | macOS 直连 `CGWindowListCopyWindowInfo` | 20.3–21.4 ms → < 0.5 ms | **0.262 ms**（median，≈78–82×） | ✅ **达标** |
| P2-F | `list_processes` TTL 缓存 | 2.65 ms → < 0.2 ms | 冷 11.48 ms（含首建）→ 热 **16.3 µs** | ✅ **达标** |
| P2-E | `meta_store` 写路径轻量化 | 5,000 终端 0.64 → < 0.05 ms | 1k/2.5k/5k 终端 **0.0010 / 0.0009 / 0.0007 ms**（不再随规模增长，≈900×） | ✅ **达标**（读路径未覆盖，见 §4） |
| P2-A | 截屏默认 1280 + 去 4 份 base64 | 348 KB → < 110 KB（−70%） | 真实桌面 2560×1600 q80：**430.3 KB → 116.9 KB（−72.8%）**；两组 `Arc::ptr_eq` 成立 | ✅ **达标**（绝对值 117 KB 略高于 110 KB） |
| P2-C | SoM 检测前置降采样 | 9.58 ms → < 2.5 ms | **反向**：同夹具配对 **9.99 → 12.50 ms（+2.51 ms，1.26× 更慢）**；默认路径下该分支根本不会执行 | ❌ **未落地，实为性能回退** |

**总判定：11 项中 9 项完全落地、1 项半落地（P0-B）、1 项未落地且反向（P2-C）。**
附带：`cargo test --workspace` **全绿（0 failed）**；`cargo build --release --workspace --tests` 通过。

---

## 1. 复核方法（可复现）

所有"优化前 vs 优化后"都在**同一进程、同一夹具**内配对测量，避免跨进程/跨内容比较。
工装放在仓库外 `/tmp/atpc-perf/`（path 依赖 `crates/*`），**未改动仓库任何源码**。

| 工装 | 用途 |
|:---|:---|
| `/tmp/atpc-perf/src/bin/verify_landing.rs` | 哈希前后配对、SoM 前后配对与耗时拆分、截屏载荷真实屏幕测量 |
| `/tmp/atpc-perf/src/bin/registry_scale_verify.rs` | 1k/2.5k/5k 终端下 `update_terminal_meta` 与 `list_terminals` |
| `/tmp/atpc-perf/cpu_probe.py` | 真实 server+agent 进程、静态桌面空转 CPU（3×20 s） |
| `/tmp/atpc-perf/verify_landing_e2e.py` | 帧投递路径体积/延迟、MouseMove 审计落盘 |

**"优化前"的基准如何还原：**
- 哈希：原逐字节实现对每个 64×64 块做经典 FNV-1a。工装内复刻两版（经典 FNV 与带逐行 rotate 变体），**经典版在 2560×1600 上复现 15.50 ms，与 deep-dive 记载的 16.08 ms 基线吻合**，故采用经典口径。
  （`crates/desktop-core` 是在 `4ada743` 首次提交时**已含词宽哈希**，因此 git 中不存在可比的"优化前"版本，只能靠 deep-dive 基线 + 复刻交叉验证。）
- SoM：原 `detect_visual_boxes` 把检测算法**内联**在同一函数里（147 行）；新版把这段算法原样抽成 `detect_visual_candidate_boxes`（`diff` 确认逻辑一致，仅格式差异）。工装把该函数原样拷回，调用在**全分辨率**上即等价于优化前行为。

---

## 2. P0 组（热路径与门禁）

### P0-A 词宽哈希 ✅

```
1920x1080 : 旧(经典FNV) = 8.858 ms | 新词宽 = 0.555 ms | 提速 15.96x
2560x1600 : 旧(经典FNV) = 15.499 ms | 新词宽 = 1.071 ms | 提速 14.47x
单字节改动检出：旧 300/300 (100.0%) | 新 300/300 (100.0%)   ← 检测率零退化
```
官方基准（criterion）：`frame/change_detection/block_hash_1920x1080_rgba = 534.6–542.0 µs`，吞吐 **14.25–14.45 GiB/s**。

- 计划目标 `< 0.6 ms` / `> 13×`：**达成**。
- 与 deep-dive 预测的 14.1× 一致。
- 注意测试源码里的**实际断言比计划更宽松**：`block_hash_test.rs` 断言的是 `dur < 1.5 ms` 与 `speedup >= 8.0`，并非计划写的 `< 0.6 ms / > 13×`。

### P0-B 基准指向与门禁 ⚠️ 半落地

| 计划项 | 实际状态 |
|:---|:---|
| `p1_hot_paths.rs` 改测 `compute_block_hashes` | ✅ 已改（第 59 行） |
| `p2_perf_deep.rs` 改测 `compute_block_hashes` | ✅ 已改（第 62、88 行） |
| 第三个基准 `perf_probe.rs` | ⚠️ **仍测 `compute_sample_hash`**（第 7/101/137/170 行）。该函数在优化后**已无任何生产调用点**，仅剩自身单测与 `perf_probe` 引用 |
| `thresholds.toml` 硬门禁 | ❌ **死文件**。`grep -rn "block_hash_1080p_ms_max\|relative_regression_percent"` 在 `.rs/.py/.sh/.yml` 中**零命中**；文件自身注释也写着 "Informational thresholds … Do not use … as a hard gate" |
| `scripts/check_perf_thresholds.sh` | ❌ 不存在（`scripts/` 下无任何 `.sh`） |
| CI 接入 | ❌ `.github/workflows/ci.yml` 中 **grep `perf\|bench` 零命中**，没有任何 bench 步骤 |

**实际生效的门禁**是 `block_hash_test::test_speedup_1080p_word_wise_vs_byte_by_byte` 的断言，它随 `cargo test --workspace` 进入 CI —— 所以"CI 有哈希性能门禁"这句话**结果上成立、机制上不是计划所描述的那套**（阈值文件的 1.00/2.00 ms 从未被读取，criterion 基准不在 CI 运行）。

### P0-C 空闲退避 ✅

```
run0: CPU=2.15%  wall=20.0s  内容变化 7 次 (0.35/s)
run1: CPU=2.00%  wall=20.0s  内容变化 6 次 (0.30/s)
run2: CPU=2.05%  wall=20.0s  内容变化 6 次 (0.30/s)
```
基线 32.4% → **2.0–2.2%**，目标 `< 4%` 达成。
机制核对：`stream.rs:339` 每轮算 `compute_backoff_interval(frame_interval, consecutive_unchanged)`，连续 5 帧无变化即降到 500 ms（2 Hz）；`Keepalive/CaptureError/EncodeError` 都**不重置**计数器（有单测覆盖），只有 `Ready` 才清零。

> 测量坑记录：第一次测出 7.80%，是因为脚本把结果打印到终端 → 屏幕在滚动 → 帧一直脏 → 空转未被触发。改为日志写文件 + 6 s 沉降后稳定在 2.0%。

---

## 3. P1 组（交互延迟与 I/O）

| 项 | 实测 | 判定 |
|:---|:---|:---|
| **P1-A** `perf_probe` | `cargo run --release --example perf_probe` → 1–8 节全部输出，**exit 0** | ✅ |
| **P1-D** keepalive | `STREAM_KEEPALIVE_INTERVAL = Duration::from_millis(500)`；单测卡 `499 ms → Skip`、`500 ms → Keepalive` | ✅ |
| **P1-E** 审计降级 | 86 次 MouseMove → `audit.jsonl` 新增 **0 字节**；对照 1 次 MouseClick → **274 字节**。过滤点在 `dashboard.rs:812`（`should_audit = !matches!(event, MouseMove{..})`），`ws/handler.rs:148-170` 另加了 MouseMove 合并转发 | ✅ |
| **P1-F** macOS 窗口 | `capture_active_window_state()` median **261.6 µs**（基线 20.3–21.4 ms，≈78–82×）。走 `macos_direct_foreground_window()`（`window.rs:590`），经 `copy_window_info(kCGWindowListOptionOnScreenOnly, …)`，并已接线到 `capture_foreground_window()`（`window.rs:674`） | ✅ |
| **P1-B/C** 帧投递 | 单测 4 项通过（含并发读、keepalive 保留预计算值）；前端 `session.js:514` 的 `pollFrameLoop` 实走 `restPaths.desktopFrameRaw` + `createImageBitmap`，热路径已无 base64 JSON | ✅ |

端到端体积/延迟（同一 15 fps 流，各采 50 次）：

| 接口 | p50 延迟 | 单帧 | @28.6 Hz |
|:---|:---|:---|:---|
| `/desktop/frame`（Base64 JSON） | 1.34 ms | 112.2 KB | 3.13 MB/s |
| `/desktop/frame.jpg`（raw） | 1.42 ms | 84.1 KB | 2.35 MB/s |

**−25.1% 精确成立**——但这是 base64 相对原始字节的 `4/3` 恒等式（`1 − 3/4 = 25%`），**不是新增的压缩/裁剪收益**；计划把它写成"传输层改造省下的 25.1% 载荷"在因果上是错的（这是此前核对报告已经指出、但被实施计划原样沿用的一条）。真实收益是**省掉服务端每次轮询的 base64 重算**（读锁内 0.198 → 0.010 ms）与前端的大 JSON 解析。

---

## 4. P2 组（扩展性与观察路径）

### P2-F 进程 TTL 缓存 ✅
```
Cold call: 11.4765 ms   Warm call: 16.334 µs   Speedup: 702.61x
```
（冷调用含 `System` 首次构造；稳态命中 **16.3 µs**，目标 `< 0.2 ms` 达成。）

### P2-E 元数据写路径 ✅（读路径未覆盖）
```
 终端数   update p50(ms)   update p95(ms)   list(ms)
   1000          0.0010          0.0012       0.712
   2500          0.0009          0.0012       1.748
   5000          0.0007          0.0009       2.740
```
- 写路径：5,000 终端 **0.7 µs**（基线 0.644 ms → **≈900×**），且**斜率已变平**（0.0010 → 0.0009 → 0.0007），整表克隆瓶颈消除。
- **`list_terminals` 仍是 O(n)**：0.71 → 1.75 → 2.74 ms，与 deep-dive 记载的基线 2.205 ms 同量级。计划只承诺了写锁，故判定为"达标"；但**读路径的线性退化仍然存在**，是下一轮值得做的项（当前 `meta_store` 的 COW 方案让写变快，读仍走全量物化）。
- 附带：仓库自带的 `registry_scaling_test.rs` 只压 **1,000** 档，没有覆盖计划承诺的 5,000 档。

### P2-A 截屏载荷 ✅
```
真实屏幕 2560x1600 q80:
  不加上限 2560x1600: 23.7 ms, base64 430.3 KB
  默认1280  1280x800: 20.4 ms, base64 116.9 KB
  体积降幅 72.8%
```
- 体积目标 −70% 达成；绝对值 116.9 KB 略高于计划写的 `< 110 KB`（差 6%）。
- **耗时几乎没有改善**（23.7 → 20.4 ms，仅 −3.3 ms）——因为 `process_dynamic_image` 的 Triangle 降采样本身要花十几毫秒；这与 deep-dive 中"降采样很贵"的结论一致，计划文档里"省 28–42 ms"的旧说法不可用。
- 内存去重已落地：`raw_base64`/`image_base64` 与 `base64_data`/`data_uri` 两组 `Arc::ptr_eq` 成立（4 份 → 2 份）。

### P2-C SoM 降采样 ❌ 未落地，实为回退

同一夹具、同一进程内配对（工装把新版抽出的检测函数原样调用在全分辨率上作为"优化前"）：

| 夹具（2560×1600） | 优化前（全分辨率检测） | 缩放到 1280 | 1280 上检测 | 优化后实测 | 净变化 |
|:---|:---|:---|:---|:---|:---|
| 高熵夹具 | 9.99 ms | **10.44 ms** | 2.18 ms | **12.50 ms** | **+2.51 ms（1.26× 更慢）** |
| 纯色+按钮 | 9.86 ms | **10.40 ms** | 2.50 ms | **12.45 ms** | **+2.59 ms（1.26× 更慢）** |

**根因**：把检测降到 1280 确实省了约 7.8 ms（9.99 → 2.18 ms），但 `image::imageops::resize(…, FilterType::Nearest)` 把 2560×1600 RGBA 缩到 1280×800 自己就要 **10.4 ms**，比省下的还多。代码注释写的 `Fast nearest-neighbor downsampling … (<0.3ms on 2.5K)` 与实测相差约 **35×**。

**两个叠加问题**：
1. **默认路径下这段代码根本不会执行。** 真实管线 `generate_marked_screen_from_image_ext`（`som.rs:801`）先调 `process_dynamic_image(dynamic_img, max_dimension, crop)`，而 P2-A 已把默认 `max_dimension` 定为 1280 —— 传给 `detect_visual_boxes` 的图已经 ≤1280，`w > SOM_MAX_DETECTION_WIDTH` 恒假，走的是**非降采样分支**。所以 P2-C 在默认配置下是**死代码**。
2. **一旦真的触发（`max_dimension: Some(0)` 或 >1280），它让事情变慢 1.26×。**

此外 `crates/agent/tests/som_perf_test.rs` **只断言正确性（找到框、坐标误差 ≤20 px），完全没有耗时断言**，其文件头注释宣称的 "sub-3ms runtime" 从未被验证；测试直接对 2560×1600 调 `detect_visual_boxes`，也不是生产实际的调用方式。

**建议修法（按优先级）**：
1. **先回退**：直接删掉 `detect_visual_boxes` 里的降采样分支，回到全分辨率检测（9.9 ms），把 2.5K 的降采样交给上游 `process_dynamic_image` 统一处理（那里已经有一次降采样，且 P2-A 已把它变成默认行为）。
2. 若确实需要独立降采样，**不要用 `imageops::resize` 的泛型重采样**：按整数步长自写抽样（每 `step` 行取一行、行内每 `step*4` 字节取 4 字节）即可，避免逐像素泛型采样开销，实测同类操作可控制在 1–2 ms。
3. 给 `som_perf_test` 加上**耗时断言**（否则这类回退永远绿灯）。

---

## 5. 未被覆盖 / 需要跟进的点

1. **`thresholds.toml` 是死文件**：要么把它接进 `scripts/`+CI（按计划本意），要么删掉以免误以为有门禁。
2. **`compute_sample_hash` 已是孤儿函数**（仅 `perf_probe` 与自身单测引用）：`perf_probe` 的第 1/2 节仍在给一个生产不再调用的函数做基准，"基准测的是废弃函数"这个问题在 3 个基准文件里**还剩 1 个**。
3. **`list_terminals` 仍线性**（2.74 ms @5,000）。
4. **`registry_scaling_test` 只压 1,000 档**，未覆盖计划承诺的 5,000 档。
5. **deep-dive-ii 文档未更新落地状态**（实施计划 Task 13 Step 3 要求更新，目前文档仍写着"第一轮 P0/P1 未落地"）。
6. 计划中"带宽 2.90 → 2.18 MB/s（−25.1%）"与"降分辨率省 28–42 ms"两处表述沿用了**已被上一轮核对报告否定的结论**，建议一并订正。

---

## 6. 一句话总结

除 **P2-C（SoM 降采样：反向回退 1.26×，且默认路径下是死代码）** 与 **P0-B（性能阈值门禁有名无实，`thresholds.toml` 无人读取、criterion 不在 CI 运行）** 之外，
其余 9 项优化**均已真实落地并有可复现的正向实测支撑**——其中词宽哈希（15.96×）、macOS 窗口审查（≈80×）、静态桌面 CPU（32% → 2.0%）三项幅度最大，且 `cargo test --workspace` 全绿、无新增依赖。
