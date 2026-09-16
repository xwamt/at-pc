# 优化落地复核（第三轮）— 33657a7

> 复核对象：`33657a7 fix(perf): resolve round 2 audit findings - center-align som downsample, wire live ci gates, and optimize list_terminals`
> 复核方式：同夹具配对实测；A/B 对照树建在仓库外；**未改动仓库任何源码**。
> 机器条件：macOS / 2560×1600 / scale 1.0。

---

## 0. 结论速览

| # | 上轮遗留项 | 本轮判定 | 一句话依据 |
|:--|:--|:--|:--|
| ① | **P0-B 门禁仍是形式主义** | ✅ **真接线了** | CI 改跑 `./scripts/check_perf_thresholds.sh`：真跑 `perf_probe` → 真断言 → 真跑 3 条 release 测试；篡改阈值为 `0.0001ms` 后 `GATE_EXIT=1` 并中止在探针断言 |
| ② | **SoM 与 `imageops::resize` 采样相位差 1px** | ✅ **完全对齐（逐字节相同）** | 高熵夹具从「100% 像素不同」→ **0/1024000**；真实屏检测框从 13 vs 14 → **8 vs 8 完全一致** |
| ③ | **`list_terminals` 仍 O(n)、无断言** | ✅ **有真实提速（1.33–1.37×）**，但仍是 O(n) | 交错 3 轮 A/B：5000 档 1.785 → **1.342 ms**；新增断言 `<15ms` |
| ④ | `perf_probe` 测废弃函数 | ✅ 上轮已修，本轮回归通过 | `compute_block_hashes` 0.49–0.52 ms |
| ⑤ | 5,000 档扩展性测试缺失 | ✅ 上轮已补，本轮回归通过 | `avg_latency 619ns` |

**同时发现 4 处新问题**（见第 4 节），核心是 `thresholds.toml` 新增的 3 个键**零消费方**，与 Rust 测试里硬编码的界构成"双重真相"。

回归：`cargo test --workspace` 全绿（0 失败）；`cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` 通过。

---

## 1. ① P0-B 门禁：这次是真的

### 1.1 改了什么

```diff
-      - name: Check benchmark compilation and performance thresholds
-        run: |
-          cargo bench -p at-pc-benchmarks --no-run
-          python3 scripts/check_perf_thresholds.py --check-config   # ← 只校验 schema，直接 return 0
-          python3 scripts/test_check_perf_thresholds.py
+      - name: Check benchmark compilation and performance thresholds
+        run: ./scripts/check_perf_thresholds.sh
```

`check_perf_thresholds.sh` 现在会：

1. `cargo bench --no-run`（编译）
2. 阈值校验器单测
3. **`cargo run --release -p at-pc-benchmarks --example perf_probe > tmp` 然后 `check_perf_thresholds.py --probe-output tmp`** ← 真断言
4. **`cargo test --release` × 3**（`block_hash_test` / `som_perf_test` / `registry_scaling_test`）← 真 release 门

### 1.2 端到端实跑（仓库内，未改动）

```
$ ./scripts/check_perf_thresholds.sh
==> Verifying benchmarks compilation without running...
==> Running unit tests for threshold validator...
==> Running release performance probe and asserting threshold limits...
     Running `target/release/examples/perf_probe`
Extracted probe metrics: {'block_hash_1080p_ms': 0.49, 'rgba_to_rgb_ms': 0.576}
[OK] All probe metrics satisfy performance threshold gates.
==> Running release profile performance test assertions...
test result: ok. 5 passed (block_hash_test)
test result: ok. 3 passed (som_perf_test)
test result: ok. 2 passed (registry_scaling_test)
==> All performance threshold gates passed successfully.
GATE_EXIT=0
```

### 1.3 两板斧：门禁真能拦住回退吗

在**仓库外副本** `/tmp/atpc-ab`（rsync 自工作树，仅 `registry.rs` 换成旧版）把阈值篡改为不可能满足的值，跑**与 CI 完全相同的命令**：

```
# 第一次：只改 toml  -> 被单测拦住（详见 4.4）
GATE_EXIT=1   FAIL: test_validate_repo_thresholds  AssertionError: 0.0001 != 1.0

# 第二次：连单测的期望值一起改，让它走到真断言
$ ./scripts/check_perf_thresholds.sh
==> Running release performance probe and asserting threshold limits...
     Running `target/release/examples/perf_probe`
[FAIL] block_hash_1080p_ms exceeded: actual=0.522ms > limit=0.000ms
GATE_EXIT=1
```

且中止位置正确——日志里**没有** `passed successfully`，也**没有**走到 `release profile performance test assertions`，说明 `set -euo pipefail` 正确短路。

**判定：机制可用 + 已接线 + 能拦住回退。上轮的核心问题已解决。**

### 1.4 debug 包的隐患也一并修了

`block_hash_test` 新增了 `else` 分支，debug 下不再整块跳过：

| profile | 断言 | 实测 | 结果 |
|:--|:--|:--|:--|
| release | `dur_new < 1.2ms` 且 `speedup ≥ 8×` | — | 通过 |
| debug（CI 的 `cargo test --workspace`） | `dur_new < 25ms` 且 `speedup ≥ 1.3×` | 15.35ms / **1.87×** | 通过 |

debug 界虽松（挡不住 2× 退化），但 release 界够紧（1.2ms / 8×），两者叠加后"退回逐字节实现"这类回退会被至少一条拦住。

---

## 2. ② SoM 采样相位：已完全对齐

`fast_downsample_rgba` 的坐标映射由 `(i × src) / dst` 改为 `floor((i + 0.5) × src / dst)`（整数式 `((i×2+1) × src) / (dst×2)`）。

### 2.1 逐像素差异（vs `imageops::resize(…, Nearest)`）

| 夹具 | 上轮 | 本轮 |
|:--|:--|:--|
| 纯色+按钮 2560×1600 | 0/1024000（0%） | **0/1024000（0%）** |
| **高熵 2560×1600** | **1024000/1024000（100%），最大通道差 254** | **0/1024000（0%），最大通道差 0** |

→ 现在与标准最近邻**逐字节相同**。

### 2.2 检测框等价性

| 夹具 | 上轮 | 本轮 |
|:--|:--|:--|
| 多目标 8 框 | ✅ 一致 | ✅ 一致 |
| 纯色+按钮 | ✅ 一致 | ✅ 一致 |
| **真实屏幕 2560×1600** | ❌ resize 13 框 / fast 14 框，9 对配对、最大偏移 64px | ✅ **resize 8 框 / fast 8 框，完全一致** |

### 2.3 性能没有因此退化

| 指标 | 上轮（跨步取点） | 本轮（中心对齐） |
|:--|--:|--:|
| `fast_downsample_rgba` 2560→1280 | 0.437 / 0.430 ms | **0.364 / 0.376 ms** |
| vs `imageops::resize`（10.20 ms） | 23.2× | **26.7–28.1×** |
| `detect_visual_boxes` 全路径 2560×1600 | 3.09 / 2.83 ms | **2.88 / 2.64 ms** |

（优化前基线 10.48 / 9.98 ms → 现版本快 3.6–3.8×。）

> 仍存的边界（与上轮一致）：**该分支在默认管线里仍是死代码**——上游 `process_dynamic_image` 默认已把图压到 1280，`w > 1280` 恒假，只有显式 `max_dimension: 0` 才触发。`som_perf_test` 直接调 `detect_visual_boxes` 绕过了上游，因此它守的是一条生产默认不走的路。

---

## 3. ③ `list_terminals`：有真实提速，但仍是 O(n)

### 3.1 做法

把"离线终端补全"从**前置**（先建 full-size `HashSet<&str>` + 遍历 `all_meta`）改为**后置 + 短路**：只有 `entries.len() < all_meta.len()` 时才建 HashSet；`sort_by` → `sort_unstable_by`。

### 3.2 A/B 实测（同工装、同机器、交错 3 轮，`list_terminals` 中位数）

| 终端数 | 旧版 (d552ab8) 3 轮 | 新版 (33657a7) 3 轮 | 加速 |
|--:|:--|:--|--:|
| 1,000 | 0.538 / 0.280 / 0.278 ms | 0.203 / 0.196 / 0.209 ms | **1.37×**（中位 0.278→0.203） |
| 2,500 | 1.128 / 0.763 / 0.785 ms | 0.567 / 0.574 / 0.590 ms | **1.37×**（0.785→0.574） |
| 5,000 | 1.932 / 1.726 / 1.785 ms | 1.302 / 1.342 / 1.258 ms | **1.33×**（1.785→1.342） |

方向一致、三轮稳定。**优化是真的**。

### 3.3 ⚠️ 对上轮数据的更正

上一轮我报的 `list_terminals` 是 **0.738 / 1.926 / 2.816 ms（旧）** 和 **0.846 / 1.117 / 2.987 ms（新）**，据此得出"优化后反而略慢"。**那个读数是错的**——当时每次只**采样 1 次**，噪声把结果抬高了约 1.6 倍。改成 15 次取中位数后，旧版 5000 档真实值是 **1.785 ms**，新版 **1.342 ms**。

**更正后的结论：`list_terminals` 优化真实有效（≈1.35×），上轮"更慢"的判断是采样假象，撤回。**

### 3.4 局限

- 仍是 **O(n)**：主要成本是构造 5,000 个 `TerminalEntry`（逐个 clone 字符串）+ 排序，短路只省掉了 HashSet 与一次 `all_meta` 遍历。
- 新增断言 `<5ms @1k` / `<15ms @5k`，实测 1.34ms → **余量 11×**。能拦住 10× 级退化，拦不住 2–3× 级。
- 正确性：短路条件依赖隐含不变量「**活跃会话 ⊆ 已持久化 meta**」。当前成立（`register()` → `record_registration()` 必定写入 `last_known_info`，且该函数无 Result、不会失败）。但若将来出现"活跃但无 meta"的路径，离线终端会被**静默漏掉**。建议把该不变量写成注释或 `debug_assert`。

---

## 4. 新发现的问题

### 4.1 ❗`thresholds.toml` 新增 3 个键，零消费方

```toml
som_detection_2560_ms_max = 8.00
registry_update_5k_micros_max = 50.0
registry_list_5k_ms_max = 15.00
```

全仓库 grep（排除 `thresholds.toml` 自身）**零命中**——既不在 `check_perf_thresholds.py` 里，也不在任何测试里。

它们"对应"的真实门禁其实是 **Rust 测试里硬编码的数字**：

| toml 键 | 实际生效的断言位置 | 值 |
|:--|:--|:--|
| `som_detection_2560_ms_max = 8.00` | `som_perf_test.rs` `median_ms < 8.0` | 8.0 ✅ 一致 |
| `registry_update_5k_micros_max = 50.0` | `registry_scaling_test.rs` `< Duration::from_micros(50)` | 50 ✅ 一致 |
| `registry_list_5k_ms_max = 15.00` | `registry_scaling_test.rs` `< Duration::from_millis(15)` | 15 ✅ 一致 |

**当前值一致，但同一份真相写了两处，漂移是时间问题**——而且 toml 那份永远是死的：改 toml 不会有任何效果，改测试才是真改门禁。
建议：要么把这 3 个键接进 `check_perf_thresholds.py`（让测试从 toml 读），要么从 toml 删掉，别留"看起来被门禁覆盖"的假象。

同理 `block_hash_2560_ms_max = 2.00` 只被校验为"正数"，**探针从不输出 2560 指标**，所以它也没有任何真断言。

### 4.2 `relative_regression_percent` 变成"幽灵默认值"

toml 里已删除，但 `check_perf_thresholds.py:89` 仍写 `thresholds.get("relative_regression_percent", 10.0)`，单测 `test_validate_repo_thresholds` 还断言它 `> 0` —— 现在测的是**代码里的默认值**，不是配置。要么删掉这条，要么真正实现"相对基线回退 10% 拦截"。

### 4.3 门禁的 release 清单漏了两个同样带耗时断言的测试

`check_perf_thresholds.sh` 只跑 3 条 release 测试。全仓库另有两条**带 `cfg!(debug_assertions)` 双界**的性能测试不在列：

| 测试 | release 界 | debug 界 | release 实测 | 余量 |
|:--|:--|:--|--:|--:|
| `crates/agent/tests/process_cache_test.rs` | warm < 300 µs | < 1500 µs | **15.9 µs**（cold 2.63 ms） | 18.9× |
| `crates/agent/tests/macos_window_review_test.rs` | median < 5 ms | < 15 ms | **299.3 µs** | 16.7× |

两者都宽松通过（顺带佐证了上轮 macOS 前台审查 0.262 ms 的结论），**风险不大**；但只要不在 release 下跑，它们的严格界就等于不存在。建议加进脚本第 4 步。

### 4.4 单测把配置值硬编码，改阈值必须改两处

`scripts/test_check_perf_thresholds.py:21` `assertEqual(result["block_hash_1080p_ms_max"], 1.0)`。

第一次篡改实验里，门禁确实红了，但**是被这条单测拦住的，不是被探针断言拦住的**。副作用是：任何**合法**的阈值调整（比如把 1.00 收紧到 0.80）都会让门禁变红，必须同时改 toml + 单测。建议单测改为断言"字段存在且 > 0"，把具体数值交给真断言。

### 4.5 `perf_probe` 由"只编译"变成"真执行"，CI 平台风险未实测

现在 CI 会在 **headless ubuntu-24.04** 上真的运行 `perf_probe`：

- 第 7 节 `xcap::Monitor::all()` 失败时走 `Err` 分支打印 `Monitor::all() unavailable: …` 后继续（已确认不 panic）；
- 但第 6 节有 **`assert_eq!` 逐字节比对**（RGBA→RGB 三个原型 vs 生产实现），这是**硬断言**——若某平台行为不同，CI 会红；
- 第 4 节用 `temp_dir()` 建审计文件、第 5 节起 tokio runtime，均无平台假设。

我在 macOS 上跑通（`GATE_EXIT=0`），**但这条 CI 路径本身没在真实 CI 上跑过**。建议合并后观察第一次 CI 结果；若 Linux 上因 xcap 或缺图形栈出问题，需要把第 6/7 节包在 `cfg` 或环境探测里。

---

## 5. 回归与复现

```bash
# 全量回归（debug，CI 的 quality job）
cargo test --workspace --no-fail-fast            # 0 失败
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # 通过

# 门禁端到端（CI 同一条命令）
./scripts/check_perf_thresholds.sh               # GATE_EXIT=0

# 门禁两板斧（仓库外副本，不动仓库）
#   /tmp/atpc-ab 内改 thresholds.toml → GATE_EXIT=1，[FAIL] ... exceeded

# 独立工装（仓库外，未改仓库源码）
cd /tmp/atpc-perf && ./target/release/verify_som_fix          # SoM 对齐 + 耗时
cd /tmp/atpc-perf && ./target/release/registry_scale_verify   # registry 扩展性
/tmp/atpc-perf-ab/target/release/registry_scale_verify        # 旧版对照（A/B）
```

---

## 6. 三轮累计状态

| 项 | 状态 |
|:--|:--|
| P0-A 词宽块哈希 | ✅ 1080p 0.555ms（16×），criterion 538.6µs |
| P0-B CI 性能门禁 | ✅ **本轮确认真接线**（探针断言 + release 测试 + 篡改可变红） |
| P0-C 抓屏前置 + 空闲退避 | ✅ 静态桌面 CPU 2.0–2.2%（基线 32.4%） |
| P1-A `perf_probe` panic | ✅ exit 0，且已改测生产函数 |
| P1-B/C base64 预计算 + 二进制流 | ✅ |
| P1-D keepalive 500ms | ✅ |
| P1-E MouseMove 审计降级 | ✅ 0 字节落盘 |
| P1-F macOS 前台审查 | ✅ 0.262 ms（基线 20.3–21.4） |
| P2-F 进程 TTL 缓存 | ✅ 热命中 16.3 µs |
| P2-E meta 写路径 | ✅ 5,000 档 0.5–0.7 µs（O(1)） |
| P2-A 截屏默认 1280 | ✅ 430.3 → 116.9 KB |
| P2-C SoM 降采样 | ✅ **本轮确认逐字节等价且快 26.7–28.1×**；分支默认仍不可达 |
| 5,000 档扩展性测试 | ✅ 断言 <50µs，实测 619ns |
| `list_terminals` | ✅ **本轮确认 1.33–1.37×**；仍 O(n)，断言余量 11× |
| `thresholds.toml` 键的消费方 | ⚠️ **3 个新键零消费方**，与测试硬编码构成双重真相 |
| `perf_probe` 在 headless Linux CI 的执行 | ⚠️ 未在真实 CI 验证过 |
