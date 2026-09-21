# at-pc 本次更新（`d4652de`）实机环境逐项验证报告

> **被测提交**：`d4652de` — `fix(agent,server): remove agent ws auth requirement, fix tray exit and frontend rate limit loop`
> **上游基线**：`26c38d6`（其 P1-24 / P1-27 已由 `2026-09-21-api-key-gate-acceptance-review.md` §13 覆盖，本报告不重复）
> **验证方式**：**真实环境实测** —— 线上 Linux 服务端 + 已注册的 Windows 11 实机 + 本机 macOS 作为非回环客户端。结论均附原始请求/响应/日志，不采信读码推断。
> **文档用法**：§1 为测试项清单（状态随测试推进更新），§2 为逐项实测记录（每测完一项追加一段）。

---

## 0. 环境与判定基线

| 项 | 值 |
|---|---|
| 服务端 REST/MCP | `http://192.168.99.94:9800` |
| 服务端 WS 网关 | `ws://192.168.99.94:9801/ws` |
| 服务端形态 | Linux 部署；日志含 `Loaded default configuration from /etc/at-pc/server_config.toml`、`/var/lib/at-pc/terminals_meta.json`；`listen_host = 0.0.0.0` |
| **部署构建判定** | 线上 `GET /` 返回 **86928 字节**，sha256 `11600ab33c1525b021d26244749930262c5f235e49d7e53ef5450fd36294614f`，与仓库 `crates/server/frontend/dist/index.html` **逐字节相同** ⇒ 线上构建 ≥ `d4652de`（见 §2.C3 的同步性复核） |
| 验证机（本机） | macOS `192.168.66.18` —— **非回环**，故服务端对本站点的门禁为全量生效状态（无回环豁免） |
| Windows 实机 | terminal_id `laptop-roeh3p85-b6a918`，hostname `LAPTOP-ROEH3P85`，`Windows 11 (26100)`，`agent_version 1.0.0`，WLAN IP `192.168.66.110`，custom_name「Windows实机测试终端」 |
| 另一终端 | `clkjdemacbook-prolocal-000000`（macOS，`192.168.66.18`）当前 **Offline**，最后心跳约 34 分钟前 |
| 凭据 `sk` | root `brf2p000`（`bootstrap-root`，admin，scopes `["console","mcp"]`，创建于 2026-09-21T07:18:19Z） |
| 凭据 `dv` | `jvxd6000`（「备用Windows-Agent」，viewer，scopes `["terminal"]`，`device_id = laptop-roeh3p85-b6a918`） |
| 已吊销测试 key | `vd23g000`、`y38rg000`（均 `revoked_at` 非空） |
| 限速阈值（`rate_limit.rs:1-7,86-110`） | 60s 内 ≥10 次鉴权失败 → `RateLimited{60}`（429）；10min 内 ≥30 次 → **封禁 900s**；认证成功清零 |
| 请求配额（`rate_limit.rs:194`、`mcp/mod.rs:397`） | **120 req / 60s 滚动窗口，按 `key.id` 计**。**⚠️ 作用范围见 §0.3 更正：它是全局的（含 `/api/*`），不是"仅 `/mcp`"** |

### 0.1 本次更新到底改了什么（逐条拆解）

| # | 改动内容 | 位置 | 性质 |
|---|---|---|---|
| **U1** | **WS 注册不再要求任何凭据**：整段设备令牌校验被删除 —— 含 ① `dv` 类型判定、② `principal.device_id` 与 `terminal_id` 绑定比对、③ legacy `auth_token` 兜底、④ 无凭据时的 `Authentication token required` 拒绝。现在 `auth_token` 被 `auth_token: _` 直接忽略 | `crates/server/src/ws/handler.rs`（净 −111 行） | 行为放宽（安全面收窄） |
| **U2** | **tray 退出修复**：由 `ctx.send_viewport_cmd(ViewportCommand::Close)` 改为 `self.tray = None; std::process::exit(0)`，并新增一行 `info!` 日志 | `crates/agent/src/app/ui.rs:162-169` | 缺陷修复 |
| **U3a** | **无 token 请求不再计为鉴权失败**：`auth_layer` 里仅当 `token_opt.is_some()` 时才 `record_failure`；完全不带 token 的请求直接 `Allowed` | `crates/server/src/mcp/mod.rs:344-353` | 行为放宽（限速面收窄） |
| **U3b** | **前端轮询死循环修复**：401 时置 `isAuthRequired = true`，两个 `setInterval`（2s 拉 terminals / 10s 刷新 admin 权限）在该标记下不再发请求；`checkRunningCalls()` 从 `if` 外移入 `res.ok` 分支 | `frontend/src/app.js`、`frontend/src/terminals/ui.js`、`frontend/dist/index.html` | 缺陷修复 |

### 0.2 更新要解决的现场故障（本报告的真实背景）

服务端日志（2026-09-21 07:18–07:25，Windows 实机 `192.168.66.110`）：

```
WARN at_pc_server::ws::handler: WebSocket registration handshake failed: Authentication token required
WARN at_pc_server::ws::handler: Rejected WebSocket connection from banned IP: 192.168.66.110   (每 5 秒一次)
```

机理：无凭据的 agent 每 5 秒重连一次 → 每次注册被拒并 `record_failure` → 10min 内累计 ≥30 次 → **封禁 900s** → 之后连"被拒"都升级为"直接拒绝连接"，形成完全无法接入的死循环。U1 从根上移除了这条失败路径。

### 0.3 ⚠️ 对上一轮文档结论的一处更正（本轮实测证伪）

| 上一轮文档的说法 | 本轮实测结论 | 证据 |
|---|---|---|
| 「MCP 配额…**仅作用于 `/mcp`**（`/api/*` 不受限）」 | **错误**。配额检查位于 `auth_layer`（`mcp/mod.rs:222` 起）内部；`auth_layer` 由 `.route_layer(axum::middleware::from_fn_with_state(...))` 挂载（`mcp/mod.rs:181-183`），是**全局中间件**，`/api/*` 一样要过。实测对 `GET /api/terminals` 直接返回 `{"error":"Too Many Requests: Request quota exceeded"}`（HTTP 429） | §2.C 的复位请求、§2.E1 |

**影响**：上一轮"REST 不吃配额、可放心用 REST 做侦察"的方法学前提**不成立**。所有 HTTP 请求（`/api/*` 与 `/mcp`）**共用同一个 120 req/60s 桶**，且实测该桶被一个**后台消费者**持续占用（见 §2.E1）。**两种 429 必须靠响应体区分**：

| 429 来源 | 响应体 | 附带头 |
|---|---|---|
| 鉴权限速（`AuthRateLimiter`） | `Too Many Requests: Rate limit exceeded` | `Retry-After` |
| 请求配额（`KeyQuotaTracker`） | `Too Many Requests: Request quota exceeded` | `Retry-After` + `x-ratelimit-limit` / `-remaining` / `-reset` |

---

## 1. 测试项清单

> 状态图例：⏳ 待测 / 🔄 进行中 / ✅ 通过 / ❌ 失败 / ⚠️ 通过但有风险

### A. U1 —— WS 免鉴权注册（`ws/handler.rs`）

| 编号 | 测试项 | 判定标准 | 状态 |
|---|---|---|---|
| **A1** | 不带 `auth_token` 的注册帧 | 收到 `RegisterAck{success:true}`，且服务端日志出现 `successfully registered via WebSocket (no key required)` | ✅ **通过** |
| **A2** | 携带**伪造** `auth_token` | 仍 `success:true`（证明凭据被完全忽略，而非"校验通过"） | ✅ **通过（=凭据被忽略）** |
| **A3** | 携带**合法 dv 令牌**（现场签发 `7ec7p000`）+ **不匹配**的 `terminal_id` | 旧行为应拒 `Terminal ID does not match bound device_id`；新行为若 `success:true` ⇒ 设备绑定失效 | ⚠️ **通过（=绑定已失效）** |
| **A4** | 携带 **root `sk` 令牌**（`brf2p000`）注册 | 旧行为应拒 `Operation token cannot be used for device registration`；新行为若 `success:true` ⇒ 设备域/操作域隔离失效 | ⚠️ **通过（=域隔离已失效）** |
| **A5** | 临时伪终端的可清理性 | `DELETE /api/terminals/:id` 返回 204，注册表恢复原状 | ✅ **通过（返回 200，终端数回到 2）** |
| **A6** | **负向对照**：HTTP 门禁未被误伤 | 非回环 `GET /api/terminals` 无凭据 → 401；`POST /mcp` 无凭据 → 401 | ✅ **通过（HTTP/MCP 两路 401，root 200）** |
| **A7** | 真机 Windows 在免鉴权注册后的可用性 | `/api/terminals` 显示 Online + 心跳 `last_heartbeat_elapsed_secs` 接近 0；一次真实工具调用成功返回 | ✅ **通过（心跳 1–2s，工具调用 exit=0 / 211ms）** |

### B. U2 —— tray 退出修复（`agent/src/app/ui.rs`）

| 编号 | 测试项 | 判定标准 | 状态 |
|---|---|---|---|
| **B1** | Windows 上 agent 运行形态确认 | 通过真实工具调用取得 `at-pc-agent.exe` 的 PID/路径/启动方式（是否 GUI 带托盘） | ⏳ |
| **B2** | 触发托盘「退出」后进程真的消失 | 旧行为会残留；新行为应为进程退出（`process::exit(0)`） | ⏳ |
| **B3** | 退出瞬间服务端是否感知断开 | 服务端日志出现 disconnect/`Marking Offline`，`/api/terminals` 状态转 Offline | ⏳ |
| **B4** | `process::exit(0)` 是否丢未 flush 的日志/审计（潜在缺陷） | 检查 agent 侧退出前日志是否完整落盘 | ⏳ |

### C. U3 —— 限流死循环修复（`mcp/mod.rs` + 前端）

| 编号 | 测试项 | 判定标准 | 状态 |
|---|---|---|---|
| **C1** | **无 token** 连续请求不计失败 | 连打 15 次无凭据请求：全部 401，**不出现 429** | ✅ **通过（15/15 全 401，零 429）** |
| **C2** | **错误 token** 仍会触发限速（负向对照） | 连打错误 token：第 10 次起出现 429（限速本体未被破坏）；控制在 <30 次以**避免真封 15 分钟** | ✅ **通过（第 10 次 429 `Rate limit exceeded`）** |
| **C3** | `dist/index.html` 与 `src/` 同步性 | 由 `src` 重新构建，产物与仓库 `dist` 一致（否则线上跑的是旧前端） | ✅ **通过（重建 sha256 逐字节相同）** |
| **C4** | 浏览器实测：无 token 打开 dashboard 后轮询是否停止 | 页面加载后**不再**每 2s 产生 `/api/terminals` 请求 | ⏳ |
| **C5** | Windows 实机不再被误封 | 观察期内 `Rejected ... banned IP` 日志不再出现，且实机保持 Online | ⏳ |

### D. 回归与门禁完整性

| 编号 | 测试项 | 判定标准 | 状态 |
|---|---|---|---|
| **D1** | 工作区全量测试 | `cargo test --workspace` 0 failed | ⏳ |
| **D2** | 计划自带门禁脚本 | `deploy/verify_gate.sh --fast` 全绿 | ⏳ |
| **D3** | 前端三件套 | lint 0 / typecheck 0 / test 全过 | ✅ **通过（lint 0 / typecheck 0 / test 34 pass 0 fail）** |
| **D4** | 门禁完整性（更新后仍成立） | 无凭据 / 伪造凭据 / **已吊销凭据** 在 HTTP 与 MCP 两路均被拒 | ⏳ |

### E. 方法学与副作用（贯穿）

| 编号 | 事项 | 状态 |
|---|---|---|
| **E1** | `/mcp` 配额被后台消费者占用的实测与影响评估 | ⏳ |
| **E2** | 测试副作用记录与清理（伪终端、可能的限速/封禁） | ⏳ |

---

## 2. 逐项实测记录

> 每测完一项追加一段：**方法 → 原始证据 → 判定**。

### 2.A U1 —— WS 免鉴权注册

**探测工具**：`/tmp/acc7/wsreg.mjs`（Node 22 全局 `WebSocket`，发一帧 `{"type":"Register","payload":{info[,auth_token]}}`，等 `RegisterAck` 后退出）。四个用例分别用独立的 `terminal_id`，全部经 `ws://192.168.99.94:9801/ws`。

#### A1 —— 不带 `auth_token`

| 项 | 值 |
|---|---|
| 请求帧 | `{"type":"Register","payload":{"info":{"terminal_id":"probe-acc7-a1-000001","hostname":"probe-host","username":"probe","lan_ip":"192.168.66.18","os_version":"Probe (macOS 27.0)","agent_version":"1.0.0"}}}`（**无 `auth_token` 字段**） |
| 响应 | `{"type":"RegisterAck","payload":{"success":true,"message":null,"heartbeat_interval_secs":5}}`（28ms 建连 / 33ms 收到 Ack） |
| 服务端日志 | `INFO ws::handler: Agent terminal [probe-acc7-a1-000001] successfully registered via WebSocket (no key required)` + `INFO ws::registry: Registered terminal: probe-acc7-a1-000001 (session: N)` + `Terminal [...] (probe-host) successfully registered (session: N) from IP: 192.168.66.18` |
| **判定** | ✅ **与更新意图一致**：无凭据即可注册。 |

#### A2 —— 携带伪造 `auth_token`

| 项 | 值 |
|---|---|
| `auth_token` | `atpc_dv_zzzz9999_AAAA…`（格式合法、库中不存在） |
| 响应 | `{"type":"RegisterAck","payload":{"success":true,"message":null,…}}`（27ms） |
| 服务端日志 | 同样 `successfully registered via WebSocket (no key required)`，**无任何 token 校验/拒绝痕迹** |
| **判定** | ⚠️ **通过但性质是"凭据被完全忽略"**：`ws/handler.rs` 已把 `auth_token` 绑成 `auth_token: _`，伪造值不再产生任何拒绝路径。这既是 U1 的预期效果，也是安全面收窄的直接证据。 |

#### A3 —— 合法 dv 令牌 + 不匹配 `terminal_id`（设备绑定失效验证）

为本次测试**现场签发**一把 dv 令牌（把线上的 `jvxd6000` 明文已不可得视为不可用）：

```
POST /api/keys  {"name":"acc7-live-verify-dv","kind":"dv","role":"viewer","scopes":["terminal"],
                 "device_id":"laptop-roeh3p85-b6a918"}
→ {"key":{"id":"7ec7p000","kind":"dv","device_id":"laptop-roeh3p85-b6a918",…},
   "plaintext":"atpc_dv_7ec7p000_NKQaMgy…"}
```

| 项 | 值 |
|---|---|
| 场景 | 用上述 dv 令牌注册 `terminal_id = probe-acc7-a3-000003`（**与绑定的 `laptop-roeh3p85-b6a918` 不一致**） |
| 响应 | `{"type":"RegisterAck","payload":{"success":true,"message":null,…}}`（30ms） |
| 旧行为对照（代码） | 删除前应返回 `Terminal ID does not match bound device_id` 并拒绝 |
| **判定** | ⚠️ **通过（行为已放宽）**：`device_id ⇄ terminal_id` 绑定在 WS 注册路径上**不再生效**。 |

#### A4 —— 携带 root `sk` 令牌注册（设备域/操作域隔离失效验证）

| 项 | 值 |
|---|---|
| 场景 | 用 `brf2p000`（`bootstrap-root`，`kind=sk`，admin）注册 `terminal_id = probe-acc7-a4-000004` |
| 响应 | `{"type":"RegisterAck","payload":{"success":true,"message":null,…}}`（32ms） |
| 旧行为对照（代码） | 删除前应返回 `Operation token cannot be used for device registration` 并拒绝 |
| **判定** | ⚠️ **通过（行为已放宽）**：操作域令牌可直接当设备令牌用，两域隔离在 WS 路径上**不再生效**。 |

#### A5 —— 伪终端清理

```
DELETE /api/terminals/probe-acc7-a1-000001 → 200
DELETE /api/terminals/probe-acc7-a2-000002 → 200
DELETE /api/terminals/probe-acc7-a3-000003 → 200
DELETE /api/terminals/probe-acc7-a4-000004 → 200
清理后 GET /api/terminals 终端数 = 2   （回到测试前的 laptop-roeh3p85-b6a918 / clkjdemacbook-prolocal-000000）
```
**判定**：✅ 通过。注册表已恢复原状，测试未留残留终端。

#### A6 —— 负向对照：HTTP/MCP 门禁未被 U1 误伤

本站点（`192.168.66.18`）对服务端是**非回环**来源，因此门禁为全量生效态：

| 请求 | 结果 |
|---|---|
| `GET /api/terminals` 无凭据 | **401** |
| `GET /api/terminals` 乱造 `sk` 令牌 | **401** |
| `GET /api/audit` 无凭据 | **401** |
| `POST /mcp`（`tools/list`）无凭据 | **401** |
| `POST /mcp`（`tools/list`）乱造令牌 | **401** |
| `GET /api/terminals` 带 root 令牌 | **200** |

**判定**：✅ 通过。U1 只放宽了 **WS 设备接入**这一条路径，HTTP REST 与 MCP 两条路径的门禁**依然成立**。

#### A7 —— 真机 Windows 在免鉴权注册后的可用性

分两拍取证：**注册状态**（`/api/terminals`）与**真实能力**（对实机发一次真实工具调用）。

**① 注册状态（REST 与 MCP 两路各取一次，互相印证）**

```json
// GET /api/terminals   →  laptop-roeh3p85-b6a918
{
  "info": {"terminal_id":"laptop-roeh3p85-b6a918","hostname":"LAPTOP-ROEH3P85","username":"CLKJ",
           "lan_ip":"169.254.190.106","os_version":"Windows 11 (26100)","agent_version":"1.0.0"},
  "custom_name":"Windows实机测试终端","notes":"自动化测试与实机核验","tags":["windows11","real-test"],
  "status":"Online",
  "latest_metrics":{"cpu_usage_percent":2.7114563,"memory_used_mb":12998,"memory_total_mb":32280,
                    "uptime_secs":14446779,"timestamp":1789979786},
  "last_heartbeat_elapsed_secs":2
}
```

| 指标 | 值 | 判定 |
|---|---|---|
| `status` | `Online` | ✅ |
| `last_heartbeat_elapsed_secs` | **1～2**（REST 与 MCP 两次取样一致） | ✅ 心跳在跑 |
| `cpu_usage_percent` | 2.71% | 指标新鲜 |
| `memory_used_mb / memory_total_mb` | 12998 / 32280 | 指标新鲜 |
| `uptime_secs` | 14446779（≈167 天） | 宿主机未重启，与"注册断开"无关 |

> **⚠️ 与 §0 基线不符的一处变化（必须记录）**：`lan_ip` 已由 `192.168.66.110`（WLAN）变为
> **`169.254.190.106`** —— 这是 **APIPA 链路本地地址**，说明实机无线网卡当前**未取得 DHCP 租约**。
> 但它**仍能与 `192.168.99.94` 建立 WS 长连接**（心跳 1–2s 为证），说明流量走的是另一条可用链路
> （有线网卡或已存在的连接）。**这一点直接改变 C5 的判据**（见 §2.C5）：服务端日志里的
> `banned IP: 192.168.66.110` 与当前实机 IP 已不是同一个地址。

**② 真实工具调用（对实机，非本机）**

| # | 工具 | 命令 | 结果 |
|---|---|---|---|
| 1 | `exec_cmd` | `hostname & whoami & ver & …` | `exit_code=0`，**211 ms**，stdout：`LAPTOP-ROEH3P85` / `laptop-roeh3p85\clkj` / `Microsoft Windows [版本 10.0.26100.3323]` |
| 2 | `exec_cmd` | `tasklist \| findstr /I at-pc` | `exit_code=0`，**331 ms**，stdout：`at-pc-agent.exe  19868 Console  2  192,520 K` |
| 3 | `exec_powershell` | `Get-Process at-pc-agent`（含 exe 路径/启动时间/主窗口句柄） | `exit_code=0`，**294 ms**，见 §2.B1 |
| 4 | `list_monitors` | — | `2880x1800, scale_factor=2.0, DISPLAY1, is_primary=true` |

**判定**：✅ **通过**。免鉴权注册后的实机**功能完整可用**：状态 Online、心跳秒级、`exec_cmd` /
`exec_powershell` / 只读枚举类工具全部在数百毫秒内成功返回。A 组结论闭环 —— U1 达到了"解开接入死循环"
的目的，代价是接入侧零门禁（见下小结）。

> **⚠️ 顺带发现的既存缺陷（非本次更新引入，记入观察项 O1）**：`exec_cmd` 回传的中文为
> **GBK 字节被按 UTF-8 解码**，`Microsoft Windows [版本 10.0.26100.3323]` 变成
> `Microsoft Windows [�汾 10.0.26100.3323]`。`exec_powershell` 侧不存在此问题（脚本内显式
> `[Console]::OutputEncoding = UTF8` 后中文完全正常）。根因指向 `exec_cmd` 通路未做 CP936→UTF-8 转码。

#### A1–A7 小结（安全影响，必须让决策人看见）

U1 把"设备接入"这条路径的门禁**整体去掉了**：任何能访问 `ws://<server>:9801/ws` 的网络实体，无需任何凭据即可注册成终端，注册后即暴露该 agent 的全部能力面（`exec_cmd` / `exec_powershell` / 读写文件 / 截屏 / UI 自动化 / 桌面流）。按 §0.2 的故障背景，这是为解开"无凭据 agent 反复失败→被误封→彻底无法接入"而死循环所做的取舍；但它把安全边界从"持有 dv 令牌"退化为"能否连到 9801 端口"。**建议**：至少保留 dv 令牌校验、仅把"无令牌"从**拒绝**改为**允许但降级/告警**，或把 `device_id` 绑定与 `kind` 判定保留下来（这两项与死循环故障无因果关系，删它们并不必要）。

### 2.B U2 —— tray 退出修复

#### B1 —— Windows 上 agent 运行形态确认

命令行与 PowerShell 两路交叉取证（`tasklist` 给出会话名，`Get-Process` 给出路径与窗口句柄）：

| 项 | 值 | 来源 |
|---|---|---|
| 进程名 / PID | `at-pc-agent.exe` / **19868** | `tasklist`、`Get-Process` |
| 可执行文件路径 | `C:\Users\CLKJ\Downloads\at-pc-windows-x86_64(1)\at-pc-agent.exe` | `Get-Process` |
| 启动时间 | **2026/9/21 16:12:14** | `Get-Process.StartTime` |
| 会话名 / 会话号 | **`Console` / 2** | `tasklist` |
| 主窗口标题 | **`at-pc 终端代理`** | `Get-Process.MainWindowTitle` |
| 主窗口句柄 | **`3344964`**（非 0） | `Get-Process.MainWindowHandle` |
| 工作集 | 196,984,832 B（≈187.9 MiB） | `Get-Process.WorkingSet64` |
| 屏幕 | 2880×1800，`scale_factor=2.0`，单显示器 | `list_monitors` |

**同目录文件清单（用于判断"是否刚重新部署"，技能里注明这是门禁失效的头号原因）**：

| 文件 | 大小 | mtime |
|---|---|---|
| `at-pc-agent.exe` | 9,671,168 | 2026/9/21 **15:51:32** |
| `agent_config.toml` | 176 | 2026/9/21 **16:12:31** |
| `at-pc-agent.log` | **0** | 2026/9/21 **16:12:14** |
| `at-pc-server.exe` | 7,241,728 | 2026/9/21 15:51:38 |
| `VERSION` | 6 | 2026/9/21 15:50:32 |

**判定**：✅ **通过**。三点结论：
1. agent 是 **GUI 形态**（`MainWindowTitle`/`MainWindowHandle` 均非空、会话名为 `Console`），
   即 `crates/agent/src/app/ui.rs` 的 eframe 主循环与托盘 `muda` 菜单**真实在跑** —— 这是 B2 的前提；
2. 本次部署时间（exe `15:51:32` / 启动 `16:12:14`）**晚于**被测提交 `d4652de` 的落地，
   与 §2.C3 的字节级指纹一致，说明实机跑的确实是含 U2 的构建；
3. **`agent_config.toml` 里 `enable_computer_use = false`** ⇒ 按 at-pc 的三层策略，**变更类工具
   （`mouse_click` / `focus_window` / `close_window` / `batch_actions` / `click_mark` …）会被
   agent 侧门禁整段拒绝**。B2 必须用鼠标点托盘菜单，**因此 B2/B3 的 GUI 操作需要先开这道闸**
   （改配置 + 重启 agent，属安全策略变更，见 §2.B2）。

> **⚠️ 观察项 O2（与 B4 直接相关）**：`at-pc-agent.log` **自进程启动（16:12:14）起一直是 0 字节**。
> 这意味着托盘退出分支里新增的那行 `tracing::info!("User requested quit from system tray. Shutting down...")`
> **本来就不会落到这个文件**（要么日志走的是别处，要么根本没初始化文件 appender）。
> 这会把 B4 的结论从"`process::exit(0)` 可能丢日志"修正为"**该路径的日志本来就不可观测**"——
> 详见 §2.B4。

#### B2 —— 触发托盘「退出」后进程是否真的消失

（待填 —— 需先开 `enable_computer_use` 门禁）

#### B3 —— 退出瞬间服务端是否感知断开

（待填 —— 依赖 B2）

#### B4 —— `process::exit(0)` 是否丢未 flush 的日志/审计

（待填）

### 2.C U3 —— 限流死循环修复

**测试端点**：`GET /api/terminals`。选它有两个理由：① 它**不带 token 时也能走到限速分支**；
② 它**经过全局 `auth_layer`**（§0.3），所以能同时暴露配额与限速两层行为。
两种 429 靠响应体区分（`Rate limit exceeded` = 限速 / `Request quota exceeded` = 配额）。

> **测试前后的自我约束**：限速按**来源 IP** 计（本站点 `192.168.66.18`）。为避免因测试把自己
> 打成 900s 封禁，每次开测前先做**一次成功鉴权**（`record_success` 会 `entries.remove(&ip)` 清空该 IP
> 的全部失败记录与封禁），且 C2 严格只打 **10** 次（阈值 30）。

#### C1 —— 无 token 连续请求不计失败

| # | 请求 | HTTP | 响应体 |
|---|---|---|---|
| 1–15 | `GET /api/terminals`（**完全不带 `Authorization` 头**） | **401 ×15** | `{"error":"Unauthorized: Invalid or missing authentication token"}` |

| 项 | 值 |
|---|---|
| 出现 429 吗 | **没有，15 次全部 401** |
| 对照旧行为的反证 | 若仍按老逻辑 `record_failure`，第 10 次就应出现 429 —— 实测未出现 |
| **判定** | ✅ **通过**。U3a 生效：`token_opt.is_none()` 的请求既不计失败，也不推进限速窗口。对前端"未登录时轮询"这一场景，服务端不再把它当攻击。 |

#### C2 —— 错误 token 仍会触发限速（负向对照）

| # | 请求 | HTTP | 响应体 |
|---|---|---|---|
| 复位 | `GET /api/terminals` + **有效** token | 429（**配额**，见下注） | `Too Many Requests: Request quota exceeded` |
| 1–9 | 同上路径 + `Authorization: Bearer atpc_sk_zzzz0000_WRONGWRONG…` | **401 ×9** | `{"error":"Unauthorized: Invalid or revoked authentication token"}` |
| **10** | 同上 | **429** | **`{"error":"Too Many Requests: Rate limit exceeded"}`** |
| 复位 | 有效 token | 429（配额） | `Too Many Requests: Request quota exceeded` |

| 项 | 值 |
|---|---|
| 触发点 | **第 10 次**，与 `rate_limit.rs` 的 `failures_60s >= 10 ⇒ RateLimited{60}` 完全一致 |
| 未误伤封禁 | 测试后未进入 10min/30 次的封禁分支（只打了 10 次） |
| **判定** | ✅ **通过**。U3a 只关闭了"**无** token"这一条计数路径，**"错** token"依旧计数并触发限速 —— 限速本体没有被削弱。这正是 U3 想要的语义：**区分"没凭证"（正常，不罚）与"凭证错"（可疑，要罚）**。 |

> **注（复位请求为何也回 429）**：复位用的是**有效 token**，鉴权本应成功；但它仍回 429，因为
> `auth_layer` 的执行顺序是 **① 解析凭据/角色 → ② `record_success` 清零 → ③ RBAC → ④ 配额检查**。
> 到第 ④ 步时配额桶已满，于是响应码被覆盖为 429。**但第 ② 步已经执行**，所以 C2 确实是在"计数已清零"
> 的干净状态下开始的 —— 这一点让 C2 的"第 10 次触发"成为强证据，而不是被历史失败数污染的巧合。

#### C3 —— `dist/index.html` 与 `src/` 同步性

| 步骤 | 命令 | 产物 sha256 | 字节 |
|---|---|---|---|
| 基线（仓库 `dist`） | `shasum -a 256 dist/index.html` | `11600ab33c1525b021d26244749930262c5f235e49d7e53ef5450fd36294614f` | 86928 |
| **由 `src` 重建** | `npm run build`（vite 7.3.6，16 modules，88 ms，exit 0） | `11600ab33c1525b021d26244749930262c5f235e49d7e53ef5450fd36294614f` | 86928 |
| 线上部署 | `curl -I http://192.168.99.94:9800/` | 同上（§0 基线已比过） | `content-length: 86928` |

**判定**：✅ **通过**。**三方逐字节一致** —— `src` 重建 == 仓库 `dist` == 线上部署，
说明：① 线上跑的前端就是含 U3b 的那份；② 仓库里的 `dist` 不是"手改过的孤儿产物"（可复现）。
**副作用**：构建过程会 `emptyOutDir` 覆盖 `dist/`，已先备份到 `/tmp/acc7/dist_before.html`；
因产物逐字节相同，工作树未被改动（`git status` 干净）。

#### C4 —— 浏览器实测：无 token 打开 dashboard 后轮询是否停止

（待填）

#### C5 —— Windows 实机不再被误封

（待填）

### 2.D 回归与门禁完整性

（待填）

### 2.E 方法学与副作用

（待填）

---

## 3. 总结论

（待全部实测完成后填写）
