# MCP 远程控制实测 · 第二轮（最新客户端 + 最新服务端）

- **日期**：2026-09-18
- **被测版本**：`855b082`（本地 HEAD；服务端 arm64 `at-pc-server-macos`，客户端 `at-pc-agent.exe`）
- **任务**：① 用最新客户端完成「打开 Lark → 给 Alan Xia 发一首诗」端到端实测；② 核验本会话讨论的性能优化是否真正落地
- **方法**：全部通过 `POST /mcp` 的 JSON-RPC 2.0 驱动真实客户端；坐标一律用 Pillow 在**原生分辨率**截图上测量；性能对比走同一条活链路
- **副作用声明**：仓库源码**零改动**（仅新增本报告与证据图）；重启了本机 server；在客户端唤醒了 Lark 窗口；向 Alan Xia 会话**真实发送**了 1 条消息；鼠标最终停在 Lark 输入框内

---

## 0. 结论速览

| 结论 | 判定 |
|:--|:--|
| 端到端远程控制（打开 Lark → 输入 → 发送） | ✅ 完成，双路独立验证通过 |
| 「最新客户端」是否真的生效 | ✅ 生效（`list_monitors` 由报错变为正常返回） |
| 本机 server 是否最新 | ❌ **不是**——重启前跑的是 09-16 16:36 的旧进程，早于本会话全部优化提交 |
| `capture_screen` 默认降采样 1280 | ✅ 活链路实测生效（JPEG −70.0%） |
| `list_processes` TTL 缓存 | ✅ 活链路实测生效（冷 70.4ms → 热 5.4ms，13.0×） |
| `list_terminals` 短路 + `sort_unstable` | ✅ 新 server 中生效（端到端 p50 1.1ms） |
| 词宽哈希 / SoM 降采样 / 5k O(1) / macOS 窗口审查 | ✅ 由 release 门禁 14 条测试全部通过背书 |
| 「MouseMove 不落盘」 | ⚠️ **范围被误解**：仅指桌面输入中继通道；MCP 工具 `mouse_move` 仍每条审计 296 字节 |
| `thresholds.toml` 5 个键的真门禁覆盖 | ❌ 仍只有 **1/5**（复现） |
| 门禁脚本 `mktemp` 路径不随机 | ❌ 仍存在（措辞修正，见 D2） |
| dist 交叉打包覆盖 arm64 | ❌ **新缺陷**：`dist/at-pc-server-macos` 已变成 x86_64，本机跑不起来（D1） |

---

## 1. 环境与链路

### 1.1 重启前的状态（重要）

`lsof` 显示 9800/9801 由 **PID 23570** 持有，`ps -o lstart` 显示其启动于 **2026-09-16 16:36:44**——而本会话的性能相关提交是：

```
33657a7  2026-09-16 18:08:17  fix(perf): resolve round 2 audit findings - center-align som downsample,
                              wire live ci gates, and optimize list_terminals
1fa7055  2026-09-16 18:29:46  chore(perf): polish thresholds validator and include process_cache
                              & window_review in release perf gate
a3cf5fb  2026-09-16 18:31:36  feat(at-pc): consolidate workspace architecture, ...
```

即 **旧的 server 进程比全部优化提交都早**，用它是无法验证服务端侧优化的。另外还发现 **3 个残留 server 进程**（3054 / 23570 / 23846，均为 09-16 启动），其中 2 个对 SIGTERM 无响应，需 `kill -9`。

### 1.2 无法直接用 dist 散落二进制启动（触发缺陷 D1）

```
$ ./dist/at-pc-server-macos --help
(eval):1: bad CPU type in executable: ./dist/at-pc-server-macos
$ file dist/at-pc-server-macos
dist/at-pc-server-macos: Mach-O 64-bit executable x86_64     ← 本机是 arm64 (Apple M4 Pro)
```

改为从 `dist/at-pc-macos-arm64.zip` 解出 arm64 二进制，放在**仓库外的独立运行目录** `/Users/clkj/atpc-gateway/` 启动。

### 1.3 新 server 的启动方式与选择理由

`--stdio` 模式下 stdin 一旦 EOF，`main.rs` 的 `tokio::select!` 会让 `run_stdio_server` 返回并**整体关闭** HTTP/WS。因此改用**独立网关模式**：

```bash
cd /Users/clkj/atpc-gateway
nohup ./at-pc-server-macos --host 0.0.0.0 --ws-port 9801 --mcp-port 9800 > server.log 2>&1 < /dev/null &
```

启动日志（PID 77138）：

```
INFO at_pc_server: Listen host: 0.0.0.0, WebSocket port: 9801, MCP port: 9800
INFO at_pc_server::ws: WebSocket gateway listening on ws://0.0.0.0:9801/ws
INFO at_pc_server::shutdown: MCP HTTP/SSE gateway listening on http://0.0.0.0:9800
INFO at_pc_server::ws::registry: Registered terminal: laptop-roeh3p85-b6a918 (session: 1)   ← 3.5s 后自动重连
```

客户端 `ws_client.rs:341` 有 `reconnect_interval_secs = 5` 的自动重连，**重启服务端是安全的**——实测 3.5 秒完成重注册。

### 1.4 落盘位置修正了上一轮的审计缺陷

本轮 server 从专用目录启动，审计写入**磁盘上真实存在**的文件：

```
lsof -p 77138 →  10w  REG  ...  inode 16120519  /Users/clkj/atpc-gateway/audit.jsonl
$ ls -la /Users/clkj/atpc-gateway/audit.jsonl  →  inode 16120519   ✅ 与 fd 一致
```

上一轮「审计写进已删除 inode」的成因是 `dist/` 被重新打包覆盖；本轮不复现，但仍属**部署方式导致的脆弱性**（见 D5）。

### 1.5 客户端确实是新版

| 探针 | 旧客户端（09-07） | 本轮 |
|:--|:--|:--|
| `list_monitors` | `Unknown or unsupported tool` | ✅ 返回 `2560×1600 @ (0,0), scale 1.0, primary` |

`tools/list` 仍为 **39 个**（33 agent + 6 server），与 `protocol/src/tools.rs` 一致。

---

## 2. 实测任务：打开 Lark，给 Alan Xia 发一首诗

### 2.1 关键前情：目标会话身份必须查证

第一次截屏发现两点，直接改变了操作路径：

1. Lark 窗口只占**上半屏**（`window_bounds = [-7, 0, 2574, 783]`，由 `get_ui_tree` 权威返回），且 `win+up` 不生效；
2. **当前打开的会话是「Jack Zhu（2 飞书）」，不是 Alan Xia**。

> 若跳过确认，诗会发错人。这一步是本轮最有价值的操作纪律。

期间还遇到一个陷阱：`list_windows` 长期报 `fg=False`，原因是 **Windows「贴靠助手」(Snap Assist)** 抢占了前台（`explorer.exe` rect `[0,776,2560,776]`）。发 `Esc` 后重新 `focus_window(Lark)`，`fg` 才变为 `True`。

### 2.2 坐标：全部实测，不目测

**输入框**（`crop=[560,600,2010,190]` 原生 PNG，逐行亮度突变定位）：

| 边界 | 物理坐标 |
|:--|:--|
| 上边框 | `y = 699` |
| 下边框 | `y = 746` |
| 左/右边框 | `x = 699 / 2526` |
| **点击中心** | **(1612, 723)** |

**左侧会话列表**（`crop=[0,30,560,400]`，扫描暗色文本行聚类）：

```
y  69 (chip 行) / 136 (Alan Xia) / 197 (Jacky Liao) / 259 (研发团队) / 318+324 (技术部) / 383 (创霖科技)
→ Alan Xia 行中心落在物理 y ≈ 166，取 (400, 167) 点击
```

点击后裁会话头确认标题为 **Alan Xia** ✅。

### 2.3 身份判定：这个「Alan Xia」是账号本人

这一步用三条独立证据交叉确认，结论与直觉相反：

| 证据 | 观察 |
|:--|:--|
| 点击会话头头像弹出的资料卡 | **`Alan Xia`**，部门 IT/研发团队，工号 **0002**，入职 2025-05-06，直属上级 Vincent Yan；签名字段为「**输入你的个人签名...**」，按钮为「**消息**」——这是**自己的资料卡** |
| 输入框占位文字 | 「按住 Alt 说话 **可以给自己发送文件或转发消息**」（对比 Jack Zhu 会话为「**发送给** Jack Zhu（飞书）」） |
| 气泡排布 + 背景水印 | 所有气泡**左对齐带头像**（自己发的应右对齐）；聊天背景水印就是 **"Alan Xia"** |

**结论**：客户端 Lark 的登录账号显示名即 **Alan Xia**，该会话是**「自己」会话**。
「给 Alan Xia 发一首诗」在语义上等于发给自己。**这一点请确认是否符合预期**——如果本意是发给某位同名同事，需要另行指定（当前列表里没有第二个 Alan Xia）。

### 2.4 执行与双路验证

输入（`batch_actions` 12 步，`delay_ms=260`，**只输入不提交**）：

```
【at-pc 远程控制·第二轮实测】
新客初登旧日关，
一屏相隔抵千山。
指端轻点无风雨，
万里机枢自往还。
```

- `type_text` 耗时 12/5/6/6/6 ms（约 0.6 ms/字），逐字符注入；输入后**放大截图逐字核对无丢字**；
- 右下角提示「**Shift + Enter 换行**」⇒ **Enter 即发送**；
- 发送后再验证输入框已清空（截图与聚焦前状态**逐字节相同**，20.2 KB → 20.2 KB）。

| 证据 | 结果 |
|:--|:--|
| ① 会话气泡 | ✅ 新消息出现在会话底部 |
| ② 左侧列表预览 | ✅ 更新为「Alan Xia ·【at-pc 远程控制·第二轮实测】新客初登旧日关，一屏相隔…」 |
| ③ 输入框状态 | ✅ 已清空，占位文字恢复 |

证据图：`2026-09-18-mcp-remote-control-assets/r2-01..07`。

---

## 3. 性能优化落地核验

### 3.1 活链路实测（真链路、可复审）

**P1 · `capture_screen` 默认降采样**（`screen.rs:16` `DEFAULT_MAX_DIMENSION = 1280`）

| 参数 | JPEG q75 | PNG |
|:--|--:|--:|
| 默认（不传 `max_dimension`） | 108,497 B (106.0 KB) | 472,022 B |
| 显式 `1280` | 108,500 B (106.0 KB) | 472,022 B |
| 显式 `2560`（原生） | 361,820 B (353.3 KB) | 1,515,057 B |
| **默认 vs 原生** | **−70.0%** | **−68.8%** |

默认与显式 1280 **产物尺寸一致**（PNG 逐字节相同），返回文本明确 `resolution 1280x800 (downscaled from 2560x1600)` ⇒ 默认值确实生效。

**P2 · `list_processes` TTL 缓存**（`process.rs:25` `PROCESS_CACHE_TTL = 1000ms`）

```
round 1..8   cold(>1.25s 空闲) = 67.9 / 60.6 / 70.5 / 68.3 / 70.5 / 70.4 / 126.1 / 74.6 ms
             hot(<1s 连击)     = 12.5 /  5.2 /  4.8 /  5.6 /  6.2 / 57.5 /   4.9 /  5.1 ms
中位数       cold = 70.4 ms      hot = 5.4 ms      → 13.0×
```

> 注意口径：5.4 ms 是**端到端**热命中（含 HTTP + WS 转发 + 序列化），不等于基准里的 **16.3 µs** 进程内缓存查询；两者不矛盾，别混用。
> 第 6 轮 hot=57.5 ms 是噪声尖峰，故取中位数而非单次。

**P3 · `list_terminals`**（`registry.rs:299` 短路、`:325` `sort_unstable_by`）

10 次端到端 **median 1.1 ms**（min 0.9 / max 1.4）。当前只有 1 台终端，O(n) 规模效应不显现——该项由单测 `registry_scaling_test` 的 5k 档背书。

### 3.2 项目自带门禁（release）

```
$ ./scripts/check_perf_thresholds.sh
==> Verifying benchmarks compilation without running...
==> Running unit tests for threshold validator...
==> Running release performance probe and asserting threshold limits...
Extracted probe metrics: {'block_hash_1080p_ms': 0.505, 'rgba_to_rgb_ms': 0.572}
==> Running release profile performance test assertions...
...
GATE_EXIT=0
```

5 个 target / **14 条测试全绿**，逐条对应「已落地的优化」：

| 优化 | 对应测试 |
|:--|:--|
| 词宽哈希（1080p） | `test_speedup_1080p_word_wise_vs_byte_by_byte`、`test_dirty_block_ratio_properties`、`test_boundary_tiles_with_unaligned_trailing_bytes`、`test_block_hash_determinism_and_single_byte_mutation`、`test_sensitivity_across_all_pixel_channels` |
| SoM 快速降采样 | `test_som_downsampled_vs_small_canvas`、`test_som_downsampled_detection_and_coordinate_mapping_accuracy` |
| 进程缓存 TTL | `test_list_processes_ttl_cache_speedup`、`test_list_processes_cache_ttl_invalidation`、`test_list_processes_cache_with_filtering_and_sorting` |
| registry 5k O(1) | `test_update_terminal_meta_latency_scaled_1000`、`..._5000` |
| macOS 前台窗口审查 | `test_macos_direct_window_review_latency` |
| 坐标缩放精度 | `test_coordinate_scaling_roundtrip_precision` |

### 3.3 只能由源码/单测背书、无法经 MCP 观测的项

| 优化 | 落地位置 | 观测手段 |
|:--|:--|:--|
| 静态桌面 CPU / 帧决策跳过 JPEG | `desktop-core/src/lib.rs:190+` `decide_frame_send` | 单测 `idle_keepalive_skips_jpeg_and_fires_at_interval` |
| 流保活 500 ms | `desktop-core/src/lib.rs:177` `STREAM_KEEPALIVE_INTERVAL` | 常量 + 门禁 |
| `update_terminal_meta` 5k O(1) | server registry | 单测 1000/5000 档 |
| macOS 前台窗口审查 0.262 ms | desktop-core | 单测（Darwin 门控） |
| 词宽哈希 0.5 ms | desktop-core | 探针 `dirty-check block hash 0.515 ms` |

### 3.4 `MouseMove 不落盘`——范围澄清（修正旧记录）

实测 10 次 MCP 工具 `mouse_move` → 审计 **+10 行 / +2960 字节（296 B/条）**，尾部可见 `action:"tool:mouse_move"`。
即 **MCP 工具调用照常审计**（安全上这是对的）。真正被豁免的是**另一条路径**：

| 机制 | 位置 | 行为 |
|:--|:--|:--|
| HTTP 桌面输入接口免审计 MouseMove | `server/src/mcp/dashboard.rs:814-816` | `should_audit = !matches!(event, MouseMove{..})`，其余事件仍审计 |
| WS 中继合并连续 MouseMove | `server/src/ws/handler.rs:157-175` | 连续 MouseMove 只保留最后一帧再转发 |

→ 之前的记录「MouseMove 不落盘」表述过宽，已按上述修正。

---

## 4. 缺陷清单

### D1【新·高】交叉打包用 x86_64 覆盖 arm64 散落产物

**现象**：`./dist/at-pc-server-macos --help` → `bad CPU type in executable`。

**根因**（`scripts/package_dist.py`）：

```python
def dist_binary_names(triple):            # 140 行：不含架构标识
    ...
    if is_apple(triple):
        return "at-pc-agent-macos", "at-pc-server-macos"
```

`package_target()` 把每个 target 的产物 `shutil.copy2` 到同一组文件名；`main()` 的 `--all` 顺序为

```python
supported_all = ["aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-pc-windows-gnu"]
```

⇒ **后构建的 x86_64 覆盖先构建的 arm64**。

**取证**：

```
$ file dist/at-pc-server-macos   → Mach-O 64-bit executable x86_64   (6,123,728 B)
$ unzip -l dist/at-pc-macos-arm64.zip | grep server
   5578112  09-18-2026 10:41   at-pc-server-macos                     ← zip 内才是 arm64
$ shasum -a 256 dist/at-pc-server-macos → 841f3a38…  （与 SHA256SUMS 中该条目一致 ⇒ 校验和"对的"，但对象是 x86_64）
```

**影响**：zip 分发不受影响（各 zip 在拷贝后立即打包，内容正确）；受影响的只有 `dist/` 里的**散落二进制**——而这正是本机部署/联调最常直接运行的形态，且文件名**不带架构**，无法从名字判断。arm64 机器上必然失败。

**建议**：`dist_binary_names()` 纳入架构标签（如 `at-pc-server-macos-arm64`），或对 macOS 不产出散落文件、只产出 zip；并在脚本里对「同一次运行中产出同名文件」做断言。

---

### D2【旧·中】门禁脚本 `mktemp` 模板不随机（措辞修正）

`scripts/check_perf_thresholds.sh:14`

```bash
PROBE_OUTPUT="$(mktemp /tmp/perf_probe_XXXXXX.txt)"
trap 'rm -f "${PROBE_OUTPUT}"' EXIT          # 第 15 行
```

BSD/macOS 的 `mktemp` 要求模板**以 X 结尾**；`.txt` 后缀使其**完全不随机化**，返回字面固定路径。

**受控复现**：

```
$ echo stale > /tmp/perf_probe_XXXXXX.txt
$ ./scripts/check_perf_thresholds.sh
mktemp: mkstemp failed on /tmp/perf_probe_XXXXXX.txt: File exists
GATE_EXIT(有残留) = 1
```

**同时修正上一轮的措辞**：第 15 行的 `trap` 会在正常退出时清理，因此**顺序重复运行是没问题的**（本轮连续跑两次均 `GATE_EXIT=0`）。准确说法是「路径不随机，导致 **① 残留文件（被 kill 的运行）会让下一次直接中止；② 并发运行互抢同一路径**」，而非「不可重复运行」。GNU/Linux（CI）会把最后一段 X 串替换掉，**CI 不受影响**。

**建议**：`mktemp "${TMPDIR:-/tmp}/perf_probe_XXXXXXXXXX"`。

---

### D3【旧·中】`thresholds.toml` 5 个键仍只有 1 个真门禁

`scripts/check_perf_thresholds.py:123-130` 是唯一的比较：

```python
def check_probe_against_thresholds(metrics, thresholds):
    violations = []
    if "block_hash_1080p_ms" in metrics:
        actual = metrics["block_hash_1080p_ms"]
        limit  = thresholds["block_hash_1080p_ms_max"]
        if actual > limit:
            violations.append(...)
    return violations
```

在**仓库外假树**（`/tmp/gate-matrix/`，未改动仓库任何文件）逐键篡改为 `0.0001`，喂真实探针输出：

| 阈值键 | EXIT | 判定 | 证据 |
|:--|:--:|:--|:--|
| `block_hash_1080p_ms_max` | 1 | ✅ **真门禁** | `[FAIL] block_hash_1080p_ms exceeded: actual=0.515ms > limit=0.000ms` |
| `block_hash_2560_ms_max` | 0 | ❌ 不生效 | 探针从不输出 2560 指标 |
| `som_detection_2560_ms_max` | 0 | ❌ 不生效 | 仅校验为正数并回显 |
| `registry_update_5k_micros_max` | 0 | ❌ 不生效 | 同上 |
| `registry_list_5k_ms_max` | 0 | ❌ 不生效 | 同上 |
| — | — | **1/5** | `rgba_to_rgb_ms` 被解析但从不比较 |

toml 注释自称 `# Performance threshold hard gates enforced in CI and local verification`，**只有 1/5 为真**。`1fa7055` 让校验器**读**了那 3 个键（schema 校验 + 回显），但**从未参与比较**——「零消费方」表面已修、实质未修。这比不写更易误导。

**建议**：三选一——① 把 `som_*`/`registry_*` 的真门禁从 Rust 单测硬编码值（`8.0` / `50µs` / `15ms`）改为从 toml 读取（消除「双重真相」）；② 让 perf_probe 输出这些指标并纳入比较；③ 从 toml 删除这些键。

---

### D4【旧·中】`tools/list` 由服务端投影，注册无版本/能力校验

服务端按**自己编译的协议表**（`server/src/mcp/tools.rs::get_mcp_tool_definitions`）投影工具清单，而非客户端上报，且注册时不做版本校验。上一轮因此出现「清单里有、调用说没有」。

**本轮变化**：客户端换成最新版后 `list_monitors` 正常，问题自然消失——说明该缺陷的**触发条件**（二进制过期）真实存在，但**机制未修**。建议注册握手时校验 `agent_version` 或能力位。

---

### D5【旧·低】审计落盘依赖启动目录，重打包即丢

上一轮 `dist/audit.jsonl` 被重新打包删除、进程仍持旧 inode 写入。本轮从专用目录启动后正常。本质是**默认审计路径 = 进程 CWD**，与「产物目录会被覆盖」冲突。建议默认落到稳定目录（如 `~/.at-pc/audit.jsonl`）或支持显式 `--audit-file`（本机已支持该参数，只是默认值不稳）。

---

### D6【旧·低】`type_text` 逐字符无延时、`\n` 不换行（本轮复现）

- 12/5/6/6/6 ms 注入 20/8/8/8/8 字 ⇒ 约 **0.6 ms/字、字符间无 sleep**，重量级应用有丢字风险；
- `wVk=0` 使 `\n` **不会换行**，多行必须显式 `Shift+Enter`（本轮按此法成功）。

**建议**：`options` 暴露可配 `inter_char_delay_ms`；或对 >32 字符的文本自动分段。

---

### D7【旧·低】Electron/Chromium 应用控件不可达（本轮复现）

`get_ui_tree(window_title="Lark")` 只返回 1 个根节点（`compact+depth=3`），拿不到 web 控件 ⇒ `click_element` / `set_element_text` 对 Lark 完全不可用，只能退回像素坐标。**这不是缺陷、是限制**：建议在工具返回值里显式提示「该窗口辅助功能未开启，请改用坐标」，避免调用方反复试探。

---

### D8【旧·低】SoM 是「视觉显著性」而非「UI 控件检测」（上一轮结论）

`get_marked_screen` 会框住桌面壁纸花纹、桌面图标，却不框聊天输入框；且 `window_title` 不裁剪。本轮据此直接改用 Pillow 测量，未再依赖 SoM。

---

## 5. 对上一轮结论的修正

| 上一轮说法 | 本轮修正 |
|:--|:--|
| `mktemp` 使脚本「在 macOS 上不可重复运行」 | ❌ 过强。第 15 行 `trap` 会清理，顺序重跑两次均通过。准确说法：**路径不随机 ⇒ 残留文件/并发运行会失败** |
| 「MouseMove 不落盘」 | ⚠️ 范围错。仅指 HTTP 桌面输入接口 + WS 中继；**MCP 工具 `mouse_move` 仍每条审计 296 B** |
| 审计写进已删除 inode | 归因正确，但属**部署方式**问题（`dist/` 被重打包），换目录启动即不复现 |
| （未提及） | 新增 **D1 交叉打包覆盖**：`dist/` 散落 macOS 二进制被 x86_64 覆盖 |

**上一轮已确认修复、本轮复核仍成立**：`1fa7055` 消除了 `relative_regression_percent` 幽灵默认值；release 清单已补齐（本轮实测 5 target / 14 测试全绿）；单测不再硬编码阈值 `1.0`。

---

## 6. 建议优先级

1. **D1 修 `dist_binary_names()`**（几行）——当前 arm64 Mac 无法直接运行 `dist/` 里的二进制，直接阻塞本机部署与联调；
2. **D2 修 `mktemp` 模板**（一行）——消除残留文件/并发导致的假失败；
3. **D3 决定那 4 个阈值键的归属**（接入断言 or 从 toml 删除）——现状「看起来覆盖 5 项、实际覆盖 1 项」；
4. **确认 2.3 的身份结论**：若本意是发给同事而非自己，需要另找目标会话。

---

## 7. 复现命令

```bash
# 服务端（仓库外独立目录，arm64）
unzip -o dist/at-pc-macos-arm64.zip -d /tmp/atpc-arm64
cd /Users/clkj/atpc-gateway && nohup ./at-pc-server-macos --host 0.0.0.0 --ws-port 9801 --mcp-port 9800 \
  > server.log 2>&1 < /dev/null &

# 活链路性能探针
/usr/bin/python3 /tmp/atpc-perf/perf_probe_live.py

# 门禁（单次）
./scripts/check_perf_thresholds.sh

# mktemp 缺陷受控复现
echo stale > /tmp/perf_probe_XXXXXX.txt && ./scripts/check_perf_thresholds.sh; rm -f /tmp/perf_probe_XXXXXX.txt

# 阈值键真门禁矩阵（仓库外假树，不动仓库）
/usr/bin/python3 /tmp/gate-matrix/matrix.py
```

> 证据图位于同目录 `2026-09-18-mcp-remote-control-assets/`（`r2-*` 前缀为本轮）。
> 注意 `.gitignore` 忽略 `*.jpg/*.jpeg/*.png`，故截图**本地可见、不入库**。
> 本轮**未修改仓库任何源码**；仅新增本报告与证据图。

---

## 8. 追加：干净重启 + 重启后冒烟复验（11:08，按用户要求"直接重启一个新的"）

本节回应"你自己重启一个新的 server，然后接着把没测完的测完"。先核对**是否存在未测项**：

- 本报告全文**无待测/未验证标记**（`grep 待测|待验证|未验证|未测|⏳` 无命中）；
- 任务清单里当时挂着的 3 条 pending（确认客户端新版 / 实测远端性能优化 / 完成发诗）与已完成项
  **内容完全重复**，是被中断的那一轮留下的**陈旧条目**，并非真实缺口 ⇒ 不重跑全量测试；
- 因此本节只做**"重启本身"的验证**（重启会换掉进程，"优化是否随新进程存在"必须重新确认一次）。

### 8.1 重启动作（全程无需人工介入）

```bash
cd /Users/clkj/atpc-gateway
mv audit.jsonl audit-round2.jsonl && mv at-pc-server.log at-pc-server-round2.log && mv server.log server-round2.log
kill 77138            # SIGTERM 即生效，端口立即释放（本轮无需 kill -9）
nohup ./at-pc-server-macos --host 0.0.0.0 --ws-port 9801 --mcp-port 9800 > server.log 2>&1 < /dev/null &
```

- 新进程 **PID 80553，启动于 2026-09-18 11:08:50**；
- 运行中的二进制与 `dist/at-pc-macos-arm64.zip` 内的 `at-pc-server-macos` **SHA256 逐字节一致**
  （`e79566e2257d1a8983a26facba81f397e010f2d6a3cb117fc39130f663d7705d`）⇒ 排除"跑的不是打包产物"；
- 客户端 **4.1 s** 自动重连注册（`ws_client.rs` 5 s 轮询，与上一轮 3.5 s 同量级）。

### 8.2 重启后冒烟（3 项，~15 s）

| 项 | 实测（新进程） | 与上一轮对比 |
|:--|:--|:--|
| `capture_screen` 默认档 | `resolution 1280x800 (downscaled from 2560x1600)`，JPEG **74,481 B (72.7 KB)** | 原生 2560 档 **280,120 B (273.6 KB)** ⇒ 默认档省 **−73.4%**（上一轮 −70.0%，差异源于画面内容） |
| `list_processes` TTL | 冷 **127.1 ms** → 热 **8.3 ms** = **15.2×**（50 进程） | 上一轮 70.4 → 5.4 ms（13.0×），同量级（冷值随进程数变化） |
| `list_terminals` | p50 **1.2 ms**（min 1.0 / max 1.9，7 次） | 上一轮 1.1 ms ✅ |
| 审计落盘 | `/Users/clkj/atpc-gateway/audit.jsonl` 8 行，尾部 `action:"tool:list_processes"` | 确认 D5（审计落盘依赖启动目录）在本目录下**不再复现** |

`tools/list` 仍为 **39** 个；`list_monitors` 返回正常 ⇒ 客户端仍是新版。

### 8.3 本节结论

**所有已规划的测试均已闭合，无遗留项**。第 6 节的 4 条建议（D1 打包覆盖、D2 `mktemp`、D3 阈值键归属、
D4 身份确认）属于**待修缺陷 / 待决策**，不是"未完成的测试"，本轮按要求**未改任何源码**。
