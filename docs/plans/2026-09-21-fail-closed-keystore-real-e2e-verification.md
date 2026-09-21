# 当前更新（`26c38d6` → `9df6fe6`）真实环境逐项测试记录

> **被测基线**：`9df6fe6`（工作树 HEAD）
> **上游已验**：`26c38d6`（P1-24 / P1-27）见 `2026-09-21-api-key-gate-acceptance-review.md` §13
> **同期他人报告**：`2026-09-21-d4652de-update-live-verification.md`（覆盖 U1/U3 主体，**B/C 组尾部与 D/E 组未完成**）
> **本文档定位**：**独立复验 + 补齐空缺**。既覆盖 `9df6fe6`（此前无任何报告），也对既报结论做对抗性复核。
> **用法**：§1 = 测试项清单（状态随测试推进更新）；§2 = 逐项实测记录（测完一项追加一段）。

---

## 0. 本次累计更新改了什么

| 提交 | 内容 | 性质 |
|---|---|---|
| `d4652de` | **U1** WS 注册**不再要求任何凭据**（`ws/handler.rs` 净 −111 行，`auth_token: _` 直接丢弃） | ⚠️ 安全面收窄 |
| `d4652de` | **U2** tray 退出修复（`agent/src/app/ui.rs`：`ViewportCommand::Close` → `tray=None; exit(0)`） | 缺陷修复 |
| `d4652de` | **U3a** 无 token 请求不再计为鉴权失败（`mcp/mod.rs`）；**U3b** 前端 401 后停轮询 | ⚠️ 限速面收窄 / 缺陷修复 |
| `9df6fe6` | **U4a** `config.rs:handler_min_role` 新增两条映射：`desktop/frame.jpg` ⇒ **Viewer**（此前**无映射**）；`desktop/input` POST ⇒ **Admin**（此前**无映射**，但 handler 内**早已有** `role != Admin → 403` 自检） | 补全 RBAC 契约 |
| `9df6fe6` | **U4b** 路径含 `/desktop/` 的请求**完全免配额**（不计入 `KeyQuotaTracker`，也不回 `x-ratelimit-*`） | ⚠️ 配额面收窄 |

> ⚠️ **本表 U4a 一处更正**：初稿曾写「`frame.jpg` 纳入 handler 级 **Admin** 要求」，经 §3.C/C1 代码比对与 C3 实测**证伪**——`frame.jpg` 实际映射为 **Viewer**（`Role` 序为 Viewer < Operator < Admin，故 Viewer 门槛**等价于「有有效凭据即可」**，不构成实质收紧）；真正被约束到 **Admin** 的是 `desktop/input`。

**U1 的现场成因**（来自同期报告 §0.2，本文档 §2.A 会独立验证）：agent 配置里**没有令牌** ⇒ 每 5 s 重连被拒 ⇒ 累计 ≥30 次失败 ⇒ 封禁 900 s ⇒ 永久无法接入。

---

## 1. 测试环境与角色

| 角色 | 值 |
|---|---|
| **现网 server**（只读对照） | Linux，`http://192.168.99.94:9800`，WS `ws://192.168.99.94:9801/ws`；**无 shell、不可重启** |
| **被测 server**（本地，可任意折腾） | 本机 `0.0.0.0:9800/9801`，二进制 = HEAD `9df6fe6` release（`--no-default-features`），数据目录 `/tmp/acc8/` |
| **真实客户端** | Windows 11 主机 `LAPTOP-ROEH3P85`（WLAN `192.168.66.110/24`，网关 `.1`），终端 `laptop-roeh3p85-b6a918`，`enable_computer_use=**false**`（实测更正，见 §3.A） |
| **主 agent 实例** | PID 19868，无命令行参数，指向现网 `ws://192.168.99.94:9801/ws` —— **全程不动**（隔离策略：另起第二实例连本机） |

> 本机 `192.168.66.18` 与 Windows 主机同网段 ⇒ 第二实例可直连本机被测 server 做真实端到端（**§3.A/A5 已实测证实**）。
> ⚠️ Windows 上报的 `lan_ip` 为 `169.254.190.106`（APIPA，agent 多网卡选网卡缺陷），**并非**其真实 WLAN 地址；主实例与第二实例均如此，属既有缺陷、非本次更新引入。

---

## 2. 测试项清单

状态：⏳ 待测 / ✅ 通过 / ❌ 失败 / ⚠️ 通过但有风险 / ⛔ 无法测（附原因）

### A 组 · 环境与构建身份

| 编号 | 测试项 | 判据 | 状态 |
|---|---|---|---|
| A1 | 本地被测构建身份 | release 二进制出自 HEAD，工作树仅含本文档 | ✅ |
| A2 | 现网 HTTP 门禁基线 | 非回环 `/health`、`/api/terminals`、`POST /mcp` 无凭据 → 401；`/` → 200 静态壳 | ✅ |
| A3 | **现网构建版本判定** | 用「无令牌 agent 仍 Online」三重证据证明现网 ≥ `d4652de` | ✅ |
| A4 | Windows agent 运行形态 | PID / exe 路径 / cmdline / `AT_PC_DEVICE_TOKEN` 环境变量 / `enable_computer_use` | ✅ |
| A5 | Windows → 本机被测 server 可达 | `Test-NetConnection 192.168.66.18 -Port 9801` = True | ✅ |

### B 组 · U1 WS 免鉴权注册（`d4652de`）

| 编号 | 测试项 | 判据 | 状态 |
|---|---|---|---|
| B1 | 本地 server：**无 token** 注册 | `success:true` + 日志 `successfully registered via WebSocket (no key required)` | ✅ |
| B2 | 本地 server：**伪造 token** 注册 | 仍 `success:true`（证明凭据被**丢弃**而非"校验通过"） | ✅ |
| B3 | 本地 server：**合法 dv** + **不匹配** `terminal_id` | 旧行为应拒 `Terminal ID does not match bound device_id`；新行为 `success:true` ⇒ 设备绑定失效 | ✅ |
| B4 | 本地 server：**root `sk`** 注册 | 旧行为应拒 `Operation token cannot be used for device registration`；新行为 `success:true` ⇒ 域隔离失效 | ✅ |
| B5 | **真实 Windows agent（第二实例）无令牌注册** | 注册成功，本地控制台可见该终端 | ✅ |
| B6 | **真实客户端**：令牌吊销后仍能注册 | 先签发 dv 并成功注册 → 控制台 `DELETE` 吊销 → **不重启**再注册仍 `success:true` | ✅ |
| B7 | 负向对照：HTTP/MCP 门禁未被误伤 | 无凭据 `GET /api/keys` / `POST /mcp` → 401；root → 200 | ✅ |

### C 组 · U4 桌面通道（`9df6fe6`）

| 编号 | 测试项 | 判据 | 状态 |
|---|---|---|---|
| C1 | `POST /api/terminals/<id>/desktop/input` 的 handler 级要求 | 提交该提交前的版本 vs 现版本对比，确认由"无要求"变为 `Admin` | ⚠️ |
| C2 | `desktop/input` 用 **viewer** 令牌调用 | 期望 **403**（收紧生效） | ✅ |
| C3 | `desktop/frame.jpg` 用 viewer 令牌调用 | ~~期望 **403**~~ **判据有误**：实测为 **204**（viewer 不被拒） | ⚠️ |
| C4 | `/desktop/` 路径**免配额** | 连打 **>120 次** `/desktop/frame.jpg`：不出现 429，且响应无 `x-ratelimit-*` | ✅ |
| C5 | 对照：非 `/desktop/` 端点仍吃配额 | 连打 >120 次普通端点：第 121 次起 429 `Request quota exceeded` | ✅ |
| C6 | 绕过探针：能否让非流式端点也免配额 | 试 `?q=/desktop/`、`/desktop/../keys`、`%2Fdesktop%2F` 等：均不得改变配额判定 | ❌ **发现绕过** |
| C7 | 风险记录：豁免端点无配额上限 | 记录"不限频次"的后果与建议 | ✅ |

### D 组 · U2 tray 退出（`d4652de`，仅在第二实例上做）

| 编号 | 测试项 | 判据 | 状态 |
|---|---|---|---|
| D1 | 第二实例启动形态 | 进程存在、exe 路径、是否带托盘 | ✅ |
| D2 | 触发托盘退出后进程真的消失 | **新行为**：进程退出；对照 `d4652de^` 旧行为残留 | ✅ |
| D3 | 退出瞬间服务端感知 | 本地 server 日志出现断开、终端转 `Offline` | ✅ |
| D4 | 退出路径是否丢未 flush 日志 | 检查退出前日志完整性 | ✅ |

### E 组 · 回归

| 编号 | 测试项 | 判据 | 状态 |
|---|---|---|---|
| E1 | 工作区全量测试 | `cargo test --workspace` 0 failed | ❌ **1 项失败** |
| E2 | 自带门禁脚本 | `deploy/verify_gate.sh --fast` 全绿 | ⏳ |
| E3 | 前端三件套 | lint 0 / typecheck 0 / test 全过 | ✅ |

### F 组 · 文档与清理

| 编号 | 测试项 | 判据 | 状态 |
|---|---|---|---|
| F1 | 安全放宽是否写入文档 | U1/U3a/U4b 三处放宽是否有任何发布说明/文档声明 | ❌ **未写入** |
| F2 | 清理与副作用核对 | 第二实例已退出、临时文件删除、端口回收、主实例仍 Online | ⏳ |

---

## 3. 逐项实测记录

> 每测完一组即追加。原始证据（请求/响应/日志片段）保留在 `/tmp/acc8/`。

### 3.A A 组 · 环境与构建身份 ✅

**A1 · 本地构建身份** ✅——二进制与 HEAD 同一代
```
target/release/at-pc-server   mtime = 2026-09-21 17:11   size = 5961456
git HEAD                      9df6fe6  commit 时间 = 2026-09-21 16:58:44
git status --porcelain        ?? docs/plans/2026-09-21-fail-closed-keystore-real-e2e-verification.md
```
⇒ 二进制 mtime **晚于** HEAD 提交 12 分钟（编译发生在提交之后），工作树仅含本文档（无其他改动）⇒ 被测二进制确为 HEAD `9df6fe6`。

> 附带观察：`at-pc-server --version` 在 CWD 存在**空 keys.json** 时会先跑 bootstrap 并 fail-closed 退出（`Refusing to start.`），即 `--version` 不能作为纯版本查询使用。属 P1-24 fail-closed 的连带表现，非本次更新引入。

**A2 · 现网 HTTP 门禁基线** ✅——`http://192.168.99.94:9800`
| 请求（无凭据） | 结果 |
|---|---|
| `GET /health` | **401** |
| `GET /api/terminals` | **401** |
| `GET /api/keys` | **401** |
| `POST /mcp`（initialize） | **401** |
| `GET /` | **200**，`content-type: text/html`，`content-length: 86928` |

401 文案：`{"error":"Unauthorized: Invalid or missing authentication token"}`。
⇒ 非回环一律要鉴权（含 `/health`），静态壳放行；`content-length 86928` 与 §0 基线 sha256 一致。

**A3 · 现网构建版本判定** ✅——三重证据，现网 **≥ `d4652de`**
1. 401 文案 `Invalid or missing authentication token` 属 ≥ `c997bbd` 的措辞。
2. **决定性**：`list_terminals`（经现网 MCP）显示 Windows 终端 `laptop-roeh3p85-b6a918` 状态 **Online**，`last_heartbeat_elapsed_secs = 2`；而该 agent 命令行**无任何令牌**（见 A4）⇒ 现网必须已含 U1「WS 免鉴权注册」，即 **≥ `d4652de`**。
3. 静态壳 `content-length = 86928` 与 §0 基线逐字节同尺寸。

**A4 · Windows agent 运行形态** ✅
```
PID            19868
ParentPID      10000 (explorer.exe)
ExecutablePath C:\Users\CLKJ\Downloads\at-pc-windows-x86_64(1)\at-pc-agent.exe
CommandLine    "C:\Users\CLKJ\Downloads\at-pc-windows-x86_64(1)\at-pc-agent.exe"      ← 无任何参数
```
配置文件二选一（无 `-c` 参数 ⇒ 走默认查找）：

| 路径 | mtime | `enable_computer_use` |
|---|---|---|
| `…\at-pc-windows-x86_64(1)\agent_config.toml`（**exe 同目录，实际生效**） | 2026/9/21 16:12:31 | **false** |
| `…\Downloads\agent_config.toml`（未使用） | 2026/9/20 17:25:09 | true |

两份均 `url = "ws://192.168.99.94:9801/ws"`、`device_id = "auto"`。
⇒ 与 §1 原写「`enable_computer_use=true`」**不符**，实测生效值为 **false**（§1 已更正）。
agent 日志佐证：`Terminal [laptop-roeh3p85-b6a918] successfully registered with server`（连 192.168.99.94，**全程无令牌**）。

**A5 · Windows → 本机被测 server 可达** ✅——**此前"不可达"是假象**
```
ping 192.168.66.18                        -> True
Test-NetConnection 192.168.66.18 -Port 9801 -> True
Test-NetConnection 192.168.66.18 -Port 9800 -> True
GET http://192.168.66.18:9800/health        -> 401（非回环，正确）
GET http://192.168.66.18:9800/api/terminals -> 401
```
**排障记录（重要）**：先前一次测得 `False`，误判为「macOS 防火墙 / 路由隔离」。复测发现：
- Windows 网卡：WLAN = `192.168.66.110/24`（Preferred），默认网关 `192.168.66.1`；`Find-NetRoute 192.168.66.18` 与 `192.168.99.94` **均走 WLAN**（前者同网段直连，后者经网关）。
- macOS 侧：`socketfilterfw` 显示 `Firewall is disabled`、block-all 关闭、隐身模式关。
- **真因**：**本地被测 server 当时并未运行**。宿主的 Bash 会话结束会回收进程组，`nohup … &` 挡不住该回收，故 server 起后即被清掉（日志有 `Starting`/`Listening` 但端口随即消失）。改用**受管后台任务**（`run_in_background`）启动后持续存活，A5 立即 True。
⇒ 结论：网络本就通，A5 不是环境障碍。**后续所有本地 server 用例均须以受管后台任务方式启动**。

### 3.B B 组 · U1 WS 免鉴权注册 ✅（**核心安全回归**）

探针：`/tmp/acc7/wsreg.mjs`（Node 22 全局 WebSocket，直接发 `{type:"Register",payload:{info[,auth_token]}}`，读 `RegisterAck.payload.success`）。
本地被测 server：`ws://127.0.0.1:9801/ws`，root key `atpc_sk_das76000_…`。

| 用例 | 发送的凭据 | `RegisterAck` | 判定 |
|---|---|---|---|
| **B1** | **无** | `{"success":true,"message":null,"heartbeat_interval_secs":5}` | ✅ |
| **B2** | `atpc_dv_fffff000_THIS_IS_A_FORGED_TOKEN_zzzz`（伪造） | `{"success":true,…}` | ✅ |
| **B3** | `atpc_dv_kyr4p000_…`（真 dv，绑定 `bound-device-aaa`）+ `terminal_id=b3-mismatch-tid`（**不匹配**） | `{"success":true,…}` | ✅ |
| **B4** | `atpc_sk_das76000_…`（**root admin sk**） | `{"success":true,…}` | ✅ |

三例全部 `success:true` 且 `message:null` ⇒ **凭据被整体丢弃**，而非"校验通过"。

**B3/B4 的旧行为对照**（`git show d4652de^:crates/server/src/ws/handler.rs` 确认旧代码确有拦截）：
```
419/424: "Operation token cannot be used for device registration"   ← 用于拦 root sk（B4）
436/441: "Terminal ID does not match bound device_id"              ← 用于拦设备绑定不匹配（B3）
 29    : AgentToServerMessage::Register { info, auth_token } =>
 30    :     if let Some(ref token) = auth_token {                 ← 旧的有凭据分支
```
现版本 `handler.rs` 已改为 `Register { info, auth_token: _ }`，两侧拦截**全部消失**。
⇒ **结论：设备绑定（B3）与凭据域隔离（B4）双双失效。**

**B5 · 真实 Windows agent 第二实例无令牌注册** ✅
在 Windows 实机以**独立进程**启动第二实例（主实例 PID 19868 不受影响）：
```
at-pc-agent.exe -s ws://192.168.66.18:9801/ws -H --no-tray      ← 无 --device-token
```
第二实例 PID **22092**，自报日志：
```
INFO at_pc_agent: Starting headless agent [laptop-roeh3p85-b6a918] targeting server: ws://192.168.66.18:9801/ws
INFO at_pc_agent::ws_client: Connecting to server WebSocket at 192.168.66.18:9801/ws (TLS: false)...
INFO at_pc_agent::ws_client: Terminal [laptop-roeh3p85-b6a918] successfully registered with server
```
本地被测 server 侧同步登记：
```
INFO at_pc_server::ws::handler: Agent terminal [laptop-roeh3p85-b6a918] successfully registered via WebSocket (no key required)
INFO at_pc_server::ws::registry: Registered terminal: laptop-roeh3p85-b6a918 (session: 6)
```
⇒ **真实客户端、真实网络、无令牌、跨机注册成功**（且为此前实测不可达的 Windows→macOS 链路，A5 复证）。

**B6 · 令牌吊销后仍能注册** ✅——单凭据双向对照（最强证据）
| 步骤 | 操作 | 结果 |
|---|---|---|
| 1 | 吊销前，用该 dv 作 API 凭据 `GET /api/terminals` | **403**（viewer 无 console scope） |
| 2 | `DELETE /api/keys/kyr4p000` | **204** |
| 3 | 吊销后，同一 dv 再作 API 凭据 | **401**（凭据确已失效） |
| 4 | 吊销后，**同一 dv** 发 WS `Register` | **`success:true`** ✅ |

`keys.json` 侧确认 `kyr4p000 … revoked_at = 2026-09-21T09:22:24Z`。
⇒ **同一个凭据，HTTP 侧已失效（401），WS 注册侧照过** —— 注册路径根本不查凭据。

**B7 · 负向对照（门禁未被误伤）** ✅——本地 server
```
无凭据 GET /api/keys      -> 401
无凭据 GET /api/terminals -> 401
无凭据 POST /mcp          -> 401
root   GET /api/keys      -> 200
root   GET /api/terminals -> 200
回环   GET /health        -> 200
```
⇒ U1 的放宽**仅限 WS Register**，HTTP/MCP 鉴权链完好。

**本组结论（安全影响）**：U1 把设备侧接入门禁**整体移除**。任何能连到 9801 端口的进程，不需任何凭据、不受设备绑定约束，即可注册为一个终端并（按 server 授权模型）获得对该终端的操作通道。这是**明确的攻击面扩大**，属**发布前须显式声明**的行为变更——目前三个提交均无任何发布说明（见 F1）。

### 3.C C 组 · U4 桌面通道与配额

**C1 · 代码层比对** ⚠️——**判据需修正**

`9df6fe6` 对 `crates/server/src/config.rs` 的实际改动（提交 stat 显示该提交**未触碰** `dashboard.rs`）：
```diff
-    if method == "GET" && … && trimmed_path.ends_with("/desktop/frame") {
+    if method == "GET" && … && (trimmed_path.ends_with("/desktop/frame") || trimmed_path.ends_with("/desktop/frame.jpg")) {
         return Some(Role::Viewer);
     }
     if method == "GET" && … && trimmed_path.ends_with("/desktop/raw") {
         return Some(Role::Viewer);
     }
+    if method == "POST" && … && trimmed_path.ends_with("/desktop/input") {
+        return Some(Role::Admin);
+    }
```
`config.rs:368 handler_min_role()` 语义（`mcp/mod.rs:381-391`）：返回 `Some(min_role)` 时做 `role < min_role → 403`；返回 **`None` 则完全不做 handler 级角色检查**。
`protocol/src/tools.rs:10`：`enum Role { Viewer, Operator, Admin }`（`Ord` 按声明序）⇒ **`min_role = Viewer` 等价于零门槛**。

⇒ 准确结论：
- **`desktop/input`**：此前 `config.rs` 无映射，但 `dashboard.rs` handler 内**早已有** `if role != Role::Admin → 403`（`9df6fe6^` 第 793 行与现版**逐字相同**）⇒ 该端点**前后都要求 Admin**，U4a 对它只是「配置层显式化」，**无安全增量**。
- **`frame.jpg`**：此前**无任何角色约束**（`config.rs` 无映射、handler 无自检）⇒ 任意有效凭据可访问；U4a 新增 `Viewer` 映射 ⇒ **仍等价于「任意有效凭据可访问」**，**同样无安全增量**。
⇒ 文档初稿「frame.jpg 纳入 Admin 要求」**错误**，已按实测更正（§0 / §2）。

**C2 · `desktop/input` 三层鉴权** ✅（本地 server，终端 `laptop-roeh3p85-b6a918` 在线）
```
无凭据   POST /api/terminals/<tid>/desktop/input  -> 401  {"error":"Unauthorized: Invalid or missing authentication token"}
viewer   POST /api/terminals/<tid>/desktop/input  -> 403  {"error":"Forbidden: requires role 'admin', current role is 'viewer'"}
admin    POST /api/terminals/<tid>/desktop/input  -> 200  {"success":true}
```
请求体 `{"action":"MouseMove","data":{"x":10,"y":10}}`（`DesktopInputEvent` 为 `#[serde(tag="action", content="data")]`）。
⇒ admin 调用**真实下发**到 Windows 实机并成功；viewer 被 403 文案明确指向 `config.rs` 的 `handler_min_role` 拦截（**非** handler 内自检，因 handler 自检文案是 `Forbidden: Only Admin role can send remote desktop input`）。**C2 通过。**

**C3 · `frame.jpg` 用 viewer 调用** ⚠️——**原判据（期望 403）被证伪**
```
viewer   GET /api/terminals/<tid>/desktop/frame.jpg  -> 204 No Content   size=0
```
viewer **未被拒**（204 = 无活跃桌面流时无帧可返回，属正常空响应）。符合 C1 分析：`min_role = Viewer` 对 viewer 恒满足。
⇒ **该项不是"收紧生效"，而是"契约补全但无实质效果"**。已在 §2 标注判据有误。

**C4 · `/desktop/` 免配额** ✅——专用 key `g70wm000`（从零计数）
```
GET /desktop/frame.jpg × 130  ->  130 × 204（无 429）
末次响应头：HTTP/1.1 204 · vary · date            ← 无 x-ratelimit-limit/remaining/reset，无 retry-after
```
**C5 · 同 key 打非 `/desktop/` 端点** ✅——**关键对照**
```
GET /api/terminals × 125  ->  120 × 200  +  5 × 429
首次 429 出现在第 121 次
429 头：retry-after: 58 · x-ratelimit-limit: 120 · x-ratelimit-remaining: 0 · x-ratelimit-reset: 58
429 体：{"error":"Too Many Requests: Request quota exceeded"}
```
⇒ **C4 与 C5 用同一 key**：130 次桌面请求**未消耗任何配额**（否则 C5 必在第 121 次之前就 429）。免配额结论**铁证成立**。

**C6 · 绕过探针** ❌——**发现真实绕过路径**
判定代码为 `mcp/mod.rs:393-394`：`let path = req.uri().path(); let is_streaming_channel = path.contains("/desktop/");` —— **子串匹配**。变体矩阵（admin key `edmkr000`，看响应是否带 `x-ratelimit-*`）：

| URL | 状态 | `x-ratelimit-*` | 判定 |
|---|---|---|---|
| `/api/keys?q=/desktop/` | 200 | **有** | 吃配额（query 不计入 path）✅ |
| `/api/keys?next=/desktop/x` | 200 | **有** | 吃配额 ✅ |
| `/api/terminals/a%2Fdesktop%2Fb/calls` | 200 | **有** | 吃配额（percent-encoding 不解码）✅ |
| `/api/terminals/DESKTOP/calls` | 200 | **有** | 吃配额（`contains` 大小写敏感）✅ |
| `/api/terminals/x/desktop/calls` | 404 | — | 路由不匹配，未进入 layer |
| **`/api/terminals/desktop/calls`** | **200** | **无** | ⚠️ **免配额——绕过成立** |
| **`/api/terminals/desktop/invoke`**（POST） | **500** | **无** | ⚠️ **免配额——绕过成立** |
| 对照：`/api/terminals/laptop-roeh3p85-b6a918/invoke` | 200 | 有 | 吃配额（基线） |

**根因**：`terminal_id` 是**路径段**，而 `contains("/desktop/")` 只看子串 ⇒ 当 `terminal_id = "desktop"` 时，`/api/terminals/desktop/<任意子路由>`（`invoke`、`calls`、`logs`…）路径中天然含有 `/desktop/`，从而**整类终端子资源端点被误判为流式通道而免配额**。
**放大因素**：U1（§3.B）允许**无凭据任意注册 `terminal_id`** ⇒ 攻击者可直接以 `terminal_id="desktop"` 注册一个终端来"认领"这条路径。**组合风险 = 配额豁免可被单方触发**。
（500 是因该终端不存在；200 的 `calls` 返回空列表 —— 两者都**不带** `x-ratelimit-*`，绕过判定与终端是否真实存在无关。）

**C7 · 风险记录** ✅（记录，非缺陷判定）
1. **豁免端点无任何频次上限**：`/desktop/*` 全路径可被无限次、无限速调用（C4 已证 130 次零成本）。`frame.jpg` 每次触发 agent 抓帧 + JPEG 编码，属**重操作**；无配额意味着单个持有 viewer 凭据者即可对 agent 做持续抓帧压力。
2. **判据应改为精确匹配**：`path.contains("/desktop/")` 应换成**基于路由/白名单的判定**（例如对 `handler_min_role` 已是 `Viewer` 且路径后缀严格等于 `/desktop/frame`、`/desktop/frame.jpg`、`/desktop/raw` 才豁免），或直接排除 `:id == "desktop"` 的歧义。
3. **与 U1 的组合**：两者叠加后，「任意可连 9801 者」可注册 `terminal_id="desktop"` 并对其滥用免配额通道。
4. 建议在修复前**不要**把 U4b 描述为"安全的性能优化"。

**本组结论**：C2 ✅ / C4+C5 ✅（免配额机制本身按设计工作）/ **C1、C3 判据更正**（U4a 无安全增量）/ **C6 发现 P2 级配额绕过**（`terminal_id="desktop"`）。

### 3.D D 组 · U2 tray 退出 ✅（**端到端真实点击验证**）

**U2 改动**（`crates/agent/src/app/ui.rs:164-170`，提交 `d4652de`）：
```diff
  } else if event.id == tray.quit_id {
+     tracing::info!("User requested quit from system tray. Shutting down...");
      self.should_quit = true;
      self.state.trigger_emergency_disconnect();
-     ctx.send_viewport_cmd(egui::ViewportCommand::Close);   // 旧：只请求关窗
+     self.tray = None;                                      // 新：销毁托盘
+     std::process::exit(0);                                 // 新：无条件终止进程
  }
```
同时 `ui.rs:188-194` 的 **close-to-tray** 逻辑决定了"点 X ≠ 退出"：
```rust
if ctx.input(|i| i.viewport().close_requested()) {
    if self.tray.is_some() && !self.should_quit {   // 托盘在且未请求退出 ⇒ 取消关闭+隐藏
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
    }
}
```

**D1 · 第二实例启动形态** ✅
```
PID 32720   CmdLine: "…\at-pc-agent.exe" -c C:\Users\CLKJ\atpc-gui\agent_config.toml
配置: enable_computer_use=true / url=ws://192.168.66.18:9801/ws / device_id="d2-gui-probe"
```
以**独立 `device_id`** 启动（关键：托盘 tooltip 因此唯一，可与用户主实例区分）：
```
[at-pc 终端代理 (d2-gui-probe)]        @(2148,1510)   ← 被测实例
[at-pc 终端代理 (laptop-roeh3p85-b6…)] @(2228,1510)   ← 用户主实例（全程未动）
```

**D2 · 触发托盘退出 → 进程消失** ✅
- **D2a 前置：验证 close-to-tray 生效**（向主窗口发 `WM_CLOSE`）
  ```
  发送 WM_CLOSE 前: 进程存活  hwnd=3673006  标题='at-pc 终端代理'
  发送 WM_CLOSE 后: 进程【仍存活】 hwnd=4131104  标题=''
  ```
  ⇒ 点 X 只**隐藏**窗口，进程继续运行。**正因如此，托盘菜单才是唯一正常退出入口**——U2 修这个入口是必要的。
- **D2b 真实托盘点击**（操作序列见下方"工装记录"）
  ```
  ① 展开隐藏图标面板 → ② 右键 d2-gui-probe 图标 → 菜单弹出（9 项，末项 @(2408,1524)）
  ③ 左键点击末项「🛑 退出 Agent」
  →  工具调用返回: Error: Terminal 'd2-gui-probe' disconnected:
     Emergency disconnect triggered by user in Agent UI
  →  进程 32720 【已退出】✅
  ```
  该错误信息本身就是 `trigger_emergency_disconnect()` 已在 `process::exit(0)` 之前执行的直接证据。

**D3 · 服务端感知** ✅——本地 server 日志三连，时序连续（同一个 140ms 窗口内）：
```
09:34:51.598335  INFO ws::handler: Terminal [d2-gui-probe] initiated disconnect: Emergency disconnect triggered by user in Agent UI
09:34:51.598492  INFO router:     Aborted 1 in-flight tool call(s) for terminal 'd2-gui-probe' (reason: Emergency disconnect triggered by user in Agent UI)
09:34:51.738795  INFO ws::handler: Terminal [d2-gui-probe] disconnected. Cleaning up session 10.
```
`/api/terminals` 中该终端转 `Offline`。⇒ 断开被**立即上报**，在途调用被**主动中止**，会话正常回收。

**D4 · 日志完整性** ✅——`process::exit(0)` **不丢日志**：
```
09:34:52.105739  INFO at_pc_agent::app::ui: User requested quit from system tray. Shutting down...
09:34:52.106173  INFO at_pc_agent::ws_client: Disconnecting agent: Emergency disconnect triggered by user in Agent UI
```
⇒ quit 分支内的两条 tracing 记录**完整落盘**（`tracing` 为同步写出，`exit(0)` 不会截断已写内容）。**无日志丢失。**

> **对照 `d4652de^` 旧行为**：本次未构建旧版二进制（macOS 上交叉编译 Windows target 成本高），故"旧行为残留"未做**二进制级**复现。但静态证据充分：旧代码依赖 `ViewportCommand::Close` → 走 `close_requested` 分支，而该分支是否真正终结进程**取决于 eframe 事件循环**（`tray-icon` 在 Windows 上持有独立消息循环线程，正是进程可能挂住的来源）；新代码 `std::process::exit(0)` 为 **OS 级无条件终止**，不依赖事件循环。**此处为"代码层确认 + 新行为实测通过"，非"新旧二进制对拍"。**

**工装记录（D2b 实现要点，供复现）**
- **合成输入无效**：PowerShell `mouse_event` / `keybd_event` **能**弹出菜单（`#32768` 窗口出现），但**无法激活菜单命令**（点 `🖥️ 显示主界面`、点 `🛑 退出 Agent`、按 `END`+`ENTER` 均无反应，日志无 quit 行）。**排除**了"窗口隐藏导致 egui 不重绘"的假设（`ShowWindow` 让主窗口可见后依然无效）。
- **有效路径**：改由 **agent 自身的输入注入**（`mouse_click` MCP 工具，Windows `SendInput` 实现）执行右键与左键，菜单命令**正常激活**。⇒ 该库对合成输入友好，对"后门式"输入不响应。
- **踩坑 1**：PowerShell 向外部程序（`curl.exe`）传含双引号的参数会**丢引号** ⇒ 必须把 body 写入临时文件用 `-d @file`。
- **踩坑 2**：D 组被测 server 在 **macOS**，Windows 侧访问必须用 `http://192.168.66.18:9800/mcp`，**不能**用 `127.0.0.1`（那是 Windows 本机）。
- **踩坑 3**：Win11 托盘图标默认折叠在"显示隐藏的图标"面板内；面板失焦即自动收起 ⇒ 需在同一脚本内"检测→展开→定位→操作"。

**本组结论**：D1–D4 **全部通过**。U2 修复**真实有效**：真实点击托盘「退出 Agent」后，紧急断开先上报服务端、进程无条件退出、日志无丢失。全程未触碰用户主实例（PID 19868 始终存活）。

### 3.E E 组 · 回归

**E3 · 前端三件套** ✅——`crates/server/frontend`
```
npm run lint      -> eslint src tests vite.config.js     （无告警输出）
npm run typecheck -> tsc --noEmit -p jsconfig.json       （无错误输出）
npm run test      -> 1..34  # pass 34  # fail 0  # skipped 0
```
⇒ lint / typecheck 干净，单测 **34/34 全过**。

**E1 · 工作区全量测试** ❌——**发现 1 项真实回归**

```
crates/e2e-tests --test milestone3_p2_test
  test test_p2_4_dashboard_rbac_and_audit_api ... FAILED
  thread 'test_p2_4_dashboard_rbac_and_audit_api' panicked at milestone3_p2_test.rs:626:5:
      Expected api:desktop_input audit entry
  test result: FAILED. 6 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.45s
error: test failed, to rerun pass `-p at-pc-e2e-tests --test milestone3_p2_test`
```
（`cargo test --workspace` 在首个失败 binary 处停止，故只跑到第 28 个 test result）

**根因（代码级定位，与 §3.C/C2 的实测文案完全吻合）**

- 测试意图（`milestone3_p2_test.rs:609-629`）：用 **viewer** 令牌调 `POST /api/terminals/<id>/desktop/input`，
  期望被 **handler 内**的 `if role != Role::Admin` 拦下，从而写出一条 `action = "api:desktop_input"`、
  `status = "DENIED"` 的审计记录（`dashboard.rs:793-813`）。
- `9df6fe6` 在 `config.rs:408-410` **新增**了：
  ```rust
  if method == "POST" && … && trimmed_path.ends_with("/desktop/input") {
      return Some(Role::Admin);
  }
  ```
- 而 `handler_min_role` 的消费点是 `mcp/mod.rs:381-391` 的 **auth_layer（handler 之前）**：
  `role < min_role ⇒ 403 "Forbidden: requires role '<min_role>', current role is '<role>'"`。
- ⇒ viewer 请求在 **auth_layer 就被 403**，**永远到不了 handler** ⇒ handler 内那段"写审计再 403"的代码
  成为**不可达代码** ⇒ 审计条目消失 ⇒ 断言失败。

**这与 C2 实测到的文案完全对应**：实测 viewer 收到的是
`{"error":"Forbidden: requires role 'admin', current role is 'viewer'"}`（**auth_layer 文案**），
而**不是** handler 内的 `Forbidden: Only Admin role can send remote desktop input`。
当时即记录"该 403 来自 config.rs 而非 handler 自检"——此处被证明是一个**真实的审计能力回归**。

**影响**：**viewer 越权尝试 `desktop/input` 不再留痕**。原设计语义是"拒绝 **并审计**"，
现在退化为"静默拒绝"。安全审计链在这一条路径上出现缺口（与 §3.B 的 U1 同属"安全面收窄"）。

**回归来源确认**：`9df6fe6` 的 stat **未包含 `crates/e2e-tests/**`**（0 处），只新增了
`crates/server/tests/p3_hardening_test.rs::test_desktop_streaming_bypasses_quota`（配额场景），
**没有同步覆盖本次 RBAC 改动**。⇒ 属于"改了行为但没改测试"的典型漂移。

**父提交对照（排除偶发与环境差异）**——用 `git worktree` 在 `9df6fe6^`（`a72b24a`）复跑同一测试：
```
test test_p2_4_rbac_router_permission_enforcement ... ok
test test_p2_4_dashboard_rbac_and_audit_api ... ok          ← 父提交：通过
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.12s
```
回到 HEAD 单独复跑：**稳定复现**同一断言失败（`6 passed; 1 failed`）。
⇒ **回归 100% 由 `9df6fe6` 引入，与偶发/环境无关。**

> **修复建议（二选一）**：
> ① **承认语义变更**：viewer 越权改由 auth_layer 统一拦（现状），则须**同步修正测试**——把断言改为
> 期望 `auth_layer` 的 403 文案，并**另找一条会产生审计的路径**来验证审计链（或明确记录"auth_layer
> 拒绝不入审计"这一新语义）；
> ② **保留审计语义**：不要给 `desktop/input` 加 `handler_min_role` 映射（让它继续靠 handler 内自检
> + 写审计），把 RBAC 收敛与审计留痕两件事解耦。
> 无论选哪个，`9df6fe6` 都必须带上**测试改动**，否则门禁一直是红的。

**E2 · 门禁脚本** ⏳

### 3.F F 组 · 文档与清理

**F1 · 安全放宽是否写入文档** ❌——**三处放宽均无任何发布说明**

| 检查项 | 结果 |
|---|---|
| 仓库根 `CHANGELOG*` / `RELEASE*` | **不存在** |
| `docs/releases/` | 仅 `v1.0.0.md`，**mtime = 2026-09-21 14:10**，**早于** `d4652de`(15:54) 与 `9df6fe6`(16:58) ⇒ 不可能覆盖本次改动 |
| `docs/releases/v1.0.0.md` 内容 | 检索 `免鉴权/免配额/no key required/quota exemption` —— **零命中** |
| 两提交 message | 均只有标题单行，**无 body**，未描述行为变更或安全影响 |
| 唯一记录处 | `docs/plans/2026-09-21-d4652de-update-live-verification.md` 与**本文件**——**事后验证报告，非发布说明** |

**具体缺口**（三处均属"用户可感知的行为/安全面变更"，按惯例应显式声明）：
1. **U1**（`d4652de`）：WS 注册**移除全部凭据校验** —— 设备令牌、`device_id` 绑定、legacy 兜底、无凭据拒绝**四道全撤**。任何可连 9801 端口者均可注册任意终端 ID。
2. **U3a**（`d4652de`）：无 token 请求不再计入鉴权失败 —— 削弱 `AuthRateLimiter` 的失败累积（防爆破）能力。
3. **U4b**（`9df6fe6`）：路径含 `/desktop/` 的请求**完全免配额**且不回 `x-ratelimit-*`；叠加 C6 的子串绕过，形成可单方触发的豁免通道。

⇒ **判定：F1 不通过。** 建议在 `docs/releases/` 补一份 `v1.0.0-updates.md`（或新版发布说明），至少覆盖上述三条，并在 U1 条目中写明"设备接入改为无凭据模型"及其影响面。

**F2 · 清理与副作用核对** ⏳（E1/E2 结束后统一执行）

---

## 4. 结论

*（待测后填写）*
