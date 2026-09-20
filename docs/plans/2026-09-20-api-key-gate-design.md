# at-pc 门禁系统（API Key Gate）落地方案设计

- 日期：2026-09-20
- 范围：`crates/server`（HTTP/MCP 网关 + Web 控制台）、`crates/agent`（设备接入）、`crates/protocol`（Role 契约）、部署清单
- 基线：`HEAD = 855b082`（本地 `main`，远端 `origin/main` 停在 `9c41689`）
- 关联文档：`2026-09-20-perception-channel-audit.md`（感知通道）、`2026-09-16-added-content-verification.md`（内容核对，冲突以此为准）
- **实施落地以 `2026-09-20-api-key-gate-implementation-plan.md` 为准**：该文件给出文件级/行级任务分解、每个里程碑的可执行检验程序，以及修改项与现状台账。本文保留"为什么这么做"的完整论证；两者冲突时以实施计划为准（例如下文 §5.5 与 §6 提到的 `into_make_service` 实际是**两处**，实施计划 M0-6 已修正）。

---

## 1. 背景与威胁模型

`at-pc` 新增 Linux target 后具备部署到公网服务器的现实可能。当前默认配置是**敞开**的，一旦 `--host 0.0.0.0` 暴露到公网，攻击者可以直接：

| 攻击面 | 后果 |
|---|---|
| 未鉴权调用 MCP 工具 | 在受控 PC 上执行任意命令、读任意文件、截屏、模拟键鼠（`exec_powershell` / `write_text_file` / `send_desktop_input`） |
| 未鉴权注册假 Agent | 冒充终端上线，把操作流量引到自己机器；或污染终端列表干扰运维 |
| 未鉴权读 `/api/*` | 拉走审计日志、服务器运行日志（可能含凭据）、终端元数据、桌面实时画面 |
| 未鉴权读 `/static/*` 与首页 | 拿到前端壳（当前 `/` 反而裸奔，见 §3） |

**核心判定**：这不是"是否需要门禁"的问题，而是"门禁已有半成品但默认态敞开、且覆盖面有洞"的问题（详见 §3）。本方案的目标是**把门禁从"可选配置项"变成"默认强制的启动前置条件"**。

信任域拆分（本方案的关键设计）：

| 域 | 入口 | 凭据类型 | 威胁模型 |
|---|---|---|---|
| **操作域** | :9800 HTTP/MCP + Web 控制台 | 客户端 API Key（带角色） | 操作者/上游 AI 被盗用 → 需要角色分级 + 吊销 |
| **设备域** | :9801 WebSocket | 设备 Token（每设备一签） | 被控 PC 被替换/伪造上线 → 需要按设备绑定 + 吊销 |

两域凭据**互不通用**，一台设备失陷不牵连操作端凭据，反之亦然。

---

## 2. 调研结论：Rust 侧"官方推荐"的准确表述

**Rust 没有官方认证规范。** 没有类似 `Node.js`、`Spring Security`、`OWASP ASVS` 那样被语言官方背书的认证标准。权威依据只有两条线：

1. **axum 官方文档**（`docs.rs/axum/latest/axum/middleware/index.html`）——明确了中间件与 extractor 两种模式，以及"mutation via request extensions 把身份传给 handler"的官方姿势。这是实现层面的权威。
2. **RustCrypto 生态 + OWASP API Security Top 10 (2023)**——算法与生命周期层面的权威。OWASP 把 Broken Authentication 列为 API2（第二位）。

### 2.1 生成的共识

- 用 **CSPRNG**，禁止时间戳 / PID / 计数器 / 自拼 UUID 派生。
- 熵 ≥ **128 bit**，长寿命密钥用 **256 bit**。`getrandom` 官方文档明确"假设系统始终提供高质量密码学随机数（硬件熵源优先）"。
- 传输编码用 hex 或 URL-safe base64，**不得为了"好看"截断熵**。

### 2.2 比对的共识

- 必须用 **`subtle::ConstantTimeEq`** 做常量时间比较。`==` 在字节级提前返回，构成时序侧信道。这是 Rust 社区对此最一致的结论（RustCrypto 官方 `subtle` crate 就是为此而存在）。
- 常量时间比较**不能替代哈希**。

### 2.3 存储的共识（本方案有一处刻意的偏离，见 §5.3）

- 服务端**不需要恢复明文**，只存哈希。明文**只在签发的瞬间展示一次**。
- 推荐算法：Argon2id / bcrypt / BLAKE3-HMAC（OWASP 表述为 "password-safe hashing algorithm"）。

### 2.4 传递的共识

- 只走 header：`Authorization: Bearer <key>` 或 `X-API-Key: <key>`。
- **禁止 query 参数**。理由被反复强调：query 会进 server access log、浏览器历史、Referer、CDN 日志——密钥"静默且持久地"泄漏。

### 2.5 生命周期的共识

- **可识别前缀**（`sk_live_` / `pk_test_` 风格），便于人眼区分与密钥扫描器（GitHub secret scanning）自动识别。
- 最小权限范围（scope / 环境 / IP）。
- 定期轮换 + 立即在泄漏后轮换；轮换时**双密钥重叠**（新旧并存 24–72 h）避免停机。
- 每密钥限速 + 记录 `last_used_at`。
- 审计密钥的创建 / 读取 / 轮换 / 吊销 / 使用；**日志里绝不出现密钥本身**。

---

## 3. 现状审计

### 3.1 已有的（不是从零开始）

| 组件 | 位置 | 状态 |
|---|---|---|
| 凭据配置 | `config.rs:95 auth_token`、`config.rs:119 roles: HashMap<String, Role>` | 有 |
| 凭据 → 角色解析 | `config.rs:170 get_role_for_token` | 明文 `==` 比对 |
| HTTP 鉴权中间件 | `mcp/mod.rs:132 auth_middleware` | 有，但覆盖不全 |
| 凭据提取 | `mcp/mod.rs:174 extract_auth_token` | header / cookie / query 三源 |
| 角色契约 | `protocol/src/tools.rs:10 Role{Viewer<Operator<Admin}` + `required_role` | 完整 |
| 工具级 RBAC 强制 | `router.rs:539` in `dispatch_tool_call_with_role` | 有，且记 `DENIED` 审计 |
| TLS / mTLS | `config.rs:110-116`、`tls.rs` | 有（可选） |
| 密钥脱敏 | `audit.rs:325 redact_token` | 有（前 3 字符 + `***`） |

### 3.2 四个入口的覆盖面

| 入口 | 现状 | 问题 |
|---|---|---|
| `:9800 /mcp` `/sse` `/messages` | `mcp/mod.rs` 内联校验 | 同一套逻辑在 `health_handler` / `sse_handler` / `messages_handler` 三处各写一遍，易漂移 |
| `:9800 /health` | 内联校验 | 要求凭据 → **Dockerfile 的 `HEALTHCHECK curl -f http://localhost:9800/health` 会 401**，容器被判 unhealthy |
| `:9800 /api/*`、`/static/*` | `dashboard.rs:79,86` 挂中间件 | ✓ |
| `:9800 /`、`/dashboard`、`/index.html` | `dashboard.rs:92-94` | **裸奔**（前端壳无任何凭证即可获取） |
| `:9801 /ws` | `ws/handler.rs:327` | 只比对单个 `config.auth_token`；**该值为空时任何设备可注册**；`roles` 里的 key 对 Agent 不可用 |

### 3.3 未做角色校验的 handler（9 个）

`api_get_logs:106`、`api_list_terminals:227`、`api_update_terminal_meta:240`、`api_list_pending_calls:657`、`api_list_terminal_pending_calls:663`、`api_start_desktop_stream:680`、`api_stop_desktop_stream:706`、`api_get_desktop_frame:717`、`api_get_desktop_frame_raw:737`

其中风险最高的是：
- **`api_get_logs`**：服务器运行日志，可能含凭据片段、内网地址、上游调用内容，Viewer 即可读。
- **`api_get_desktop_frame_raw`（`.jpg`）**：Viewer 即可拉取实时桌面画面。
- **`api_start_desktop_stream`**：Viewer 即可开启持续推流（既是信息泄漏也是资源消耗）。

### 3.4 四个根因

1. **开放态回落**（`config.rs:188-191`）：`auth_token` 与 `roles` 双空 → `get_role_for_token` 返回 `Some(Admin)`。这与 `server_config.example.toml` 中 `[roles]` 全被注释、`docker-compose.yml` 把 9800/9801 发布到 `0.0.0.0` 组合起来，就是"公网无门禁"的完整成因。
2. **明文比对**：`get_role_for_token` 用 `==`；`ws/handler.rs:330` 也是 `!=`。均非常量时间。
3. **query 通道为主路径**：`extract_auth_token` 接受 `?token=` / `?pin=`，且 `sse_handler`（`mcp/mod.rs:321-327`）主动把 `?token=<明文>` 拼进 SSE 的 endpoint 事件；前端 `api/index.js` 从 URL 读 token 并写入 sessionStorage。密钥全程走 URL。
4. **无密钥生命周期**：没有生成 / 持久化 / 列表 / 吊销 / 轮换 / 过期 / `last_used` / 限速。密钥要么硬编码在 TOML 里，要么靠 `--auth-token` 手工传。

---

## 4. 已确认的四项设计决策

> 状态：**2026-09-20 用户确认，全部锁定**。以下四项不再作为开放问题，实施时按此执行。

| # | 决策点 | 结论 | 状态 |
|---|---|---|---|
| 1 | 密钥存储形态 | **只存哈希**。明文仅在签发瞬间返回一次 | 已确认 |
| 2 | Agent 是否纳入同一体系 | **分离**。客户端 API Key 与设备 Token 两套凭据，互不通用 | 已确认 |
| 3 | "无门禁"时的行为 | **硬拒绝启动**：非回环监听且无任何凭据 → 退出并打印解法 | 已确认 |
| 4 | 首启 root key 下发通道 | **stdout + `0600` 文件双通道**（容器靠 `docker logs` 也能取到） | 已确认 |

### 4.1 连带确认的三项（原为待定项）

| 决策点 | 结论 | 对方案的影响 |
|---|---|---|
| 桌面帧接口 `api_get_desktop_frame` / `_raw` 的最低角色 | **`Viewer`** | 任何有效只读凭据都能获取实时桌面画面。**含义**：`Viewer` 角色在本系统中**不再等同于"无敏感信息"**，它能看到被控 PC 的屏幕。若后续要把 Viewer 发给外部只读方，需重新评估此条 |
| 现有 docker-compose 部署首启后需输入 key | **接受** | §7 的"默认部署行为变化"不再是风险项，而是预期行为。README / 部署文档必须同步说明 |
| `/health` 回环豁免策略 | **接受豁免** | `Dockerfile` 的 `HEALTHCHECK` 零改动；接受"同主机其他进程可无凭据探活"的代价 |
| 是否保留"人工短 token 走 Argon2id"的双算法路径 | **不需要** | KeyStore 只实现单算法 `sha256-salt-v1`，不做算法分支。§5.3 的偏离成为最终结论，§9 对应条目降级为"已否决" |

**由此产生的唯一遗留约束**：因为只做单算法且不做人工短 token 路径，**系统所有凭据都必须由服务端用 CSPRNG 生成**。不可支持"运维手填一个自己想的字符串当 key"。控制台与文档须明确写出这一点，否则用户会尝试粘贴一个自设的弱口令，得到一个永远不会匹配的 key。

---

## 5. 落地方案设计

### 5.1 密钥格式

```text
atpc_<kind>_<id>_<secret>

kind    = sk（客户端 API Key） | dv（设备 Token）
id      = 8 字符 Crockford base32（小写），来自 5 字节 CSPRNG
secret  = 43 字符 base64url-no-pad，来自 32 字节 CSPRNG（256 bit）
```

示例：`atpc_sk_7f3ak9m2_Xy7QmV2pW8tR4nL6bC0dF5gH1jK3sZ9uA2eI4oP6q`

设计要点：
- **前缀可识别**（`atpc_sk_` / `atpc_dv_`）：可写进密钥扫描规则；人眼可分辨类型。
- **`id` 作为索引**：校验时先用 `id` 做 O(1) 哈希表查找拿到唯一候选，再对**单个**候选做常量时间比对。避免"遍历所有密钥逐个比对"（那既慢又放大时序面）。
- `id` 不要求保密，只要求唯一；`secret` 是全部安全性所在。

### 5.2 数据模型

`<data_dir>/keys.json`（`0600`，原子写：写临时文件 + `rename`）：

```json
{
  "version": 1,
  "keys": [
    {
      "id": "7f3ak9m2",
      "prefix": "atpc_sk_7f3ak9m2",
      "kind": "sk",
      "role": "admin",
      "scopes": ["console", "mcp"],
      "name": "bootstrap-root",
      "salt": "base64(32B)",
      "hash": "base64(sha256(salt || secret))",
      "hash_algo": "sha256-salt-v1",
      "created_at": "2026-09-20T14:00:00Z",
      "created_by": null,
      "expires_at": null,
      "revoked_at": null,
      "last_used_at": null,
      "last_used_ip": null,
      "device_id": null
    }
  ]
}
```

字段说明：
- `scopes`：客户端 Key 的细粒度开关（`console` / `mcp`），可在角色之上再收窄。
- `created_by`：签名链（哪个 key 建的这个 key），用于审计溯源。
- `device_id`：仅 `kind=dv` 使用。非空时**绑定该 terminal_id**，注册时的 `info.terminal_id` 必须匹配，否则拒绝。这解决"一个设备 token 被复制到另一台机器上顶替上线"。
- `hash_algo`：为后续算法迁移留出空间（遵循 `kerkour.com/api-keys` 建议的 version 字段思路）。
- 不存 `last_used_ip` 之外的任何请求内容。

复用 `meta_store.rs` 的既有模式（后台写线程 + 原子 flush + `shutdown_persistence` 集成）来实现 KeyStore 的持久化，避免引入新的写路径。

### 5.3 哈希算法选择（对 §2.3 的一处刻意偏离，需明确说明）

**推荐 `SHA-256(salt || secret)`，而非 Argon2id。**

理由：
- Argon2id / bcrypt 的设计目标是抵抗**低熵口令**的字典与暴力破解——它们的"慢"是特性（拉长每次猜测成本）。
- 本方案的 `secret` 是 **256 bit 均匀随机**，猜测空间是 2²⁵⁶，不存在字典攻击。加盐后彩虹表亦无效。
- 反过来，Argon2id 单次验证约 50–100 ms，而这是**每个 HTTP 请求都要走的热路径**（MCP 工具调用、控制台 2 s 轮询 `/api/terminals`）。用 Argon2 等于给服务端装一个自伤型 DoS 放大器。
- 这是主流实践：GitHub PAT、Stripe restricted key 这类高速路径 token 都走 SHA-256 系列；Argon2 留给人类口令。

**可选加固：server pepper。** 额外生成 32 字节 pepper 存于 `<data_dir>/keys_pepper.bin`（`0600`），参与哈希：`sha256(pepper || salt || secret)`。收益是"只泄漏 `keys.json` 的备份/导出无法离线爆破"。成本是 pepper 丢失则全部密钥不可用（需在文档中写明"两者必须一起备份"）。**建议作为可选项，默认关闭。**

依赖：`sha2`（RustCrypto）+ `subtle` + `getrandom`。**不引入 `rand`**（`rand` 在此前优化中已被有意移除，见记忆 `MEMORY.md` §归因铁律 5），也不引入 `uuid`（`id` 直接用随机字节编码）。

### 5.4 校验流程

```text
1. 提取凭据（header → cookie → query，保持现有优先级）
2. 解析格式 → (kind, id, secret)。不可解析 → 401（统一错误文案，不透露原因）
3. KeyStore 按 id 查找
   ├─ 不存在 → 401
   ├─ revoked_at 非空 → 401
   ├─ expires_at 已过 → 401
   └─ kind 与入口不匹配（如 dv 用来说 MCP） → 403
4. 重算 hash，subtle::ConstantTimeEq 常量时间比对 → 不等则 401
5. 检查 scopes 是否覆盖当前入口
6. 刷新 last_used_at / last_used_ip
   └─ 节流：同一 key 最快 60 s 落盘一次（内存态先更新），避免写放大
7. 构造 Principal { key_id, name, kind, role, scopes } 插入 request extensions
8. 进入 RBAC（复用现有 is_tool_allowed_for_role）
```

失败路径统一记录审计 `status: "AUTH_FAILED"`，仅写**可解析出的 prefix**（复用 `audit.rs:325 redact_token`），绝不写密钥明文。

### 5.5 入口改造（逐个）

**统一 auth layer。** 把 `auth_middleware` 提升为 :9800 的全局 `route_layer`，用**白名单**表达豁免，消除三处内联重复：

```rust
let app = Router::new()
    .merge(dashboard::create_dashboard_router(state.clone()))
    .route("/", post(messages_handler))          // /mcp /sse /messages 不再各自校验
    .route("/mcp",  post(messages_handler).get(sse_handler))
    .route("/sse",  get(sse_handler).post(messages_handler))
    .route("/messages", post(messages_handler).get(sse_handler))
    .route("/health", get(health_handler))
    .layer(cors)
    .route_layer(middleware::from_fn_with_state(state.clone(), auth_layer))
    .with_state(state);
```

`auth_layer` 内维护豁免表：

| 路径 | 豁免规则 |
|---|---|
| `OPTIONS *` | 放行（CORS 预检，沿用 `mcp/mod.rs:138` 现有行为） |
| `/health` | **来源为回环地址**时放行，返回**精简载荷**；非回环必须带凭据 |
| `/`、`/dashboard`、`/index.html` | 放行，但**只返回不含任何数据的登录壳** |

> `/` 的处理说明：浏览器首次访问时还没有 key，无法要求凭据。正确做法是"壳公开、数据全门禁"，而不是"整个前端裸奔"。当前 `/api/*` 已经在门禁内，所以实际泄漏面仅是 HTML/CSS/JS 静态内容——但把规则写明确比留一个隐式缺口好。

**`/health` 与 Docker HEALTHCHECK 的冲突（重要）**：容器内 `curl localhost:9800/health` 的来源**就是回环**，因此"回环免凭据"策略让 `Dockerfile` 的 `HEALTHCHECK` **无需任何修改**即可继续工作。这需要把 `shutdown.rs:173` 的 `app.into_make_service()` 换成 `app.into_make_service_with_connect_info::<SocketAddr>()`，并在 handler 里取 `ConnectInfo<SocketAddr>`。

- 未鉴权（回环）载荷只回 `{"status":"ok"}`，**去掉** `connected_terminals` / `ws_port` / `mcp_port` / `version`（现状是这四项全给未鉴权访问者，属信息泄漏）。
- 已鉴权载荷保持现状完整字段，避免破坏现有监控脚本。
- 反向代理场景下回环判断会失效（`X-Forwarded-For` 不可信），因此**带有效凭据一律放行**，作为代理后的正解。
- 逃生阀：`AT_PC_HEALTH_INSECURE=1` 强制 `/health` 公开（供不支持自定义 header 的健康检查使用），启动时打 WARN。

**`sse_handler` 的 token 泄漏修复**（`mcp/mod.rs:321-327`）：不再把 `?token=<明文>` 拼进 endpoint 事件。SSE 客户端本就要在后续 `POST /messages` 里带同一凭据，endpoint URI 直接返回 `/messages` 即可。

**query 通道的降级处理**：
- `extract_auth_token` 的签名与行为**保持不变**（`tests/p1_auth_rbac_call_id_test.rs:18,41` 把优先级、别名大小写、百分号解码的语义固化成了断言，改动会红）。
- 新增一个包装 `extract_credential(headers, query) -> Option<(String, CredentialSource)>`，把来源带出来。
- 来源为 `Query` 时：记审计 `warn`、响应加 `Warning: 199 - "API key in query string is deprecated"`、并在文档中标注为过渡兼容。**不直接移除**，因为部分 MCP 客户端只能配 URL。

**`:9801 /ws`（设备域）**：
- `ws/handler.rs:327` 的 `Register` 分支改为：优先校验 `kind=dv` 的设备 Token → 命中 KeyStore 则通过（若 `device_id` 非空，要求 `info.terminal_id` 匹配）；未配置任何设备 Token 时回退到兼容路径（见下）。
- `--auth-token` / `config.auth_token` **保留为兼容凭据**（等价 admin），但启动时打弃用 WARN。
- 注册失败按来源 IP 限速（见 §5.8）。

**补上 9 个 handler 的角色校验**（建议矩阵）：

| Handler | 建议最低角色 | 理由 |
|---|---|---|
| `api_get_logs` | `Admin` | 日志可能含敏感内容 |
| `api_list_terminals` | `Viewer` | 只读清单 |
| `api_update_terminal_meta` | `Operator` | 写元数据 |
| `api_list_pending_calls` / `api_list_terminal_pending_calls` | `Viewer` | 只读 |
| `api_start_desktop_stream` / `api_stop_desktop_stream` | `Operator` | 资源占用 + 画面访问 |
| `api_get_desktop_frame` / `..._raw` | `Viewer` | 已确认。画面归为只读监控能力（见 §4.1 的连带含义） |

> 这一项可以做得更体系化：把 handler→工具名 的映射塞进 `is_tool_allowed_for_role` 的既有分支（`config.rs:72`），让 HTTP 层与 MCP 层共用同一张权限表，避免两处漂移。

### 5.6 首启引导 + 硬拒绝启动

启动序列（`main.rs::run_server_pipeline` 之前）：

```text
1. 载入 KeyStore（文件不存在 → 空表，不报错）
2. 若空表 且 未指定 --no-bootstrap：
   ├─ 生成 root key（kind=sk, role=admin, name="bootstrap-root"）
   ├─ 明文打印到 stdout（醒目框线 + 使用说明）
   ├─ 明文写入 <data_dir>/bootstrap_key.txt（0600）
   └─ 日志 WARN："请立即保存并从磁盘删除该文件"
3. validate_boot_gate(config, &keys)：
   is_loopback = listen_host ∈ {127.0.0.1, ::1, localhost}
   has_creds   = !keys.is_empty() || auth_token.is_some() || !roles.is_empty()
   if !is_loopback && !has_creds && !insecure_no_auth → 打印解法并 exit(1)
4. 非回环 && 未启用 TLS → WARN（P3 起升级为拒绝，可用 --allow-plaintext 豁免）
```

`bootstrap_key.txt` 的内容为明文 + 三行删除提示，权限 `0600`（Unix）。Windows 下走 ACL 收紧（`icacls` 等价逻辑）或至少保证同用户可读。

**逃生阀**：`--insecure-no-auth` 显式声明"我确实要开放模式"（例如整套服务已置于 VPN / 堡垒机之后）。此开关必须显式传参，且启动时打印大号 WARN、每条审计记录带 `insecure: true` 字段。这样"敞开的部署"从**默认**变成**显式且有痕迹的选择**。

### 5.7 控制台密钥管理

**REST（`/api/keys`，全部要求 `Admin`）**

| 方法 | 路径 | 说明 |
|---|---|---|
| `GET` | `/api/auth/me` | 返回当前 `Principal`（任意角色可调，供控制台显示身份） |
| `GET` | `/api/keys` | 列表：`id / prefix / kind / role / scopes / name / created_at / expires_at / last_used_at / revoked`。**永不返回明文或 hash** |
| `POST` | `/api/keys` | 创建。body `{name, kind, role, scopes?, expires_in_days?, device_id?}`。**响应含一次性明文**，附 `Cache-Control: no-store` |
| `DELETE` | `/api/keys/:id` | 吊销（软删除：写 `revoked_at`）。已吊销的记录可用 `?purge=true` 硬删 |
| `POST` | `/api/keys/:id/rotate` | 轮换：签发新记录并把旧记录 `expires_at = now + grace`（默认 24 h，可传 `grace_hours`） |

**两条必须守住的护栏**：
1. **不能吊销/降级最后一个 Admin 级客户端 Key** —— 否则运维会把自己永久锁在门外。校验失败返回 409 并给出明确理由。
2. 创建/吊销/轮换全部写审计（`action: "key:create" | "key:revoke" | "key:rotate"`，含 `created_by` 溯源）。

**MCP 侧的建议**：**不要**把"创建/吊销 Key"暴露为 MCP 工具。MCP 工具由上游 AI 调用，一旦模型被诱导即可自行签发一把新 admin key 完成权限维持，这是不可接受的特权提升路径。最多暴露一个**只读**的 `list_api_keys`（Admin 级），用于让操作助手回答"我的 key 快过期了吗"。

**前端改造**（`frontend/src/`）：
- 新增"密钥管理"面板：列表 + 创建 + 吊销 + 轮换。
- 创建成功后弹**一次性明文对话框**：大字号展示、复制按钮、醒目的"仅显示一次，关闭后无法找回"提示。
- `api/index.js`：从 URL 读到 `?token=` 后**立即 `history.replaceState` 清掉**，避免留在浏览器历史与 Referer；存储保持 sessionStorage（比 localStorage 安全，代价是关标签页需重输），可加"记住此设备"显式勾选切到 localStorage。
- `handleUnauthorized`（已存在于 `app.js`）沿用，弹窗文案从"输入 Token"改为"输入 API Key"。
- **不新增**"服务器是否开启门禁"的公开探测端点（会向未授权访问者泄漏部署状态）；沿用"打 `/api/terminals` 拿 401 就弹窗"的现有机制。

### 5.8 审计与限速

**限速（内存态，无需新依赖）**：
- 失败计数：`HashMap<IpAddr, (fail_count, window_start)>`。同一 IP 60 s 内失败 ≥ 10 次 → `429 + Retry-After`；10 min 内 ≥ 30 次 → 临时封禁 15 min。
- 成功即清零。
- 同时限制 `:9800` 与 `:9801`（Agent 注册失败同样计数）。
- 按 Key 的请求配额（`scopes` 之外的独立维度）放 P3；届时给响应加 `X-RateLimit-Limit/Remaining/Reset`。

**审计新增事件**：`AUTH_FAILED`、`key:create`、`key:revoke`、`key:rotate`、`insecure_mode`。全部复用 `audit.rs` 既有 JSONL 通道与 `redact_token`。

### 5.9 依赖变更汇总

| Crate | 版本 | 用途 | 备注 |
|---|---|---|---|
| `sha2` | 0.10 | 密钥哈希 | RustCrypto |
| `subtle` | 2.6 | 常量时间比对 | RustCrypto 官方 |
| `getrandom` | 0.2 | CSPRNG | 无传递依赖；**刻意不引回 `rand`** |

加进 `[workspace.dependencies]`（`at-pc/Cargo.toml`），仅 `server` crate 引用。`zeroize` 列为可选（明文展示后擦除内存副本）。

---

## 6. 分阶段实施计划

每阶段独立可交付、独立可测、可单独回滚。

### P0 — 止血（最高优先，不依赖 KeyStore）
1. 首启 bootstrap root key 生成 + stdout 打印 + `0600` 落盘
2. `validate_boot_gate`：非回环 + 无凭据 → 拒绝启动；`--insecure-no-auth` 显式豁免
3. `/health` 回环免凭据 + 精简载荷；`shutdown.rs:173` 改 `into_make_service_with_connect_info`
4. `/`、`/dashboard`、`/index.html` 定级为"只回登录壳"；把 `auth_layer` 提升为全局 layer
5. 修 `sse_handler` 的 `?token=` 泄漏

> P0 完成即可挡住"公网部署无门禁"这个最主要的风险。

#### P0 文件级改动清单

P0 需要随机数生成 bootstrap key，而 `rand` 已被有意移除（记忆 `MEMORY.md` §归因铁律 5），因此 **P0 即引入 `getrandom`**（`sha2` / `subtle` 留到 P1）。

| # | 文件 | 改动 |
|---|---|---|
| 0 | `at-pc/Cargo.toml` | `[workspace.dependencies]` 加 `getrandom = "0.2"`；`crates/server/Cargo.toml` 引用 |
| 1 | **新增** `crates/server/src/bootstrap.rs` | `generate_root_key() -> String`：`getrandom::getrandom(&mut [0u8;32])` → base64url-no-pad；生成 5 字节 `id` → Crockford base32；拼 `atpc_sk_<id>_<secret>`。`print_bootstrap_banner()` 打印醒目框线。`write_bootstrap_file(data_dir, plaintext)` 以 `0600` 写入（Unix 用 `OpenOptions::new().mode(0o600)`；Windows 走 ACL 收紧或退化为默认 ACL + WARN） |
| 2 | `crates/server/src/config.rs` | 新增 `pub enum BootGateError` 与 `pub fn validate_boot_gate(cfg: &ServerConfig, has_creds: bool, insecure_no_auth: bool) -> Result<(), BootGateError>`；新增 `pub fn is_loopback(host: &str) -> bool`（`127.0.0.1` / `::1` / `localhost` / `127.x.x.x`）。`ServerConfig` 加 `bootstrap_disabled: bool`、`insecure_no_auth: bool`、`allow_plaintext: bool`（后者 P3 用，P0 先占位） |
| 3 | `crates/server/src/main.rs` | 解析 `--no-bootstrap` / `--insecure-no-auth`；在 `run_server_pipeline` 进入前执行启动序列 §5.6 的 1–4 步；exit code `1` + 解法文案 |
| 4 | `crates/server/src/mcp/mod.rs` | ① 新增 `auth_layer`（含豁免表：`OPTIONS`、`/health` 回环、三个 HTML 路由）；② `create_mcp_http_router` 把 `.layer(cors)` 之后加 `.route_layer(...)`；③ 删除 `health_handler` / `sse_handler` / `messages_handler` 里重复的内联校验；④ `health_handler` 取 `ConnectInfo<SocketAddr>`，回环未鉴权时只回 `{"status":"ok"}`；⑤ `sse_handler` 的 endpoint 事件去掉 `?token=`；⑥ 新增 `CredentialSource` 与 `extract_credential()` 包装（`extract_auth_token` 原样保留） |
| 5 | `crates/server/src/shutdown.rs` | 第 173 行 `into_make_service()` → `into_make_service_with_connect_info::<std::net::SocketAddr>()`（TLS 分支的 `axum_server` 同样处理） |
| 6 | `crates/server/src/mcp/dashboard.rs` | 三个 HTML 路由保持在 `auth_layer` 豁免表内，并加注释说明"只返回不含数据的登录壳"这一**约定**，防止后续有人往壳里塞数据 |
| 7 | `crates/server/src/audit.rs` | 新增 `AUTH_FAILED` / `insecure_mode` 两个 action 常量 |
| 8 | `crates/server/tests/p0_boot_gate_test.rs`（新增） | 见 §8 的"启动门禁"行 4 个用例 |
| 9 | `crates/server/tests/p0_health_loopback_test.rs`（新增） | 回环未鉴权 200 且载荷**不含** `connected_terminals` / `ws_port` / `mcp_port` / `version`；非回环未鉴权 401 |
| 10 | `deploy/deploy.sh` | 顺带补 `standalone` 分支缺失的 `--audit-file` / `--meta-file`（既有缺陷，见记忆 `atpc-open-issues.md`），并预留 `--keys-file` |

**P0 不碰的东西（避免范围蔓延）**：`extract_auth_token` 的语义、`get_role_for_token` 的 legacy 分支、`roles` 配置解析、Agent 注册路径（`ws/handler.rs` 留到 P2）、9 个 handler 的角色校验（留到 P1）。

**P0 的已知临时状态**：bootstrap key 此时还不是"库里的记录"，只存在于 stdout 与 `bootstrap_key.txt`；校验时仍走 `config.auth_token` 兼容路径（把 bootstrap key 注入 `config.auth_token`）或直接在 `auth_layer` 里额外比对一个内存态 bootstrap key。**P1 把它迁入 KeyStore 后即成为正规记录**。

### P1 — 密钥库
6. KeyStore：生成 / 解析 / 常量时间校验 / 持久化（复用 `meta_store` 模式）
7. 失败限速与临时封禁
8. `/api/keys` CRUD + `/api/auth/me` + "最后一个 Admin 不可吊销"护栏
9. 控制台密钥管理面板 + 一次性明文弹窗
10. 审计事件补全
11. 补 §5.5 表中 9 个 handler 的角色校验

### P2 — 设备域分离
12. 设备 Token（`kind=dv`）+ `device_id` 绑定
13. `ws/handler.rs` 注册校验改走 KeyStore；`--auth-token` 标记弃用
14. 设备注册失败限速
15. `agent_config.toml` / `deploy.sh` / `docker-compose.yml` / README 文档同步

### P3 — 加固
16. 非回环无 TLS → 拒绝（`--allow-plaintext` 豁免）
17. 浏览器登录换取 **HttpOnly 会话 cookie**（前端不再持有长期 Key）
18. 按 Key 的请求配额与响应头
19. 遗留明文凭据（`config.auth_token` / `roles`）迁移工具；迁移后拒绝明文配置
20. （可选）只读 MCP 工具 `list_api_keys`

---

## 7. 破坏性变更与回归风险

| 项 | 影响 | 处理 |
|---|---|---|
| **默认部署行为变化** | `docker-compose.yml` + `server_config.example.toml`（`[roles]` 全注释）当前是开放态。P0 后首启会生成 root key，控制台开始要求输入 | **用户已确认接受**（§4.1）。仍需在 README 与部署文档显著标注，并在启动日志醒目提示；`--insecure-no-auth` 保留旧行为 |
| **`/health` 语义** | 回环来源变为免凭据；非回环需凭据；未鉴权载荷字段减少 | 豁免策略**已确认接受**（§4.1）。`Dockerfile` 的 `HEALTHCHECK` **无需修改**（容器内即回环）；已有外部监控若走非回环需补 header |
| `tests/p1_auth_rbac_call_id_test.rs:18,41` | 固化 `extract_auth_token` 的优先级/别名/百分号解码语义 | 保留原函数不动，新增 `extract_credential` 包装；旧测试应继续全绿 |
| `tests/p1_auth_rbac_call_id_test.rs:68` | 固化 `get_role_for_token` 的 legacy 与 dev-mode 分支 | dev-mode 回落需**限定为回环来源**才能保留语义；该测试需扩展为"回环→Admin / 非回环→拒绝"两个分支 |
| `at-pc` 审计落盘 | 新增 5 类审计事件，`audit.jsonl` 体积上升 | 失败路径审计做采样（同 IP 同分钟内合并计数），避免被刷爆 |
| `deploy.sh` standalone 分支 | 已知缺 `--audit-file` / `--meta-file`（见记忆 `atpc-open-issues.md`） | 本轮顺带修，避免新加的 `keys.json` 路径重蹈覆辙 |
| 容器重建 | key 持久性 | `docker-compose.yml` 已挂 `at-pc-data:/var/lib/at-pc` 且 `AT_PC_DATA_DIR=/var/lib/at-pc` → `keys.json` 落在该卷内即持久化 ✓ **需在文档中确认此点**，否则每次重建都会重新 bootstrap |

**回滚**：KeyStore 为纯新增文件，删除 `keys.json` 即回到 P0 行为；`--insecure-no-auth` 是完整的行为开关。三层都可独立回退。

---

## 8. 测试矩阵

| 层 | 用例 |
|---|---|
| 单元（`config` / `keystore`） | 密钥格式解析（合法 / 缺段 / 大小写 / 超长）；哈希确定性；`expires_at` 边界；`revoked_at` 拒绝；`kind` 与入口不匹配拒绝；`device_id` 绑定匹配与不匹配；常量时间比对函数对被篡改密钥返回 false |
| 单元（限速） | 第 9 次失败放行、第 10 次 429；窗口滚动；成功清零；封禁到期解封 |
| 集成（`server/tests/`） | 无凭据 → 401（所有入口）；错误密钥 → 401；`Viewer` 调 Admin 工具 → 403 + 审计 `DENIED`；`dv` token 打 MCP → 403；`/health` 回环免凭据 200、非回环 401；`/` 与 `/dashboard` 200 且响应体不含终端数据 |
| 启动门禁 | 非回环 + 空 KeyStore + 无 legacy 凭据 → `exit(1)`；非回环 + bootstrap 开启 → 生成并启动；回环 + 空 → 正常启动；`--insecure-no-auth` → 启动且审计带 `insecure` |
| 密钥生命周期 | 创建返回明文一次且落盘记录不含明文/hash 泄露；列表不含明文；吊销后立即 401；轮换后新旧同时在 grace 内有效、grace 后旧的 401；**吊销最后一个 admin → 409** |
| 回归 | `tests/p1_auth_rbac_call_id_test.rs` 全绿；`tests/static_asset_security_test.rs`、`tests/ws_gateway_test.rs` 全绿；release 门禁 14 测试全绿 |
| 门禁真实性（按 `gate-verification.md` 两板斧） | 把"最后 admin 不可吊销"改成允许 → 对应测试必须变红；喂真实错误密钥 → 401 断言必须真的 FAIL 过 |

---

## 9. 未采纳项与可选加固

| 项 | 说明 |
|---|---|
| Argon2id 用于 API Key | **已否决（2026-09-20 用户确认）**。见 §5.3：256 bit 熵下 key stretching 无收益，且有性能自伤。KeyStore 只实现 `sha256-salt-v1` 单算法，不做"人工短 token 走 Argon2id"的分支。**代价**：系统不再支持运维自设的短口令式凭据，所有 key 必须由服务端 CSPRNG 生成（见 §4.1 末段） |
| server pepper | 默认关闭（§5.3）。需要"只泄漏 keys.json 也炸不开"的收益时开启，代价是必须与 keys.json 一同备份 |
| 把密钥管理暴露为 MCP 工具 | 建议不做（§5.7）。仅保留只读 `list_api_keys` |
| OAuth 2.0 / JWT | 单租户自托管运维工具，引入 OAuth 会显著推高复杂度而收益有限。若未来需要多用户与委托授权再评估 |
| 按 Key 的 IP allowlist | P3 之后视需要；反向代理场景下 IP 语义容易失真 |
| `list_windows` 恒过滤"最小化+空标题"窗口 | 与门禁无关的既有行为，见 `atpc-open-issues.md` |

---

## 10. 决策台账与后续待办

### 10.1 已闭环的开放问题（2026-09-20 确认）

| 原问题 | 结论 | 落点 |
|---|---|---|
| 密钥存储形态 | 只存哈希 | §4 决策 1 |
| Agent 是否纳入同一体系 | 分离 | §4 决策 2 |
| 无门禁时行为 | 硬拒绝启动 | §4 决策 3 |
| 首启 root key 下发通道 | stdout + `0600` 文件 | §4 决策 4 |
| 桌面帧最低角色 | `Viewer` | §4.1 |
| 接受 docker-compose 部署首启需输 key | 接受 | §4.1 / §7 |
| `/health` 回环豁免 | 接受 | §4.1 / §5.5 |
| 是否需要 Argon2id 双算法路径 | 不需要 | §4.1 / §9 |

**§10.1 已全部闭环，无遗留待确认项。**

### 10.2 实施过程中需要即时拍板的两处细节（不阻塞 P0）

1. **`api_get_logs` 的定级与 §5.5 矩阵存在不对称。**
   我把它定为 `Admin`（理由：日志可能含凭据片段、内网地址、上游调用体）。但按 §4.1 的确认，`Viewer` 已经能拉取**实时桌面画面**——画面里可能就显示着同一批日志。
   两种自洽的做法：
   - (a) 保持 `Admin`，接受"能看屏幕但看不到日志文件"的不对称；
   - (b) 也降到 `Viewer`，让 `Viewer` 明确定义为"完全只读的观察者"，把敏感度判断交给 key 的发放流程。
   我倾向 **(a)**：日志是"可整段导出、可离线检索"的形态，泄漏后的可检索性显著高于一张截图，值得单独提级。但这条与你的实际使用场景强相关，实施到 P1 第 11 项时我会请你确认。

2. **`bootstrap_key.txt` 的生命周期。**
   §5.6 只写了"写入 + 提示删除"。可以更严格：
   - (a) 只提示，人工删除（最简单，但文件可能长期留存）；
   - (b) 首次成功的凭据认证后自动删除（自我清理，但若运维还没抄下 key 就登了一次控制台，文件会消失）；
   - (c) 加 TTL（如 30 分钟）后自动删除。
   我倾向 **(a) + 启动横幅里给出精确的删除命令**（`rm` 与 `del` 两条），把决定权留给人。实施 P0 第 1 项时确认。

### 10.3 当前状态与下一步

- **方案设计：已完成并锁定**（本文件）。
- **代码：未改动**。`at-pc` 仓库仍为 `HEAD = 855b082`，P0 尚未开始。
- **下一步**：按 §6 的 P0 文件级清单实施（10 个文件，含 2 个新增测试文件），预计引入 1 个新依赖 `getrandom`。

**P0 的验收口径**（对应 §8 的两组用例）：
1. 非回环 + 无凭据 → 进程退出码 1 且 stdout 含解法；
2. 非回环 + 首启 → 生成 root key、打印、`bootstrap_key.txt` 权限为 `0600`；
3. 回环 `/health` 未鉴权 → 200 且载荷仅 `{"status":"ok"}`；非回环未鉴权 → 401；
4. 容器内 `curl -f localhost:9800/health` 仍成功（HEALTHCHECK 不被破坏）；
5. `/`、`/dashboard`、`/index.html` 可访问但响应体不含任何终端/审计数据；
6. `tests/p1_auth_rbac_call_id_test.rs`、`static_asset_security_test.rs`、`ws_gateway_test.rs` 全绿（零回归）。
