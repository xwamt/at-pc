//! Embedded Web Dashboard & Management UI for at-pc-server.
//! Provides a responsive, zero-external-dependency web interface
//! to inspect active terminals, CPU/RAM metrics, and run quick diagnostics.

use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::McpHttpState;

/// Create dashboard sub-router
pub fn create_dashboard_router() -> Router<McpHttpState> {
    Router::new()
        .route("/", get(dashboard_html_handler))
        .route("/dashboard", get(dashboard_html_handler))
        .route("/api/terminals", get(api_list_terminals))
        .route("/api/terminals/:id/invoke", post(api_invoke_tool))
}

/// Handler for `GET /` and `GET /dashboard`
async fn dashboard_html_handler() -> impl IntoResponse {
    Html(DASHBOARD_HTML)
}

/// Handler for `GET /api/terminals`
async fn api_list_terminals(State(state): State<McpHttpState>) -> impl IntoResponse {
    let entries = state.router.list_terminals().await;
    Json(entries)
}

#[derive(Deserialize)]
struct InvokeRequest {
    tool: String,
    arguments: Option<Value>,
}

/// Handler for `POST /api/terminals/:id/invoke`
async fn api_invoke_tool(
    State(state): State<McpHttpState>,
    Path(terminal_id): Path<String>,
    Json(payload): Json<InvokeRequest>,
) -> impl IntoResponse {
    let args = payload.arguments.unwrap_or_else(|| json!({}));
    match state
        .router
        .invoke_tool(&terminal_id, &payload.tool, args, 30)
        .await
    {
        Ok(res) => Json(json!({ "success": true, "result": res })),
        Err(err) => Json(json!({ "success": false, "error": err })),
    }
}

pub const DASHBOARD_HTML: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>AT-PC 集中管控平台</title>
  <style>
    :root {
      --bg: #0f172a;
      --card-bg: #1e293b;
      --card-border: #334155;
      --text: #f8fafc;
      --text-muted: #94a3b8;
      --primary: #38bdf8;
      --primary-hover: #0284c7;
      --success: #22c55e;
      --warning: #f59e0b;
      --danger: #ef4444;
      --badge-bg: #0f172a;
    }
    * { box-sizing: border-box; margin: 0; padding: 0; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif; }
    body { background-color: var(--bg); color: var(--text); padding: 24px; min-height: 100vh; }
    .container { max-width: 1200px; margin: 0 auto; }
    
    header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 24px; padding-bottom: 20px; border-bottom: 1px solid var(--card-border); }
    .logo-area { display: flex; align-items: center; gap: 14px; }
    .logo-icon { width: 42px; height: 42px; border-radius: 10px; background: linear-gradient(135deg, #38bdf8, #818cf8); display: flex; align-items: center; justify-content: center; font-size: 22px; font-weight: bold; color: #0f172a; }
    .title-box h1 { font-size: 22px; font-weight: 700; }
    .title-box p { font-size: 13px; color: var(--text-muted); margin-top: 2px; }
    
    .status-tags { display: flex; gap: 10px; align-items: center; }
    .tag { display: inline-flex; align-items: center; gap: 6px; padding: 6px 12px; border-radius: 9999px; background: var(--card-bg); border: 1px solid var(--card-border); font-size: 13px; }
    .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--success); }
    .dot.pulse { box-shadow: 0 0 8px var(--success); }
    
    .btn { background: var(--primary); color: #0f172a; border: none; padding: 8px 16px; border-radius: 6px; font-size: 13px; font-weight: 600; cursor: pointer; transition: 0.15s; }
    .btn:hover { background: var(--primary-hover); }
    .btn-secondary { background: #334155; color: var(--text); }
    .btn-secondary:hover { background: #475569; }
    .btn-sm { padding: 5px 10px; font-size: 12px; }

    .toolbar { display: flex; justify-content: space-between; align-items: center; margin-bottom: 20px; gap: 16px; }
    .search-box { flex: 1; max-width: 400px; }
    .search-box input { width: 100%; padding: 9px 14px; background: var(--card-bg); border: 1px solid var(--card-border); border-radius: 8px; color: var(--text); font-size: 14px; outline: none; }
    .search-box input:focus { border-color: var(--primary); }

    .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(360px, 1fr)); gap: 20px; }
    
    .card { background: var(--card-bg); border: 1px solid var(--card-border); border-radius: 12px; padding: 20px; display: flex; flex-direction: column; gap: 16px; transition: transform 0.15s, border-color 0.15s; }
    .card:hover { border-color: var(--primary); transform: translateY(-2px); }
    
    .card-header { display: flex; justify-content: space-between; align-items: flex-start; }
    .device-name { font-size: 17px; font-weight: 700; color: #fff; word-break: break-all; }
    .device-id { font-size: 12px; color: var(--text-muted); font-family: monospace; margin-top: 2px; }
    
    .status-badge { font-size: 11px; padding: 3px 8px; border-radius: 6px; font-weight: 600; text-transform: uppercase; }
    .status-badge.online { background: rgba(34, 197, 94, 0.2); color: var(--success); border: 1px solid var(--success); }
    .status-badge.offline { background: rgba(239, 68, 68, 0.2); color: var(--danger); border: 1px solid var(--danger); }
    
    .metrics { display: flex; flex-direction: column; gap: 10px; background: #0f172a80; padding: 12px; border-radius: 8px; }
    .metric-row { display: flex; justify-content: space-between; font-size: 12px; margin-bottom: 4px; }
    .progress-bar-bg { width: 100%; height: 6px; background: #334155; border-radius: 3px; overflow: hidden; }
    .progress-bar-fill { height: 100%; border-radius: 3px; transition: width 0.3s ease; }
    .fill-green { background: var(--success); }
    .fill-yellow { background: var(--warning); }
    .fill-red { background: var(--danger); }

    .info-list { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; font-size: 12px; color: var(--text-muted); }
    .info-item b { color: var(--text); }

    .card-actions { display: flex; gap: 8px; margin-top: auto; padding-top: 10px; border-top: 1px solid #33415550; }
    
    /* Modal / Drawer */
    .modal-backdrop { display: none; position: fixed; inset: 0; background: rgba(0,0,0,0.7); z-index: 99; align-items: center; justify-content: center; padding: 20px; }
    .modal { background: var(--card-bg); border: 1px solid var(--card-border); border-radius: 12px; width: 100%; max-width: 750px; max-height: 85vh; display: flex; flex-direction: column; overflow: hidden; }
    .modal-header { padding: 16px 20px; border-bottom: 1px solid var(--card-border); display: flex; justify-content: space-between; align-items: center; }
    .modal-body { padding: 20px; overflow-y: auto; font-family: monospace; font-size: 13px; line-height: 1.5; color: #e2e8f0; }
    .modal-footer { padding: 14px 20px; border-top: 1px solid var(--card-border); display: flex; justify-content: flex-end; gap: 10px; }
    .close-btn { cursor: pointer; font-size: 20px; color: var(--text-muted); background: none; border: none; }
    .close-btn:hover { color: #fff; }
    pre { background: #0f172a; padding: 14px; border-radius: 8px; overflow-x: auto; white-space: pre-wrap; }
    .screenshot-img { width: 100%; max-height: 500px; object-fit: contain; border-radius: 6px; }

    .empty-state { grid-column: 1 / -1; padding: 60px 20px; text-align: center; color: var(--text-muted); background: var(--card-bg); border-radius: 12px; border: 1px dashed var(--card-border); }
  </style>
</head>
<body>
  <div class="container">
    <header>
      <div class="logo-area">
        <div class="logo-icon">AT</div>
        <div class="title-box">
          <h1>AT-PC 集中管控平台</h1>
          <p>Multi-Terminal MCP Remote Gateway & Device Pool</p>
        </div>
      </div>
      <div class="status-tags">
        <div class="tag"><div class="dot pulse"></div> 服务端在线</div>
        <div class="tag">WS: <b>9801</b></div>
        <div class="tag">MCP: <b>9800</b></div>
        <button class="btn btn-secondary btn-sm" onclick="showMcpConfigModal()">📋 复制 MCP 配置</button>
      </div>
    </header>

    <div class="toolbar">
      <div class="search-box">
        <input type="text" id="searchInput" placeholder="🔍 搜索主机名、IP、系统版本..." oninput="renderCards()">
      </div>
      <div style="font-size: 13px; color: var(--text-muted);">
        在线终端: <b id="onlineCount" style="color: var(--success); font-size: 15px;">0</b> / 总计: <span id="totalCount">0</span>
      </div>
    </div>

    <div id="terminalGrid" class="grid">
      <div class="empty-state">
        <h3>正在加载终端列表...</h3>
        <p style="margin-top: 8px; font-size: 13px;">请稍候，系统正在从服务端获取在线设备</p>
      </div>
    </div>
  </div>

  <!-- Result Modal -->
  <div id="resultModal" class="modal-backdrop">
    <div class="modal">
      <div class="modal-header">
        <h3 id="modalTitle" style="font-size: 16px;">诊断结果</h3>
        <button class="close-btn" onclick="closeModal()">&times;</button>
      </div>
      <div class="modal-body" id="modalContent">
        <pre>加载中...</pre>
      </div>
      <div class="modal-footer">
        <button class="btn btn-secondary" onclick="closeModal()">关闭</button>
      </div>
    </div>
  </div>

  <script>
    let terminalsData = [];

    async function fetchTerminals() {
      try {
        const res = await fetch('/api/terminals');
        if (res.ok) {
          terminalsData = await res.json();
          renderCards();
        }
      } catch (err) {
        console.error('Failed to fetch terminals:', err);
      }
    }

    function renderCards() {
      const filter = document.getElementById('searchInput').value.toLowerCase().trim();
      const grid = document.getElementById('terminalGrid');
      
      const filtered = terminalsData.filter(t => {
        if (!filter) return true;
        const h = (t.info?.hostname || '').toLowerCase();
        const ip = (t.info?.lan_ip || '').toLowerCase();
        const id = (t.info?.terminal_id || '').toLowerCase();
        const os = (t.info?.os_version || '').toLowerCase();
        return h.includes(filter) || ip.includes(filter) || id.includes(filter) || os.includes(filter);
      });

      const onlineTotal = terminalsData.filter(t => t.status === 'Online').length;
      document.getElementById('onlineCount').innerText = onlineTotal;
      document.getElementById('totalCount').innerText = terminalsData.length;

      if (filtered.length === 0) {
        grid.innerHTML = `
          <div class="empty-state">
            <h3>暂无匹配终端</h3>
            <p style="margin-top: 8px; font-size: 13px;">请在被协助电脑上启动 <code>at-pc-agent.exe</code>，启动后将自动注册在此处</p>
          </div>
        `;
        return;
      }

      grid.innerHTML = filtered.map(t => {
        const isOnline = t.status === 'Online';
        const metrics = t.latest_metrics || {};
        const cpu = (metrics.cpu_usage_percent || 0).toFixed(1);
        const memUsed = ((metrics.memory_used_mb || 0) / 1024).toFixed(1);
        const memTotal = ((metrics.memory_total_mb || 1) / 1024).toFixed(1);
        const memPercent = Math.min(100, Math.round(((metrics.memory_used_mb || 0) / (metrics.memory_total_mb || 1)) * 100));

        let cpuColor = 'fill-green';
        if (cpu > 80) cpuColor = 'fill-red';
        else if (cpu > 50) cpuColor = 'fill-yellow';

        let memColor = 'fill-green';
        if (memPercent > 80) memColor = 'fill-red';
        else if (memPercent > 50) memColor = 'fill-yellow';

        return `
          <div class="card">
            <div class="card-header">
              <div>
                <div class="device-name">${t.info?.hostname || 'Unknown Host'}</div>
                <div class="device-id">${t.info?.terminal_id || ''}</div>
              </div>
              <span class="status-badge ${isOnline ? 'online' : 'offline'}">
                ${isOnline ? '● Online' : '○ Offline'}
              </span>
            </div>

            <div class="metrics">
              <div>
                <div class="metric-row">
                  <span>CPU 占用</span>
                  <b>${cpu}%</b>
                </div>
                <div class="progress-bar-bg">
                  <div class="progress-bar-fill ${cpuColor}" style="width: ${Math.min(100, Math.max(2, cpu))}%"></div>
                </div>
              </div>
              <div>
                <div class="metric-row">
                  <span>内存使用</span>
                  <b>${memUsed} GB / ${memTotal} GB (${memPercent}%)</b>
                </div>
                <div class="progress-bar-bg">
                  <div class="progress-bar-fill ${memColor}" style="width: ${memPercent}%"></div>
                </div>
              </div>
            </div>

            <div class="info-list">
              <div class="info-item">系统: <b>${t.info?.os_version || '-'}</b></div>
              <div class="info-item">内网 IP: <b>${t.info?.lan_ip || '-'}</b></div>
              <div class="info-item">用户: <b>${t.info?.username || '-'}</b></div>
              <div class="info-item">心跳: <b>${t.last_heartbeat_elapsed_secs}秒前</b></div>
            </div>

            <div class="card-actions">
              <button class="btn btn-secondary btn-sm" onclick="runOverview('${t.info?.terminal_id}')">📊 系统概况</button>
              <button class="btn btn-secondary btn-sm" onclick="runScreenshot('${t.info?.terminal_id}')">📸 截图</button>
              <button class="btn btn-secondary btn-sm" onclick="runCommandPrompt('${t.info?.terminal_id}')">⚡ 跑命令</button>
            </div>
          </div>
        `;
      }).join('');
    }

    async function runOverview(terminalId) {
      showModal(`终端 [${terminalId}] - 系统完整概况`, '<pre>正在查询目标机器完整硬件及网络指标...</pre>');
      try {
        const res = await fetch(`/api/terminals/${encodeURIComponent(terminalId)}/invoke`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ tool: 'get_system_overview', arguments: {} })
        });
        const data = await res.json();
        if (data.success) {
          showModal(`终端 [${terminalId}] - 系统完整概况`, `<pre>${JSON.stringify(data.result, null, 2)}</pre>`);
        } else {
          showModal(`查询失败`, `<pre style="color: var(--danger)">${data.error || 'Unknown error'}</pre>`);
        }
      } catch (err) {
        showModal(`请求错误`, `<pre style="color: var(--danger)">${err.message}</pre>`);
      }
    }

    async function runScreenshot(terminalId) {
      showModal(`终端 [${terminalId}] - 屏幕截图`, '<pre>正在请求屏幕截图...</pre>');
      try {
        const res = await fetch(`/api/terminals/${encodeURIComponent(terminalId)}/invoke`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ tool: 'capture_screen', arguments: { quality: 80 } })
        });
        const data = await res.json();
        if (data.success && data.result?.image_base64) {
          const imgUrl = `data:image/jpeg;base64,${data.result.image_base64}`;
          showModal(`终端 [${terminalId}] - 屏幕截图 (${data.result.width}x${data.result.height})`, `<img src="${imgUrl}" class="screenshot-img" />`);
        } else {
          showModal(`截图失败`, `<pre style="color: var(--danger)">${data.error || JSON.stringify(data.result)}</pre>`);
        }
      } catch (err) {
        showModal(`截图错误`, `<pre style="color: var(--danger)">${err.message}</pre>`);
      }
    }

    async function runCommandPrompt(terminalId) {
      const script = prompt(`请输入要在终端 [${terminalId}] 执行的 PowerShell 脚本:`, "Get-Process | Select-Object -First 5");
      if (!script) return;
      showModal(`终端 [${terminalId}] - 执行命令`, `<pre>正在执行: ${script}...</pre>`);
      try {
        const res = await fetch(`/api/terminals/${encodeURIComponent(terminalId)}/invoke`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ tool: 'exec_powershell', arguments: { script: script, timeout_secs: 30 } })
        });
        const data = await res.json();
        if (data.success) {
          const out = data.result?.stdout || data.result?.stderr || '(无输出)';
          showModal(`执行结果 (Exit Code: ${data.result?.exit_code})`, `<pre>${out}</pre>`);
        } else {
          showModal(`执行失败`, `<pre style="color: var(--danger)">${data.error}</pre>`);
        }
      } catch (err) {
        showModal(`请求错误`, `<pre style="color: var(--danger)">${err.message}</pre>`);
      }
    }

    function showMcpConfigModal() {
      const host = window.location.host;
      const sseUrl = `http://${host}/sse`;
      const configJson = JSON.stringify({
        mcpServers: {
          "at-pc-central": {
            url: sseUrl
          }
        }
      }, null, 2);
      showModal('AI Agent MCP 配置 JSON', `<p style="margin-bottom: 10px; font-size: 13px; color: var(--text-muted);">复制以下配置填入 Cursor 或 Claude 的 mcp.json：</p><pre>${configJson}</pre>`);
    }

    function showModal(title, htmlContent) {
      document.getElementById('modalTitle').innerText = title;
      document.getElementById('modalContent').innerHTML = htmlContent;
      document.getElementById('resultModal').style.display = 'flex';
    }

    function closeModal() {
      document.getElementById('resultModal').style.display = 'none';
    }

    window.onclick = function(e) {
      if (e.target.id === 'resultModal') closeModal();
    }

    // Initial fetch & 2s auto-refresh
    fetchTerminals();
    setInterval(fetchTerminals, 2000);
  </script>
</body>
</html>
"#;
