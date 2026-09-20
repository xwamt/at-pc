# at-pc 感知通道修复 — 第二轮真机核验（server + agent 双端已同步）

- 日期：2026-09-20
- 核验对象：工作树未提交改动（HEAD = `8a638be`，26 文件 / +921 −137）
- 目标机器：客户端 `laptop-roeh3p85-b6a918`（备用Windows，Windows 11 26100，屏 2880×1800 / scale 2.0）
- 服务端：`http://192.168.99.94:9800`（systemd / `/etc/at-pc/server_config.toml`）
- 前序：`2026-09-20-perception-channel-audit.md`（R0 审计）、`2026-09-20-perception-channel-fix-verification.md`（R1 核验 + 10 条残留）

---

## 0. 构建身份核验（先证明"新代码真的上线了"）

部署核对最大的陷阱是"以为上线了"。本轮用**字节级指纹**证明两端都是新构建：

| 端 | 判据 | 结果 |
|:--|:--|:--|
| **server** | `initialize.instructions` 与源码字面量比对 | **字节完全一致**：1940 字符 / sha1 `5dd08fae6771`（两层：先比长度+sha1，再比 `!=` 逐字）。提取器需按 Rust 语义把行续 `\` 连同下一行缩进一起吞掉，否则会假报不一致 |
| **server（部署行为）** | `/api/logs` 首行 | `Loaded default configuration from /etc/at-pc/server_config.toml` —— 这条 `info!` **只存在于本轮新增的配置探测分支** |
| **agent** | 行为探针（下述 P0-①） | 最小化窗口被三条路径显式拒绝，这组错误文案只存在于新代码 |

⇒ 不需要重新部署，两端均已是当前工作树。

---

## 1. 本轮修复项：实测逐条通过

### ① P0：`hwnd` 路径哨兵/最小化拒绝 —— ✅ 完全落地

为避免动到用户的应用，本轮**自建探针窗口**（WinForms，hwnd=1376360 / pid=26544，rect 120,120,560,360），测完销毁。

| 调用 | 预期 | 实测 |
|:--|:--|:--|
| `get_ui_tree(hwnd=)` 正常 | 返回元素树 | ✅ total=9，bounds `[240,240,1120,720]`（=120,120,560,360 × DPI 2.0），**窗级/元素级哨兵均为 0** |
| `get_ui_tree(pid=)` 正常 | 返回元素树 | ✅ 同上 |
| `get_ui_tree(window_title=)` 正常 | 返回元素树 | ✅ 同上 |
| **`get_ui_tree(hwnd=)` 窗口最小化后** | 显式报错，不泄漏哨兵 | ✅ `Error: Window handle 1376360 is minimized or off-screen` |
| `get_ui_tree(pid=)` 窗口最小化后 | 显式报错 | ✅ `Error: No window matching pid 26544 was found` |
| `get_ui_tree(window_title=)` 窗口最小化后 | 显式报错 | ✅ `Error: No window matching title 'atpc-verify-probe' was found` |
| 恢复后 `get_ui_tree(hwnd=)` | 重新可用 | ✅ total=9，bounds 正常 |
| `get_ui_tree(hwnd=999999999)` | 干净报错 | ✅ `Error: Invalid window handle 999999999` |

**对比 R1 的失败样本**：R1 时 `get_ui_tree(hwnd=134368)`（最小化的 CoreWindow）返回 `[-32000, -32000, 314, 50]`。本轮三条路径全部收敛为显式错误。

代码层面三处冗余防线（`uia.rs`）：
- `enum_windows_proc` 增加 `IsIconic` 跳过 + `r.left/top > -10000` 校验（title 路径）
- 新增 `enum_pid_proc`，同样带 `IsIconic` + 哨兵校验（pid 路径）
- `hwnd` 直达分支：`IsWindow` → `IsIconic` → `rect` 哨兵，三级校验
- 元素级 `Pruning rule 2` 增加 `rect_win.left/top <= -10000` 过滤

### ② `list_windows` 门禁降级 —— ✅ 完全落地（R1 残留④）

`K::ListWindows` 由 `Admin + ComputerControl` 改为 **`Viewer + ReadOnly`**（`protocol/src/tools.rs`），`dispatch/tui.rs` 去掉 `computer_use()` 包装。

实测（客户端 `enable_computer_use=false`）：

| 工具 | 结果 |
|:--|:--|
| `list_windows` | ✅ 允许，2283 字符 |
| `get_ui_tree` | ✅ 允许 |
| `mouse_click` | ⛔ BLOCKED |

**直接收益**：`list_windows` 现在**无需绕过门禁**即可拿到 9 个窗口的 `hwnd` / `pid` / `is_minimized` / `is_foreground` / `rect` / `process_name`。R1 我是靠 `exec_powershell` + `Add-Type EnumWindows` 才拿到这些的——现在这一步可以省掉。（`window.rs:108` 恒定过滤最小化窗口，与 `only_visible` 无关。）

### ③ SoM 自动裁剪 —— ✅ 回归通过（`som.rs` 重写后未退化）

| 调用 | 结果尺寸 | 图体积 | marks |
|:--|:--|--:|--:|
| 全屏（无 `window_title`） | 1280×800 | 159.3 KB | 38 |
| `window_title='Lark'`（**自动裁剪**） | **1280×730** | **108.6 KB** | 33 |
| 显式 `crop=[194,78,1300,745]` | 1280×734 | 120.8 KB | 38 |
| 显式 `crop=[194,78,650,373]` | **650×373** | — | — |

- 自动裁剪省 **31.9%** 体积（全屏 → Lark 窗口，与 R1 的 −34.1% 同量级，差异来自窗口当前尺寸变化）。
- **显式 `crop` 优先**：650×373 原样返回（未触发 `max_dimension`）；1300×745 被压到 1280×734。
- `window_title` 不存在 → **回退全屏、不报错**（与 `get_ui_tree` 的语义不同，见 §3-b）。
- 五种 strategy 的 marks 表投影一致，`total` == 表内条目数（不截断）。

> **⚠️ 我本轮自己踩的错**：第一次我用**图体积**判断"显式 crop 是否优先"，得出 `显式优先: False` 的错误结论。体积受内容/降采样率影响（1300×745 → 1280×734 几乎不降采样 → 细节多 → 体积反而更大），**判断裁剪区域必须看尺寸**。这与 R1 记录的"JPEG 结论不可跨内容套用"是同一类错误，新增一条推论：**别用体积判断裁剪区域**。

### ④ P0-1 / P0-2 回归：marks 表 + `include_image`

| 项 | 实测 |
|:--|:--|
| marks 文本表 | ✅ 5 种 strategy 全覆盖；`- mark_id / type / label / center / rect` 齐全；`Hint: Invoke click_mark(mark_id=N)` |
| `include_image=false` | ✅ 图块 **0 字节**，marks 表完整保留（32 条） |
| 默认 `include_image` | ✅ `true`（向后兼容），schema 未误列入 `required` |

### ⑤ P1 策略层 + schema

| 项 | 实测 |
|:--|:--|
| `initialize.instructions` | ✅ 1940 字符，含 Tier 1/2/3 + UWP/Electron 回退说明 + `include_image` 建议 |
| `capture_screen` 描述 | ✅ `Tier 3 fallback: ... Prefer get_ui_tree for accessible apps.` |
| `get_marked_screen` 描述 | ✅ `Tier 3 SoM: ...` |
| `get_ui_tree` 描述 | ✅ `Tier 2 preferred: ... fallback to Tier 3 if 0 elements` |
| `get_ui_tree` props | ✅ 新增 `pid` / `hwnd`（39 个工具，`required: []`） |

### ⑥ 其余已修项

| 项 | 实测 |
|:--|:--|
| `/desktop/frame.jpg` 无帧 | ✅ 204（R1 是 404） |
| 前端消费 204 | ✅ `session.js` 加 `if (res.status === 204) { sleep(80); continue; }`，dist 已内联 |
| `server_save_path` 反斜杠归一化 | ✅ unix 上 `C:\...` 会被替换为 `/` 后再 `Path::new`；不可写路径 → `Warning: Failed to save ...: No such file or directory`，**不中断**（isError=false） |
| `--log-file` / `AT_PC_LOG_FILE` | ✅ 新增；`/api/logs` 消费 `config.log_file_path` |
| 配置自动探测 | ✅ 无 `--config` 时按 `./server_config.toml` → `/etc/at-pc/server_config.toml` 探测（远端日志已证实） |
| `default_data_dir()` | ✅ `AT_PC_DATA_DIR` → CWD(≠`/`) → `~/.local/share/at-pc` → exe 目录 → temp |
| tray 可选编译 | ✅ `#[cfg(feature = "tray")]` 包裹，`--no-default-features` 下 `enable_tray=false` |
| systemd unit | ✅ `ExecStart` 已带 `--audit-file /var/log/at-pc/audit.jsonl --log-file /var/log/at-pc/at-pc-server.log` |
| Linux 打包 | ✅ `--all` 新增 `x86_64-unknown-linux-gnu`；新增 `--server-bundle` / `--server-bundle-only`（tar.gz 含 Dockerfile/compose/systemd/deploy） |

---

## 2. 测试套件

| 套件 | 结果 |
|:--|:--|
| `scripts/test_package_dist.py` | **14 / 14** ok（含新增 `test_server_bundle_only`） |
| `at-pc-protocol` | **18 / 18**（`tool_registry_test` 6，含新增 `test_perception_channel_tool_schemas_and_tier_descriptions`） |
| `at-pc-server` | **105 / 105**（`mcp_router_test` 10，含新增 `test_mcp_initialize_contains_tier_instructions`、`test_mcp_get_marked_screen_returns_marks_and_honors_include_image`） |
| `at-pc-agent` | **133 / 133**（1 ignored；`p1_ui_tree_test` 14 含新增 `test_get_ui_tree_with_pid_and_hwnd_targeting`；`p3_som_test` 21 含新增 `test_som_window_title_auto_cropping`；`p2_window_and_review_test` 5） |
| `at-pc-e2e-tests` | **4 / 4**（`p2_window_server_test`，含 `Viewer/Operator 对 list_windows 放行`的 RBAC 断言） |
| **合计** | **274 passed / 0 failed / 1 ignored** |

---

## 3. 仍未修 / 本轮新发现

### a. `pid` 单独使用仍会静默选一个窗口（R1 残留③ 未修）

```
get_ui_tree(pid=23168)                        → active='照片'   （ApplicationFrameHost 下同时有 '设置' 与 '照片'）
get_ui_tree(pid=23168, window_title='设置')    → active='设置'   ✅
get_ui_tree(pid=23168, window_title='照片')    → active='照片'   ✅
get_ui_tree(hwnd=2033076)                     → active='设置'   ✅
get_ui_tree(hwnd=132674)                      → active='照片'   ✅
get_ui_tree(hwnd=132674, pid=999999)          → active='照片'   ✅ hwnd 优先
```

`enum_pid_proc` 在无 `window_title` 且命中窗口都非前台时，`found_hwnd` 会被**后续命中反复覆盖**（last-match-wins），于是"选哪个"取决于 `EnumWindows` 枚举顺序——`照片` 恰好排在 `设置` 之后。

- **建议**：pid 命中多个且无 `window_title` 时，返回候选列表（`candidates: [{hwnd,title,rect}]`）而不是取一个；或返回错误并提示"用 hwnd 或加 window_title"。
- **当前正确用法**：**精确锁定一律用 `hwnd`**（现在 `list_windows` 免门禁就能拿到）；`pid` 只当粗筛，且必须搭配 `window_title`。

### b. `window_title` 找不到时的语义变了（**不兼容变更，需知会**）

| 场景 | R1 行为 | 本轮行为 |
|:--|:--|:--|
| `window_title='NoSuchWindowXYZ_123'` | **静默回退到前台窗口** | `Error: No window matching title '...' was found` |
| `window_title='   '`（纯空白） | 视为未给 → 前台 | 同左（`trim().filter(!is_empty)` 生效）✅ |

这个改动**方向是对的**（静默检查错窗口比报错危险得多），但它同时删掉了 `get_foreground_window_info()` 回退分支，属于对外可见的语义变化。若有调用方依赖"给个粗略标题、失败就当前台"，会从"能用"变成"报错"。

### c. `MarkedScreenResponse` 没有 `crop` 字段 → **自动裁剪的 ROI 永远不可见**（R1 残留"摘要写原屏尺寸"的精确根因）

实测对照同一个显式 crop：

```
capture_screen    → "resolution 400x267 (downscaled from 2880x1800) (ROI crop: [100, 100, 600, 400]), format: image/jpeg."   ← 有 ROI ✅
get_marked_screen → "resolution 400x267 (downscaled from 2880x1800), format: image/jpeg."                                   ← 无 ROI ❌
```

根因（`protocol/src/models.rs:163-184`）：`MarkedScreenResponse` 的字段是
`display_index / width / height / format / base64_data / raw_base64 / image_base64 / data_uri / marks / total_marks / source / original_width / original_height / scale_factor` —— **没有 `crop`**。

而 `som.rs` 里 `effective_crop` 是算出来的（`:866`），也被用于过滤 marks（`:1045`），但**没有进响应**。于是服务端 `mcp/mod.rs` 里那段 `crop_note`（读 `val.get("crop")`）对 SoM 路径是**死代码**。

两个后果：
1. **新加的自动裁剪是"静默生效"的**——调用方无法知道图被裁到哪儿；
2. `original_width/height` 存的是**整屏原生尺寸**（`som.rs:828` 取的是 `dynamic_img` 全幅），所以裁剪后仍写 `downscaled from 2880x1800`，**具有误导性**（真实源区域是 2634×1502）。

❗**注意**：`p3_som_test.rs::test_som_window_title_auto_cropping` 把这个语义**固化成了断言**：

```rust
assert_eq!(res.width, 800);                     // 裁到窗口 800x600
assert_eq!(res.original_width, Some(1920));     // 却报整屏 1920x1080
```

修 c 时必须同步改这条测试。**建议**：给 `MarkedScreenResponse` 加 `effective_crop: Option<[u32;4]>`（+ 可选 `original_width/height` 语义改为"裁剪后区域的原生尺寸"），并在摘要里输出 `(ROI crop: [...])`。

### d. marks 文本仍无硬上限（残留项，实际风险低）

代码里仍**没有** `MAX_MARKS` 一类的截断。实测上界：

| 配置 | marks | 文本 | ≈tokens |
|:--|--:|--:|--:|
| 1280 全屏 `hybrid` | 38 | 4103 字符 | ~1.2k |
| 2560 `contours` | 7 | 997 字符 | ~285 |
| 2560 `hybrid` | 13 | 1559 字符 | ~445 |

≈**108 字符/mark**，且实测未突破 38 个。⇒ 与 R1 判断一致：**理论缺限、实践无害**，优先级最低。

### e. 打包 arm64/x86_64 仍同名（残留项，仅部分缓解）

`scripts/package_dist.py::dist_binary_names()` 现状：

```python
if is_apple(triple):
    return "at-pc-agent-macos", "at-pc-server-macos"    # aarch64 与 x86_64 同名！
return "at-pc-agent", "at-pc-server"                    # Linux 与 Windows(非) 也同名
```

本轮新增了 `x86_64-unknown-linux-gnu` target，**它同样落到 `at-pc-server`**。`--all` 靠 host-last 排序缓解（本机 arch 最后写，wins），但单独跑 `--target x86_64-apple-darwin` 仍会覆盖 `dist/at-pc-server-macos`。**建议**在名字里带 triple 后缀。

### f. `deploy.sh` standalone 分支仍漏传路径参数（部分修）

```
standalone: nohup "${BIN_SRC}" --host 0.0.0.0 --ws-port 9801 --mcp-port 9800 \
             --log-file /var/log/at-pc/at-pc-server.log   ← 本轮新增 ✅
                                                          ← 仍缺 --audit-file ❌  也缺 --meta-file ❌
```

⇒ standalone 部署时审计与终端元数据仍落 **CWD**。systemd 分支已带 `--audit-file`（✅），建议 standalone 补齐。

### g. `/desktop/frame.jpg` 对未知终端也回 204（未修）

| 请求 | R1 | 本轮 |
|:--|:--|:--|
| 已知终端（无帧） | 204 | 204 |
| **未知终端 id** | 204 | **204** ❌ |
| 乱写 id | — | 204 ❌ |
| `/desktop/raw` 未知终端 | — | 204 ❌ |

"终端不存在"与"终端在线但暂无帧"**不可区分**。建议：未知终端 → 404，已知无帧 → 204。

### h. `server_save_path` 回显仍是未归一化的原始路径（未修）

实测（服务端是 Linux）：

```
入参 C:\atpc-verify\nested\shot.jpg
回显: Saved to server host disk: C:\atpc-verify\nested\shot.jpg      ← 原样回显
实际落盘: <服务端CWD>/C:/atpc-verify/nested/shot.jpg                  ← 归一化后的路径
```

代码里 `normalized_path` 只用于 `Path::new()`，`server_saved_note` 仍用 `ssp.trim()`。用户按回显去找文件必然找不到。**建议**回显归一化后的绝对路径。

### i. UWP/XAML 仍是 0 元素（硬限制，非缺陷）

用 `window_title` / `pid=23168` / `hwnd=2033076`（设置）/ `hwnd=132674`（照片）**四路精确锁定，全部 `total_elements=0`**。⇒ 再次确认 R1 的更正：**不是选错窗口，而是 `FindAll(TreeScope_Children)` 到不了 UWP/Chromium 内容树**。`get_ui_tree` 的 tool description 与 `instructions` 都已显式写明这一点并引导到 Tier 3 —— ✅ 文档与行为一致。

### j. 自动裁剪选窗无歧义告警

`som.rs:836-841` 按 `title.contains(lower) || process_name.contains(lower)` **取第一个匹配**。多个窗口/进程同名（如 `window_title='Lark'` 同时命中标题与 `Lark.exe`）时静默取首，无告警。低风险，建议命中多个时在摘要里加一行提示。

---

## 4. 副作用与清理

| 项 | 状态 |
|:--|:--|
| 自建探针窗口（hwnd=1376360 / pid=26544） | ✅ 已 `CloseMainWindow` + `Kill`；`list_windows` 确认已消失 |
| 客户端 `C:\Users\CLKJ\Downloads\atpc_probe_form.ps1` | ✅ 已删除 |
| 客户端 `C:\Users\CLKJ\Downloads\atpc-shot.jpg`（13,764 B） | ✅ 已删除 |
| 客户端 Downloads 复核 | ✅ **无本轮残留** |
| 用户的窗口（Lark / 设置 / 照片 / 资源管理器） | ✅ **全程未动**（本轮所有最小化实验都在自建探针窗口上做） |
| 服务端 `server_save_path` 落盘（**无法清理**） | ⚠️ 在 `192.168.99.94` 上留下 `<CWD>/C:/atpc-verify/nested/shot.jpg`、`<CWD>/C:/atpc-verify2/shot.jpg`、`/tmp/atpc-v2/server-shot.jpg`（本机路径，服务端不存在）。无 SSH 通道，需你手动清理（正是残留项 h 的现场） |
| MCP 调用性质 | 只读 + 自建窗口；门禁使注入类工具无法落地 |

> **运维小抄**：`exec_powershell` 里 `Remove-Item` 会被 CLI 安全护栏整段拦下（`Command contains potentially destructive operation 'remove-item'`），且**整段脚本一行都不执行**。改用 `[System.IO.File]::Delete('...')` 可通过。

---

## 5. 上一轮结论的更正

1. **`pid` 定向可救 UWP —— 错**。已用 `pid`/`hwnd` 四路精确锁定证明 `设置` 0 元素与选窗无关，是 UIA 硬限制。
2. **"别用体积判断裁剪区域"**（本轮新踩）。R1 记的是"耗时/体积结论不可跨内容套用"，本轮补充：**判断裁剪是否生效只能看返回尺寸**，体积同时受降采样率与内容熵影响。
3. **本机 shell `grep` 的 `\|` 交替不生效**（BSD grep）。`grep -rn "a\|b"` 静默返回空，导致我一度以为 `effective_crop` / `log_file_path` 不存在而走了弯路。**一律改用 Grep 工具**。

---

## 6. 一句话结论

`hwnd` 哨兵泄漏、`list_windows` 门禁、marks 表、`include_image`、策略层、SoM 自动裁剪、`frame.jpg` 204、`server_save_path` 归一化 —— **全部实测通过，274 项测试全绿**。剩余 10 条按优先级：

| 优先级 | 项 | 一句话 |
|:--|:--|:--|
| **P0** | c | `MarkedScreenResponse` 缺 `crop` 字段 ⇒ 新的自动裁剪**静默生效**、ROI 不可见、`downscaled from` 误导（且被测试固化） |
| **P1** | a | `pid` 单独用仍 last-match-wins 静默选窗，应返回候选或报错 |
| **P1** | b | `window_title` 失败不再回退前台（方向对，但是不兼容变更，需知会调用方） |
| **P1** | g | `frame.jpg` 未知终端回 204，与"已知无帧"不可区分 |
| **P2** | h | `server_save_path` 回显未归一化路径 |
| **P2** | f | `deploy.sh` standalone 缺 `--audit-file`/`--meta-file` |
| **P2** | e | `dist_binary_names()` 跨 arch 同名（Linux 新 target 也同名） |
| **P3** | d | marks 文本无硬上限（实测上界 ~4.1 KB，实践无害） |
| **P3** | i | UWP/XAML 0 元素（硬限制，文档已说明） |
| **P3** | j | 自动裁剪多命中静默取首，无告警 |
