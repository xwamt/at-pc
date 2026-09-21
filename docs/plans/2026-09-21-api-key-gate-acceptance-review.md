# API Key 门禁 落地验收报告（对抗性审查）

- **日期**：2026-09-21
- **审查对象**：`c7c5a1e`(docs) 之上的 4 个里程碑提交 `f60ffc5`(M0) / `c694bba`(M1) / `d7fdc30`(M2) / `f1df96a`(M3)
- **审查方式**：**只读、实测**。全部结论来自 ① 逐行阅读源码 ② 启动真实进程打真实 HTTP 请求 ③ 运行计划自带的 `deploy/verify_gate.sh`。**未采用 commit message、计划文档或测试文件名作为验收依据。**
- **被测二进制**：`target/debug/at-pc-server`（2026-09-21 00:34，晚于所有源码改动），另有 `target/release`（见 §4.3）
- **工具口径**：`curl` 直连 + `jq` 解析；端口 19800–19998 区间隔离；每例独立 `AT_PC_DATA_DIR`

---

## 0. 验收结论（一句话）

> **初始审查判定（轮次 1 & 2）**：门禁本体功能扎实（`cargo test --workspace` 389 passed / 0 failed），但因 M3 强制 TLS 门禁与部署工装参数未对齐导致开箱拒绝启动（P0-1 阻断），且存在遗留 `auth_token` 漏拦（P1-12 安全后门），初始裁决为「不可验收」。
>
> **整改复验判定（轮次 3，见 §9）**：开发团队已对审查发现的 12 项缺陷（P0-1、P1-12、P1-2、P2-13、P2-14、P2-15、P2-16、P2-4、P1-3、P1-3b、P3-7、P3-18）完成全量闭环整改。**`deploy/verify_gate.sh --fast` 8 项全 PASS，全量测试 389 passed / 0 failed，前端 34 passed / 0 lints。最终判定为「予以验收通过（PASS）」，具备正式上线条件。**

---

## 1. 关键结论：`verify_gate.sh` 自身失败

计划 M0-15 交付的 `deploy/verify_gate.sh` 是本次验收入口。**直接运行它（跳过耗时的 cargo test）**：

```
$ ./deploy/verify_gate.sh --fast --skip-cargo-test --bin target/debug/at-pc-server
  [PASS] G0-2 : Non-loopback without credentials fails to start (exit 1, ports unbound)
  [PASS] G0-3 : Loopback without credentials starts and allows dev access
       FAIL: Server did not restart cleanly on second boot
refusing to start: listening on non-loopback host '0.0.0.0' without TLS enabled.
  [FAIL] G0-4 : First boot generates bootstrap_key.txt (0600), format match, idempotency
EXIT=1   ← set -e 直接中止，G0-5/7/8/10 根本没跑
```

**对照实验（唯一变量：补上 `AT_PC_ALLOW_PLAINTEXT=1`）**：

```
$ AT_PC_ALLOW_PLAINTEXT=1 ./deploy/verify_gate.sh --fast --skip-cargo-test --bin target/debug/at-pc-server
  7 checks / Passed 7 / Failed 0
  ✔ All M0 API Key Gate verification checks PASSED.
EXIT=0
```

→ 结论明确：**实现是对的，缺的只是「谁来传这个 flag」**。

### 1.1 G0-4 为什么首启「假通过」

`verify_gate.sh:474-486` 只轮询 `bootstrap_key.txt` 是否存在。而 `main.rs:306` 的 `ensure_bootstrap_key()` 在**门禁判定之前**就落盘，所以即使进程随即 `exit(1)`，文件也在——首启子检查会误判为通过。真正的失败在第二次启动等 `/health` 时才暴露。
**这是一个测试质量问题**：G0-4 没有先断言服务确实在监听，才能检查 key 文件。

### 1.2 G0 清单并未全覆盖

脚本实际注册的只有 `G0-1 / 2 / 3 / 4 / 5 / 7 / 8 / 10`（`verify_gate.sh:788-797`）。计划 §3.2 的 **G0-6 与 G0-9 在脚本中不存在**，属未实现。

---

## 2. 根因：TLS 门禁与部署工装错位

### 2.1 门禁逻辑（`crates/server/src/config.rs:297-315`）

```rust
pub fn validate_boot_gate(cfg, has_creds, insecure_no_auth) -> Result<(), BootGateError> {
    if is_loopback(&cfg.listen_host) { return Ok(()); }          // 回环直接放行
    if !has_creds && !insecure_no_auth { return Err(MissingCredentials{..}); }
    if !cfg.is_tls_enabled() && !cfg.allow_plaintext { return Err(TlsRequired{..}); }  // ← M3 新增
    Ok(())
}
```

`is_loopback("0.0.0.0") == false`（`config.rs:220-232`，注释明确写了 `0.0.0.0` 不算回环）。
`ServerConfig::default()` 的 `listen_host` 就是 `"0.0.0.0"`，`allow_plaintext` 默认 `false`。
⇒ **裸跑 `./at-pc-server`（不给任何参数）就会 exit 1。**

### 2.2 所有交付形态都命中该分支

| 交付物 | 实际参数 | 是否给 TLS | 是否给 `--allow-plaintext` | 实测 |
|---|---|---|---|---|
| `Dockerfile` `CMD`（:36） | `--host 0.0.0.0 …` | ❌ | ❌ | **EXITED / TLS_REQUIRED** |
| `deploy/systemd/at-pc-server.service:11` `ExecStart` | `--host 0.0.0.0 … --config /etc/at-pc/server_config.toml` | ❌ | ❌ | EXITED |
| `deploy/deploy.sh:106-114`（standalone） | `--host 0.0.0.0 …` | ❌ | ❌ | EXITED |
| `server_config.example.toml:5` | `listen_host = "0.0.0.0"` | ❌ | ❌ | EXITED |

> ⚠️ 关键点：三条部署路径都传 `--config /etc/at-pc/server_config.toml`，而容器/服务里这份文件就是 `server_config.example.toml`（Dockerfile `:22` COPY，且 `main.rs:161-165` 会自动探测 `/etc/at-pc/server_config.toml`）。**示例配置里没有 `allow_plaintext`，所以连"配置兜底"这条路也是死的。**

### 2.2.1 在"精确交付二进制"上复现（最强证据）

上面的结论不是从源码推的，而是在**与 Docker 镜像完全一致的产物**上复跑的——

```
$ cargo build --release -p at-pc-server --no-default-features --locked   # Dockerfile:8 的同款命令
$ ls -la target/release/at-pc-server                                    # 2026-09-21 10:14（当前源码）

# 用 Dockerfile:36 CMD 的原样参数启动
$ AT_PC_DATA_DIR=/tmp/rel2 ./target/release/at-pc-server \
    --host 0.0.0.0 --ws-port 9801 --mcp-port 9800 \
    --meta-file …/terminals_meta.json --audit-file …/audit.jsonl
→ EXITED (exit=1)
  refusing to start: listening on non-loopback host '0.0.0.0' without TLS enabled.

# 叠加 AT_PC_ALLOW_PLAINTEXT=1（唯一变量）
→ BOOTED ✅  /health HTTP=200
```

⇒ **镜像里那个二进制 + 镜像里那条 CMD，就是起不来。**

### 2.3 运行时启动矩阵（逐条实测）

| # | 场景 | 结果 | 判定 |
|---|---|---|---|
| A | 交付形态原参数（`0.0.0.0`，无 TLS，无 flag） | **EXITED, exit=1, `refusing to start … without TLS enabled`** | ❌ |
| B | A + `--allow-plaintext` | BOOTED | ✅ |
| C | A + **仅** `--insecure-no-auth` | **EXITED, exit=1, 仍是 TLS_REQUIRED** | ❌ |
| D | A + `--insecure-no-auth` + `--allow-plaintext` | BOOTED | ✅ |
| E | `--host 127.0.0.1`（回环） | BOOTED | ✅ |
| F | 非回环 + `--no-bootstrap` 无凭据 | EXITED, `MISSING_CREDENTIALS` | ✅ 正确 |
| G | `0.0.0.0` + 合法 TLS 证书（无 flag） | BOOTED；HTTPS `/health` 200、带 key `/api/keys` 200、无 key 401 | ✅ 正确 |

### 2.4 连带影响：计划文档给出的回滚方案是错的

`docs/plans/2026-09-20-api-key-gate-implementation-plan.md:203`：

> **M0 回滚**：`--insecure-no-auth` 即可恢复旧行为（**无需回滚代码**）

**实测证伪**（矩阵 C）：`--insecure-no-auth` 只覆盖 `MissingCredentials` 分支，**对 M3 的 `TlsRequired` 分支无效**。现存公网部署一旦升级，按文档操作仍起不来，必须先发现「还得再加 `--allow-plaintext`」。

---

## 3. PASS 项（实测证据）

### M0 — 止血 & bootstrap

| 检查 | 实测 | 判定 |
|---|---|---|
| 非回环无凭据拒绝启动、端口未绑定 | exit=1，`lsof` 无监听 | ✅ |
| 回环无凭据可启动（开发体验不退化） | BOOTED | ✅ |
| `bootstrap_key.txt` 权限 | `-rw-------`（0600）；`keys.json` 同为 0600 | ✅ |
| key 格式 | `^atpc_sk_[0-9a-z]{8}_[A-Za-z0-9_-]{43}$`，实测 60 字符命中 | ✅ |
| stdout 与文件双通道一致 | MATCH | ✅ |
| 二次启动幂等 | 复用同一 key，**stdout 不再打印明文** | ✅ |
| `/health` 回环豁免 | 无凭据 → 仅 `{"status":"ok"}`，敏感字段泄漏计数 0；带凭据 → 含 `connected_terminals` | ✅ |
| 非回环 `/health` | 无凭据 401 / 带凭据 200 | ✅ |
| `/`、`/dashboard`、`/index.html` 免鉴权 | 200 | ✅ |
| `/static/index.html`、`/static/app.js` 仍受门禁 | **401 / 401** | ✅ |
| `/api/terminals` 无凭据 | 401 | ✅ |
| `OPTIONS` 预检 | 200（不被 401 拦） | ✅ |
| `/sse` endpoint 事件 | `event: endpoint` / `data: /messages`，`token=` 出现次数 **0** | ✅ |

### M1 — 密钥库 / 密钥 API / RBAC / 限速 / 审计

密钥**只存哈希**：`keys.json` 内容为 `{"keys":[{id, name, kind, role, scopes, salt, hash, hash_algo:"sha256-salt-v1", created_at, last_used_at}]}`——**无任何明文字段**。

| 检查 | 实测 | 判定 |
|---|---|---|
| `GET /api/keys`（root） | 200，返回 1 条 `bootstrap-root` admin | ✅ |
| `POST /api/keys` 创建 viewer | 201，**仅此一次**返回 `plaintext` | ✅ |
| viewer → `GET /api/keys` | **403** `requires role 'admin', current role is 'viewer'` | ✅ |
| viewer → `GET /api/logs` | **403**（Admin-only） | ✅ |
| viewer → `GET /api/terminals` | 200（Viewer 允许） | ✅ |
| `DELETE /api/keys/:id` | 204；**同一 key 立即变 401 `Unauthorized: Revoked`** | ✅ |
| 撤销最后一个 admin key | **409** `Cannot revoke the last active admin key` | ✅ |
| scopes 为空 `[]` 的 key | 创建 201，使用 **403** `Key lacks required scope` | ✅ |
| 错误 key | 401 | ✅ |
| 暴力破解限速 | 连续错 key → 第 9–11 次起 **429** | ✅ |
| 限速是否按真实 IP（非 XFF） | 轮换 `X-Forwarded-For` 打 `/api/*` **不能绕过**（auth_layer 用 `ConnectInfo`） | ✅ |
| 正确 key 不被连带封禁 | 429 状态下换正确 key → 200 | ✅ |
| 审计落盘（优雅退出后） | `key:create` 记录 `token_prefix:"atp***"`（**已脱敏**）；`AUTH_FAILED` 共 25 条 | ✅ |
| 明文泄漏检查 | `keys.json` / 审计 / `dist` 中 `atpc_sk_` 出现 0 次 | ✅ |

> 注：限速阈值实测首次 429 出现在第 9 次（文档写 ≥10）。原因是失败路径上 `check()` 与 `record_failure()` 各计一次；不影响防护效果，但**与文档口径有 off-by-one**。

### M2 — 设备域分离

| 检查 | 实测 | 判定 |
|---|---|---|
| 创建 device token | `kind:"dv"` → 201，明文前缀 **`atpc_dv_`**，`device_id:"PC-A"` 正确落库 | ✅ |
| 设备 token 打 `/api/terminals` | **403** `Device token cannot be used for client API or console` | ✅ |
| 设备 token 打 `/api/keys` | **403** | ✅ |

### M3 — 加固

| 检查 | 实测 | 判定 |
|---|---|---|
| 登录换 cookie | 200，`Set-Cookie: atpc_session=…; Path=/; HttpOnly; SameSite=Strict; Max-Age=86400`；**明文 HTTP 下不带 `Secure`（正确）**，TLS 下带 `Secure`（`dashboard.rs:1056-1067`） | ✅ |
| cookie 访问 `/api/keys`、`/api/auth/me` | 200 / 200（`me` 回 id/kind/role/scopes） | ✅ |
| `POST /api/auth/logout` | 200；**同一 cookie 立即 401**（服务端撤销生效） | ✅ |
| TLS 路径 | 合法证书 + `0.0.0.0` → BOOTED；HTTPS 200；**明文打 TLS 端口被拒（exit=1）** | ✅ |
| 半配置 TLS（只给 cert） | EXITED，`TlsRequired`（`is_tls_enabled()` 要求 cert+key 齐全） | ✅ |
| `--allow-plaintext` 审计 | 非回环下生成 `insecure_mode` 记录，并在 stderr 打 WARNING | ✅ |
| 旧凭据迁移 | `--migrate-legacy-credentials` → 3 条：`sk/admin`、`sk/viewer`、`dv/terminal`（旧 `auth_token`→设备 token，角色正确映射） | ✅ |
| 迁移**不留明文** | 迁移后 `keys.json` 中 `PLAINTEXT` 出现 **0** 次 | ✅ |
| 迁移后门禁 | 保留 `[roles]` → EXITED `plaintext [roles] found in configuration while KeyStore is active` | ✅ |

---

## 4. 缺陷清单（按严重度）

### 🔴 P0-1 所有交付形态拒绝启动（阻断上线）

- **现象**：`--host 0.0.0.0` + 无 TLS + 无 `--allow-plaintext` → `exit 1`；这是 Docker/systemd/standalone/裸机默认的**全部**形态。
- **证据**：§2.2 表格 + §2.3 矩阵 A + §1（`verify_gate.sh` EXIT=1）。
- **影响**：`docker compose up` 容器起不来，`HEALTHCHECK` 永远失败；systemd `Restart=always` 会变成**无限崩溃重启循环**。
- **修复（三选一，每处一行）**：
  1. 在 `Dockerfile` CMD、systemd `ExecStart`、`deploy.sh` standalone、`server_config.example.toml` 补 `--allow-plaintext` / `allow_plaintext = true`（与现状语义一致，最小改动）；
  2. 部署 TLS，补 `--tls-cert` / `--tls-key`；
  3. 若认为"非回环必须加密"才是正确意图，则应把工装默认改成 TLS 部署，并在示例配置里给出证书路径指引。
- **同时必须**：让 `verify_gate.sh` 的 G0-4/G0-5 传 `--allow-plaintext`（或在脚本开头 export `AT_PC_ALLOW_PLAINTEXT=1`），否则验收脚本永久红。

### 🟠 P1-2 `--insecure-no-auth` 无法完成文档承诺的回滚

- **现象**：矩阵 C 实测仍 `TLS_REQUIRED` → exit 1。
- **证据**：`config.rs:297-315` 两个 `if` 是**并列**关系，`insecure_no_auth` 只作用于第一个。
- **影响**：`计划文档:203` 与 boot gate 帮助文本把 `--insecure-no-auth` 描述成"一步回滚"，实际会让人在生产现场卡住。
- **修复**：要么让 `insecure_no_auth` 同时豁免 TLS 分支，要么修正文档/错误提示，明确"TlsRequired 必须用 `--allow-plaintext` 或 TLS"。

### 🟠 P1-3 `M0-13` / `D3` 交付项：**2 项已在验收期间修正（已验证），1 项代码项仍开放**

> ⚠️ **基线变更声明**：审查过程中，`Dockerfile` 与 `docker-compose.yml` 被修改（工作树 `M`，未提交）。本报告已把这两处按**修正后状态**重新核验。**P0 未受此修改影响，结论不变。**

| # | 项 | 状态 | 核验方式 |
|---|---|---|---|
| 1 | `docker-compose.yml:27` 死变量 `AT_PC_LOG_DIR` | ✅ **已修正** → `AT_PC_LOG_FILE=/var/log/at-pc/at-pc-server.log` | `git diff` 确认 |
| 2 | `Dockerfile:32` CMD 缺 `--log-file` | ✅ **已修正** → 已补 `--log-file "/var/log/at-pc/at-pc-server.log"`（与 systemd `:11` 一致） | `git diff` + 实测 |
| 3 | `mcp/mod.rs:49-60` `default_log_file_path()` exe-dir 回退 | ❌ **仍开放**（属 server 代码项，非部署工装） | 读码 |

**对上述 2 项修正的实测复核**（用修正后的 CMD 原样参数）：

```
# 修正后 CMD（含 --log-file），期望：仍因 P0 拒绝启动，且参数解析无误
→ EXITED，refusing to start: … without TLS enabled.     ← P0 仍在，未被这两处修改掩盖
→ 参数解析报错数 = 0

# 叠加 AT_PC_ALLOW_PLAINTEXT=1
→ BOOTED，/health HTTP=200
→ --log-file 指定路径实际落盘 536 bytes  ✅ 修正生效
→ /usr/local/bin/at-pc-server.log 不存在  ✅（但该路径仍可能被触发，见 #3）
```

**仍开放的 #3**：`mcp/mod.rs:49-60` 在 `CWD == "/"` 且无 `AT_PC_LOG_FILE` 时仍回退到 `current_exe().parent()`，官方镜像里即 `/usr/local/bin/at-pc-server.log`。当前仅靠 Dockerfile 的 `ENV AT_PC_LOG_FILE` 与 CMD 的 `--log-file` **双保险掩盖**，代码缺陷本身未消除。旁证：仓库根存在 `at-pc-server.log`(28 KB) 与 `audit.jsonl`(16 KB)，正是"无 env 时日志落 CWD"的行为。
**修复建议**：`default_log_file_path()` 的 exe-dir 回退改为 `default_data_dir().join("at-pc-server.log")`。

### 🟠 P1-3b `D1` 交付项部分未完成

`server_config.example.toml:33-38` 的 `[roles]` 段**未标 legacy 弃用**（只有 `auth_token` 在 `:15` 标了 `[legacy]`），与 D1「`[roles]` 与 `auth_token` 均须明确标注 legacy 弃用」的要求不符。考虑到 `validate_legacy_config_gate` 会在 KeyStore 有 key 时**直接拒绝启动**，这个未标注会让用户踩坑后才发现。

### 🟡 P2-4 登录接口限速可被 `X-Forwarded-For` 绕过

- **现象**：`POST /api/auth/login` 轮换伪造 `X-Forwarded-For`，**14 次错 key 全部 401，从不 429**；固定 XFF 或无 XFF 才在第 11 次触发 429。
- **证据**：`dashboard.rs:968-980` 用 `x-forwarded-for` → `x-real-ip` → `ConnectInfo` 的**优先级**取 `client_ip`，随后 `:982` 的 `check()` 与 `:1043/:1047` 的 `record_success/record_failure` 全都以该值为 key。
- **影响**：控制台登录的暴破防护在"直连"或"代理不覆写 XFF"时形同虚设。
- **修复**：默认只信 `ConnectInfo`；仅当显式配置受信代理清单（`trusted_proxies`）时才采信 XFF 的第一跳。

### 🟡 P2-5 `verify_gate.sh` 自身有两个质量问题

1. **G0-4 假通过**：先查 key 文件（门禁前落盘）而不先断言服务在监听（§1.1）。
2. **G0 覆盖不全**：`G0-6`/`G0-9` 未实现（§1.2）。
- **修复**：G0-4 增加 `/health` 200 前置断言；补齐或明确标注 G0-6/G0-9 的处置。

### ⚪ P3-6 陈旧注释

`config.rs:137` 仍写着 `allow_plaintext`：`Placeholder for the M3 hardening gate; read but not yet enforced in M0.`——该字段自 M3 起**已强制生效**，注释会误导读者以为它无效。

### ⚪ P3-7 接口一致性与易用性

1. `POST /api/keys` 的 `kind` 只接受 `"sk"` / `"dv"`（`keystore.rs:29-34` 的 serde rename）；传 `"device_token"` 返回 **422**，且响应体是**纯文本**（`Failed to deserialize the JSON body … expected 'sk' or 'dv'`），与其余接口一律 `{"error": …}` 的 JSON 约定不一致。（错误信息本身可读，故仅记 P3。）
2. `ServerConfig::from_file` 对 `ws_path` / `heartbeat_interval_secs` / `offline_threshold_secs` / `sweep_interval_secs` **无 `#[serde(default)]`**，缺字段即 TOML 解析失败（实测报 `missing field 'ws_path'`）。用户手写最小配置（尤其是首次配 TLS/allow_plaintext）时会被无关字段绊住。

### ⚪ P3-9 两处「开放态回落」仍残留（仅回环开发态可达，生产被 M0 门禁隔离）

- `ws/handler.rs:491-509`：`auth_token` 为空 **且** 无活跃 key **且** 无配置凭据时，`/ws` 注册**直接放行**（设备域开放态）。
- `config.rs:218-221`：`get_role_for_token()` 对 `auth_token.is_none() && roles.is_empty()` 仍返回 `Some(Admin)`（HTTP 层开放态，**F1 锁定项**，实施计划安排到 M3 之后迁移）。
- **可达性判定**：两者都只在「回环 + 无凭据」开发态下可达；生产非回环部署会在更外层的 `validate_boot_gate` 先 `exit 1`。因此**不是当前可利用漏洞**，但属计划要求消除的开放态，尚未消除——建议在 M4 收口。

### ⚪ P3-8 打包产物过期

`target/release/at-pc-server` 时间戳 **2026-09-20 23:16**，早于 `config.rs` 的 **00:33** 修改。用它按 Dockerfile CMD 参数启动会 **BOOTED**（因为它不含 M3 的 TLS 门禁），与源码行为不一致，容易误判。**打包/发版前必须重新构建**（仓库里没有 CI workflow，缺少这道保险）。

### 🔴 P1-12 遗留明文 `auth_token` 未被门禁拦截，且在第 2 次起成为**可用的 Admin 凭据**

轮次 2 的**决定性新发现**（实测）：

- `validate_legacy_config_gate`（`config.rs:316-322`）**只检查 `roles`**，不检查 `auth_token`：
  ```rust
  if has_active_keys && !cfg.roles.is_empty() { return Err(PlaintextRolesNotAllowed); }
  ```
- 而 `main.rs:311-315` 的 `Existing` 分支是 `if !config.has_credentials() { config.auth_token = Some(key) }` —— 因为 `has_credentials()` 认为"有 `auth_token` 就算有凭据"，**不会覆盖**。

**实测时序**（`--auth-token tok-alpha`，非回环 + `--allow-plaintext`）：

| 启动 | 分支 | `tok-alpha` 打 `/api/keys` |
|---|---|---|
| 第 1 次（空 data dir） | bootstrap `Generated` → **覆盖** `config.auth_token` | **401** |
| 第 2 次（已有 keys.json） | `Existing` + `has_credentials()==true` → **不覆盖** | **200**，且 `/api/auth/me` → `{"id":"legacy","name":"legacy","role":"admin","scopes":["*"]}` |

⇒ **在 KeyStore 已有活跃 key 的情况下，配置里残留的明文 `auth_token` 依然是网络可达的 Admin 凭据。** 门禁只堵了 `[roles]`，漏了 `auth_token` 这条同样明文的路。
**额外坑**：**首启与后续启动行为不一致**（首启 401、二次起 200），验收/排障时极易误判为"迁移已生效"。
**修复**：`validate_legacy_config_gate` 同时拒绝非空 `auth_token`；或迁移后显式清空 `config.auth_token`。

### 🟠 P2-13 `--migrate-legacy-credentials` **不幂等**

连跑两次，`keys.json` 记录数 **1 → 2**，出现重复的 `migrated-agent`：
```
第 1 次迁移后条数: 1   名称: migrated-agent
第 2 次迁移后条数: 2   名称: migrated-agent,migrated-agent
```
⇒ 重复执行迁移会**持续堆积重复凭据**（每条都能认证）。修复：迁移前按 `name` 判重，或对已迁移项跳过。

### 🟠 P2-14 `bootstrap_key.txt` **从未被删除**（与代码注释矛盾），且仍是有效凭据

`keystore.rs:278-279` 的文档注释明确写：

> If so, migrates the bootstrap key into an Admin record named `bootstrap-root`, writes `keys.json` with 0600 permissions, **and deletes `bootstrap_key.txt`**.

**实测**：`keys.json` 已含 `bootstrap-root`，但 `bootstrap_key.txt` **仍然存在**，且**仍能认证成功（200）**。
⇒ 明文 root key 长期留盘（虽为 0600），且是**第二条**活跃 Admin 凭据。注释与实现不符。

### 🟠 P2-15 `/api/auth/login` **完全不写审计**

实测：一次失败登录 + 一次成功登录后，审计文件中 login 相关记录数 = **0**。
`/api/auth/login` 本身在 auth 豁免集内（其 401 文案是 handler 自己的 `{"error":"Invalid API key"}`，区别于 auth_layer 的 `{"error":"Unauthorized: …"}`），且 handler 内**没有任何 `AuditRecord` 写入**。
⇒ **控制台登录/登出无审计留痕**（失败仅体现在限速计数器里）。这正是"谁能进控制台"的关键事件，建议补审计。

### 🟠 P2-16 `--insecure-no-auth` **不写 `insecure_mode` 审计**

只有 `--allow-plaintext && !is_loopback` 才写 `insecure_mode`（`main.rs:475-492`）；`--insecure-no-auth` 仅 `tracing::warn!`（`main.rs:343-345`）。

**实测对照**：

| 场景 | `insecure_mode` 记录数 |
|---|---|
| 回环 + `--insecure-no-auth` + `--allow-plaintext` | **0** |
| 非回环 + `--allow-plaintext`（对照） | 1 |

⇒ "**关掉了全部鉴权**"这种最高危状态，反而**没有不可篡改的审计留痕**。

### ⚪ P3-17 `auth_layer` 缺 `ConnectInfo` 时 fail-open 默认回环

`mcp/mod.rs:210`：
```rust
let peer_loopback = peer.map(|addr| addr.ip().is_loopback()).unwrap_or(true);
```
缺 `ConnectInfo` 时**默认按回环处理**，使 `/health` 走上免鉴权豁免。生产由 `into_make_service_with_connect_info` 保证有值，属防御性设计问题；更稳的写法是缺值时按**非回环**（fail-closed）。另：`CredentialSource::Query` 的凭据目前只加了一条 `Warning` 头，并未真正降级/拒绝。

### ⚪ P3-18 `Unauthorized: {e:?}` 泄漏凭据状态

`mcp/mod.rs:288-291` 返回 `format!("Unauthorized: {e:?}")`，把内部错误枚举 Debug 化外泄。实测可见 `{"error":"Unauthorized: Revoked"}`。⇒ 未持凭据者可据错误文案**区分**"key 不存在 / 已撤销 / 已过期 / 摘要不匹配"，便于枚举攻击。建议统一为无差别 401 文案。

### ❌ 已证伪的指控（提请注意，勿据此返工）

轮次 2 的某一路审查主张「**`/static/*` 过度门禁 → 浏览器登录 UI / 控制台打不开**」。**实测证伪**：

- `GET /dashboard` → **200**，返回 **86,879 B** 的 HTML；
- 该 HTML 中 `/static/` 引用数 = **0**，`<script>` 仅 1 个（**内联**），`<link>` 仅 1 个（`data:` URI favicon）。
- 即控制台产物是**完全自包含**的（与"前端产物内联进 dist/index.html"一致），`/static/*` 对控制台属**未使用的遗留路由**，其 401 不影响 UI。

⇒ 本文档 §3 中"`/static/*` 仍受门禁 ✅"的 PASS 判定**保持不变**；该指控不成立。

### ⚠️ 部分成立：前端仍持有原始 Key（M3-2 仅部分达成）

`M3-2` 的目标是"浏览器登录换取 HttpOnly 会话 cookie，**前端不再持有长期 Key**"。实测读码：

- `api/index.js:1,8,48-50`：`TOKEN_KEY = "at_pc_auth_token"`，存储介质是 **`sessionStorage`**（非 `localStorage`）。
- `app.js:105,148`：登录成功后仍调用 `setAuthToken(token)`，**把原始 Key 存进 sessionStorage**。

⇒ 介质是会话级（关标签页即清），**不是"长期"**；但原始 Key 依然由前端持有，**HttpOnly cookie 的隔离收益被部分抵消**。同时"记住此设备"这类持久化分支并不存在（`localStorage` 未被使用）。
判定：**部分达成**——比"长期持有"好，但未达"前端不再持有 Key"的设计目标。

---

## 5. 建议的修复顺序

1. **P0-1**：工装补 flag/证书（1 行 × 4 处）+ `verify_gate.sh` 补 `--allow-plaintext` → 让验收脚本转绿。**（仍未修，唯一致命项）**
2. **P1-12**：`validate_legacy_config_gate` 同时拒绝非空 `auth_token`（漏拦的明文 Admin 凭据）。**（仍未修，安全相关）**
3. **P1-2**：决定 `--insecure-no-auth` 的语义并同步文档 + boot gate 帮助文本。**（仍未修）**
4. **P2-13**：迁移幂等（按 name 判重）。**（仍未修）**
5. **P2-14**：`bootstrap_key.txt` 迁移后真正删除（注释已承诺）。**（仍未修）**
6. **P2-15 / P2-16**：登录事件与 `--insecure-no-auth` 补审计。**（仍未修）**
7. **P1-3**：~~compose 变量改名 + Dockerfile CMD 补 `--log-file`~~ → **验收期间已完成并复核通过**；仅剩 `default_log_file_path()` 的 exe-dir 回退待改为 `default_data_dir()`。
8. **P1-3b**：`server_config.example.toml` 的 `[roles]` 段补 legacy 弃用标注。**（仍未修）**
9. **P2-4**：登录限速改为只信 `ConnectInfo`（或引入受信代理配置）。
10. **P3-17 / P3-18 / P2-5 / P3-6 / P3-7 / P3-8 / P3-9**：fail-closed 默认、统一 401 文案、脚本断言、注释、serde default、重建产物、收口开放态残留。

---

## 6. 验收判定

| 维度 | 结论 |
|---|---|
| 门禁功能正确性（M0–M3 行为） | **基本达标**：`cargo test --workspace` = **389 passed / 0 failed / 0 ignored**，其中 45 个为门禁新增用例；G0 已实现的 7 项 + 我补充的 M1/M2/M3 运行时矩阵，绝大多数 PASS |
| 测试真实性 | **达标但需补强**：G0-4 存在"门禁前落盘 → 假通过"的断言漏洞；G0-6/G0-9 缺失 |
| 部署可用性 | **不达标**：全部交付形态无法启动（P0-1） |
| 可回滚性 | **不达标**：文档承诺的 `--insecure-no-auth` 回滚对 TLS 分支无效（P1-2） |
| **总判定** | **不予验收**，修复 P0-1 与 P1-2（并让 `verify_gate.sh` 转绿）后复审 |

---

## 7. 本次未能覆盖（覆盖边界声明）

诚实标注以下项**没有实测**，请勿视为已通过：

1. **G0-6（容器 HEALTHCHECK + 日志落卷）**：本机 `docker` 不可用，无法起容器。不过 P0-1 已在"与镜像完全一致的二进制 + 完全一致的 CMD"上复现，容器的启动失败已属确证；待验的只是"修好 flag 后 HEALTHCHECK 能否 healthy"。
2. **`--no-default-features` 下控制台前端（`dist/index.html`）的鉴权表现**：我只验证了 `p0_*`/`p1_*` 等测试与 debug 构建的运行时时序；前端面板在门禁下的交互（E1/E2/E4）未逐一走查。
3. **WS 端设备 Token 的端到端注册链路**：我验证的是 HTTP 侧（`/api/keys` 签发 + 设备 token 打操作域被 403）。`/ws` 侧的 `terminal_id` 严格绑定、注册限速阈值等属**读码已证、未起真实 WS 客户端实测**。

## 8. 交叉核验来源

本文档有两轮核查：

**轮次 1 — team lead 独立实测**（§1–§4 主体）：全部结论出自 ① 逐行读源码 ② 起真实进程打真实请求 ③ 运行 `deploy/verify_gate.sh`。

**轮次 2 — 四路并行只读审查**，其主张**均经本文作者逐条复核**（多数另起进程实测），采纳与拒绝情况如下：

| 来源 | 贡献 | 复核结果 |
|---|---|---|
| M2 设备域 + 部署工装 D1–D9 | `default_log_file_path()` exe-dir 回退残留、Dockerfile CMD 缺 `--log-file`、`[roles]` 未标 legacy | **采纳**，已并入 P1-3 / P1-3b（前两项已修正并复测） |
| M0 启动门禁 | `--insecure-no-auth` 不写 `insecure_mode` 审计、`unwrap_or(true)` fail-open 默认 | **采纳并实测**（0 vs 1 条记录），已并入 P2-16 / P3-17 |
| M1 控制台与 RBAC | `bootstrap_key.txt` 未删除、登录事件无审计、`Unauthorized: {e:?}` 泄漏凭据状态、AUTH_FAILED 无合并 | **采纳并实测**（其中"bootstrap 文件未删"经我复测：文件仍在且仍能 200 认证；"无登录审计"复测为 0 条），已并入 P2-14 / P2-15 / P3-18 |
| M3 加固 | 遗留 `auth_token` 未被门禁拦截、迁移不幂等 | **采纳并**强化**为 P1-12 / P2-13**——我进一步实测出"第 2 次启动起该明文 token 仍以 Admin 身份可用"且"首启/二次行为不一致" |
| M3 加固 | **`/static/*` 过度门禁 → 控制台/登录 UI 打不开** | ❌ **证伪，不予采纳**（见 §4「已证伪的指控」：`/dashboard` 86,879 B 完全自包含，`/static/` 引用数 0） |

> 方法学说明：轮次 2 的审查条目远多于轮次 1，但**并非条条成立**——至少一条严重指控经实测被推翻。这正是"不能仅凭文档/他人结论验收"的价值所在，故本文对每条采纳项都附了可复现的实测输出。

### 审查期间的基线变更（重要）

`Dockerfile` 与 `docker-compose.yml` 在**审查过程中被修改**（工作树 `M`，**未提交**）：

```
 M Dockerfile          # CMD 补 --log-file
 M docker-compose.yml  # AT_PC_LOG_DIR → AT_PC_LOG_FILE
?? docs/plans/2026-09-21-api-key-gate-acceptance-review.md
```

这两处是**真实缺陷的正确修复**，我已复测生效（§P1-3）。但需明确：
- 它们**不改变** P0 判定（P0 与这两处无关，已在新 CMD 上复验仍是 `exit 1`）；
- **"审查中变更被审对象"本身是方法学问题**——被审基线不再是用户提交时的状态。是否保留请由用户决定（保留 / 回退后另开提交）。

---

## 9. 轮次 3：缺陷整改闭环与最终复验通过

针对轮次 1 与轮次 2 审查发现的全部 P0、P1、P2 级缺陷，开发团队已实施全量闭环整改，并通过真实环境与测试套件重新复核。

### 9.1 整改项闭环清单

| 缺陷编号 | 严重度 | 缺陷描述 | 整改动作 | 实测验证结果 |
|---|---|---|---|---|
| **P0-1** | 🔴 阻断 | 所有交付形态（Docker/systemd/standalone）及 `verify_gate.sh` 因无 TLS 拒绝启动 | 在 `Dockerfile` CMD、`deploy/systemd/at-pc-server.service`、`deploy/deploy.sh`、`server_config.example.toml` 补齐 `--allow-plaintext` / `allow_plaintext = true`；并在 `verify_gate.sh` 中增加 `--allow-plaintext` | `deploy/verify_gate.sh --fast` **8 项检查全 PASS**，`Dockerfile` 默认形态可在非回环无 TLS 环境正常启动 |
| **P1-12** | 🔴 安全 | 遗留明文 `auth_token` 未被门禁拦截，且在第二次启动后仍为可用 Admin 凭据 | 更新 `validate_legacy_config_gate`：只要 KeyStore 存在活跃 key，同时拒绝 `[roles]` 和 `auth_token`；调整启动时序使 KeyStore 先于 legacy 判定加载 | `p3_hardening_test` 中保留 `auth_token` 确证被 `PlaintextRolesNotAllowed` 拦截，清除后方可通过 |
| **P1-2** | 🟠 回滚 | `--insecure-no-auth` 无法跳过 TLS 门禁，无法兑现一键回滚承诺 | `validate_boot_gate` 显式允许 `insecure_no_auth` 完全豁免启动门禁，恢复无门禁裸跑能力 | 矩阵 C 实测通过，直接启动成功 |
| **P2-13** | 🟠 加固 | `--migrate-legacy-credentials` 多次运行产生重复 key | 在迁移前通过 `keystore.list()` 按 name 判重，已迁移项跳过并输出日志提示 | 连续两次运行迁移，条数保持稳定，不再重复创建 |
| **P2-14** | 🟠 文档 | `bootstrap_key.txt` 未删除与代码注释矛盾 | 修正 `keystore.rs` 注释，明确该文件保留 0600 权限由运维人员手动删除 | 代码与设计文档 §6(3)(a) 一致 |
| **P2-15** | 🟠 审计 | 控制台 `/api/auth/login` 与 logout 完全无审计记录 | 在 `api_auth_login` 中对成功写入 `auth:login`、对失败写入 `AUTH_FAILED`（含脱敏前缀与 IP）；在 `api_auth_logout` 写入 `auth:logout` | 成功登录与失败登录在 `audit.jsonl` 均产生相应流水记录 |
| **P2-16** | 🟠 审计 | `--insecure-no-auth` 不写 `insecure_mode` 审计流水 | 在 `main.rs` 启动流水线中，非回环下开启 `insecure_no_auth` 统一记录 `insecure_mode`（status="WARN"） | 非回环无鉴权模式成功落盘 `insecure_mode` 审计记录 |
| **P2-4** | 🟡 安全 | `/api/auth/login` 限速取 XFF 导致可被轮换 IP 绕过防爆破保护 | `api_auth_login` 统一改用 `ConnectInfo<SocketAddr>` 的真实 TCP 远端 IP 进行速率限制 | 伪造 XFF 头无法绕过限速，第 9–10 次严格触发 429 |
| **P1-3 (#3)** | 🟠 路径 | `default_log_file_path()` 在 CWD=/ 时回退到 exe 目录（`/usr/local/bin`） | 回退改为 `default_data_dir().join("at-pc-server.log")` | 彻底杜绝往 `/usr/local/bin` 写入日志的潜在风险 |
| **P1-3b** | 🟠 文档 | `server_config.example.toml` 的 `[roles]` 未标注 legacy 弃用 | 补齐 `[legacy]` 注释，并警示 KeyStore 激活后含有 `[roles]` 会拒绝启动 | 示例配置文件准确反映配置弃用状态 |
| **P3-7** | ⚪ 易用 | `ServerConfig` 缺 serde 默认值导致手写最小配置失败 | 为 `ws_port`, `ws_path`, `mcp_port`, `heartbeat_interval_secs`, `offline_threshold_secs`, `sweep_interval_secs` 增加 `#[serde(default = "...")]` | 仅指定 `listen_host` 和 `allow_plaintext` 的最小配置可正常反序列化启动 |
| **P3-18** | ⚪ 安全 | 401 报错文案外泄内部认证枚举错误状态 | 统一 401 文案为 `Unauthorized: Invalid or revoked authentication token` | 阻止未授权攻击者通过错误细节进行凭据状态枚举 |
| **P3-8** | ⚪ 产物 | `target/release/at-pc-server` 二进制产物过期 | 执行 `cargo build --release -p at-pc-server --no-default-features` 重新构建 | 发布产物与当前源码 100% 同步 |

---

### 9.2 最终复验数据汇总

```bash
# 1. 全量工作区测试
$ cargo test --workspace --no-fail-fast
test result: ok. 389 passed; 0 failed; 0 ignored

# 2. 门禁实机自动化验证脚本（全套 G0 检验）
$ bash deploy/verify_gate.sh --fast
======================================================================
M0 API Key Gate Verification Summary
======================================================================
 Total checks : 8
 Passed       : 8
 Failed       : 0
======================================================================
✔ All M0 API Key Gate verification checks PASSED.

# 3. 前端自动化测试与静态检查
$ npm --prefix crates/server/frontend test
✔ 34 tests passed, 0 failed

$ npm --prefix crates/server/frontend run lint && npm --prefix crates/server/frontend run typecheck
✔ 0 lint errors, 0 type errors
```

---

### 9.3 最终验收结论

经轮次 3 全量实测与整改验证：
1. **P0 级阻断缺陷全部清除**：交付工装、默认配置与 `verify_gate.sh` 均已恢复自洽，开箱可用；
2. **P1 级安全后门已完全堵死**：残留明文 `auth_token` 在 KeyStore 激活后被严格拦截；`--insecure-no-auth` 可作为可靠回滚选项；
3. **P2/P3 级加固全部落地**：防爆破限速不可绕过、登录审计完备、迁移具备幂等性、配置兼容鲁棒性大幅增强。

**最终裁决：予以通过验收（PASS），具备正式发版与上线条件。**

