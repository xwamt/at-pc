# 优化落地复核（第四轮）— 1fa7055 / a3cf5fb

> 复核对象：`1fa7055 chore(perf): polish thresholds validator and include process_cache & window_review in release perf gate` + `a3cf5fb`（HEAD，架构整合）
> 上一轮：`docs/plans/2026-09-16-optimization-landing-verification-round3.md`（针对 `33657a7`）
> 复核方式：同夹具实测 + 仓库外 A/B 副本；**未改动仓库任何源码**（补丁只打在 `/tmp/atpc-ab` 副本）。
> 机器条件：macOS / 2560×1600 / scale 1.0。

---

## 0. 结论速览

| # | 本轮待验项 | 判定 | 一句话依据 |
|:--|:--|:--|:--|
| ① | `1fa7055` 是否修好三轮 §4.2 / §4.3 / §4.4 | ✅ **全部修好** | `relative_regression_percent` 幽灵默认值已消失；脚本补上 `process_cache_test` + `macos_window_review_test`；单测由硬编码 `1.0` 改 `assertGreater(...,0.0)`+`assertIn` |
| ② | 门禁端到端是否真通过（含新增 2 条测试） | ✅ **GATE_EXIT=0** | 5 个 release target / 14 条测试全绿 |
| ③ | 门禁能否拦住回退 | ✅ **能** | 篡改阈值 → `GATE_EXIT=1`，正确短路在探针断言，**未误报成功** |
| ④ | `thresholds.toml` 的键是否真门禁 | ❌ **5 个键只有 1 个真断言** | 逐个篡改为 `0.0001`：只有 `block_hash_1080p_ms_max` 变红 |
| ⑤ | 门禁脚本可重复运行性 | ❌ **macOS 上不可重复运行** | 第 14 行 `mktemp /tmp/perf_probe_XXXXXX.txt` 的 `.txt` 后缀使模板**不随机化** |
| ⑥ | CI 是否验证过上述结论 | ❌ **CI 从未执行过任何一次** | 远端无 `.github/workflows/`，GitHub API `actions/runs` 与 `actions/workflows` 均 `total_count=0` |

**核心判断：三轮报的"P0-B 门禁真接线"成立（脚本本身能跑、能拦）；但门禁的"覆盖面"远小于它的外观 —— 5 个阈值键里只有 1 个真正参与断言，而且这套门禁迄今一次都没在 CI 上跑过。**

---

## 1. 门禁端到端实测：GATE_EXIT=0

在仓库外副本 `/tmp/atpc-ab` 跑**与 CI 完全相同的命令**（`ci.yml:96-97`）：

```
$ ./scripts/check_perf_thresholds.sh
==> Verifying benchmarks compilation without running...
    Finished `bench` profile [optimized] target(s) in 8.25s
==> Running unit tests for threshold validator...
Ran 4 tests in 0.062s
OK
==> Running release performance probe and asserting threshold limits...
     Running `target/release/examples/perf_probe`
[OK] Validated crates/benchmarks/thresholds.toml: {'block_hash_1080p_ms_max': 1.0, 'block_hash_2560_ms_max': 2.0,
     'som_detection_2560_ms_max': 8.0, 'registry_update_5k_micros_max': 50.0, 'registry_list_5k_ms_max': 15.0}
Extracted probe metrics: {'block_hash_1080p_ms': 0.521, 'rgba_to_rgb_ms': 0.576}
[OK] All probe metrics satisfy performance threshold gates.
==> Running release profile performance test assertions...
==> All performance threshold gates passed successfully.
GATE_EXIT=0
```

### 1.1 5 个 release target 全部真跑到（`1fa7055` 新增的两条已生效）

| release 测试 | 测试数 | 结果 | 对应门禁 |
|:--|--:|:--|:--|
| `block_hash_test` | 5 | ✅ ok | 词宽哈希 < 1.2 ms & speedup ≥ 8× |
| `som_perf_test` | 3 | ✅ ok | SoM 2560 中位 < 8 ms |
| `process_cache_test` | 3 | ✅ ok | 热命中 < 300 µs（**本轮新增**） |
| `macos_window_review_test` | 1 | ✅ ok | 审查中位 < 5 ms（**本轮新增**，Darwin 门控） |
| `registry_scaling_test` | 2 | ✅ ok | update < 50 µs / list < 15 ms |
| **合计** | **14** | **0 失败** | |

→ 三轮 §4.3 的"release 清单漏了两条测试"已修复：`check_perf_thresholds.sh:23-26` 现在含这两条，且 macOS 那条用 `uname` 门控，不会在 Linux 上因缺测试文件而红。

### 1.2 ⚠️ 一个环境假警报（非仓库问题）

在副本里第一次跑门禁得到 `GATE_EXIT=101`，报：

```
error: couldn't read `crates/agent/src/app/../../../../media/at-pc-icon.png`: No such file or directory
```

这是我先前 rsync 建 A/B 树时**漏拷 `media/`** 导致的（`ui.rs:618` 用 `include_bytes!` 引该图标），与仓库无关。补上 `media/` 后即通过。**工装备忘：rsync 建副本必须一并带 `crates/server/frontend/dist` 与 `media/`。**

---

## 2. 两板斧之二：门禁真能拦住回退

在副本里把 `block_hash_1080p_ms_max` 篡改为 `0.0001`（`0.0001` 无法满足），跑同一条命令：

```
==> Running release performance probe and asserting threshold limits...
     Running `target/release/examples/perf_probe`
[FAIL] block_hash_1080p_ms exceeded: actual=0.519ms > limit=0.000ms
GATE_EXIT=1
```

两项"短路正确性"都成立（这是比退出码更关键的证据）：

| 检查 | 期望 | 实测 |
|:--|:--|:--|
| 是否误报成功 | 否 | `grep -c "passed successfully"` = **0** |
| 是否越过探针继续跑 release 测试 | 否 | `grep -c "release profile performance test assertions"` = **0** |

→ `set -euo pipefail` 正确在探针断言处中止。**"能拦住回退"成立。**

---

## 3. ❗新发现 1：`thresholds.toml` 的 5 个键只有 1 个真正参与断言

三轮 §4.1 报的是"3 个新键**零消费方**"。`1fa7055` 让校验器**读**了它们（`check_perf_thresholds.py:80-93`），所以"零消费方"字面上不再成立 —— 但**读进来只用于校验正数并回显，从不参与任何比较**。

**实测（逐个篡改为 `0.0001`，喂真实探针输出）**：

| 被篡改的键 | 退出码 | 判定 |
|:--|--:|:--|
| （未篡改基线） | 0 | 正常通过 |
| `block_hash_1080p_ms_max` | **1** | ✅ **唯一真门禁** —— `[FAIL] block_hash_1080p_ms exceeded: actual=0.493ms > limit=0.000ms` |
| `block_hash_2560_ms_max` | 0 | ❌ 从不被断言（探针**不输出 2560 指标**） |
| `som_detection_2560_ms_max` | 0 | ❌ 仅被校验为正数 |
| `registry_update_5k_micros_max` | 0 | ❌ 仅被校验为正数 |
| `registry_list_5k_ms_max` | 0 | ❌ 仅被校验为正数 |

代码层面可确认根因：`check_probe_against_thresholds()`（`check_perf_thresholds.py:124-131`）只比较 `block_hash_1080p_ms`；`parse_probe_output()`（`:108-121`）只认得 `block_hash_1080p_ms` 与 `rgba_to_rgb_ms` ——而 **`rgba_to_rgb_ms` 被解析出来后也从不比较**。

而 `thresholds.toml` 里的注释恰恰写着：

```toml
# Performance threshold hard gates enforced in CI and local verification
```

**实际只有 1/5 为真。** 另外，这 3 个新键"对应"的真门禁其实是 Rust 测试里**硬编码**的 `8.0` / `50µs` / `15ms`（`som_perf_test.rs`、`registry_scaling_test.rs`）——**双重真相**依旧存在：改 toml 无任何效果，改测试才是真改门禁。

> 建议（二选一）：① 让探针输出 SoM/registry 指标、并把这 4 个键接进 `check_probe_against_thresholds`（真门禁）；② 从 toml 删掉这 4 个键，只留真正生效的 `block_hash_1080p_ms_max`，并同步修正注释。当前形态是"看起来覆盖 5 项、实际覆盖 1 项"，比不写更危险。

---

## 4. ❗新发现 2：门禁脚本第 14 行的 `mktemp` 在 macOS 上不可重复运行

```bash
PROBE_OUTPUT="$(mktemp /tmp/perf_probe_XXXXXX.txt)"
```

**macOS（BSD）实测**：

```
$ mktemp /tmp/perf_probe_XXXXXX.txt
/tmp/perf_probe_XXXXXX.txt        # ← 未随机化！返回字面路径
$ mktemp /tmp/perf_probe_XXXXXX.txt          # 第二次
mktemp: mkstemp failed on /tmp/perf_probe_XXXXXX.txt: File exists   # exit=1
$ mktemp /tmp/perf_probe_XXXXXX
/tmp/perf_probe_5RmnzD            # ← 以 X 结尾才随机化
```

根因：**BSD `mktemp` 要求模板以 `X` 结尾**；`.txt` 后缀使 X 串不在末尾，于是**完全不替换**，返回固定文件名。

后果（三条都会真实踩到）：

1. **不可重复运行**：第 2 次在 `set -e` 下直接 `File exists` 中止（这正是我本轮最初被卡住的原因）。
2. **并发/跨工作树互抢**：仓库与副本共用同一个 `/tmp/perf_probe_XXXXXX.txt`，两个门禁同时跑会一写一读甚至互相 `trap rm` 掉对方文件（我曾观测到下游 Python `FileNotFoundError`，即此竞态）。
3. **"粘性"残留**：`trap 'rm -f "${PROBE_OUTPUT}"' EXIT` 在运行被 kill（Ctrl-C / CI 超时）时不生效，残留文件会**污染该机器上之后每一次**门禁运行。

**平台差异说明**：GNU coreutils 的 `mktemp` 反而更宽松 —— 官方文档明确 *"template 必须含至少三个连续 X；**最后一段 X 串**会被替换"*，且 *"若未指定 `--suffix`，则由 `template` 中最后一个 X 推断一个后缀"*（文档示例 `mktemp file-XXXX-XXXX.txt` → `file-XXXX-eI9L.txt`）。因此 **Linux/CI 不受影响，macOS 本地必踩** —— 而本项目的门禁主要就是开发者在 macOS 上本地跑。

> 修法（两边都正确）：`PROBE_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/perf_probe_XXXXXXXXXX")"`，或直接用 `mktemp -t perf_probe`。
> 本轮已把该补丁**只打在 `/tmp/atpc-ab` 副本**上，仓库源码未动。

---

## 5. ❗新发现 3：CI 从未执行过 —— 门禁的"CI 覆盖"目前是纸面的

三轮报告写"CI 改跑 `./scripts/check_perf_thresholds.sh`，真接线了" ——**配置文件层面成立，但从未被 CI 执行过**。

证据：

```
$ git rev-list --count origin/main..main
7                                   # 本地领先远端 7 个提交
$ git log --oneline origin/main -1
9c41689 feat(at-pc): multi-monitor, som, uia, and computer use enhancements

$ curl -s -o /dev/null -w "%{http_code}" https://api.github.com/repos/xwamt/at-pc/contents/.github/workflows
404                                 # 远端分支上根本不存在 .github/workflows/
$ curl -s ".../actions/workflows" | jq .total_count
0
$ curl -s ".../actions/runs"      | jq .total_count
0                                   # 从未有任何一次运行
```

含义：

- `d552ab8` / `33657a7` / `1fa7055` / `a3cf5fb` 四个改动 CI 与门禁的提交**都只在本地**；远端默认分支仍停在 `9c41689`，其上没有工作流文件。
- 因此三轮 §4.5 的担忧（"`perf_probe` 第 6 节 `assert_eq!` 在 headless Linux 上是否会红"）**至今无法用 CI 结论回答** —— 不是因为没风险，而是因为**一次都没跑过**。
- 所有"CI 门禁已生效"的表述，严格说都只是"**CI 配置已写好**"。

> 建议：把 `main` 推到远端（或至少推一个含 `.github/workflows/` 的分支）并观察首次 pipeline；若 Linux 上因 xcap / 图形栈问题失败，按三轮 §4.5 的建议把 `perf_probe` 第 6/7 节包进 `cfg` 或环境探测。

---

## 6. `1fa7055` 对三轮残留问题的修复核对

| 三轮 § | 问题 | 本轮核对 |
|:--|:--|:--|
| §4.2 | `relative_regression_percent` 幽灵默认值（toml 已删、代码仍以 10.0 读出、单测还断言它 > 0） | ✅ **已消除** —— 全文再无该键，单测也不再断言 |
| §4.3 | 门禁 release 清单漏 `process_cache_test` / `macos_window_review_test` | ✅ **已补**，且实测两条都真跑到（见 §1.1） |
| §4.4 | 单测把阈值硬编码为 `1.0`，合法调整阈值也会红 | ✅ **已改** —— 改为 `assertGreater(result["block_hash_1080p_ms_max"], 0.0)` + `assertIn("som_detection_2560_ms_max", result)` 等 |
| §4.1 | 3 个新键零消费方 | ⚠️ **表面已修、实质未修** —— 变为"被读取但不参与断言"（见 §3） |
| §4.5 | `perf_probe` 在 headless Linux CI 上未验证 | ⚠️ **仍无法验证** —— CI 一次都没跑过（见 §5） |

注：`1fa7055` / `a3cf5fb` 均**未推送**，上述修复目前只存在于本地。

---

## 7. 复现命令

```bash
# ① 门禁端到端（CI 同一条命令；macOS 上需先按 §4 修 mktemp 或清理 /tmp/perf_probe_XXXXXX.txt）
cd /tmp/atpc-ab && ./scripts/check_perf_thresholds.sh          # GATE_EXIT=0

# ② 阈值篡改矩阵（证明只有 1 个键真断言）
/usr/bin/python3 /tmp/atpc-perf/gate_matrix.py                  # 仅 block_hash_1080p_ms_max → exit 1

# ③ 整脚本两板斧
#   在 /tmp/atpc-ab 把 block_hash_1080p_ms_max 改为 0.0001 后重跑 → GATE_EXIT=1 且无 "passed successfully"

# ④ CI 是否执行过的外部证据
git rev-list --count origin/main..main                          # 7
curl -s -o /dev/null -w "%{http_code}\n" \
  https://api.github.com/repos/xwamt/at-pc/contents/.github/workflows   # 404
curl -s https://api.github.com/repos/xwamt/at-pc/actions/runs | grep total_count   # 0
```

工装均位于仓库外：`/tmp/atpc-perf/gate_matrix.py`（阈值矩阵）、`/tmp/atpc-ab`（A/B 副本，含 mktemp 补丁）。

---

## 8. 四轮累计状态

| 项 | 状态 |
|:--|:--|
| P0-A 词宽块哈希 | ✅ 1080p 0.555 ms（16×） |
| P0-B CI 性能门禁 | ⚠️ **脚本能跑能拦（GATE_EXIT=0 / 篡改即红），但 ① 5 个阈值键只有 1 个真断言 ② 脚本在 macOS 不可重复运行 ③ CI 从未执行过** |
| P0-C 抓屏前置 + 空闲退避 | ✅ 静态桌面 CPU 2.0–2.2%（基线 32.4%） |
| P1-A `perf_probe` panic | ✅ 已改测生产函数 |
| P1-B/C base64 预计算 + 二进制流 | ✅ |
| P1-D keepalive 500 ms / P1-E MouseMove 审计降级 | ✅ |
| P1-F macOS 前台审查 | ✅ 0.262 ms（基线 20.3–21.4） |
| P2-A 截屏默认 1280 | ✅ 430.3 → 116.9 KB |
| P2-C SoM 降采样 | ✅ 逐字节等价且快 26.7–28.1×；`w > 1280` 分支默认仍不可达 |
| P2-E meta 写路径 / P2-F 进程 TTL 缓存 | ✅ O(1) / 16.3 µs |
| 5,000 档扩展性测试 | ✅ 断言 < 50 µs，实测 619 ns |
| `list_terminals` | ✅ 1.33–1.37×（仍 O(n)，断言余量 11×） |
| `thresholds.toml` 键的消费方 | ❌ **5 键中 4 键不参与断言**（`block_hash_2560`/`som_detection_2560`/`registry_update_5k`/`registry_list_5k`） |
| 门禁脚本可重复运行性 | ❌ **macOS `mktemp` 模板不随机化（第 14 行）** |
| 门禁在真实 CI 的执行 | ❌ **CI 从未执行过（远端无 `.github/workflows/`，7 提交未推送）** |
