# at-pc 实机环境全量功能测试报告

## 一、测试环境概要
- **服务端**: `http://192.168.99.94:9800` (MCP/REST), `ws://192.168.99.94:9801/ws` (WebSocket Gateway)
- **被测 Windows 终端**:
  - **Terminal ID**: `laptop-roeh3p85-b6a918`
  - **主机名 (Hostname)**: `LAPTOP-ROEH3P85`
  - **自定义名称**: `Windows实机测试终端` (已通过元数据动态修改生效)
  - **用户名**: `CLKJ`
  - **操作系统**: `Windows 11 (26100)`
  - **Agent 版本**: `v1.0.0`
  - **在线状态**: `Online`
- **执行时间**: 2026-09-21 16:16 ~ 16:18 (GMT+8)
- **总体结论**: **全量 17 项实机测试用例 100% 全部通过 (17 Passed / 0 Failed)**。

---

## 二、测试用例清单与实时执行结果矩阵

| 序号 | 模块 / 类目 | 测试用例项 | 验证重点 | 执行状态 | 测试结果与关键指标 |
|:---:|---|---|---|:---:|---|
| **01** | 安全与门禁 | **TC-AUTH-01: 未认证访问防护** | 未带 Token 访问受限接口返回 401 且连续请求不触发误封 IP | ✅ **通过** | HTTP 401 Unauthorized，连续多次请求未封禁 IP |
| **02** | 安全与门禁 | **TC-AUTH-02: Root Key 身份认证** | 携带 Root Key 访问 `/api/auth/me` | ✅ **通过** | 返回 `role: admin`, `scopes: ["console","mcp"]`, HTTP 200 |
| **03** | 密钥生命周期 | **TC-KEY-01: 密钥增删查改与轮换** | 动态签发 Operator Key、查看列表、轮换密钥与吊销 | ✅ **通过** | 成功签发 `atpc_sk_vd23g000_...`，优雅轮换后注销 (HTTP 204) |
| **04** | 终端拓扑与感知 | **TC-TERM-01: 终端状态与心跳感知** | 查询在线终端列表，验证 CPU/内存遥测与秒级心跳 | ✅ **通过** | 实时获取到 Windows 终端 CPU `2.04%`，内存 `12881MB/32280MB`，心跳 `1s` |
| **05** | 终端拓扑与元数据 | **TC-TERM-02: 终端元数据动态修改** | 修改 custom_name、notes、tags，验证持久化生效 | ✅ **通过** | `POST /api/terminals/:id/meta` 成功设置并持久化至 `terminals_meta.json` |
| **06** | 系统概览与诊断 | **TC-SYS-01: 硬件与系统概览采集** | 调用 `get_system_overview` 获取 CPU/内存/磁盘/网络 | ✅ **通过** | 获取到 Intel Ultra 5 125H (18核)、C盘 486GB (剩余 193GB)、WLAN 网卡 |
| **07** | 远程指令执行 | **TC-CMD-01: CMD 命令行执行** | 调用 `exec_cmd` 执行 `whoami` | ✅ **通过** | 返回 `laptop-roeh3p85\clkj`，exit_code: 0，耗时 43ms |
| **08** | 远程指令执行 | **TC-CMD-02: PowerShell 脚本执行** | 调用 `exec_powershell` 执行日期与操作系统版本查询 | ✅ **通过** | 返回 `Microsoft Windows 11 家庭中文版`，耗时 666ms |
| **09** | 进程管理 | **TC-PROC-01: 进程列表检索与过滤** | 调用 `list_processes` 过滤查询 `at-pc` 进程 | ✅ **通过** | 准确获取 `at-pc-agent.exe` (PID: 19868, 内存: 162MB, 状态: Run) |
| **10** | Windows 服务管理 | **TC-SVC-01: 系统服务状态查询** | 调用 `manage_service` 查询 `Spooler` 状态 | ✅ **通过** | Win32 SCM 返回 `Print Spooler`，状态 `Running`，启动类型 `Automatic` |
| **11** | 文件系统操作 | **TC-FS-01: 目录列表与文件搜索** | 调用 `list_directory` 检索 Downloads 目录 | ✅ **通过** | 获取到 10 项文件与子目录（包含文件名、体积、修改时间、权限） |
| **12** | 文件系统操作 | **TC-FS-02: 文件安全读写验证** | 在 Temp 目录下通过 `write_text_file` / `read_text_file` 验证 | ✅ **通过** | 写入 42 字节并完整读回，内容 100% 一致 |
| **13** | 视觉与多显示器 | **TC-VIS-01: 显示器拓扑探测** | 调用 `list_monitors` 查询主副屏分辨率与缩放比 | ✅ **通过** | 主屏 `\\.\DISPLAY1`，分辨率 `2880 x 1800`，缩放比 `2.0 (200% High-DPI)` |
| **14** | 视觉与屏幕捕获 | **TC-VIS-02: 实时屏幕截图与画质** | 调用 `capture_screen` 截取当前屏幕 | ✅ **通过** | 实时生成 JPEG 图像（`data:image/jpeg;base64,...`），耗时 345ms |
| **15** | UI 自动化与树解析 | **TC-UI-01: Windows UI 自动化树解析** | 调用 `get_ui_tree` 获取当前前台窗口控件与层次结构 | ✅ **通过** | 准确识别前台窗口“文件资源管理器”及原生 UI 控件树（Pane/StatusBar/Group等） |
| **16** | 桌面流传输 | **TC-STR-01: 桌面视频流抽帧与控制** | 测试 `/api/terminals/:id/desktop/stream` 开启、抽帧与停止 | ✅ **通过** | Stream 开启 -> `desktop/frame` 抽帧正常 -> Stream 停止，全生命周期通畅 |
| **17** | 审计与调用追踪 | **TC-AUDIT-01: 实时审计日志检索与导出** | 检索各项操作的 AuditRecord，验证完整性与导出 | ✅ **通过** | 每一项调用均生成唯一 `call_id` 审计记录（角色、参数、耗时、状态），支持 JSONL 导出 |

---

## 三、逐项测试实录与实机输入输出详情

### TC-AUTH-01: 未认证访问防护
* **请求方式**: `GET http://127.0.0.1:9800/api/terminals` (无 Authorization Header)
* **响应状态码**: `401 Unauthorized`
* **响应 Payload**: `{"error":"Unauthorized: Invalid or missing authentication token"}`
* **连续未认证测试**: 连续发起 5 次未认证请求，每次均返回 401，未触发频控封禁（429）。

### TC-AUTH-02: Root Key 身份认证
* **请求方式**: `GET http://127.0.0.1:9800/api/auth/me` (携带 Root Key)
* **响应 Payload**:
  ```json
  {
    "id": "brf2p000",
    "kind": "sk",
    "name": "bootstrap-root",
    "role": "admin",
    "scopes": ["console", "mcp"]
  }
  ```

### TC-KEY-01: 密钥生命周期
* **签发操作**: `POST /api/keys` (`name: test-op-key`, `role: operator`)
  * 生成 Plaintext: `atpc_sk_vd23g000_SKkQ49edwQVFSyb0raOwyO6ymy4sl1Si4kWQ9K3POm0` (ID: `vd23g000`)
* **轮换操作**: `POST /api/keys/vd23g000/rotate` (`grace_hours: 1`)
  * 生成新 Key: `atpc_sk_y38rg000_...` (ID: `y38rg000`)
* **注销操作**: `DELETE /api/keys/vd23g000` 与 `DELETE /api/keys/y38rg000`
  * 响应状态: `204 No Content`，删除成功。

### TC-TERM-01 & TC-TERM-02: 终端元数据与状态感知
* **修改元数据**: `POST /api/terminals/laptop-roeh3p85-b6a918/meta`
  ```json
  {
    "custom_name": "Windows实机测试终端",
    "notes": "自动化测试与实机核验",
    "tags": ["windows11", "real-test"]
  }
  ```
* **查询终端**: `GET /api/terminals`
  * 成功读取到 `custom_name: "Windows实机测试终端"`, `status: "Online"`，最新心跳遥测 CPU `2.04%`，内存 `12881 MB / 32280 MB`。

### TC-SYS-01: 系统概览采集 (`get_system_overview`)
* **实机硬件数据**:
  * **CPU**: Intel(R) Core(TM) Ultra 5 125H (18 逻辑核心)
  * **内存**: 总量 32,280 MB，已用 12,878 MB
  * **存储**: C盘 (NTFS) 总计 486,108 MB，可用 193,516 MB
  * **网络**: WLAN 网卡 (MAC: `80:84:89:b6:a9:18`, IP: `192.168.66.110`)

### TC-CMD-01: CMD 命令行执行 (`exec_cmd`)
* **指令**: `whoami`
* **结果**: `laptop-roeh3p85\clkj` (exit_code: 0, 耗时 43ms)。

### TC-CMD-02: PowerShell 脚本执行 (`exec_powershell`)
* **指令**: `(Get-Date).ToString(); (Get-CimInstance Win32_OperatingSystem).Caption`
* **结果**:
  ```text
  2026/9/21 16:17:27
  Microsoft Windows 11 家庭中文版
  ```
  *(注：测试中包含安全围栏验证，含潜在高危词指令会被 Server 端安全围栏主动拦截保护)*

### TC-PROC-01: 进程管理 (`list_processes`)
* **过滤条件**: `filter: "at-pc"`
* **检索结果**:
  * PID: `19868`
  * 名称: `at-pc-agent.exe`
  * 路径: `C:\Users\CLKJ\Downloads\at-pc-windows-x86_64(1)\at-pc-agent.exe`
  * 内存占用: `162 MB`
  * 运行状态: `Run`

### TC-SVC-01: Windows 服务管理 (`manage_service`)
* **查询服务**: `Spooler`
* **检索结果**:
  * 显示名称: `Print Spooler`
  * 运行状态: `Running`
  * 启动类型: `Automatic`

### TC-FS-01 & TC-FS-02: 文件系统读写与目录检索
* **目录检索**: 成功列举 `C:\Users\CLKJ\Downloads` 下 10 项文件与文件夹。
* **读写校验**:
  * 写入文件 `C:\Users\CLKJ\AppData\Local\Temp\at_pc_verify_test.txt`（42 字节）
  * 读取文件回显: `AT-PC Real Verification Test OK 2026-09-21` (完整一致)。

### TC-VIS-01 & TC-VIS-02: 显示器拓扑与屏幕捕获
* **显示器拓扑**: 探测到 `\\.\DISPLAY1`，分辨率 `2880 x 1800`，缩放因子 `2.0`。
* **屏幕截屏**: 成功截取实时高质量 JPEG 图像（345ms 毫秒级生成）。

### TC-UI-01: Windows UI 自动化树解析 (`get_ui_tree`)
* **解析结果**: 实时获取到前台窗口 `at-pc-windows-x86_64(1) - 文件资源管理器` 及其子控件树（Pane, StatusBar, Group 等坐标边界与交互属性）。

### TC-STR-01: 桌面视频流抽帧与控制
* **执行序列**:
  1. `POST /desktop/stream` (`fps: 5, quality: 60`) -> 返回 `{"message":"Stream started","success":true}`
  2. `GET /desktop/frame` -> 成功抽取到最新实时帧 Base64 编码
  3. `POST /desktop/stop` -> 返回 `{"message":"Stream stopped","success":true}`

### TC-AUDIT-01: 实时审计日志检索与导出
* **检索验证**: `GET /api/audit` 实时返回刚才执行的每一笔指令流水（包含唯一 `call_id`、调用身份 `admin`、脱敏令牌前缀 `atp***`、终端 ID、参数及执行耗时）。
* **导出验证**: `GET /api/audit/export?format=jsonl` 成功以行式 JSONL 流式输出。
