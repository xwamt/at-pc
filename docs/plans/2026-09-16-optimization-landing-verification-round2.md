# 优化落地复核（第二轮）— d552ab8

> 复核对象：`d552ab8 fix(perf): resolve P2-C SoM regression, wire P0-B CI thresholds, and expand 5k scale test`
> 复核方式：全部为**同进程、同夹具配对实测**；工装置于仓库外 `/tmp/atpc-perf/`，**未改动仓库任何源码**。
> 机器条件：macOS / 2560×1600 / scale 1.0 / release（除非注明 debug）。

---

## 0. 结论速览

| # | 上轮扫出的缺口 | 本轮判定 | 一句话依据 |
|:--|:--|:--|:--|
| ① | **P2-C SoM 降采样反向回退**（9.99→12.50ms，慢 1.26×） | ✅ **已修复，且超预期** | 新 `fast_downsample_rgba` 使降采样 10.16ms→**0.437ms**；全路径 12.58→**3.09ms**，比优化前(10.16ms)还快 3.3× |
| ② | **P0-B 性能门禁有名无实** | ❌ **仍未落地**（新增了脚手架，但 CI 路径拦不住任何东西） | 篡改阈值为 `0.0001ms` 仍 `EXIT=0`；且 **CI 用 debug profile 跑测试，所有 release 门的耗时断言被 `cfg!(debug_assertions)` 整块跳过** |
| ③ | `registry_scaling_test` 只压 1,000 档 | ✅ **已补齐 5,000 档**，且带硬断言 | 新增 `avg_latency < 50µs`，实测 619ns(release) 通过 |
| ④ | `perf_probe` 仍在测已废弃的 `compute_sample_hash` | ✅ **已改测生产函数** | 改用 `compute_block_hashes`，输出 0.503ms，exit 0 |
| ⑤ | `list_terminals` 仍线性、无断言 | ⚠️ **未处理** | 独立复测 0.738 / 1.926 / 2.816 ms @1k/2.5k/5k，仍 O(n)，零断言 |

回归测试：`cargo test --workspace` 全绿（55 个测试目标，0 失败）；`cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` 通过。

---

## 1. ① P2-C SoM：真的修好了

### 1.1 降采样本身的开销（2560×1600 → 1280×800，RGBA）

| 夹具 | `imageops::resize(Nearest)` | `fast_downsample_rgba` | 提速 |
|:--|--:|--:|--:|
| 纯色+按钮 | 10.155 ms | **0.437 ms** | 23.2× |
| 高熵 | 9.944 ms | **0.430 ms** | 23.1× |

新函数确实绕开了 `imageops::resize` 的通用插值开销，只做跨步取点。
> 注：代码注释写的是 `<0.3ms for 2560x1600 -> 1280x800`，实测 0.43ms（高估约 1.4×）。量级正确，非问题。

### 1.2 全路径三段对照（`detect_visual_boxes` @ 2560×1600）

| 夹具 | 优化前（全分辨率检测） | 回退版（resize+检测） | 修复版（现仓库） | 相对回退版 |
|:--|--:|--:|--:|--:|
| 纯色+按钮 | 10.16 ms | 12.58 ms | **3.09 ms** | **4.07× 更快** |
| 高熵 | 9.73 ms | 12.10 ms | **2.83 ms** | **4.27× 更快** |

不仅消除了回退，还把这条路径做成了**净收益**：现版本比"优化前"还快 3.3×（3.09 vs 10.16 ms）。

### 1.3 结果等价性（是否改变了检测输出）

| 夹具 | 优化前全分辨率 | resize 降采样 | fast 降采样 | 现仓库 API | 逐框一致？ |
|:--|--:|--:|--:|--:|:--|
| 多目标 8 框 | 9 框 | 8 框 | 8 框 | 8 框 | ✅ 完全一致，偏移 0 px |
| 纯色+按钮 | 1 框 | 1 框 | 1 框 | 1 框 | ✅ 完全一致，偏移 0 px |
| **真实屏幕** | 104 框 | **13 框** | **14 框** | 14 框 | ❌ 不一致 |

**真实屏幕夹具上两种降采样不等价**，需要说明清楚：

- 根因是**采样相位差 1 像素**。`image::imageops::resize(…, Nearest)` 取 `floor((x + 0.5) × ratio)` → ratio=2 时取**奇数列/奇数列**；新函数取 `(x × src_w) / target_w` → 取**偶数列/偶数行**。
- 高熵夹具上逐像素对比：**100% 像素不同，最大通道差 254**；纯色夹具上 0% 不同（平滑区域无差异）。
- 检测结果：9 对框成功配对（最大坐标偏移 64 px），另有 4~5 个框各侧独有（如 resize 侧 `[416,160,704,960]` / fast 侧 `[416,192,704,960]`）。
- 判定：属于**边缘候选框的取舍差异**，不影响主目标框；且相对于 104 框的全分辨率地面真值，降采样本身就是大幅裁剪（这是既有设计，非本次引入）。但**不能宣称"完全等价"**，如果后续有依赖 SoM 框集合稳定性的用例（快照测试、坐标回归），需要留意这 1 px 相位。

### 1.4 ❗该修复在生产默认路径下仍是死代码

```
默认 (max_dimension = None → 1280)  : 处理后 1280x800  → detect_visual_boxes 内 `w > 1280` = false
显式 Some(0)（关闭下采样）           : 处理后 2560x1600 → `w > 1280` = true
```

上游 `process_dynamic_image` 的默认上限就是 `DEFAULT_MAX_DIMENSION = 1280`（P2-A 引入），而 SoM 三个调用点都用 `processed_img` 调 `detect_visual_boxes`。因此：

- **默认 SoM 调用（`max_dimension` 缺省）：该分支不执行，本修复零收益。**
- 只有显式传 `max_dimension: 0`（工具参数可传）才会触发，此时：回退版 12.58ms → 修复版 3.09ms，**避免了 4× 的净损失**。

建议二选一：
1. 承认上游已限流，删掉 `SOM_MAX_DETECTION_WIDTH` 分支与 `fast_downsample_rgba`；
2. 保留但改由上游显式标记（例如 `process_dynamic_image` 返回 `already_downscaled: bool`），不要依赖隐式不变量维持一段永不执行的代码。

---

## 2. ② P0-B 性能门禁：脚手架有了，门还是没有

### 2.1 新增了什么

| 文件 | 作用 |
|:--|:--|
| `scripts/check_perf_thresholds.py` | 校验 `thresholds.toml` schema；可选 `--probe-output` 做真断言 |
| `scripts/check_perf_thresholds.sh` | 组合脚本 |
| `scripts/test_check_perf_thresholds.py` | 4 个单测（用**硬编码字符串**测 parser） |
| `.github/workflows/ci.yml`（新增性能步骤） | `cargo bench --no-run` + `--check-config` + 单测 |

### 2.2 实测：CI 走的分支拦不住任何东西

CI 步骤（`ci.yml:96-100`）实际执行的是：

```yaml
cargo bench -p at-pc-benchmarks --no-run      # 只编译，不跑
python3 scripts/check_perf_thresholds.py --check-config
python3 scripts/test_check_perf_thresholds.py
```

而 `--check-config` 在 `check_perf_thresholds.py:131` 直接 `return 0`，只校验 TOML 能否解析、两个阈值字段是否为正数。**实验（在仓库外副本做，未改仓库）**：

```
# 把 block_hash_1080p_ms_max 改成 0.0001（不可能满足）、relative_regression_percent 改成 0.0001
$ python3 scripts/check_perf_thresholds.py --check-config
[OK] Validated crates/benchmarks/thresholds.toml: {... 'block_hash_1080p_ms_max': 0.0001, ...}
EXIT=0
```

→ **门禁无法识别恶意/错误的阈值。**

### 2.3 真断言机制本身是好的，只是没人调用

`--probe-output` 只在脚本自身的 argparse 里出现过一次，**全仓库无任何调用方**；`perf_probe` 也从未在 CI 或任何脚本中被运行。手工喂入真实探针输出后：

```
$ python3 scripts/check_perf_thresholds.py --probe-output /tmp/probe_out.txt      # 阈值 1.00ms
Extracted probe metrics: {'block_hash_1080p_ms': 0.503, 'rgba_to_rgb_ms': 0.571}
[OK] All probe metrics satisfy performance threshold gates.        EXIT=0

$ （阈值篡改为 0.0001ms 后）同一份真实输出
[FAIL] block_hash_1080p_ms exceeded: actual=0.503ms > limit=0.000ms  EXIT=1
```

→ 机制可用、判断正确。**缺的只是一行接线**：CI 里跑 `perf_probe` 并把 stdout 交给 `--probe-output`。

### 2.4 ❗更严重的问题：CI 用 debug profile 跑测试，所有 release 门的耗时断言全部失效

`ci.yml:94`：`cargo test --workspace --all-features --locked` —— **没有 `--release`**，走 dev profile，`debug_assertions = true`。

而唯一的"真耗时断言"被写在 `if !cfg!(debug_assertions) { … }` 里：

```rust
// crates/desktop-core/tests/block_hash_test.rs:172-181
if !cfg!(debug_assertions) {
    assert!(dur_new.as_micros() < 1500, "…should take <1.5ms…");
    assert!(speedup >= 8.0, "Expect >=8x-10x speedup…");
}
```

在 CI 的 debug 下，**这一整块被跳过**。实测（debug）：

```
1080p Buffer Benchmark (iterations=10):
  Old FNV-1a = 29.070 ms, New Word-Wise = 14.617 ms, Speedup = 1.99x
test test_speedup_1080p_word_wise_vs_byte_by_byte ... ok     ← 照样通过
```

即：14.6 ms（阈值 1.5ms 的 **9.7 倍**）、1.99×（阈值 8× 的 **1/4**），用例仍然 ok。
**推论：把哈希退回逐字节实现，CI 依然全绿。** 这就是上轮那个 240× 退化能一路绿灯的同一个结构性原因——门禁的写法和运行 profile 对不上。

`crates/agent/tests/som_perf_test.rs:50-63` 有同样的问题（debug 走 `best_ms < 100.0` 的宽松分支，release 才是 `<5.0`）：

```rust
if !cfg!(debug_assertions) { assert!(best_ms < 5.0, …); } else { assert!(best_ms < 100.0, …); }
```

release 下实测 `best=4.64ms, median=5.11ms` —— 断言只看 `best_ms`，**中位数已经越过 5.0ms**，余量很薄，换台慢一点的机器或 CI runner 就会红。

**唯一在 debug 下依然生效**的耗时断言是 `registry_scaling_test` 的 `<50µs`（未被 cfg 包裹），实测 1.94µs @5000 通过。

### 2.5 `thresholds.toml` 里大半字段没有消费方

| 字段 | 有消费方？ |
|:--|:--|
| `block_hash_1080p_ms_max` / `block_hash_2560_ms_max` | 仅在 `--probe-output`（CI 不调用） |
| `relative_regression_percent = 10.0` | ❌ 被读进 dict 后**从未参与任何比较**（仅单测里断言 `> 0.0`） |
| `baseline_name` / `comparison_target` | ❌ 零消费方 |
| `minimum_repeated_runs` / `same_host` / `same_power_profile` / `release_profile` | ❌ 零消费方 |

"相对基线回退 10% 才拦"这条策略**根本没有实现**，字段只是摆在那里。而文件自身的注释已经写明 `Informational thresholds … Do not use wall-clock values from shared CI runners as a hard gate` —— 与 CI 步骤名 `Check benchmark compilation and performance thresholds` 的期望不一致。

### 2.6 修法建议（按性价比排序）

1. **让 release 断言真的跑起来**：CI 增加 `cargo test --workspace --release --all-features --locked`（至少覆盖 `at-pc-desktop-core` / `at-pc-agent` / `at-pc-server` 三个包）。这是**一行改动**，直接把 P0-A/P2-C 的真门禁激活。
2. **接线探针**：CI 里 `cargo run --release -p at-pc-benchmarks --example perf_probe > /tmp/probe.txt`，再 `check_perf_thresholds.py --probe-output /tmp/probe.txt`。
3. **删掉或实现 `relative_regression_percent`**：要么接 criterion baseline 做同机对比，要么从 toml 移除，避免"看起来有门禁"。
4. `som_perf_test` 断言改看中位数并放宽到合理余量（当前 best=4.64 / median=5.11）。

---

## 3. ③ 5,000 终端扩展性测试：已补齐

`crates/server/tests/registry_scaling_test.rs` 新增 `test_update_terminal_meta_latency_scaled_5000`，注册 5,000 个终端、压 2,000 次更新，并带硬断言 `avg_latency < 50µs`（未被 `debug_assertions` 包裹，因此 CI 下也生效）。

| profile | 1,000 档 avg | 5,000 档 avg | 判定 |
|:--|--:|--:|:--|
| release | 1.111 µs | **619 ns** | ✅ 通过，且规模变大反而更快（写路径已是 O(1)） |
| debug（CI） | 2.287 µs | **1.941 µs** | ✅ 通过 |

上轮标的"写路径随规模增长"确实已被消除。

## 4. ⑤ 仍未处理：`list_terminals` 仍是 O(n) 且无断言

独立复测（release，`/tmp/atpc-perf`）：

```
     终端数 update p50(ms) update p95(ms)       list(ms)
    1000         0.0010         0.0012          0.738
    2500         0.0009         0.0011          1.926
    5000         0.0008         0.0008          2.816
```

- `update_terminal_meta` 已是 O(1)（p50 0.8–1.0 µs，与规模无关）。
- `list_terminals` 仍随规模线性增长（0.74 → 2.82 ms，约 0.56 µs/终端），**测试里只 `assert_eq!(list.len(), 5000)`，没有任何耗时断言**。
- 建议：加分页/索引，或至少加一条上界断言把它纳入门禁。

## 5. ④ 其它：废弃函数与探针

- `crates/desktop-core/src/lib.rs` 给 `compute_sample_hash` 加了 `#[deprecated]`；现存引用只剩它自己的单测（已加 `#[allow(deprecated)]`），`crates/agent/tests/stream_blocking_pool_test.rs:96` 还有一条源码级守卫 `!src.contains("compute_sample_hash")`。`cargo clippy … -D warnings` 通过，未因 deprecation 破门。
- `perf_probe` 已改测 `compute_block_hashes`（第 2 节 0.503 ms / 2.5%），1–8 节全量输出、exit 0。

---

## 6. 复现命令

```bash
# 全量回归
cargo test --workspace --no-fail-fast
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

# release 门（CI 里没跑的那条）
cargo test --release -p at-pc-desktop-core --test block_hash_test -- --nocapture
cargo test --release -p at-pc-agent --test som_perf_test -- --nocapture
cargo test --release -p at-pc-server --test registry_scaling_test -- --nocapture

# 探针 → 门禁（手工接线验证）
cargo run --release -p at-pc-benchmarks --example perf_probe > /tmp/probe_out.txt
python3 scripts/check_perf_thresholds.py --probe-output /tmp/probe_out.txt

# 独立工装（仓库外，未改仓库源码）
cd /tmp/atpc-perf && ./target/release/verify_som_fix
cd /tmp/atpc-perf && ./target/release/registry_scale_verify
```
