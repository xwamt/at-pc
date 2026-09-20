# at-pc 门禁系统（API Key Gate）落地实施计划

- 日期：2026-09-20
- 代码基线：`HEAD = 45067bb`（本地 `main`；R2 感知通道修复与部署工装已提交）
- 全量测试基线：`cargo test --workspace` → **336 passed / 0 failed / 1 ignored**（本文件写作时实测）
- 关联文档：
  - `2026-09-20-api-key-gate-design.md` —— **设计文档**（威胁模型、调研结论、四项已确认决策、密钥格式与数据模型）
  - `2026-09-20-perception-channel-audit.md` / `-fix-verification-round2.md` —— 感知通道现状
  - `.workbuddy/memory/atpc-open-issues.md` —— 项目已知缺陷台账

## 本文与设计文档的分工

| | 设计文档 | **本文（实施计划）** |
|---|---|---|
| 回答 | **为什么这么做**：威胁模型、算法选择理由、密钥长什么样 | **怎么做、怎么验、现在什么样** |
| 粒度 | 架构级 | 文件级 + 行级 + 命令级 |
| 关键内容 | §4 决策、§5 方案设计 | §1 修改项与现状台账、§2 里程碑分解、**§3 可执行检验程序** |

**新增内容（设计文档未覆盖）**：本轮摸底在部署工装里发现 3 处缺陷（D3/D4/D5），已并入 M0 清单；并把设计文档 §7 里"`shutdown.rs:173` 一处改 `into_make_service`"修正为**两处**（§2 M0-6）。

---

## 0. 实施总览

### 0.1 里程碑一览

| 里程碑 | 名称 | 一句话目标 | 新增依赖 | 独立回滚 | 主要交付物 |
|---|---|---|---|---|---|
| **M0** | 止血 | 让"公网无门禁"从**默认态**变成**无法启动** | `getrandom` | ✅ | 启动门禁 + bootstrap key + 统一 auth layer + `/health` 回环豁免 + 部署工装同步 + 2 个测试文件 |
| **M1** | 密钥库 | 凭据从"配置文件里的明文"变成"库里可管理的哈希记录" | `sha2` `subtle` | ✅ | KeyStore + `/api/keys` CRUD + 控制台密钥面板 + 9 个 handler 角色 + 失败限速 |
| **M2** | 设备域分离 | 设备凭据与操作凭据**互不通用** | 0 | ✅ | `kind=dv` 设备 Token + `device_id` 绑定 + `/ws` 注册改造 |
| **M3** | 加固 | 把"能被忽略的告警"升级为拒绝 | 0 | ✅ | TLS 强制 + 会话 cookie + 每 key 配额 + 明文配置迁移 |

### 0.2 里程碑 ↔ 检验程序索引

**每个里程碑都有一套独立可执行、可重复跑的检验程序**，出口条件一律是"该套全部通过 + 全量回归 0 failed"。

| 里程碑 | 任务分解 | 检验程序 | 检验编号 | 出口判据（一句话） |
|---|---|---|---|---|
| M0 | §2 M0（15 项） | **§3.2 G0**（10 项） | G0-1 … G0-10 | 非回环无凭据起不来；首启 key 双通道一致且 `0600`；容器仍 `healthy`；SSE 不再漏 token |
| M1 | §2 M1（12 项） | **§3.3 G1**（6 项） | G1-1 … G1-6 | 密钥库只存哈希；吊销即时生效；最后 admin 不可吊销；9 个 handler 角色矩阵符合；限速生效 |
| M2 | §2 M2（6 项） | **§3.4 G2**（9 个场景） | G2 ①②–⑨ | 设备 Token 与操作 Key 互不通用；`device_id` 不匹配即拒；legacy 路径仍可注册 |
| M3 | §2 M3（5 项） | **§3.5 G3**（4 项） | G3-①…④ | 非回环无 TLS 拒启动；会话 cookie 是 HttpOnly；配额响应头齐全；明文配置迁移后拒明文 |
| 全程 | — | **§3.6 门禁真实性** | 板斧一 + 板斧二 | 每条门禁断言都做过"改坏必须变红"与"真输入必须真失败" |

验收基线矩阵（跨里程碑的完整映射）见 **§4**。

### 0.3 依赖与交付顺序

```text
M0 ──▶ M1 ──▶ M2 ──▶ M3
 │      │      │      └── 依赖 M1（配额需要 key 维度；M3-16/M3-18 彼此独立）
 │      │      └───────── 依赖 M1（dv 记录与 sk 记录同库，复用校验链）
 │      └──────────────── 无（但 M0 的 bootstrap key 在 M1 迁入 KeyStore 后成为正规记录）
 └─────────────────────── M0 是硬前置：M1 的密钥库需要一个已经能生成密钥的起点
```

- **M0 可单独交付并长期使用**：它不引入 KeyStore，只把"启动前置条件"和"入口鉴权覆盖"补齐。
- **M1 之后不得回退 M0**：M1 的 KeyStore 依赖 M0 的 bootstrap 流程产出第一条记录。
- **M2 与 M3 无先后硬依赖**，但 M3 的"明文配置迁移"应在 M2 之后做（设备 Token 也要一起迁）。

### 0.4 全局前置与工程约束

| 项 | 约定 |
|---|---|
| 依赖引入 | 全部加进 `at-pc/Cargo.toml` 的 `[workspace.dependencies]`，仅 `at-pc-server` 引用。**不引回 `rand`**（此前有意移除），**不引入 `uuid`** |
| 提交粒度 | 每里程碑内部按"可编译、测试全绿"切分若干提交，提交信息带 `[gate:M0]` 等前缀，便于回溯 |
| **前端构建（最容易踩的坑）** | `crates/server/src/mcp/dashboard.rs:30` 是 `include_str!("../../frontend/dist/index.html")`。**改 `frontend/src` 后必须执行 `npm --prefix crates/server/frontend run build` 并提交重新生成的 `dist/index.html`**，否则运行时行为零变化 |
| 每里程碑收尾必跑 | `cargo test --workspace --no-fail-fast`（0 failed）+ `npm --prefix crates/server/frontend test` + `npm --prefix crates/server/frontend run lint` |
| 门禁真实性 | 每条"门禁"级判据都要做反向注入验证（§3.6） |
| 构建变体 | 门禁检查一律用 `cargo build --release -p at-pc-server --no-default-features`（与 Dockerfile / deploy.sh 的 Linux 部署形态一致，且无 tray 依赖）。**另需用默认 feature 跑一次启动门禁**，因为 tray 分支会走同一份 `config`（见 M0-4 的实现要求） |

### 0.4 术语

| 术语 | 含义 |
|---|---|
| **操作域** | `:9800` HTTP/MCP + Web 控制台，凭据为客户端 API Key（`kind=sk`，带角色） |
| **设备域** | `:9801` WebSocket，凭据为设备 Token（`kind=dv`，可绑 `device_id`） |
| **开放态回落** | 现有代码在"未配置任何凭据"时把请求当作 `Admin`。全文共 **3 处**（B1/B4/B5） |
| **留痕** | 审计事件 + `token_prefix`（复用 `AuditLogger::redact_token`，`audit.rs:325`），**任何路径都不写密钥明文** |

---

## 1. 修改项与现状台账

> 行号全部基于基线 `45067bb`。**"现状"列是代码事实，不是判断**；"目标"列是 M0–M3 结束时的状态。

### 1.1 A 组 — 入口与鉴权链路

| 编号 | 位置 | 现状 | 目标 | 里程碑 |
|---|---|---|---|---|
| A1 | `mcp/mod.rs:120-128` | 路由表只有 `.layer(cors)`，**没有全局鉴权层**；鉴权逻辑分散在 3 个 handler 里各写一遍 | 加 `.route_layer(middleware::from_fn_with_state(.., auth_layer))`，豁免走白名单表 | M0 |
| A2 | `mcp/mod.rs:265-287` `health_handler` | ① 要求凭据 → **`Dockerfile` 的 `HEALTHCHECK curl -f localhost:9800/health` 直接 401**；② 即便放行，未鉴权也回 `version/mcp_port/ws_port/connected_terminals` | 回环来源免凭据且只回 `{"status":"ok"}`；带凭据回完整字段 | M0 |
| A3 | `mcp/mod.rs:321-327` `sse_handler` | 主动把 `?token=<明文>` 拼进 SSE 的 `endpoint` 事件 | endpoint 固定回 `/messages`（SSE 客户端后续请求自会带凭据） | M0 |
| A4 | `mcp/mod.rs:335-360` `messages_handler` | 第三份内联校验（token→role→401），与 A1/A2 语义重复 | 统一到 `auth_layer`，handler 只从 extensions 读 `Role` | M0 |
| A5 | `mcp/mod.rs:230-242` `is_request_authenticated` | "`auth_token` 与 `roles` 双空 → 返回 `true`"——**第二处开放态回落** | 仅在"回环来源 + 显式放行"时返回 true | M0 |
| A6 | `dashboard.rs:92-94` | `/`、`/dashboard`、`/index.html` **无任何中间件**，前端壳完全裸奔 | 保留可达（浏览器首访还没有 key），但正式纳入全局 layer 的豁免表，并加"只返回不含数据的登录壳"注释约定 | M0 |
| A7 | `dashboard.rs`：`api_get_logs:106`、`api_list_terminals:227`、`api_update_terminal_meta:240`、`api_list_pending_calls:657`、`api_list_terminal_pending_calls:663`、`api_start_desktop_stream:680`、`api_stop_desktop_stream:706`、`api_get_desktop_frame:717`、`api_get_desktop_frame_raw:737` | **9 个 handler 无角色校验**。对比：`api_delete_terminal:284`、`api_invoke_tool:363`、`api_cancel_call:519`、`api_cancel_terminal_call:596`、`api_send_desktop_input:781`、`api_get_audit_logs:884`、`api_export_audit_logs:908` 都有 | 补角色矩阵（§3.3 G1-3） | M1 |
| A8 | `ws/handler.rs:325-345` | 仅比对单个 `config.auth_token`；**该值为 `None` 时无条件放行**，任何设备都能注册；`roles` 里的 key 对 Agent 不可用 | 走设备域 KeyStore 校验 + `device_id` 绑定 + 失败限速；`--auth-token` 降级为兼容凭据并打弃用 WARN | M2 |

### 1.2 B 组 — 凭据解析与比对

| 编号 | 位置 | 现状 | 目标 | 里程碑 |
|---|---|---|---|---|
| B1 | `config.rs:170-194` `get_role_for_token` | 明文比较：`self.roles.get(clean)` + `if !primary.is_empty() && primary == clean`。**非常量时间** | 对库内记录改 `subtle::ConstantTimeEq` 常量时间比对 | M1 |
| B2 | `config.rs:188-191` | `if self.auth_token.is_none() && self.roles.is_empty() { return Some(Role::Admin) }`——**第一处开放态回落**（最根本的一处） | **函数体不改**（被 F1 锁定），改由**调用方**传入 `peer_is_loopback`，非回环直接拒绝 | M0 |
| B3 | `config.rs:119` | `roles: HashMap<String, Role>` —— 密钥明文存在配置里 | 新增 `<data_dir>/keys.json`，只存 `sha256(salt‖secret)`；`roles` 保留为 legacy 兼容到 M3 迁移 | M1 |
| B4 | `mcp/mod.rs:174-227` `extract_auth_token` | header / cookie / query 三源，**query 与 header 同优先级**，query 成为实际主路径之一 | **函数签名与语义完全不动**（F1 锁定）；新增 `extract_credential()` 包装把来源带出，来源为 `Query` 时记审计 warn + 响应加 `Warning: 199` 头 | M1 |
| B5 | `mcp/mod.rs:143-152` `auth_middleware` | `None` 分支再次判断"双空 → Admin"——**第三处开放态回落** | 由 `auth_layer` 统一处理（同 B2） | M0 |
| B6 | 全局 | 无哈希、无盐、无 pepper | `sha256-salt-v1` 单算法（**不做 Argon2id**，见设计文档 §5.3 与 §9）；pepper 可选、默认关闭 | M1 |

### 1.3 C 组 — 密钥生命周期

| 编号 | 位置 | 现状 | 目标 | 里程碑 |
|---|---|---|---|---|
| C1 | 全局 | **无任何密钥生成路径**；`rand` 已被有意移除，代码库里没有 CSPRNG 调用 | `getrandom` 取 32 字节 secret + 5 字节 id → `atpc_sk_<id>_<secret>` | M0（bootstrap）/ M1（通用签发） |
| C2 | 全局 | **无密钥持久化**：密钥要么手写在 TOML，要么靠 `--auth-token` 手工传 | `<data_dir>/keys.json`，`0600`，写临时文件 + `rename` 原子落盘；复用 `meta_store.rs` 的后台写线程模式并接入 `shutdown_persistence` | M1 |
| C3 | 全局 | 无列表 / 吊销 / 轮换 / 过期 | `GET/POST/DELETE /api/keys` + `POST /api/keys/:id/rotate`（`grace_hours` 默认 24） | M1 |
| C4 | 全局 | 无 `last_used_at` | 校验成功时刷新（内存态即时，落盘节流 60 s/key，避免写放大） | M1 |
| C5 | 全局 | **无限速**：可以无限次试 key | 内存态 `HashMap<IpAddr, (fail_count, window_start)>`；60 s 内 ≥10 次 → `429 + Retry-After`；10 min 内 ≥30 次 → 封禁 15 min | M1（`:9800`）/ M2（`:9801` 注册失败） |
| C6 | `audit.rs:203-230` `AuditRecord`、`:325` `redact_token`、`:400` `log()` / `:408` `log_async()` | **通道已完备**：有 `token_prefix` 字段、有脱敏函数、有 JSONL 落盘与轮转；但 `action` 目前只承载工具调用 | 复用通道，新增 `AUTH_FAILED`、`insecure_mode`、`key:create`、`key:revoke`、`key:rotate` 五类 action | M0（前两个）/ M1（后三个） |

### 1.4 D 组 — 部署与工装（本轮新发现 3 处）

| 编号 | 位置 | 现状 | 目标 | 里程碑 |
|---|---|---|---|---|
| D1 | `server_config.example.toml:15,34-37` | `auth_token` 被注释、`[roles]` 三行**全部被注释** → 配置解析后 `roles` 为空 = 开放态。而该文件被 `docker-compose.yml:17` 以**只读挂载**进容器 | 示例文件改为"必须先生成 key"的引导；`[roles]` 明确标注为 legacy 弃用 | M0 |
| D2 | `docker-compose.yml:12-14` | `9800:9800` / `9801:9801` 直接发布到宿主全部网卡 | 保持（用户已确认接受首启需输 key，见设计文档 §4.1），但补注释说明门禁状态 | M0 |
| D3 | `Dockerfile:24-25` + `docker-compose.yml:19` **vs** `mcp/mod.rs:36-47` | ENV 设的是 **`AT_PC_LOG_DIR`**，而代码读的是 **`AT_PC_LOG_FILE`** → **死变量**。更严重的是回退链：`default_log_file_path()` 在"CWD 不是 `/`"时用 CWD，否则用**可执行文件所在目录**（**不经 `default_data_dir()`**）。容器 CWD 恰为 `/`（runner 阶段未设 `WORKDIR`），`Dockerfile` CMD 又没传 `--log-file` → **运行日志实际落到 `/usr/local/bin/at-pc-server.log`**，既不在任何卷内、也不在 `/var/log/at-pc`。`docker-compose.yml:16` 声明的 `at-pc-logs` 卷只承载 audit | ① `Dockerfile` / compose 改用 `AT_PC_LOG_FILE=/var/log/at-pc/at-pc-server.log`；② 把 `default_log_file_path()` 的 exe-dir 回退改为 `default_data_dir()` 回退（消除"写进 `/usr/local/bin`"这一路径）；③ `Dockerfile` CMD 显式补 `--log-file`，与 `--meta-file`/`--audit-file` 保持一致 | M0 |
| D4 | `deploy/deploy.sh:83-115` `deploy_standalone` | 三个部署分支里**唯一漏参数**的：只传 `--host/--ws-port/--mcp-port/--log-file`，**未传 `--config` / `--meta-file` / `--audit-file`**（systemd 分支全传、Dockerfile CMD 也全传）→ standalone 模式下 meta 与 audit 落 CWD（即"落进已删除 inode"那个已知缺陷的成因） | 补齐三个参数，并与 `--keys-file`（M1 新增）一起集中到一处变量 | M0 |
| D5 | `deploy/verify.sh:36-58` | 4 项检查全部**不带凭据**：`/health`、`/sse`、`/dashboard`、WS 端口探测。M0 之后 `/sse` 必然 401 → **验收脚本自己会误报 FAIL** | 支持 `AT_PC_TOKEN` 环境变量：有 token 就给 `/sse` 带上；`/health` 保持无凭据（依赖回环豁免）。**这是 M0 的隐藏必改项** | M0 |
| D6 | `deploy/systemd/at-pc-server.service:11` | 三个分支里**唯一参数完整**的：`--config` + `--meta-file` + `--audit-file` + `--log-file` 全传，`WorkingDirectory=/var/lib/at-pc` | 保持不动，作为 D3/D4 的对齐样板 | — |
| D7 | `docker-compose.yml:15-16` | `AT_PC_DATA_DIR=/var/lib/at-pc` + 具名卷 `at-pc-data` → `keys.json` 落卷内即持久化 ✓ | 在 README 明确写出"重建容器不会重新 bootstrap"，否则每次 `up --build` 都会以为丢了 key | M0 |
| D8 | `Dockerfile:29-30` | `HEALTHCHECK ... curl -f http://localhost:9800/health` | **零改动**（容器内请求来源就是回环，依赖 A2 的豁免策略） | — |
| D9 | `main.rs:136-152` | 配置自动探测 `server_config.toml`（CWD）→ `/etc/at-pc/server_config.toml`；Dockerfile CMD **不传** `--config`，容器 CWD=`/`，因此实际是自动探测命中 compose 挂载的 `/etc/at-pc/server_config.toml` | 保持；但要在文档写清"容器里配置来自自动探测"，避免运维改了挂载文件却不生效时无从排查 | M0（文档） |

### 1.5 E 组 — 前端

| 编号 | 位置 | 现状 | 目标 | 里程碑 |
|---|---|---|---|---|
| E1 | `frontend/src/api/index.js`（`readAuthToken`） | 从 URL 读取 `?token=` / `?pin=` 并写入 sessionStorage。**行为被 `frontend/tests/api.test.js:31` 锁定** | 读取后立即 `history.replaceState` 清掉 URL（避免留在浏览器历史与 Referer）；存储默认 sessionStorage，加"记住此设备"显式勾选才切 localStorage | M1 |
| E2 | `frontend/src/app.js`（`handleUnauthorized`） | 已有 401 弹窗机制，文案是"输入 Token" | 文案改"输入 API Key"；**不新增**"服务器是否开启门禁"的公开探测端点（会向未授权访问者泄漏部署状态），沿用"打 `/api/terminals` 401 就弹窗" | M1 |
| E3 | 前端 | 无密钥管理界面 | 新增面板：列表 / 创建（**一次性明文弹窗**）/ 吊销 / 轮换 | M1 |
| E4 | `dashboard.rs:30` | `include_str!("../../frontend/dist/index.html")`，即 `dist/index.html` 是**编译期内联的构建产物**（且已纳入 git） | 见 §0.3 的构建约束 | M1 |

### 1.6 F 组 — 既有测试与不可破坏的约束

| 编号 | 位置 | 现状（这些是**保护栏**，不是待修项） | 本次约束 |
|---|---|---|---|
| F1 | `crates/server/tests/p1_auth_rbac_call_id_test.rs` | 6 例：`:18` header/cookie/query 优先级与别名、`:41` 百分号解码与畸形输入、`:68` `get_role_for_token` 三分支（**`:96-100` 断言 `ServerConfig::default()` 对任意非空 token 返回 `Admin`**）、`:104` RBAC 矩阵全覆盖 | **一行断言都不改**。因此回环限制必须放在**调用方**（`auth_layer`）而不是 `get_role_for_token` 内部 —— 这是本计划对设计文档 §7 的收敛 |
| F2 | `crates/server/tests/static_asset_security_test.rs:115` | 静态资源需鉴权且被限制在 asset root 内 | M0 保持全绿 |
| F3 | `crates/server/tests/ws_gateway_test.rs:130` | `test_ws_gateway_auth_failure` 锁定注册失败语义 | M2 只做**新增用例**，旧例保持绿 |
| F4 | 全量 | `cargo test --workspace` = **336 passed / 0 failed / 1 ignored** @ `45067bb` | 每里程碑不得让任何既有用例转红 |
| F5 | `crates/server/frontend/tests/` | `api.test.js` / `desktop.test.js` / `terminals.test.js` / `typecheck-guard.test.js` | E1 改动须同步更新 `api.test.js:31` 的期望 |

### 1.7 明确不动的清单（防止范围蔓延）

- `extract_auth_token` 的签名、优先级与百分号解码语义（F1 锁定）
- `get_role_for_token` 的 legacy / dev 分支函数体（F1 锁定）
- `[roles]` / `auth_token` 的配置解析（M0–M2 保留为兼容凭据，M3 才迁移）
- `crates/protocol/src/tools.rs` 的 `Role{Viewer<Operator<Admin}` 与 `is_tool_allowed_for_role`（已完整可用）
- TLS / mTLS 实现与 `tls.rs`
- `Dockerfile` 的 `HEALTHCHECK` 行（D8）
- `deploy/systemd/at-pc-server.service`（D6）
- 感知通道相关的一切（`get_marked_screen` / `include_image` / `get_ui_tree` / SoM）

---

## 2. 里程碑分解

### M0 — 止血

**目标**：非回环部署在没有凭据时**无法启动**；首启自动产出 root key 并落到 stdout + `0600` 文件；入口鉴权从"三处内联 + 半覆盖"收敛为一处全局层；容器 `HEALTHCHECK` 不被破坏。

**前置**：无。

**任务清单**

| # | 文件 | 改动 |
|---|---|---|
| M0-1 | `at-pc/Cargo.toml`、`crates/server/Cargo.toml` | `[workspace.dependencies]` 加 `getrandom = "0.2"`；server 引用 |
| M0-2 | **新增** `crates/server/src/bootstrap.rs` | `generate_root_key() -> String`：`getrandom` 取 32 字节 → base64url-no-pad（43 字符）+ 5 字节 → Crockford base32（8 字符 lowercase），拼 `atpc_sk_<id>_<secret>`；`print_bootstrap_banner(key)`；`write_bootstrap_file(data_dir, key)`（Unix 用 `OpenOptions::new().mode(0o600)`；Windows 走 ACL 收紧，失败则退化为默认 ACL + WARN） |
| M0-3 | `crates/server/src/config.rs` | 新增 `pub fn is_loopback(host: &str) -> bool`（`127.0.0.1` / `::1` / `localhost` / `127.0.0.0/8`）、`pub enum BootGateError`、`pub fn validate_boot_gate(cfg, has_creds, insecure_no_auth) -> Result<(), BootGateError>`；`ServerConfig` 加 `bootstrap_disabled: bool`、`insecure_no_auth: bool`、`allow_plaintext: bool`（后者 M3 用，M0 先占位） |
| M0-4 | `crates/server/src/main.rs` | 解析 `--no-bootstrap` / `--insecure-no-auth`（并加进 `--help` 文本）；启动序列：载入 KeyStore（M0 阶段为"无 → 空"）→ 若无记录且未禁用则 bootstrap → `validate_boot_gate` → 失败时打印解法并 `exit(1)`。**实现要求：门禁判定必须放在 `main()` 里、tray 分支之前**（当前约 `:192`），否则 tray 模式会先弹图标再退出；`rt.block_on` 之后放就晚了 |
| M0-5 | `crates/server/src/mcp/mod.rs` | ① 新增 `auth_layer`（含豁免表：`OPTIONS *`、`/health` 且来源回环、三个 HTML 路由）；② `create_mcp_http_router` 在 `.layer(cors)` 后加 `.route_layer(...)`；③ **删除** `health_handler` / `sse_handler` / `messages_handler` 里三份重复内联校验；④ `health_handler` 取 `ConnectInfo<SocketAddr>`，回环未鉴权只回 `{"status":"ok"}`，带凭据回完整；⑤ `sse_handler` 的 endpoint 事件去掉 `?token=`；⑥ 新增 `CredentialSource` 与 `extract_credential()`（`extract_auth_token` **原样保留**，见 F1） |
| M0-6 | `crates/server/src/shutdown.rs` | **两处**都要改：`:97` `axum::serve(listener, app)` → `axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())`；`:173` `.serve(app.into_make_service())` → 同样换 `into_make_service_with_connect_info`。建议直接改 `serve_with_graceful_shutdown` 的函数体，保持签名不变，这样所有调用点零改动 |
| M0-7 | `crates/server/src/mcp/dashboard.rs` | 三个 HTML 路由留在豁免表内，加注释写明"**只返回不含任何终端/审计数据的登录壳**"这一约定，防止后续有人往壳里塞数据 |
| M0-8 | `crates/server/src/audit.rs` | 新增 `AUTH_FAILED`、`insecure_mode` 两个 action 常量 |
| M0-9 | **新增** `crates/server/tests/p0_boot_gate_test.rs` | 4 例（见 §3.2 G0-1） |
| M0-10 | **新增** `crates/server/tests/p0_health_loopback_test.rs` | 2 例（见 §3.2 G0-1） |
| M0-11 | `deploy/deploy.sh` | `deploy_standalone` 补 `--config /etc/at-pc/server_config.toml`、`--meta-file`、`--audit-file`（D4） |
| M0-12 | `deploy/verify.sh` | 支持 `AT_PC_TOKEN`；带 token 时 `/sse` 检查加 `-H "Authorization: Bearer $AT_PC_TOKEN"`（D5） |
| M0-13 | `Dockerfile`、`docker-compose.yml` | `AT_PC_LOG_DIR` → `AT_PC_LOG_FILE=/var/log/at-pc/at-pc-server.log`（D3） |
| M0-14 | `server_config.example.toml`、`README.md` | 示例文件改为"先生成 key"的引导文案；README 写清 D7（卷持久化）与 D9（容器内配置来自自动探测） |
| M0-15（建议） | **新增** `deploy/verify_gate.sh` | 把 §3.2 的 G0-1…G0-10 自动化成一个可重复执行的脚本，后续里程碑往里追加检查项 |

**M0 不做的事**：9 个 handler 的角色校验（A7）、KeyStore、`/ws` 改造、query 通道降级、前端任何改动。

**M0 的已知临时态**：bootstrap key 此时**还不是库里的记录**，只存在于 stdout 与 `bootstrap_key.txt`；校验走"内存态 bootstrap key 与 `config.auth_token` 兼容路径"。M1 把它迁入 KeyStore 后成为正规记录（`name="bootstrap-root"`），此时需要一次幂等迁移：若 KeyStore 为空但存在 `bootstrap_key.txt`，则把该 key 的哈希登记为 admin 记录后删除文件。

**M0 交付物**：见 §0.1；新增 2 个测试文件（+1 个建议的验收脚本）。

**M0 回滚**：`--insecure-no-auth` 即可恢复旧行为（**无需回滚代码**）；彻底回滚 = 删除 `bootstrap.rs` 与 `.route_layer` 一行。三层独立。

---

### M1 — 密钥库

**目标**：凭据从"配置文件里的明文"变成"库里可管理的哈希记录"，并补齐控制台与 HTTP 层的角色分权。

**前置**：M0。

**任务清单**

| # | 文件 | 改动 |
|---|---|---|
| M1-1 | `at-pc/Cargo.toml` | 加 `sha2 = "0.10"`、`subtle = "2.6"` |
| M1-2 | **新增** `crates/server/src/keystore.rs` | `KeyRecord` 数据模型（设计文档 §5.2）；`KeyStore::load/save`（`0600` + 原子写）；`verify(credential) -> Result<Principal, AuthError>`（解析 → 按 `id` 查表 O(1) → `subtle` 常量时间比对 → 检查 `kind`/`expires_at`/`revoked_at`/`scopes` → 刷新 `last_used_at` 节流 60 s）；`issue/revoke/rotate/list`。持久化复用 `meta_store.rs` 的后台写线程 + 原子 flush + `shutdown_persistence` 集成 |
| M1-3 | `crates/server/src/config.rs` | `get_role_for_token` **函数体不改**，新增旁路 `resolve_role_from_keystore()`；新增 `handler_min_role(path, method) -> Option<Role>`（把 §5.5 的 9 行矩阵集中成一张表，供 HTTP 层与 MCP 层共用） |
| M1-4 | `crates/server/src/mcp/mod.rs` | `auth_layer` 接入 KeyStore（先试库、再落 legacy 配置）；`extract_credential()` 的 `Query` 来源加 `Warning: 199` 响应头 + 审计 warn |
| M1-5 | `crates/server/src/mcp/dashboard.rs` | ① 新增 `/api/auth/me`；② 新增 `/api/keys` 的 GET/POST/DELETE + `/api/keys/:id/rotate`，全部要求 `Admin`；③ **护栏：不能吊销/降级最后一个 admin 级客户端 Key** → 409；④ 补 §5.5 的 9 个 handler 角色校验（A7） |
| M1-6 | `crates/server/src/rate_limit.rs`（新增） | 内存态失败计数 + 429 + `Retry-After` + 临时封禁（C5） |
| M1-7 | `crates/server/src/audit.rs` | 新增 `key:create` / `key:revoke` / `key:rotate` action；失败路径审计做同 IP 同分钟合并计数，避免被刷爆 |
| M1-8 | `crates/server/frontend/src/` | 密钥管理面板 + 一次性明文对话框 + 401 文案 + URL token 清理（E1/E2/E3） |
| M1-9 | `crates/server/frontend/dist/index.html` | **必须重新构建并提交**（E4） |
| M1-10 | **新增** `crates/server/tests/p1_keystore_test.rs`、`p1_keystore_lifecycle_test.rs` | 见 §3.3 |
| M1-11 | `crates/server/frontend/tests/api.test.js` | 同步 E1 的期望（F5） |
| M1-12 | `deploy/deploy.sh`、`docker-compose.yml`、`server_config.example.toml` | 引入 `--keys-file` / `AT_PC_KEYS_FILE`，默认 `<data_dir>/keys.json`；确认落卷内（D7） |

**M1 不做的事**：设备域（`:9801`）、TLS 强制、会话 cookie、明文配置迁移。

**M1 的已知临时态**：`[roles]` 与 `auth_token` 仍可用（等价 legacy 凭据），文档需标注为"待 M3 迁移"。

---

### M2 — 设备域分离

**目标**：设备凭据与操作凭据互不通用；一台设备失陷不牵连操作端。

| # | 文件 | 改动 |
|---|---|---|
| M2-1 | `crates/server/src/keystore.rs` | 支持 `kind=dv` 记录与 `device_id` 绑定 |
| M2-2 | `crates/server/src/ws/handler.rs:325-345` | `Register` 分支改为优先校验 `kind=dv` 的设备 Token（命中则通过；`device_id` 非空时要求 `info.terminal_id` 严格匹配）；未配置任何设备 Token 时回退到 `config.auth_token` 兼容路径并打弃用 WARN |
| M2-3 | `crates/server/src/ws/handler.rs` + `rate_limit.rs` | 注册失败按来源 IP 限速（C5 的 `:9801` 侧） |
| M2-4 | **新增** `crates/server/tests/p2_device_token_test.rs` | 见 §3.4 |
| M2-5 | `crates/agent/src/config.rs`、`agent_config.toml` | 设备 Token 字段与文档；`--auth-token` 标注弃用 |
| M2-6 | `deploy/deploy.sh`、`docker-compose.yml`、`README.md` | 设备 Token 的签发与下发说明 |

---

### M3 — 加固

| # | 文件 | 改动 |
|---|---|---|
| M3-1 | `crates/server/src/config.rs` + `main.rs` | 非回环且未启用 TLS → **拒绝启动**；`--allow-plaintext` 显式豁免并打 WARN，审计带 `insecure_mode` |
| M3-2 | `crates/server/src/mcp/mod.rs` + 前端 | 浏览器登录换取 **HttpOnly 会话 cookie**（前端不再持有长期 Key）；`Set-Cookie: HttpOnly; SameSite=Strict; Secure` |
| M3-3 | `crates/server/src/rate_limit.rs` | 按 Key 的请求配额 + `X-RateLimit-Limit/Remaining/Reset` 响应头 |
| M3-4 | **新增** `crates/server/src/migrate.rs`（或 `bootstrap.rs` 内） | `--migrate-legacy-credentials`：把 `[roles]` / `auth_token` 逐条转成 KeyStore 记录、明文各展示一次；迁移后启动**拒绝**明文配置 |
| M3-5（可选） | `crates/server/src/mcp/tools.rs` | 只读 `list_api_keys` MCP 工具（`Admin` 级）。**不做**创建/吊销类 MCP 工具（会形成模型可自我提权路径，见设计文档 §5.7） |

---

## 3. 检验程序

> 每条都给出**可复制执行的命令**与**判据**。判据尽量做成"数值/状态码/计数"而不是"看起来对"。
> 约定：`$DIR` 为该实例的数据目录，`$KEY` 为该实例的 root key，`$LAN` 为本机非回环地址。

### 3.1 通用准备（G-COMMON）

```bash
cd /Users/clkj/项目/at/at-pc
git rev-parse --short HEAD          # 记录基线

# 1) 全量测试汇总（一条命令给出 passed/failed）
cargo test --workspace --no-fail-fast 2>&1 \
  | grep -E "^test result" \
  | awk -F'[ ;]' '{p+=$4; f+=$6} END {print "passed="p, "failed="f}'
# 期望：passed=336 failed=0（M0 之后为 336+新增）

# 2) headless 二进制（与 Linux 部署形态一致）
cargo build --release -p at-pc-server --no-default-features
BIN=./target/release/at-pc-server

# 3) 隔离实例管理（每次检查都用干净 data dir，避免互相污染）
start() { DIR=$(mktemp -d "$TMPDIR/atpc-XXXX"); \
  AT_PC_DATA_DIR="$DIR" "$BIN" "$@" >"$DIR/stdout.log" 2>&1 & echo $! >"$DIR/pid"; }
stop()  { kill "$(cat "$DIR/pid")" 2>/dev/null || true; wait "$(cat "$DIR/pid")" 2>/dev/null || true; }

# 4) 提 key
KEY_FILE=$(grep -oE 'atpc_sk_[A-Za-z0-9_-]+_[A-Za-z0-9_-]+' "$DIR/bootstrap_key.txt" | head -1)
KEY_STDOUT=$(grep -oE 'atpc_sk_[A-Za-z0-9_-]+_[A-Za-z0-9_-]+' "$DIR/stdout.log" | head -1)

# 5) 非回环地址（用于验证"非回环必须带凭据"）
LAN=$(ipconfig getifaddr en0 2>/dev/null || hostname -I | awk '{print $1}')
```

> **重要**：`--host 127.0.0.1` 的实例用于验证"回环豁免"；`--host 0.0.0.0` + 通过 `$LAN` 访问用于验证"非回环必须带凭据"。两者必须都做——只测回环会漏掉整条门禁逻辑。

---

### 3.2 G0 — M0 检验程序

#### G0-1 编译与回归（门禁）

```bash
cargo test --workspace --no-fail-fast 2>&1 | grep -E "^test result" | awk -F'[ ;]' '{p+=$4;f+=$6} END {print p, f}'
cargo test -p at-pc-server --test p0_boot_gate_test --test p0_health_loopback_test
cargo test -p at-pc-server --test p1_auth_rbac_call_id_test --test static_asset_security_test --test ws_gateway_test
```
**判据**：① 全量 `failed=0`；② 新增两文件 **6 passed / 0 failed**（`p0_boot_gate_test` 4 例 + `p0_health_loopback_test` 2 例）；③ F1/F2/F3 三个既有文件**零改动且全绿**（`git diff` 该三文件应为空）。

`p0_boot_gate_test` 的 4 例（纯函数级，不起网络）：
1. `validate_boot_gate("0.0.0.0", has_creds=false, insecure=false)` → `Err`
2. `validate_boot_gate("127.0.0.1", false, false)` → `Ok`
3. `validate_boot_gate("0.0.0.0", true, false)` → `Ok`
4. `validate_boot_gate("0.0.0.0", false, insecure=true)` → `Ok`
外加 `is_loopback` 的边界：`"127.0.0.1"`/`"::1"`/`"localhost"`/`"127.5.5.5"` → true；`"0.0.0.0"`/`"192.168.1.10"`/`"::"` → false。

`p0_health_loopback_test` 的 2 例（起内存 router，打到 with_connect_info 的 service）：
1. 回环来源 + 无凭据 → `200`，且 JSON 的 key 集合恰为 `{"status"}`
2. 非回环来源 + 无凭据 → `401`

#### G0-2 非回环 + 无凭据 → **拒绝启动**（门禁）

```bash
start --host 0.0.0.0 --no-bootstrap
wait "$(cat "$DIR/pid")"; echo "exit=$?"
grep -cE "credential|--insecure-no-auth" "$DIR/stdout.log"
lsof -iTCP:9800 -sTCP:LISTEN | wc -l
```
**判据**：`exit=1`；日志出现凭据缺失说明与 `--insecure-no-auth` 解法；`lsof` 计数为 **0**（拒绝启动必须早于端口绑定）。

#### G0-3 回环 + 无凭据 → 允许启动（本地开发体验不退化）

```bash
start --host 127.0.0.1 --no-bootstrap
sleep 1
curl -s -o /dev/null -w "%{http_code}\n" http://127.0.0.1:9800/health     # 200
curl -s http://127.0.0.1:9800/api/terminals | head -c 200                  # 200 且是终端列表
```

#### G0-4 首启 bootstrap：生成、双通道一致、权限 0600（门禁）

```bash
start --host 0.0.0.0; sleep 2
stat -f "%Lp" "$DIR/bootstrap_key.txt"        # macOS；Linux 用 stat -c "%a" → 期望 600
[ "$KEY_FILE" = "$KEY_STDOUT" ] && echo MATCH # 期望 MATCH
echo "$KEY_FILE" | grep -cE '^atpc_sk_[0-9a-z]{8}_[A-Za-z0-9_-]{43}$'   # 期望 1
```
**判据**：权限 `600`；两通道 key **逐字符相同**；格式严格匹配（8 字符 lowercase Crockford id + 43 字符 base64url secret）。
**幂等性**：`stop` 后**不删** `$DIR` 重启 → `bootstrap_key.txt` **不再重写**，`stdout.log` 中不出现新 key（同一实例只有一条 root key）。

#### G0-5 `/health` 回环豁免 + 载荷最小化（门禁）

```bash
# 回环、无凭据
curl -s http://127.0.0.1:9800/health                                   # {"status":"ok"}
curl -s http://127.0.0.1:9800/health | grep -cE "version|mcp_port|ws_port|connected_terminals"  # 期望 0
# 回环、带凭据
curl -s -H "Authorization: Bearer $KEY" http://127.0.0.1:9800/health | grep -c connected_terminals  # 期望 1
# 非回环、无凭据 / 带凭据
curl -s -o /dev/null -w "%{http_code}\n" "http://$LAN:9800/health"                                    # 期望 401
curl -s -o /dev/null -w "%{http_code}\n" -H "Authorization: Bearer $KEY" "http://$LAN:9800/health"     # 期望 200
```
**判据**：4 个计数/状态码分别 `0 / 1 / 401 / 200`。
**反证（必做）**：把回环判定临时改成恒 `true` → 第 5 行必须变成 `200`，即"非回环 401"这条断言是**真的在生效**，而不是因为别的原因碰巧 401。

#### G0-6 容器 `HEALTHCHECK` 未被破坏（门禁）

```bash
docker compose down -v && docker compose up --build -d && sleep 15
docker inspect --format '{{.State.Health.Status}}' at-pc-server        # 期望 healthy
docker compose logs at-pc-server | grep -c atpc_sk_                    # 期望 >= 1
docker exec at-pc-server ls -l /var/lib/at-pc/                         # 期望含 terminals_meta.json
docker exec at-pc-server ls -l /var/log/at-pc/                         # 期望含 audit.jsonl
```
**判据**：`healthy`（不是 `unhealthy`）；能从 `docker logs` 取到 root key；两个卷各自承载预期文件（验证 D3 修复）。
**注意**：必须 `down -v` 之后测。卷保留时 `keys.json` 已存在 → 不再 bootstrap → 第二次跑看不到 key，会误判为"bootstrap 失效"（D7）。

#### G0-7 静态壳可达、数据全门禁（门禁）

```bash
curl -s -o /dev/null -w "%{http_code}\n" "http://$LAN:9800/"                  # 期望 200
curl -s -o /dev/null -w "%{http_code}\n" "http://$LAN:9800/dashboard"         # 期望 200
curl -s -o /dev/null -w "%{http_code}\n" "http://$LAN:9800/api/terminals"     # 期望 401
curl -s "http://$LAN:9800/api/terminals" | grep -c terminal_id                # 期望 0
curl -s "http://$LAN:9800/" | grep -cE "terminal_id|audit|atpc_sk_"           # 期望 0
```

#### G0-8 SSE 不再泄漏 token（门禁）

```bash
curl -sN -H "Authorization: Bearer $KEY" --max-time 2 http://127.0.0.1:9800/sse | head -3
curl -sN -H "Authorization: Bearer $KEY" --max-time 2 http://127.0.0.1:9800/sse | grep -c "token="   # 期望 0
```
**判据**：`endpoint` 事件的 data 恰为 `/messages`。
**反证（必做）**：临时恢复 `format!("?token={}", token)` → 该计数必须变成 `1`。

#### G0-9 部署工装自检（覆盖 D3/D4/D5）

```bash
# D5：verify.sh 在门禁开启后仍能全 PASS
bash deploy/verify.sh 127.0.0.1 9800 9801                     # 期望 5 passed, 0 failed
AT_PC_TOKEN=$KEY bash deploy/verify.sh 127.0.0.1 9800 9801    # 期望 5 passed, 0 failed

# D4：standalone 分支落盘位置正确
sudo bash deploy/deploy.sh standalone
ls -l /var/log/at-pc/audit.jsonl /var/lib/at-pc/terminals_meta.json   # 两文件都存在
ls -l ./audit.jsonl 2>/dev/null                                       # 期望"不存在"（证明没落 CWD）

# D3：日志落盘位置已修复（含"不再写进 /usr/local/bin"这条）
docker compose config | grep -E "AT_PC_LOG(_FILE|_DIR)"       # 期望只出现 AT_PC_LOG_FILE
docker exec at-pc-server ls -l /var/log/at-pc/                # 期望含 at-pc-server.log + audit.jsonl
docker exec at-pc-server ls /usr/local/bin/at-pc-server.log 2>/dev/null && echo "FAIL: 仍在写 /usr/local/bin" || echo "OK"
```

#### G0-10 门禁真实性（§3.6 的具体落地）

```bash
# 板斧一：改坏必须变红
#   把 validate_boot_gate 的 Err 分支临时改成 Ok(())  →
cargo test -p at-pc-server --test p0_boot_gate_test        # 必须 FAIL
#   改回 → 必须 PASS

# 板斧二：真输入必须真的走失败路径
curl -s -o /dev/null -w "%{http_code}\n" \
  -H "Authorization: Bearer atpc_sk_aaaaaaaa_ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ" \
  "http://$LAN:9800/api/terminals"                          # 期望 401
jq -r 'select(.action=="AUTH_FAILED") | .token_prefix' "$DIR/audit.jsonl" | tail -1   # 期望 "atp***"
grep -c "atpc_sk_aaaaaaaa" "$DIR/audit.jsonl"               # 期望 0（永不落明文）
```

---

### 3.3 G1 — M1 检验程序

#### G1-1 密钥库单元测试（门禁）

```bash
cargo test -p at-pc-server --test p1_keystore_test
```
必备用例（每条一个断言，全部要能独立红）：
- 格式解析：合法；**缺段**；段数过多；`ATPC_SK_` 大写前缀 → 拒；secret 长度 ≠ 43 → 拒；id 长度 ≠ 8 → 拒
- 哈希确定性：同 `salt+secret` 两次一致；`salt` 改 1 位 → 结果变
- **常量时间比对**：篡改首字节 / 末字节 / 中间字节 → 均返回 false
- `expires_at` 边界：`now-1s` → 拒；`now+1s` → 通过
- `revoked_at` 非空 → 拒
- `kind` 与入口不匹配：`dv` 打 MCP → 403；`sk` 打 `/ws` → 403
- `device_id`：匹配 → 通过；不匹配 → 拒；`device_id` 为空 → 不校验机器身份
- `scopes`：无 `mcp` 的 key 打 `/mcp` → 403

#### G1-2 密钥生命周期端到端（门禁）

```bash
A="$KEY"                                    # root key
NEW=$(curl -s -X POST -H "Authorization: Bearer $A" -H 'Content-Type: application/json' \
  -d '{"name":"ops-viewer","kind":"sk","role":"viewer","scopes":["console"]}' \
  http://127.0.0.1:9800/api/keys | jq -r .plaintext)
ID=$(curl -s -H "Authorization: Bearer $A" http://127.0.0.1:9800/api/keys | jq -r '.[-1].id')

# ① 明文只出现一次
curl -s -H "Authorization: Bearer $A" http://127.0.0.1:9800/api/keys \
  | jq -e '[.[] | has("plaintext") or has("hash") or has("salt")] | any | not'      # 期望 true
grep -c "$NEW" "$DIR/keys.json"                                                      # 期望 0
jq -r '.keys[].hash_algo' "$DIR/keys.json" | sort -u                                 # 期望仅 sha256-salt-v1
stat -f "%Lp" "$DIR/keys.json"                                                       # 期望 600

# ② 吊销即时生效
curl -s -X DELETE -H "Authorization: Bearer $A" "http://127.0.0.1:9800/api/keys/$ID" >/dev/null
curl -s -o /dev/null -w "%{http_code}\n" -H "Authorization: Bearer $NEW" http://127.0.0.1:9800/api/terminals   # 期望 401

# ③ 轮换 grace（用 grace_hours=0 在同一次会话里验完）
ROT=$(curl -s -X POST -H "Authorization: Bearer $A" -H 'Content-Type: application/json' \
  -d '{"grace_hours":0}' "http://127.0.0.1:9800/api/keys/$ID/rotate")
NEW2=$(echo "$ROT" | jq -r .plaintext)
curl -s -o /dev/null -w "%{http_code}\n" -H "Authorization: Bearer $NEW2" http://127.0.0.1:9800/api/terminals  # 期望 200
# （grace>0 的场景用 grace_hours=24 再跑一次，确认旧 key 仍在有效窗口内）

# ④ 护栏：最后一个 admin 不可吊销
curl -s -o /dev/null -w "%{http_code}\n" -X DELETE -H "Authorization: Bearer $A" \
  "http://127.0.0.1:9800/api/keys/$(jq -r '.keys[]|select(.role=="admin" and .revoked_at==null)|.id' "$DIR/keys.json" | tail -1)"   # 期望 409
```

#### G1-3 角色矩阵（9 个 handler，A7）（门禁）

准备三把 key：`V`(viewer) / `O`(operator) / `A`(admin)。逐行填结果：

| 请求 | Viewer | Operator | Admin |
|---|---|---|---|
| `GET /api/logs` | **403** | **403** | **200** |
| `GET /api/terminals` | 200 | 200 | 200 |
| `POST /api/terminals/:id/meta` | **403** | 200 | 200 |
| `GET /api/calls` | 200 | 200 | 200 |
| `GET /api/terminals/:id/calls` | 200 | 200 | 200 |
| `POST /api/terminals/:id/desktop/start` | **403** | 200 | 200 |
| `POST /api/terminals/:id/desktop/stop` | **403** | 200 | 200 |
| `GET /api/terminals/:id/desktop/frame` | **200** | 200 | 200 |
| `GET /api/terminals/:id/desktop/raw` | **200** | 200 | 200 |

> `frame` / `raw` 给 Viewer 是**已确认决策**（设计文档 §4.1）。注意这意味着 `Viewer` 在本系统中**不再等同于"无敏感信息"**；若后续要把 Viewer 发给外部只读方，需重新评估此条。

自动化脚本骨架：
```bash
for t in logs "terminals" "calls"; do
  for k in "$V:viewer" "$O:operator" "$A:admin"; do
    printf "%-9s /api/%-10s -> %s\n" "${k#*:}" "$t" \
      "$(curl -s -o /dev/null -w '%{http_code}' -H "Authorization: Bearer ${k%%:*}" "http://127.0.0.1:9800/api/$t")"
  done
done
```

#### G1-4 失败限速（门禁）

```bash
for i in $(seq 1 12); do
  curl -s -o /dev/null -w "%{http_code} " \
    -H "Authorization: Bearer atpc_sk_bbbbbbbb_ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ" \
    http://127.0.0.1:9800/api/terminals
done; echo
curl -s -o /dev/null -D - -H "Authorization: Bearer atpc_sk_bbbbbbbb_ZZZZZZ..." http://127.0.0.1:9800/api/terminals | grep -i retry-after
```
**判据**：前 9 次 `401`，第 10 次起 `429` 且带 `Retry-After`。
**清零验证**：用真 key 打一次 `200` 后，再数一次错误应从 `401` 重新开始。
**回归验证**：限速不得作用于成功的请求（Viewer 连续 20 次 `200`，不应出现 429）。

#### G1-5 控制台密钥面板（人工 + 两条反向检查）

人工：打开 `http://127.0.0.1:9800/` → 输入 root key → 密钥管理面板 → 创建 viewer key（弹一次性明文框）→ 关闭弹窗后列表只剩前缀 → 吊销后状态变 revoked。

反向检查（防"改前端没重新构建"）：
```bash
grep -c "atpc_sk_" crates/server/frontend/dist/index.html     # 期望 0（前端不得内置任何 key）
# 改一处 src 文案但不跑 build：
curl -s http://127.0.0.1:9800/ | grep -c "<新文案>"            # 期望 0  → 证明确实走 include_str!
npm --prefix crates/server/frontend run build
curl -s http://127.0.0.1:9800/ | grep -c "<新文案>"            # 期望 1  → 证明构建后才生效
```

#### G1-6 审计事件齐全且不含明文

```bash
jq -r '.action' "$DIR/audit.jsonl" | sort | uniq -c | sort -rn
jq -r 'select(.action|test("^(key:|AUTH_FAILED|insecure_mode)")) | .action' "$DIR/audit.jsonl" | sort -u
grep -c "atpc_sk_" "$DIR/audit.jsonl"        # 期望 0
```
**判据**：`AUTH_FAILED` / `insecure_mode` / `key:create` / `key:revoke` / `key:rotate` 五类都出现过；明文计数为 0。

---

### 3.4 G2 — M2 检验程序

```bash
# ① 签发设备 Token
DV=$(curl -s -X POST -H "Authorization: Bearer $A" -H 'Content-Type: application/json' \
  -d '{"name":"laptop-01","kind":"dv","device_id":"laptop-roeh3p85-b6a918"}' \
  http://127.0.0.1:9800/api/keys | jq -r .plaintext)
echo "$DV" | grep -cE '^atpc_dv_'      # 期望 1
```
| # | 场景 | 期望 | 判据 |
|---|---|---|---|
| ② | `/ws` 注册不带 token | 拒绝 | `RegisterAck.success=false`，连接关闭 |
| ③ | 带错误 token | 拒绝 | 同上 |
| ④ | 带 `§DV` 且 `info.terminal_id` 匹配 `device_id` | 通过 | `success=true`；`GET /api/terminals` 出现该 terminal |
| ⑤ | 带 `DV` 且 `terminal_id` 不匹配 | 拒绝 | `success=false`（这是"token 被复制到另一台机器"的防线） |
| ⑥ | `DV` 打 MCP（`GET /api/terminals`） | 403 | 设备令牌不能操作控制台 |
| ⑦ | `sk` key 打 `/ws` | 拒绝 | 操作凭据不能冒充设备 |
| ⑧ | 兼容：`--auth-token legacy` 启动 + agent 用 legacy token 注册 | 通过，且日志有弃用 WARN | 保 F3 全绿 |
| ⑨ | 同一 IP 连续 10 次注册失败 | 第 10 次起 429/拒绝 | 与服务端限速日志一致 |

工装复用：`crates/server/tests/ws_gateway_test.rs` 已有 `connect_client` / `send_json` / `receive_server_message` 三个 helper，新用例直接沿用（新增文件 `p2_device_token_test.rs`）。

文档一致性：
```bash
grep -rn "auth_token" agent_config.toml crates/agent/src/config.rs deploy/ README.md | head -20
```
**判据**：凡提到 `auth_token` 的地方都同时给出设备 Token 的替代说明，不留"只写了旧方式"的文档。

---

### 3.5 G3 — M3 检验程序

```bash
# ① 非回环 + 无 TLS → 拒绝
start --host 0.0.0.0; sleep 1; wait "$(cat "$DIR/pid")"; echo "exit=$?"       # 期望 1
start --host 0.0.0.0 --allow-plaintext; sleep 1                                # 期望启动且 stdout 有 WARN
jq -r 'select(.action=="insecure_mode")' "$DIR/audit.jsonl" | head -1          # 期望有记录

# ② 会话 cookie
curl -s -D - -o /dev/null -X POST http://127.0.0.1:9800/api/auth/login \
  -H 'Content-Type: application/json' -d "{\"key\":\"$KEY\"}" | grep -i "^set-cookie"
# 期望含 HttpOnly、SameSite=Strict；浏览器 DevTools 里 document.cookie 看不到它

# ③ 每 key 配额
curl -s -D - -o /dev/null -H "Authorization: Bearer $V" http://127.0.0.1:9800/api/terminals | grep -iE "^x-ratelimit"
# 期望 Limit / Remaining / Reset 三个头齐全

# ④ 明文配置迁移
start --host 0.0.0.0 --migrate-legacy-credentials --config server_config.example.toml
jq -r '.keys[] | "\(.name) \(.role)"' "$DIR/keys.json"      # 期望 [roles] 的每一条都成了记录
# 迁移后再普通启动一次 → 若配置里仍有明文 [roles] 则拒绝启动
```

---

### 3.6 门禁真实性总则

项目既有约定（两板斧），每个里程碑的**门禁级**判据都必须走一遍：

**板斧一 —— 改坏必须变红。** 对每条门禁断言，临时注入一个反向改动，确认对应检查确实 FAIL，再改回确认 PASS。最低覆盖集：

| 里程碑 | 必做的反向注入 |
|---|---|
| M0 | `validate_boot_gate` 的 `Err` → `Ok(())`；`/health` 回环判定 → 恒 `true`；`sse_handler` 的 endpoint → 恢复 `?token=` |
| M1 | "最后一个 admin 不可吊销"护栏 → 允许；`subtle` 比对 → 改回 `==`；限速阈值 `10` → `10000` |
| M2 | `device_id` 匹配校验 → 恒通过 |
| M3 | `--allow-plaintext` 判定 → 恒 `true` |

**板斧二 —— 真输入必须真的走失败路径。** 错误凭据要真的打到网络上（不是 mock），并同时断言"状态码 + 审计记录"两件事成立：

```bash
curl -s -o /dev/null -w "%{http_code}\n" -H "Authorization: Bearer <错 key>" "http://$LAN:9800/api/terminals"  # 401
jq -r 'select(.action=="AUTH_FAILED") | .status' "$DIR/audit.jsonl" | tail -1                                  # 与上一条同一次请求
```

---

## 4. 验收基线矩阵

`门禁` = 该里程碑必须过，且后续里程碑不得回退；`新增` = 本里程碑引入的检查；`回归` = 既有检查保持；`—` = 不适用。

| 检查项 | M0 | M1 | M2 | M3 |
|---|---|---|---|---|
| 全量 `cargo test --workspace` 0 failed（F4） | 门禁 | 门禁 | 门禁 | 门禁 |
| F1/F2/F3 三个既有测试文件零改动全绿 | 门禁 | 门禁 | 门禁 | 门禁 |
| 前端 `npm test` + `lint` | 回归 | 门禁 | 门禁 | 门禁 |
| 非回环 + 无凭据 → exit 1（G0-2） | 门禁 | 门禁 | 门禁 | 门禁 |
| 回环 + 无凭据 → 可启动（G0-3） | 门禁 | 门禁 | 门禁 | 门禁 |
| bootstrap 双通道一致 + 0600 + 幂等（G0-4） | 门禁 | 门禁 | 门禁 | 门禁 |
| `/health` 回环豁免 + 载荷最小化（G0-5） | 门禁 | 门禁 | 门禁 | 门禁 |
| 容器 `HEALTHCHECK` = healthy（G0-6） | 门禁 | 门禁 | 门禁 | 门禁 |
| 静态壳可达、数据全门禁（G0-7） | 门禁 | 门禁 | 门禁 | 门禁 |
| SSE 无 `token=` 泄漏（G0-8） | 门禁 | 门禁 | 门禁 | 门禁 |
| `deploy/verify.sh` 门禁下全 PASS（G0-9） | 门禁 | 门禁 | 门禁 | 门禁 |
| `keys.json` 0600 + 无明文/无 hash 外泄（G1-2） | — | 门禁 | 门禁 | 门禁 |
| 吊销即时 401 / 轮换 grace 语义（G1-2） | — | 门禁 | 门禁 | 门禁 |
| 最后 admin 不可吊销 → 409（G1-2④） | — | 门禁 | 门禁 | 门禁 |
| 9 个 handler 角色矩阵（G1-3） | 可选追加 | 门禁 | 门禁 | 门禁 |
| 失败限速 10 次 → 429（G1-4） | — | 门禁 | 门禁 | 门禁 |
| 审计五类 action 齐全且无明文（G1-6） | 新增 2 类 | 门禁 | 门禁 | 门禁 |
| 控制台一次性明文 + 构建一致性（G1-5） | — | 门禁 | 门禁 | 门禁 |
| 设备 Token 与 `device_id` 绑定（G2 ②–⑦） | — | — | 门禁 | 门禁 |
| `auth_token` 兼容路径仍可注册（G2 ⑧） | 回归 | 回归 | 门禁 | 门禁 |
| 非回环无 TLS → 拒绝（G3 ①） | — | — | — | 门禁 |
| 会话 cookie HttpOnly（G3 ②） | — | — | — | 门禁 |
| 每 key 配额与响应头（G3 ③） | — | — | — | 门禁 |
| 明文配置迁移 + 迁移后拒绝明文（G3 ④） | — | — | — | 门禁 |
| 门禁真实性两板斧（§3.6） | 门禁 | 门禁 | 门禁 | 门禁 |

---

## 5. 回归风险与回滚

| 风险 | 影响 | 处理 |
|---|---|---|
| **默认部署行为变化** | `docker-compose.yml` + `server_config.example.toml` 当前是开放态；M0 后首启会生成 root key、控制台开始要求输入 | 用户已确认接受（设计文档 §4.1）。仍需 README + 启动日志醒目提示；`--insecure-no-auth` 保留旧行为 |
| **`/health` 语义变化** | 回环免凭据；非回环需凭据；未鉴权载荷字段减少 | 已确认接受。`Dockerfile` HEALTHCHECK 零改动（D8）；外部监控若走非回环需补 header。**`deploy/verify.sh` 必须同步改（D5），否则脚本自己会 FAIL** |
| **`deploy/deploy.sh` standalone** | 补参数后行为变化：meta/audit 不再落 CWD | 这是修复而非破坏。需在 README 注明落盘位置 |
| **`AT_PC_LOG_DIR` → `AT_PC_LOG_FILE`** | 若已有用户依赖旧变量名 | 旧名本来就是死变量（代码不读），因此无实际破坏面。为稳妥可在代码里同时接受 `_DIR`（取其中 `at-pc-server.log`） |
| **F1 单元测试** | `p1_auth_rbac_call_id_test.rs:96-100` 断言 default 配置对任意非空 token 回 Admin | **不改断言**：回环限制放在调用方（§1.6 F1）。若实现时改成动函数体，该测试会红 → 说明走错了路线 |
| **前端构建** | 改 `src` 不重建 → 运行时无变化，且容易误判"改了没用" | §0.3 强制约束 + G1-5 的反向检查 |
| **审计落盘体积** | 新增 5 类事件，`audit.jsonl` 体积上升（轮转已在 `audit.rs` 内实现） | 失败路径做同 IP 同分钟合并计数，避免被刷爆 |
| **容器重建与 key 持久性** | 卷保留则不重新 bootstrap；卷删除则 root key 变化，旧 key 全部失效 | D7 写入 README；G0-6 用 `down -v` 明确测"干净卷"场景 |

**三层回滚**：
1. `--insecure-no-auth` —— 不改代码即恢复旧行为（M0）；
2. 删除 `keys.json` —— 回到 M0 行为（M1）；
3. `--auth-token` 兼容路径 —— 设备侧回到旧校验（M2）。

---

## 6. 待即时拍板项（不阻塞 M0 开工）

1. **A7（9 个 handler 角色校验）是否提前到 M0？**
   它是 9 处 `Extension<Role>` + 一个角色判断，技术上零依赖、可以直接做。但 M0 的章程是"挡住公网裸奔"，而这些 handler 在 M0 之后已需持有效 key 才能到达。
   **我的倾向：留在 M1。** 理由：M0 要能"一步上线、一步回滚"；提前会把 M0 的测试文件扩成两张矩阵，拖慢最关键的修复。若你希望先关掉"Viewer 能拉实时桌面画面"这条，我可以把它作为 **M0-OPT** 单独塞进 M0 的尾巴（一个小提交）。
2. **`api_get_logs` 定 `Admin` 还是 `Viewer`？**
   设计文档定 `Admin`（日志可整段导出、可离线检索，泄漏后危害高于一张截图）。但 `frame`/`raw` 已给了 `Viewer`，而画面里可能正显示着同一批日志，存在不对称。倾向保持 `Admin`，做到 G1-3 时再最终确认。
3. **`bootstrap_key.txt` 的生命周期**：(a) 只提示人工删除 + 启动横幅给精确命令；(b) 首次成功认证后自动删除；(c) 加 TTL 自动删除。倾向 **(a)**（(b) 的风险是运维还没抄下 key 就登了一次控制台，文件消失）。M0-2 落地时确认。
4. **设备 Token 的绑定强度**：绑 `terminal_id`（agent 自报，可被改写）还是绑机器指纹（需 agent 采集，跨平台成本高）？倾向先绑 `terminal_id`，M2 实施时若发现 agent 侧 `terminal_id` 可被随意伪造，再加指纹。

---

## 7. 附录：变更影响面（按里程碑）

| 里程碑 | 新增文件 | 修改文件 |
|---|---|---|
| M0 | `crates/server/src/bootstrap.rs`、`tests/p0_boot_gate_test.rs`、`tests/p0_health_loopback_test.rs`、`deploy/verify_gate.sh`(建议) | `Cargo.toml`(workspace)、`crates/server/Cargo.toml`、`config.rs`、`main.rs`、`lib.rs`(导出)、`shutdown.rs`(2 处)、`mcp/mod.rs`、`mcp/dashboard.rs`、`audit.rs`、`deploy/deploy.sh`、`deploy/verify.sh`、`Dockerfile`、`docker-compose.yml`、`server_config.example.toml`、`README.md` |
| M1 | `crates/server/src/keystore.rs`、`crates/server/src/rate_limit.rs`、`tests/p1_keystore_test.rs`、`tests/p1_keystore_lifecycle_test.rs` | `Cargo.toml`(workspace)、`crates/server/Cargo.toml`、`config.rs`、`mcp/mod.rs`、`mcp/dashboard.rs`、`audit.rs`、`frontend/src/*`、`frontend/dist/index.html`(重建)、`frontend/tests/api.test.js`、`deploy/deploy.sh`、`docker-compose.yml`、`server_config.example.toml` |
| M2 | `tests/p2_device_token_test.rs` | `keystore.rs`、`ws/handler.rs`、`rate_limit.rs`、`crates/agent/src/config.rs`、`agent_config.toml`、`deploy/deploy.sh`、`docker-compose.yml`、`README.md` |
| M3 | `crates/server/src/migrate.rs`、`tests/p3_hardening_test.rs` | `config.rs`、`main.rs`、`mcp/mod.rs`、`rate_limit.rs`、`mcp/tools.rs`(可选)、`frontend/src/*` |

---

## 8. 下一步

1. 你先看 §6 的 4 项（尤其第 1 项：A7 是否提前到 M0）。
2. 我按 M0 的 15 项任务清单开工，任务粒度就是"每项一个可编译的提交"，收尾跑 §3.2 的 G0-1…G0-10。
3. M0 完成后我会用同一份检验程序出一份 "M0 落地验证报告"，把每条判据的实测值填进 §3.2 的表格（而不是只写"已通过"）。
