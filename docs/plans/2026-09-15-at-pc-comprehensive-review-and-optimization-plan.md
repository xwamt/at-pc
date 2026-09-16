# at-pc 全量代码审查与优化建议报告

- **审查对象**：`/Users/clkj/项目/at/at-pc`（crates: `at-pc-protocol` / `at-pc-agent` / `at-pc-server`）
- **审查日期**：2026-09-15
- **审查方式**：全量源码通读 + 工具链实测（`cargo clippy` / `cargo test` / `cargo fmt` / `cargo tree`）+ 运行时黑盒验证（启动 release 二进制后发起真实 HTTP 请求）
- **结论可复现性**：本报告所有"已复现"项均在附录 A 给出可直接执行的复现命令

---

## 实施进度（2026-09-15 更新）

> 本节是当前实施状态的唯一准确信息源；下文“问题/现状”保留的是 2026-09-15 初始审查结论，不代表当前代码仍未修改。
>
> **验收口径**：只有实现完成、针对性测试通过、相关 workspace 检查通过且复检无阻断问题，才标记为 `✅ 已验收`。仅完成编码但尚未跑完合并后门禁的节点一律标记为 `🚧 待验收`。

### 总体状态

- **已验收：18 / 18**：P0-1、P0-2、P1-1、P1-2、P1-3、P1-4、P1-5、P1-6、P1-7、P1-8、P2-1、P2-2、P2-3、P2-4、P2-5、P2-6、P2-7、P2-8。
- **实现中 / 待最终验收：0 / 18**。独立终审 ✅ Ready to merge。未 commit。
- **未开始：0 / 18**。
- **当前工作树**：18 个节点与批次 4 合并门禁均已通过，仍为未提交的大规模 dirty tree；提交时必须带上 `package-lock.json`、`frontend/dist/index.html`、`crates/e2e-tests/**`，且不得加入 `target-p*`。
- **插件核心规范**：截至本次更新，改动只涉及 at-pc 自身协议、工具注册表和运行时，没有修改 AT-Core Plugin Core 对外接口，因此 `at-core/docs/plugin-specification.md` 暂不需要同步；后续若接入 Plugin Core，必须在同一节点同步规范与宿主实现。

### 里程碑状态

| 批次 | 状态 | 已完成 / 当前工作 | 进入下一批次前的硬门禁 |
| :--- | :--- | :--- | :--- |
| 批次 1：止血 | ✅ **2/2 已验收** | P0-1 路径/鉴权；P0-2 UTF-8 安全截断 | P0 安全回归、UTF-8 边界、全 workspace 测试已通过 |
| 批次 2：架构债 | ✅ **4/4 已验收** | P1-8、P1-5、P1-4、P2-1；P2-6 随 P1-5 合并完成 | 标准 WS/DFRM、registry 并发、GUI/headless、CI/toolchain 门禁已验证 |
| 批次 3：架构与 I/O | ✅ **3/3 已验收** | P1-7、P1-1、P1-2 | workspace fmt/clippy `-D warnings`/tests/release/bench no-run/P0 安全回归已通过 |
| 批次 4：规模化与体验 | ✅ **8/8 已验收** | P1-3、P1-6、P2-2、P2-3、P2-4、P2-5、P2-7、P2-8 | workspace 合并门禁已通过；独立终审 ✅。未 commit |

### 18 个节点逐项跟踪

| 节点 | 状态 | 已落地内容 / 证据 | 尚欠事项 |
| :--- | :--- | :--- | :--- |
| **P0-1** | ✅ 已验收 | `/static/*` 纳入鉴权；拒绝 traversal/绝对路径/Windows 分隔符；canonical root 防 symlink 逃逸；release 仅 embed；新增 `static_asset_security_test.rs`。debug/release 定向测试、既有鉴权测试与 workspace 测试通过 | Windows junction/reparse-point 仍需 Windows CI/真机补充验证；本地可写资源目录的 canonicalize/read TOCTOU 属低风险残余 |
| **P0-2** | ✅ 已验收 | Agent 参数摘要与 Server token 前缀按 Unicode scalar 截断；ASCII 80/81、中文、é、emoji、组合字符测试通过，无 U+FFFD、无完整 token 泄漏 | 无阻断项；若未来不需要 token 前缀关联，可改不可逆指纹进一步减小披露 |
| **P1-1** | ✅ 已验收 | 共享 ToolSpec 注册表；Agent dispatch 按域拆分；`handles()` 锁定 33 kind；kind 兜底 `unreachable!`。补回 `list_monitors` / `display_index` 合同文案。规格+质量复检 ✅。合并门禁：fmt、Clippy `-D warnings`、workspace tests **0 failed / 1 ignored**、release、bench no-run、P0 static 安全测试通过 | Minor：`dispatch_kind` 与 `domain_handle_fns` 仍是两份并列名单 |
| **P1-2** | ✅ 已验收 | AuditLogger：BufWriter、启动 open、deadline 周期 flush、背压、已入队不静默丢、poison 后新 try_log 失败、barrier 才 fsync。MetaStore 500ms 合写。Dashboard 固定尾读。规格+质量复检 ✅。同上 workspace 合并门禁通过 | Minor：ShuttingDown 锁范围、Drop 同步 shutdown、audit.rs 体积。P2-5 轮转/导出不属于本节点 |
| **P1-3** | ✅ 已验收 | 64×64 块哈希 + 0.5% 脏块比；连续 `stream_output_size`（1280×scale，无 >1920 悬崖）；`scale` 经 dashboard→router→ws_client→`fast_rgba_to_rgb_scaled`；静止 5s 空 JPEG 保活。提交点 `what_to_send`/`apply_send_outcome`：EncodeError/首帧 Full/过期 generation 不提前提交。硬件编码仅评估、无新 crate。规格+质量复检 ✅。desktop-core **14**、stream pool **10**、p2 stream **5**；clippy lib `-D warnings` 通过 | Minor：`jpeg_bytes.clone()`；EncodeError 不按帧间隔休眠；`quality.min(85)`。真机 Windows/macOS 吞吐与硬件编码仍待后续 |
| **P1-4** | ✅ 已验收 | `list_terminals`/`get_terminal`/`unregister` 在 meta await 前释放 sessions 锁；单 metadata 快照保持在线优先、离线补齐、无重复；并发 gate 测试证明 register/heartbeat 写侧可推进 | 两阶段快照允许短暂陈旧读，已明确为接受的语义；无阻断项 |
| **P1-5** | ✅ 已验收 | rust-embed 移除 axum feature；Agent GUI 改可选且默认保持 GUI，no-default 自动 headless；Server 测试依赖关闭 Agent 默认 feature；headless/GUI、本机与 Windows GNU check 通过 | Windows MSVC 真机运行、PE subsystem、console attach 与 DPI context 仍需平台验收 |
| **P1-6** | ✅ 已验收 | PART A：signal + HTTP/HTTPS 10s drain、sweep/WS join、persistence flush；生产 exec_cmd 走 async。Item 2：推流 `tokio::spawn` + `tokio::time::sleep`；每帧短 `spawn_blocking` 做 capture+hash+convert+encode。规格+质量复检 ✅。关停测试 11、命令测试 8、`stream_blocking_pool_test` 4 passed | Minor：find_monitor 仍在 async；stdio EOF 硬 abort HTTP；persistence 失败仍可能 exit 0 |
| **P1-7** | ✅ 已验收 | 新增 `at-pc-desktop-core` 纯逻辑与 5 项测试；新增精确 pin Criterion benchmark、`PERFORMANCE.md`、阈值文件与保存 baseline；新增 auth/percent/RBAC/call-id 5 项测试；bench no-run、实际 baseline、定向 Clippy、fmt/diff 通过 | 物理屏幕捕获与真实端到端编码吞吐需在 P1-3 的 Windows/macOS 真机验收中完成 |
| **P1-8** | ✅ 已验收 | 删除自研 `ws/codec.rs`、手写握手、死 ToolPermission、legacy Base64 stream 与 JSON DesktopFrame；测试迁到 tokio-tungstenite，DFRM binary→router cache 链路通过；仓内残余引用为 0 | 删除 public Rust/JSON variant 对外部自建 client 可能是 breaking change，发布时需说明最低兼容版本 |
| **P2-1** | ✅ 已验收 | 固定 Rust 1.96.0；新增 rustfmt、workspace lint、GitHub Actions、cargo-deny/audit；执行全仓 fmt；批次 3 合并后 workspace tests **0 failed / 1 ignored**，Clippy `-D warnings`、release build、bench no-run 通过 | 无阻断项 |
| **P2-2** | ✅ 已验收 | host/cross 用 `CARGO_TARGET_DIR=target/mac`、`target/linux`、`target/win`（不写死 `[build] target-dir`）；CI `quality`/`platform-check` 用 `Swatinem/rust-cache@v2`；约定检查按 YAML/TOML 结构解析并接入 `quality` job。规格+质量复检 ✅。检查器 OK；单测 **7 passed**。遗留 mixed `target/` 约 42G，未 `cargo clean` | Minor：README 常用命令仍写默认 `target/`；rust-cache `@v2` 浮动 tag；3.9 fallback 对任意 `[env]`/`[build]` 偏严。测试合并已由 P2-8 完成 |
| **P2-3** | ✅ 已验收 | Vite + 原生 ES 模块：`api/`、`desktop/`、`terminals/`；产物内联 `frontend/dist/index.html`，`rust-embed` 只打 dist；`AT_PC_FRONTEND_DIR` 仅 debug。去掉四份模块 `@ts-nocheck`；卡片/进程 `escapeHtml` + onclick `escapeJsString`；CI `frontend` job 与 `platform-check` 会 `npm ci`/`build`。规格+质量复检 ✅。npm **20** 项测试 + lint/typecheck/build；P0-1 安全测试通过 | Minor：弹窗二次 `onclick` 未再 JS 转义；`session.js`/`terminals/ui.js` 仍大；热加载需 `vite build -w`。提交时勿漏 `package-lock.json` 与 `dist/index.html` |
| **P2-4** | ✅ 已验收 | 私有 crate `at-pc-build-support`；agent/server `build.rs` 只调 `embed_windows_resources`。MSVC 走 `rc.exe`（PATH/`$RC`/Windows Kits/vswhere），失败 panic；`.rc` 剥 `\\?\`；缺 icon/指定 manifest 在 Windows 目标 panic。规格+质量复检 ✅。`at-pc-build-support` **14 passed** | Minor：路径须落在仓库内、编排层测试、`.rc` 引号转义、`rustc-link-arg-bins`。真机 MSVC 图标/manifest 仍待 Windows 验证 |
| **P2-5** | ✅ 已验收 | 单 writer 上按大小轮转（默认 100MB、保留 5 含当前）；`read_recent` 尾读，cap 截断不跨文件补洞；导出/尾读为 WriterCommand（先钉 current fd、inode 去重）；磁盘满 poison；`GET /api/audit/export` Admin；Unix 0600。规格+质量复检 ✅。`audit::` **20 passed** | Minor：periodic_flush 遇 StorageFull 只打日志；导出整包进内存；audit.rs 体积；retry_io 包非幂等 rotate |
| **P2-6** | ✅ 已验收 | workspace 与 Agent 的未使用直接 `rand` 已删除；仅允许依赖树中的传递 rand | 无阻断项 |
| **P2-7** | ✅ 已验收 | `package_dist.py` 先 `cargo build --release`（`--target` 可选）；版本以 `cargo metadata` 为准（agent/server 必须同版本，`CARGO_PKG_VERSION` 若设置必须一致）；打包用 metadata `target_directory`；`dist/VERSION` 写入 zip 与 `SHA256SUMS`；签名失败 `SystemExit(1)` 且诊断到 stderr；去掉 `at-pc.exe`/`at-pc-macos` 别名。规格+质量复检 ✅。`scripts/test_package_dist.py` **12 passed** | Minor：多 `--target` 同名覆盖、`dist/` 陈旧 extras、`_returncode(None)==0`、host zip 无额外断言 |
| **P2-8** | ✅ 已验收 | 新建 `crates/e2e-tests`（`at-pc-e2e-tests`）；10 个双 crate 集成测试迁入；agent/server 不再互指。`check_no_agent_server_dev_cycle.py` 解析 Cargo.toml，CI `quality` 跑检查器 + 4 条夹具。规格+质量复检 ✅。检查器 OK；夹具 **4 passed**；`cargo metadata` 无 agent↔server 边 | Minor：检查器认 TOML 键名不解析 `package`/`path` 别名；Python 3.9 fallback 漏引号键与 dotted key；空 `[lib]` 可关 test/bench harness |

### 当前验证证据与阻塞

1. **批次 3 合并门禁（2026-09-15 独立复验）**：`cargo fmt --all -- --check` 退出 0；`cargo clippy --workspace --all-targets --locked -- -D warnings` 退出 0；`cargo test --workspace --locked --no-fail-fast` **0 failed / 1 ignored**（ignored 为需物理显示器/录屏权限的 `test_capture_screen_downsampling_and_crop`）；`cargo build --release --locked` 退出 0；`cargo bench --no-run --locked` 退出 0；`static_asset_security_test` 1 passed。
2. **P1-1**：规格+质量复检 ✅；合并门禁通过后标验收。Minor：域列表仍两份。
3. **P1-2**：规格+质量复检 ✅；`audit::` 12 passed；合并门禁通过后标验收。Minor：ShuttingDown 锁范围、Drop、文件体积。
4. **P1-1/P1-2 已验收**。当前工作树仍 dirty、未 commit，不得当发布物。
12. **P2-4**：规格+质量复检 ✅（Ready to merge Yes）。`at-pc-build-support` 14 passed。真机 MSVC 仍待验证。
13. **P2-8**：规格+质量复检 ✅（Ready to merge Yes）。agent↔server 环已解开；10 个测试在 `at-pc-e2e-tests`。检查器别名/`package =` 与 3.9 fallback 覆盖为非阻断残余。
14. **P1-3**：规格+质量复检 ✅（Ready to merge Yes）。分块脏检、连续 scale、5s 空保活、提交点修复已落地。
15. **批次 4 合并门禁（2026-09-16）**：`cargo fmt --all -- --check` 退出 0；`cargo clippy --workspace --all-targets --locked --offline -- -D warnings` 退出 0（顺手修了 audit 测试 type_complexity / is_multiple_of / redundant_closure，以及 shutdown/command `single_match`）；`cargo test --workspace --locked --offline --no-fail-fast` **0 failed / 1 ignored**（ignored 仍为 `test_capture_screen_downsampling_and_crop`）；`cargo build --release --locked --offline` 退出 0；`cargo bench --no-run --locked --offline` 退出 0；`static_asset_security_test` 1 passed。
16. **独立终审（2026-09-16）**：Ready to merge Yes。Python 检查器与前端 20 项测试/lint/typecheck 复验通过。提交约束：必带 `package-lock.json` 与 `dist/index.html`；禁止 `target-p*`。GitHub Actions 目前在 `at-pc/.github/`，若合入父仓库 `at` 根则不会触发。

### 下一步（严格顺序）

1. 等明确要求后再 commit / 开 PR。提交时按路径添加源码与前端产物，排除所有 `target-p*`。
2. 若目标远程是父仓库 `at` 而不是独立 `at-pc` 仓库，需要把 workflow 接到仓库根，或拆成独立仓库。

---

## 0. 结论摘要（TL;DR）

1. **代码基线是健康的**：`cargo test --workspace` 182 个用例全绿、`cargo clippy` 仅 6 条 warning、上一版优化计划（`2026-09-05`）中的 M1/M2 主体项已真实落地。这不是一个"需要抢救"的项目，而是一个**已过功能验证期、尚未进入工程化收敛期**的项目。
2. **存在 1 个已实测复现的 P0 安全漏洞**：Web 控制台 `/static/*` 路由存在路径穿越，**在已配置 `--auth-token` 的情况下**仍可无凭证读取进程可达的任意文件（实测成功读取了 `at-pc-server.log`）。
3. **存在 1 个可稳定触发的 P0 崩溃点**：Agent 审计日志对参数做**字节切片**截断（`&args_str[..80]`），任何含中文/非 ASCII 且长度超阈值的工具调用都会 panic。
4. **最大的架构负债是"工具契约五处重复"**：新增一个 MCP 工具需要在 **5 个文件**中同步修改（schema ×2、RBAC、权限分级、转发白名单），当前 33 个工具已在各处出现 33/33、32/33、33/33、33/33 次命中。这是后续所有功能迭代的复利成本来源。
5. **最大的性能瓶颈在推流链路**：每帧执行"全帧 RGBA→RGB 转换 + 全帧 JPEG 编码"，去重仅靠整帧哈希相等，任何微小变化（时钟、光标闪烁）都会触发全帧重编码；`scale` 协议字段在全链路被忽略。这是纯粹的工程优化空间，不需要改协议。

---

## 1. 量化基线

| 维度 | 指标 | 实测值 |
| :--- | :--- | :--- |
| 代码规模 | 生产代码 | 17,818 行 / 45 文件（protocol 344、agent 11,815、server 5,659） |
| | 测试代码 | 9,138 行（测试/生产 ≈ 0.51） |
| | 合计 | 27,055 行 |
| 测试 | 用例数 | 182 个（`#[test]` / `#[tokio::test]`） |
| | 执行结果 | **全绿**，0 failed，耗时 75s |
| | 单元测试分布 | **仅 9 个源文件含 `#[cfg(test)]`**，且全部是叶子模块（`config`/`som`/`file_ops`/`screen`/`directory`/`network`/`command`/`process_registry`/`meta_store`） |
| 静态检查 | clippy | 6 条 warning（4 条为 `await_holding_lock`，均在测试内），0 error |
| | rustfmt | **571 处格式差异**，代码未过 rustfmt |
| 依赖 | Cargo.lock 包数 | 518 |
| | 多版本共存包 | **46 个**（含 `axum` 0.7+0.8、`image` 0.24+0.25、`rand` 0.8+0.10、`getrandom` ×3） |
| | 未使用直接依赖 | `rand`（agent 声明，全仓库 0 处引用） |
| 构建产物 | release 二进制 | agent 8.4 MB、server 5.3 MB（`lto="thin"` + `codegen-units=1` + `strip`） |
| | `target/` 体积 | **27 GB**（debug 20 G、windows-gnu 3.5 G、release 3.2 G、windows-msvc 855 M） |
| 工程规范 | CI / toolchain / lint 配置 | **全部缺失**（无 `.github/workflows`、无 `rust-toolchain.toml`、无 `rustfmt.toml`、无 `[workspace.lints]`） |

---

## 2. 已达标项（避免重复投入）

上一版 `docs/plans/2026-09-05-at-pc-optimization-milestone-plan.md` 的多数条目**已实现**，建议将该文档标记为"已归档"，避免后续重复排期：

| 原计划项 | 落地情况 | 证据 |
| :--- | :--- | :--- |
| M1-P0-1 REST API 鉴权中间件 | ✅ 已落地 | `mcp/mod.rs:112 auth_middleware`；实测 `/api/terminals`、`/api/audit` 无 token 返回 **401** |
| M1-P0-2 严格 CORS + 监听地址白名单 | ✅ 已落地 | `config.rs:97 listen_host` 默认 `127.0.0.1`；`mcp/mod.rs:37` 按 `allowed_origins` 分支 |
| M1-P0-3 `read_text_file` 逆向流式 Tail | ✅ 已落地 | `file_ops.rs:109` 64 KB 分块反向读取，含单 chunk 快路径 |
| M1-P0-4 断连即时失败 | ✅ 已落地 | `router.rs:410 abort_pending_calls_for_terminal`，在 `ws/handler.rs:367` 挂接 |
| M1-P0-5 MCP Stdio 异步化 | ✅ 已落地 | `mcp/mod.rs:791` mpsc 汇聚 + `tokio::spawn` 逐请求派发 |
| M2-P1-1 迁移标准 WebSocket 栈 | ⚠️ 部分 | 生产链路已全部改用 `tokio-tungstenite`；**自研 codec 未删除**（见 P1-8） |
| M2-P1-2 细粒度任务取消 | ✅ 已落地 | `executor.rs:150 cancel` + `process_registry.rs:96 kill_call_processes` |
| M2-P1-3 二进制推流 + 静态帧去重 | ✅ 已落地 | `stream.rs:185 BinaryDesktopFrame` + `stream.rs:56 compute_sample_hash` |
| M2-P1-4 Agent 无头模式 | ✅ 已落地 | `lib.rs:23 run_headless` + `main.rs:128 --headless/--service` |
| M2-P1-5 前端解耦（rust-embed） | ✅ 已落地 | `dashboard.rs:22 #[derive(Embed)]` + `frontend/index.html` |
| M2-P1-6 进程采样缓存 | ✅ 已落地 | `process.rs:22 SYSTEM_CACHE` 长驻单例 |
| M3-P2-1 Computer-Use 工具 | ✅ 已落地 | 20 个键鼠/UI 工具，`--enable-computer-use` 门控 |
| M3-P2-3 WSS / mTLS | ✅ 已落地 | `tls.rs`（server） / `tls.rs`（agent），ring provider |
| M3-P2-4 RBAC + 审计 | ✅ 已落地 | `config.rs:44 is_tool_allowed_for_role` + `audit.rs` JSONL |

---

## 3. 问题清单与优化建议

### P0 — 立即修复（安全 / 崩溃）

#### 【P0-1】Web 控制台静态资源路径穿越 → 未授权任意文件读取 · *维度：安全*

- **位置**：`crates/server/src/mcp/dashboard.rs:94-101`
- **问题**：`static_asset_handler` 直接把 URL 路径拼接到文件系统路径（`Path::new(dir).join(&path)`），未做 `..` 规范化，也未挂在 `auth_middleware` 之下（`create_dashboard_router` 第 65-68 行的中间件只作用于 `nest("/api", api_routes)`，`/static/*path` 与 `/` 在中间件之外）。
- **实测证据**（已复现，服务端带 `--auth-token SECRET123` 启动）：

  ```
  GET /static/../Cargo.toml                          -> HTTP 200，返回 crates/server/Cargo.toml 内容
  GET /static/../../../at-pc-server.log              -> HTTP 200，返回服务端运行日志
  GET /api/terminals            (无 token)           -> HTTP 401  ← 鉴权本身是好的
  ```

- **影响**：局域网内任何未认证主体可读取进程权限范围内的任意文件；服务端运行日志中含终端 hostname、内网 IP、工具调用轨迹，直接构成横向渗透的信息基座。
- **优化建议**：
  1. 立即修复：对 `path` 做 `std::path::Component` 归一化，拒绝含 `ParentDir`/`RootDir`/`Prefix` 的请求；
  2. 兜底防护：拼接后 `canonicalize()` 并断言 `starts_with(frontend_root)`；
  3. 将 `/static/*` 一并纳入 `auth_middleware`（生产期静态资源无鉴权暴露没有正当性）；
  4. 移除"从磁盘热加载前端"的 `crates/server/frontend/...` 与 `AT_PC_FRONTEND_DIR` 两条磁盘分支在生产构建中的启用（改为 `#[cfg(debug_assertions)]` 门控）。
- **预期收益**：消除唯一已确认的未授权数据外泄通道；同时把"开发期便利代码"与"生产期攻击面"解耦。工作量约 0.5 人日。

#### 【P0-2】非 ASCII 参数截断导致 panic · *维度：代码质量*

- **位置**：`crates/agent/src/app/mod.rs:249-251`
- **问题**：

  ```rust
  let summary = if args_str.len() > 80 { format!("{}...", &args_str[..80]) } else { args_str };
  ```

  `len()` 是字节长度，`[..80]` 是字节切片。当第 80 字节落在 UTF-8 多字节字符中间时 **panic（`byte index is not a char boundary`）**。
- **影响**：本项目工具参数高频包含中文路径、中文脚本、中文文件名，触发概率高。该代码位于 `AgentEventListener::on_tool_start`，**在每一次工具调用的请求处理路径上**，一旦 panic 将杀死承载它的任务/线程，直接表现为"Agent 执行工具后失联"。
- **同类次级风险**：`crates/server/src/audit.rs:59` `&clean[..3.min(clean.len())]`（非 ASCII token 前缀截断，同样越界风险）。
- **优化建议**：统一改用字符安全截断，建议抽为公共工具函数：

  ```rust
  fn truncate_chars(s: &str, max: usize) -> String {
      if s.chars().count() <= max { s.to_string() }
      else { s.chars().take(max).collect::<String>() + "..." }
  }
  ```
- **预期收益**：消除一类"偶发、难复现、现场无堆栈"的进程级故障。工作量约 0.2 人日。

---

### P1 — 架构与性能主线（建议 2 周内启动）

#### 【P1-1】MCP 工具契约在 5 处重复声明 · *维度：架构设计*

- **位置与命中统计**（对 agent dispatch 的 33 个工具名做全仓库字面量命中）：

  | 文件 | 需同步修改的内容 | 命中率 |
  | :--- | :--- | :--- |
  | `crates/agent/src/tools/mod.rs:24` | `get_mcp_tool_definitions()` JSON Schema | 33/33 |
  | `crates/agent/src/tools/mod.rs:811` | `dispatch_tool_with_call_id_and_options` 分发分支 | 33/33 |
  | `crates/server/src/mcp/tools.rs:4` | 服务端**近乎逐字复制**的 Schema | 33/33 |
  | `crates/server/src/router.rs:32` | `get_tool_permission` 权限分级（**已死代码，仅测试引用**） | 33/33 |
  | `crates/server/src/router.rs:716` | `execute_dispatch_inner` 转发白名单 match | 33/33 |
  | `crates/server/src/config.rs:44` | `is_tool_allowed_for_role` RBAC 三层白名单 | 32/33 |

- **影响**：新增/改名一个工具是"改 5 个文件、跨 3 个 crate"的操作，且**漏改任何一处都不会编译失败**——漏改 `router.rs` 转发白名单会导致工具静默返回 `Unknown or unsupported tool`；漏改 `config.rs` 会导致 RBAC 越权或误拒。同时 `dispatch_tool_with_call_id_and_options` 单函数 **538 行 / 33 分支**，最大分支 59 行；`handle_jsonrpc_request_with_context` 约 **310 行**。
- **优化建议**（分三步，可增量落地）：
  1. **单一事实源**：在 `at-pc-protocol` 中定义 `ToolSpec { name, description, schema, required_role, forward: bool }` 的静态表（`&'static [ToolSpec]`），三端全部从该表派生——server 的 `tools/list`、agent 的 schema、RBAC 判定、转发白名单均由表驱动；
  2. **消除 agent/server Schema 双份**：把 `mcp/tools.rs` 的 40 条 Schema 中与 agent 重叠的部分直接引用共享表，服务端只保留 7 个"终端管理类"元工具；
  3. **细分发**：把 33 个分支按域拆成 `tools/{system,process,file,screen,uia,input,tui}.rs` 各自 `pub fn dispatch(name, args) -> Option<Result<Value,String>>`，主 match 只做路由。
- **预期收益**：新增工具的改动面从 5 文件降至 1 处（表里加一行）；消除"漏改静默失效"这一类最难排查的缺陷；单文件复杂度从 1348 行降至各 200 行量级，`tools/mod.rs` 的 code review 成本显著下降。工作量约 2–3 人日（纯重构，有 182 个测试兜底）。

#### 【P1-2】阻塞 I/O 混入 async 运行时 · *维度：性能 / 架构*

- **量化**：生产代码中 `std::fs::*` 出现 **54 处**，`tokio::fs::*` **0 处**。其中位于 `async fn` 请求路径上的关键点：

  | 位置 | 行为 | 频次 |
  | :--- | :--- | :--- |
  | `meta_store.rs:87 save_to_disk` | 在持有 `records.write().await` 写锁期间，**全量序列化整张 HashMap 并同步写盘 + rename** | 每次终端注册 / 每次 meta 修改 |
  | `audit.rs:64 log` | 每条审计记录 **open + write + close**，无文件句柄复用、无缓冲 | 每次工具调用 1–2 次 |
  | `dashboard.rs:30, 80, 94` | 每次请求 `read_to_string` 整个前端 / 日志文件 / 静态资源 | 控制台轮询 |
  | `main.rs:47`、`agent/main.rs:23` | 日志 writer 持 `std::sync::Mutex<File>` 同步写 | 每条日志 |

- **影响**：
  - `meta_store` 是**写放大 + 阻塞**的组合：终端重连（默认 5s 间隔的重连风暴场景）会触发 N 次全量落盘，且每次都阻塞一个 tokio worker 线程；
  - `audit.rs` 每条记录 3 次 syscall，高并发工具调用时形成串行化热点；
  - 这些是"低负载看不出来、压力上来后表现为整体 RT 抖动"的典型问题。
- **优化建议**：
  1. `AuditLogger` 改为 **channel + 单一后台 writer 任务**：进程启动时 open 一次文件，业务侧只做 `send()`（非阻塞），后台任务批量 `writeln!` + 定期 `flush`；
  2. `TerminalMetaStore::save_to_disk` 移出写锁（先在锁内 `clone` 快照，锁外落盘），并加**写合并/去抖**（例如 500ms 窗口内多次修改只落盘一次）；同时改为 `tokio::task::spawn_blocking` 或 `tokio::fs`；
  3. `dashboard.rs` 三个读文件点：HTML 与静态资源用 `OnceLock` 缓存（生产期内容不变），日志读取改为读取固定大小的尾部区间（`SeekFrom::End`）而非全量 `read_to_string`。
- **预期收益**：消除 async worker 线程被文件 I/O 抢占的抖动源；终端注册风暴下的写盘次数从 O(N) 降至 O(1)（去抖窗口）；审计写入路径的 syscall 从 3 次/条降至摊薄后的近 0 次。工作量约 2 人日。

#### 【P1-3】桌面推流链路：每帧全量重编码 · *维度：性能*

- **位置**：`crates/agent/src/stream.rs:125-211`
- **问题链**：
  1. **去重粒度过粗**：`stream.rs:158` 只比较整帧采样哈希是否**完全相等**。判据一旦不等即执行全帧转换 + 全帧 JPEG 编码。桌面场景下时钟秒数、输入光标闪烁、任务栏图标动画都会让哈希恒不相等 → **去重实际近乎失效**，退化为"每帧全量重编码"；
  2. **无硬件加速的逐像素 Rust 转换**：`stream.rs:18 fast_rgba_to_rgb` 手写 per-pixel 循环（1080p = 207 万次迭代 / 帧；4K = 829 万次），未用 SIMD、未用 `image` 的批量 API；
  3. **分辨率阶梯不连续**：`stream.rs:23` 硬编码 `if orig_w > 1920 { 减半 }`——1920 宽屏**完全不降采样**，1921 宽直接砍半；阈值应改为基于目标带宽/目标宽度的连续缩放；
  4. **`scale` 协议字段全链路被忽略**：`ServerToAgentMessage::StartDesktopStream` 定义了 `scale`，但 `router.rs:816` 恒发 `1.0`，`ws_client.rs:661` 用 `..` 丢弃该字段，`dashboard.rs` 也未透传。这是一个已定义但无实现的协议字段；
  5. **JPEG 编码质量硬夹紧**：`stream.rs:118` `quality.min(85)`，调用方传入的 90–100 无效（`dashboard.rs:543` 默认 60，符合预期但接口语义有歧义）；
  6. **静态帧"保活"误伤**：`stream.rs:160` 即使画面完全静止也每 1s 强制重编码一帧，配合第 1 点形成"1s 一次全量编码"的稳态开销。
- **影响**：单路 1080p@15fps 推流的单核 CPU 占用主要消耗在 RGB 转换与 JPEG 编码上（估算量级：单帧转换 + 编码 10–25 ms，即约 15–35% 单核），多路并发时线性叠加。这直接决定"单台 Agent 能同时支撑几路远程桌面"。
- **优化建议**（按性价比排序）：
  1. **恢复去重的有效性**：把"整帧哈希相等"改为**分块哈希（如 64×64 宏块）差异率阈值**判定（差异像素 < 0.5% 即视为同一帧），或至少改为"哈希变化 + 最小重发间隔"双条件；
  2. **接入编码器能力**：优先评估用 OS 侧硬件编码（Windows Media Foundation / VideoToolbox）替代纯 Rust JPEG，或改用 `image` 的批量转换 API + `turbojpeg` 级别实现；
  3. **让 `scale` 真正生效**：打通 `capture_screen` / `StartDesktopStream` 的 `scale`，并删除 `>1920 硬编码减半`，改为 `target_width` 驱动；
  4. **补充静态帧语义**：静止时只发轻量心跳（如每 5s 一帧），而非 1s 全量重编码。
- **预期收益**：在典型办公桌面（大量静止/近静止画面）下，编码次数可降一个数量级；推流 CPU 占用与带宽同步下降，直接提升单机可承载的并发远程会话数。工作量约 3–5 人日（需配套压测）。

#### 【P1-4】注册表锁粒度与跨 await 持锁 · *维度：性能 / 架构*

- **位置**：`crates/server/src/ws/registry.rs:199-239 list_terminals`
- **问题**：函数在持有 `sessions.read().await` 读锁期间，**对每一个终端调用 `meta_store.get().await`**（内部再取 `records.read().await`），随后又调用 `meta_store.list_all().await`（内部 `map.clone()` 克隆整张表）。tokio 的 `RwLock` 是公平写优先锁，长时读锁会**阻塞所有 `register` / `update_heartbeat` / `sweep_offline` 写入**。
- **放大因素**：`list_terminals` 是**高频只读接口**——`/api/terminals`、MCP `list_terminals` 工具、`mcp/mod.rs:244 health_handler` 都会调用它。控制台轮询 + AI 轮询叠加时，注册表写入侧被反复饿死。
- **优化建议**：
  1. 先在读锁内把 sessions 快照成 `Vec<(TerminalInfo, status, metrics)>` 并**立即释放锁**，再在锁外补齐 meta；
  2. `meta_store.list_all()` 改为返回 `Arc<HashMap>` 或用 `im`/`arc-swap` 做写时复制快照，避免每次全量 clone；
  3. `TerminalEntry` 引入版本号 / `updated_at`，为后续做 ETag 与增量推送留出空间。
- **预期收益**：读路径不再阻塞心跳与注册写入，消除规模增长后的尾延迟尖刺。工作量约 1 人日。

#### 【P1-5】依赖双版本共存（`axum` 0.7+0.8、`image` 0.24+0.25）· *维度：依赖管理*

- **实测来源链**（`cargo tree -i`）：

  ```
  axum v0.8.9
  └── rust-embed v8.12.0
      └── at-pc-server            ← 服务端自身用 axum 0.7，被 rust-embed 带进 axum 0.8

  image v0.24.9
  └── eframe v0.27.2
      └── at-pc-agent             ← agent 自身用 image 0.25，被 eframe 带进 image 0.24
  ```

- **问题**：
  - `Cargo.toml:30` 声明 `rust-embed = { features = ["axum"] }`，但代码只用 `FrontendAssets::get(&path)`（`dashboard.rs:103`），**完全没有使用 rust-embed 的 axum 集成**。这个 feature 白白引入第二条 axum 主版本线（连带 `axum-core` 0.5、`matchit` 0.8）；
  - `eframe 0.27` 绑定 `image 0.24`，而 `xcap 0.9` 与项目直接依赖都要求 `image 0.25`（`stream.rs:40` 直接把 `xcap::image::RgbaImage` 传给 `image::DynamicImage`，两者必须是同一 crate 版本），于是 `image`、`png`(0.17/0.18)、`miniz_oxide`(0.8/0.9) 全部双份。
- **优化建议**：
  1. **零成本项**：`rust-embed` 去掉 `axum` feature（改为 `rust-embed = "8.5"`）——预计直接消除 `axum 0.8` / `axum-core 0.5` / `matchit 0.8` 三条依赖链；
  2. **中成本项**：把 GUI 变为**可选特性**（`[features] gui = ["eframe", "egui"]`，`app` 模块 `#[cfg(feature = "gui")]`），无头生产构建不再链接 eframe —— 同时消除 `image 0.24` / `png 0.17` / `miniz_oxide 0.8` / `raw-window-handle 0.5` / 全量 `objc2`+`calloop` 链。这与"headless 是生产主形态"的产品定位一致（`lib.rs:23 run_headless` 已验证可用）；
  3. **低成本项**：删除未使用的直接依赖 `rand`（agent `Cargo.toml`，全仓库 0 引用；`rand 0.8` 已由 `tungstenite` 传递提供）。
- **预期收益**：全量 clean 构建需编译的 crate 数量下降（46 个多版本包中的相当一部分消失），交叉编译 Windows 目标时 GUI 依赖链（需要 C++ 工具链与平台 SDK）可整体剔除，无头二进制体积与产物扫描面同步缩小。工作量约 1 人日（第 2 项需回归 headless 与 GUI 两条路径）。

#### 【P1-6】服务端无优雅关停 + `spawn_blocking` 池被长任务独占 · *维度：性能 / 运维*

- **位置**：
  - `crates/server/src/main.rs:177-204` —— 全仓库 **0 处 `tokio::signal` / `ctrl_c`**（agent 侧在 `lib.rs:53` 已正确处理），服务端 `let _sweep_handle = ...` 与 `tokio::spawn(... WS server ...)` 均**丢弃 JoinHandle**，进程只能被强杀；
  - `crates/agent/src/stream.rs:125` —— 整个推流会话跑在一个 `spawn_blocking` 闭包内，内部用 `std::thread::sleep` 控速，**长时间独占一个 blocking 线程**；
  - `crates/agent/src/tools/command.rs:126-146` —— 命令执行用 `try_wait()` + `thread::sleep(10ms)` **忙轮询**，最长 30s（默认）即 3000 次唤醒，且同样独占 blocking 线程。
- **影响**：tokio 默认 blocking 池上限 512 线程。推流会话 + 并发命令执行叠加时存在池耗尽风险，届时**所有** `spawn_blocking` 调用（含工具执行）排队，表现为"Agent 整体无响应"。服务端缺少关停钩子则意味着：滚动升级时在途 MCP 调用被直接切断、审计可能丢失尾部记录、`terminals_meta.json` 写入可能被中断（虽有 tmp+rename 保护，但语义上仍是脏退出）。
- **优化建议**：
  1. 服务端接入 `tokio::signal` + `axum::serve(...).with_graceful_shutdown(...)`，并对 sweep / WS 任务做 `JoinHandle` 管理；
  2. 推流改为 `tokio::time::sleep` 的 async 循环 + 仅在"转换 + 编码"这两段纯 CPU 工作上调用 `spawn_blocking`（每帧一次短任务，而非整会话独占）；
  3. 命令执行改用 `tokio::process::Command` + `tokio::time::timeout`（或 `wait_timeout`），彻底去掉 10ms 忙轮询。
- **预期收益**：消除 blocking 池耗尽这一"雪崩式"故障模式；服务端具备可运维的关停语义（升级不断流、审计完整）。工作量约 1.5 人日。

#### 【P1-7】生产代码近乎零单元测试 · *维度：代码质量*

- **量化**：9,138 行测试中，**仅 9 个叶子模块**含单元测试（约 24 个用例）。以下**最关键、行数最多、分支最密**的文件**单元测试数为 0**：

  | 文件 | 行数 | 单元测试 |
  | :--- | ---: | --- |
  | `server/src/mcp/mod.rs` | 915 | 0 |
  | `server/src/router.rs` | 906 | 0 |
  | `server/src/mcp/dashboard.rs` | 723 | 0 |
  | `server/src/ws/handler.rs` | 503 | 0 |
  | `server/src/ws/registry.rs` | 342 | 0 |
  | `server/src/audit.rs` | 109 | 0 |
  | `agent/src/ws_client.rs` | 685 | 0 |
  | `agent/src/stream.rs` | 250 | 0 |

- **问题**：几乎所有测试都是**集成级**（需要真实起 server/agent、绑端口、跨进程），单次全量 75s，且天然覆盖不到"分支级"逻辑（如 RBAC 三层白名单的边界、`extract_auth_token` 的 3 条来源优先级、`percent_decode` 的畸形输入、`sweep_offline` 的阈值边界、`generate_call_id` 唯一性）。**P0-1 的路径穿越能长期存在，正是这个缺口的直接后果**——没有任何针对 `static_asset_handler` 的测试。
- **优化建议**：
  1. 为纯逻辑函数补单元测试（优先：`extract_auth_token` / `percent_decode` / `is_tool_allowed_for_role` / `get_role_for_token` / `redact_token` / `resolve_target_terminal_with_session` 的分支矩阵 / `sweep_offline`）；这些不需要起服务，毫秒级；
  2. 为 `dashboard` 路由补 `tower::ServiceExt::oneshot` 级测试（`tower` 已在 dev-dependencies 中），覆盖鉴权 401、路径穿越拒绝、RBAC 403 三类断言；
  3. 补**回归测试**锁定 P0-1 与 P0-2；
  4. 引入 `criterion` 对 4 个热路径建立基准（`fast_rgba_to_rgb`、JPEG 编码、`compute_sample_hash`、`list_terminals`），使 P1-3 的优化有可量化验收标准。
- **预期收益**：把"必须起整套系统才能验证"降为"毫秒级单测"，反馈周期从 75s 降至秒级；安全/权限类回归被永久锁定。工作量约 3 人日。

#### 【P1-8】死代码与双权限模型 · *维度：代码质量 / 架构*

- **位置**：
  1. `crates/server/src/ws/codec.rs`（196 行）—— `WsReader` / `WsWriter` / `WsMessage` / `perform_ws_handshake`（`ws/handler.rs:96`）在生产链路**零引用**，仅被 `tests/milestone1_p0_test.rs`、`tests/ws_gateway_test.rs` 使用。生产已全部走 `tokio-tungstenite`，但测试仍在断言自研实现，形成"测试保护死代码"的反模式。附带风险：`codec.rs:165` 的 WS 掩码用**系统纳秒时间戳**伪造随机数（RFC 6455 要求不可预测掩码），一旦被误用即产生安全缺陷；
  2. `crates/server/src/router.rs:22-78` `ToolPermission` + `get_tool_permission`（57 行）—— **仅被测试引用**，且与 `config.rs:44 is_tool_allowed_for_role` 构成**两套并行但语义不同的权限模型**（前者 4 档纯分类、后者 3 角色白名单）。两套模型的工具分类已经出现不一致（例如 `cancel_tool` 在 `ToolPermission` 中为 `Admin`）。

- **优化建议**：
  1. 删除 `ws/codec.rs` 中除 `compute_accept_key` 外的全部内容（该函数被 `handler.rs` 的 fallback 路径使用），同步改写两个测试文件改走 `tokio-tungstenite` 客户端；
  2. 删除 `ToolPermission` / `get_tool_permission`，RBAC 单一事实源收敛到 P1-1 的 `ToolSpec` 表中；
  3. 顺带清理：`stream.rs:215 start()` 这一"legacy 转发器"（把二进制帧重新 base64 回文本帧，引入 33% 冗余）同样已无调用方。
- **预期收益**：删除约 250 行生产/测试代码；消除"改错权限模型"的认知陷阱；把潜在的 WS 掩码安全缺陷彻底移出代码库。工作量约 1 人日。

---

### P2 — 工程质量与流程（建议 1 个月内收敛）

#### 【P2-1】构建与发布流程缺口 · *维度：构建流程*

- **现状**：全仓库无 `.github/workflows`、无 `rust-toolchain.toml`（README 写 "Rust 1.78+"，实际环境为 1.96.0）、无 `rustfmt.toml`（**`cargo fmt --all --check` 报 571 处差异**）、无 `[workspace.lints]`。
- **优化建议**（按顺序执行，避免一次性大 diff）：
  1. 加 `rust-toolchain.toml` 固定 toolchain（含 `rustfmt`、`clippy` 组件），消除"我这儿能编"问题；
  2. **独立一个 commit** 执行全仓 `cargo fmt --all`，之后在 CI 加 `cargo fmt --all --check` 门禁——这一步必须先做，否则后续每次 PR 都会混入格式噪声；
  3. 在 `Cargo.toml` 增加 `[workspace.lints.clippy]`，至少开启 `await_holding_lock = "deny"`、`unwrap_used`（对生产代码 `warn`）、`panic` 相关 lint，配合 `#![warn(missing_docs)]` 逐步补文档；
  4. 建最小 CI：`fmt --check` → `clippy --all-targets -D warnings` → `test --workspace` → `build --release`，并缓存 `target/`（可显著缓解 P2-2 的体积问题）；
  5. 加 `cargo-deny` / `cargo-audit` 检查 license 与安全公告。
- **预期收益**：把当前"靠人肉纪律维持"的代码一致性转为机器门禁；`cargo fmt` 一次到位后，后续所有 diff 只含语义变更，AI/人工 review 的信噪比显著提升。工作量约 1.5 人日（含全仓格式化）。

#### 【P2-2】`target/` 达 27 GB · *维度：构建流程*

- **现状**：`target/debug` 20 G + `x86_64-pc-windows-gnu` 3.5 G + `release` 3.2 G + `x86_64-pc-windows-msvc` 855 M。仓库本体（git 跟踪 97 文件）仅约 1.3 MB——**构建缓存是源码的 2 万倍**。
- **成因**：`cargo test --workspace` 会为 lib + 每个 integration test 各产出一份可执行文件（本项目有 18 个 integration test target × 多 profile），叠加三套 target-triple 且无任何清理策略。
- **优化建议**：
  1. `cargo install cargo-sweep` 或定期 `cargo clean -p <crate>`，把"跨平台交叉编译产物"与"日常开发产物"分离到不同 `CARGO_TARGET_DIR`（例如 `target/win` 与 `target/mac`），避免混在同一个目录；
  2. 评估把 integration test 合并为更少的 target（Rust 每个 `tests/*.rs` 都是一个独立 crate，拆分过细会显著放大编译与链接开销）；`p3_som_test.rs` 725 行、`multi_monitor_test.rs` 633 行可考虑并入统一测试 crate；
  3. CI 上使用 `sccache` 或 `swatinem/rust-cache`；
  4. 本地加 `cargo` 配置文件 `.cargo/config.toml` 统一 target-dir 约定。
- **预期收益**：磁盘占用下降数倍；减少"增量编译变慢→全量重编"的反复；多目标切换不再相互污染指纹。工作量约 0.5 人日。

#### 【P2-3】前端 2181 行单文件、无构建与校验 · *维度：架构设计*

- **现状**：`crates/server/frontend/index.html` 单文件 2181 行 / 89 KB，内联全部 CSS 与 JS（46 处函数/脚本声明），无构建步骤、无 lint、无类型检查、无模块化。
- **优化建议**：
  1. 引入**极轻量**构建（不必上框架）：Vite 单入口 + 原生 ES 模块，产出到 `dist/webview/`，由 `rust-embed` 打包；保留"单可执行文件、零外部依赖"的分发优势（这一优势必须守住）；
  2. 只拆模块不引入运行时框架：至少把 `api/`（REST 调用）、`desktop/`（画面渲染与输入上报）、`terminals/`（列表与元数据）分离；
  3. 保留 `AT_PC_FRONTEND_DIR` 的开发期热加载能力，但按 P0-1 建议用 `debug_assertions` 门控。
- **预期收益**：前端具备语法/类型校验与模块边界，改动不再"牵一发动全身"；画面渲染可独立演进（配合 P1-3 的 Blob/`createImageBitmap` 优化）。工作量约 2–3 人日。

#### 【P2-4】`build.rs` 在两个 crate 中逐字重复 · *维度：代码质量*

- **现状**：`crates/agent/build.rs` 与 `crates/server/build.rs` 内容几乎**逐行相同**（Windows 图标 + manifest 资源编译、`which_windres` 查找逻辑），唯一差异是 agent 多拼一个 `app.manifest`。
- **优化建议**：抽为 workspace 内的 `build-support` 私有 crate（`build-dependencies`），或使用 `embed-resource` crate 替代手写 windres 调用（后者还能正确处理 MSVC 工具链，当前脚本只会找 mingw 的 `windres`，在 `x86_64-pc-windows-msvc` 下静默跳过程序图标）。
- **预期收益**：消除双份维护；修复"MSVC 目标下图标/manifest 实际未生效"的隐性缺陷。工作量约 0.5 人日。

#### 【P2-5】审计日志无轮转、读取为全量 · *维度：性能 / 合规*

- **现状**：`audit.rs:64 log` 追加写，**无大小上限、无轮转、无导出**；`audit.rs:86 read_recent(limit)` 实现为**读取整个文件 → 全量反序列化 → 取尾部 N 条**。当前 `audit.jsonl` 仅 136 KB 属正常，但这是无界增长设计。
- **优化建议**：
  1. 落盘侧复用 P1-2 的后台 writer，加入按大小轮转（如 100 MB / 文件，保留 N 份）；
  2. `read_recent` 改为 `SeekFrom::End` 反向读取与 `limit` 成正比的字节区间（复用 `file_ops.rs:109` 已经实现的逆向分块读取思路，可抽取为公共工具）；
  3. 提供显式导出接口（当前仅 `/api/audit?limit=`），满足等保审计留存要求。
- **预期收益**：审计查询从 O(文件大小) 降为 O(limit)；长期运行不再有磁盘耗尽风险。工作量约 1 人日。

#### 【P2-6】`rand` 为未使用的直接依赖 · *维度：依赖管理*

- **证据**：agent `Cargo.toml` 声明 `rand = { workspace = true }`，全仓库对 `rand::` 的引用为 **0**；`rand 0.8.8` 已由 `tungstenite` 间接提供。
- **建议**：删除该直接依赖（并同步清理 `Cargo.toml` 中 workspace 的 `rand` 声明）。
- **预期收益**：依赖树与审计面小幅收窄；`Cargo.lock` 更干净。工作量约 5 分钟。

#### 【P2-7】打包脚本不可复现 · *维度：构建流程*

- **位置**：`scripts/package_dist.py`
- **问题**：
  1. **不含构建步骤**——脚本假设 `target/release` 与 `target/x86_64-pc-windows-gnu/release` 已被人工编译，无版本一致性校验（可能把旧产物打进新版包）；
  2. target triple 硬编码为 `x86_64-pc-windows-gnu`，但仓库同时存在 `x86_64-pc-windows-msvc` 产物目录，语义歧义；
  3. 版本号硬编码在 `print('=== at-pc v1.0.0 ...')` 中，与实际 `CARGO_PKG_VERSION` 可能漂移；
  4. 产出重复别名（`at-pc.exe` 与 `at-pc-server.exe` 内容完全相同，体积各 5.3 MB），且 SHA256 仅打印不落盘，无校验和清单文件；
  5. `os.system('xattr .../codesign ...')` 忽略返回码，签名失败静默通过（Gatekeeper 拦截将在用户侧才暴露）。
- **优化建议**：改为"一条命令从源码到制品"：`cargo build --release --target <triple>` → 校验 `CARGO_PKG_VERSION` → 打包 → 生成 `SHA256SUMS` + 内嵌版本号 → 签名失败即 `exit 1`；移除重复别名。
- **预期收益**：发布产物可追溯、可校验，消除"签名静默失败 + 旧产物入包"两类发布事故。工作量约 1 人日。

#### 【P2-8】dev-dependency 成环（agent ↔ server） · *维度：依赖管理*

- **现状**：`crates/agent` 的 `[dev-dependencies]` 依赖 `at-pc-server`，`crates/server` 的 `[dev-dependencies]` 依赖 `at-pc-agent`。
- **影响**：Cargo 虽允许 dev-dep 成环，但会导致两者**单元的测试编译被绑在一起**（任一侧改动都会触发另一侧测试 target 重建），进一步放大 P2-2 的编译开销；也阻塞未来把任一 crate 独立发布。
- **优化建议**：把跨 crate 的集成测试下沉到工作区级的 `tests/` 元 crate（例如新建 `crates/e2e-tests`，仅含 dev-dependencies），让 `agent` 与 `server` 各自的 dev-dep 保持单向。
- **预期收益**：打破编译耦合，增量编译范围更小；为 crate 独立发布留出空间。工作量约 1.5 人日。

---

## 4. 优先级总表

| 编号 | 维度 | 问题 | 严重度 | 预期收益 | 工作量 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **P0-1** | 安全 | `/static/*` 路径穿越 → 未授权任意文件读取（**已实测复现**） | 严重 | 关闭唯一确认的未授权外泄通道 | 0.5 人日 |
| **P0-2** | 代码质量 | 非 ASCII 参数字节切片截断 panic（**2 处**） | 严重 | 消除工具调用路径上的进程级崩溃 | 0.2 人日 |
| **P1-1** | 架构 | 工具契约 5 处重复；单函数 538 行 / 33 分支 | 高 | 新增工具改动面 5 文件 → 1 处；消除漏改静默失效 | 2–3 人日 |
| **P1-2** | 性能 | 阻塞 I/O 混入 async（54 处 `std::fs`，0 处 `tokio::fs`） | 高 | 消除 worker 线程抢占与写放大；注册风暴下写盘 O(N)→O(1) | 2 人日 |
| **P1-3** | 性能 | 推流每帧全量重编码；去重失效；`scale` 字段未实现 | 高 | 静止场景编码次数降一个数量级；提升单机并发会话数 | 3–5 人日 |
| **P1-4** | 性能 | `list_terminals` 跨 await 持读锁 + 每次全量 clone | 高 | 高频只读不再饿死心跳/注册写入 | 1 人日 |
| **P1-5** | 依赖 | `axum` 0.7+0.8、`image` 0.24+0.25 双版本共存 | 中 | 依赖树收窄；无头/交叉编译链路大幅简化 | 1 人日 |
| **P1-6** | 性能 | 服务端无优雅关停；blocking 池被长任务独占 | 中 | 消除 blocking 池耗尽的雪崩模式；可滚动升级 | 1.5 人日 |
| **P1-7** | 代码质量 | 关键文件单元测试为 0；无性能基准 | 中 | 反馈周期 75s → 秒级；安全回归被永久锁定 | 3 人日 |
| **P1-8** | 代码质量 | 死代码（自研 WS codec、`ToolPermission`、legacy 转发器） | 中 | 删除约 250 行；移除潜在 WS 掩码安全缺陷 | 1 人日 |
| **P2-1** | 构建流程 | 无 CI / toolchain / lint 门禁；571 处 fmt 差异 | 中 | 一致性从人肉纪律转为机器门禁；diff 信噪比提升 | 1.5 人日 |
| **P2-2** | 构建流程 | `target/` 27 GB | 中 | 磁盘下降数倍；编译开销可控 | 0.5 人日 |
| **P2-3** | 架构 | 前端 2181 行单文件、无构建无校验 | 中 | 前端改动具备模块边界与静态检查 | 2–3 人日 |
| **P2-4** | 代码质量 | `build.rs` 双份逐字重复；MSVC 下图标失效 | 低 | 消除双份维护 + 修复隐性功能缺陷 | 0.5 人日 |
| **P2-5** | 性能/合规 | 审计日志无轮转；读取为全量 | 低 | 查询 O(文件) → O(limit)；消除磁盘耗尽风险 | 1 人日 |
| **P2-6** | 依赖 | `rand` 未使用 | 低 | 依赖树与审计面收窄 | 5 分钟 |
| **P2-7** | 构建流程 | 打包脚本不可复现、无校验和、签名失败静默 | 低 | 发布产物可追溯可校验 | 1 人日 |
| **P2-8** | 依赖 | dev-dependency 成环 agent ↔ server | 低 | 打破编译耦合，为独立发布留路 | 1.5 人日 |

**合计**：约 26–33 人日。其中 **P0 两项合计 0.7 人日**，建议当天完成。

---

## 5. 建议实施顺序

```
批次 1（当天，~0.7 人日）—— 止血
  P0-1 路径穿越 + 鉴权覆盖  ┐
  P0-2 非 ASCII 截断 panic  ┘ 同步补两条回归测试（P1-7 的子集）

批次 2（第 1 周，~4.5 人日）—— 收口已知架构债
  P1-8 删死代码（先做，为 P1-1 让路）
  P1-5 依赖去重（零成本项先行：rust-embed 去 axum feature、删 rand）
  P1-4 注册表锁粒度
  P2-1 CI + toolchain + 全仓 fmt（独立 commit）

批次 3（第 2–3 周，~9 人日）—— 性能与架构主线
  P1-1 工具契约单一事实源（依赖 P1-8 完成）
  P1-2 阻塞 I/O 异步化（审计 writer + meta 落盘去抖 + dashboard 缓存）
  P1-7 单元测试与 criterion 基准（为 P1-3 提供验收标尺）

批次 4（第 4 周起，~11 人日）—— 规模化与体验
  P1-3 推流优化（依赖 P1-7 基准）
  P1-6 优雅关停 + blocking 池治理
  P2-3 前端模块化    P2-5 审计轮转    P2-7 发布流水线
  P2-2 / P2-4 / P2-8 随手清理
```

**排序依据**：批次 1 是唯一有"已确认可利用漏洞"的批次；批次 2 的 P1-8 必须先于 P1-1（否则会在死代码上做重构）；批次 3 的 P1-7 必须先于 P1-3（否则推流优化无法量化验收）；批次 4 的其余项互不阻塞，可并行。

---

## 6. 风险与不确定性说明

1. **P1-3 的 CPU 收益为量级估算**，非实测：报告未在本机触发真实屏幕捕获（macOS 需屏幕录制授权），给出的 "10–25 ms/帧、15–35% 单核" 是基于 1080p 帧像素量与 JPEG 编码常规吞吐的推算。**落地前请先按 P1-7 建立基准再验收**。
2. **P1-5 第 2 项（GUI 特性化）会触碰 `main.rs` / `lib.rs` 的模块可见性**，需同时回归 GUI 与 headless 两条启动路径（`main.rs:227 run_agent_app` 与 `lib.rs:23 run_headless`）。
3. **P0-1 的修复需确认产品意图**：若控制台确实需要"未鉴权也能看到登录页"，则修复范围应为"仅开放 `GET /` 与明确的登录静态资源，`/static/*` 全部纳入鉴权"，而非简单全量挂中间件。
4. 本报告未覆盖的领域：Windows 侧真实行为（UIA / EventLog / Service 的原生 API 路径）、多显示器坐标换算的正确性、以及跨平台交叉编译产物的运行时验证——这些需要 Windows 环境实测，建议单独立项。

---

## 附录 A：复现命令与证据

```bash
cd /Users/clkj/项目/at/at-pc

# 1. 基线：测试与静态检查
cargo test  --workspace --no-fail-fast      # 182 tests, 0 failed, 75s
cargo clippy --workspace --all-targets      # 6 warnings, 0 error
cargo fmt --all --check | grep -c '^Diff in'  # 571（未过 rustfmt）

# 2. P0-1 路径穿越（已复现）
./target/release/at-pc-server --host 127.0.0.1 --mcp-port 19803 --ws-port 19804 \
    --auth-token SECRET123 --meta-file /tmp/m3.json --audit-file /tmp/a3.jsonl &
sleep 3
curl -s -o /dev/null -w '%{http_code}\n' --path-as-is \
  'http://127.0.0.1:19803/static/../Cargo.toml'            # 200  ← 穿越成功
curl -s --path-as-is 'http://127.0.0.1:19803/static/../../../at-pc-server.log' | head
                                                            # 200  ← 无凭证读到服务端日志
curl -s -o /dev/null -w '%{http_code}\n' 'http://127.0.0.1:19803/api/terminals'  # 401 ← 对照：API 鉴权正常
pkill -f 'at-pc-server --host 127.0.0.1 --mcp-port 19803'

# 3. P1-5 依赖双版本溯源
cargo tree -i axum@0.8.9   -e normal   # → rust-embed v8.12.0 → at-pc-server
cargo tree -i image@0.24.9 -e normal   # → eframe v0.27.2   → at-pc-agent

# 4. P1-1 工具契约重复度（33 个工具名的字面量命中）
grep -c '"name":' crates/server/src/mcp/tools.rs crates/agent/src/tools/mod.rs   # 40 / 34

# 5. P2-2 构建缓存体积
du -sh target && du -sh target/*
```

**关键代码位置索引**

| 问题 | 文件:行 |
| :--- | :--- |
| P0-1 路径穿越 | `crates/server/src/mcp/dashboard.rs:94-101`、`dashboard.rs:47-76`（中间件作用域） |
| P0-2 字节切片 panic | `crates/agent/src/app/mod.rs:249-251`；同类 `crates/server/src/audit.rs:54-61` |
| P1-1 工具契约 5 处 | `agent/src/tools/mod.rs:24` / `:811`；`server/src/mcp/tools.rs:4`；`server/src/router.rs:32` / `:716`；`server/src/config.rs:44` |
| P1-2 阻塞 I/O | `server/src/meta_store.rs:87`、`server/src/audit.rs:64`、`server/src/mcp/dashboard.rs:30/80/94` |
| P1-3 推流 | `agent/src/stream.rs:18`（转换）、`:56`（哈希）、`:118-160`（质量与去重）、`:215`（legacy 转发） |
| P1-4 锁粒度 | `server/src/ws/registry.rs:199-239` |
| P1-6 关停 / blocking 池 | `server/src/main.rs:177-204`（无关停）、`agent/src/stream.rs:125`、`agent/src/tools/command.rs:126-146` |
| P1-8 死代码 | `server/src/ws/codec.rs`（全文件）、`server/src/ws/handler.rs:96`、`server/src/router.rs:22-78` |
| P2-4 build.rs 重复 | `crates/agent/build.rs`、`crates/server/build.rs` |
| P2-7 打包 | `scripts/package_dist.py:28-47`（硬编码 triple 与版本） |
