export function escapeHtml(str) {
  if (!str) return "";
  return String(str)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

/** Escape a value for a single-quoted JavaScript string inside an HTML attribute. */
export function escapeJsString(str) {
  if (str == null || str === "") return "";
  return String(str)
    .replace(/\\/g, "\\\\")
    .replace(/'/g, "\\'")
    .replace(/\r/g, "\\r")
    .replace(/\n/g, "\\n")
    .replace(/\u2028/g, "\\u2028")
    .replace(/\u2029/g, "\\u2029")
    .replace(/</g, "\\u003c")
    .replace(/>/g, "\\u003e");
}

/**
 * @param {any} t
 * @returns {string}
 */
export function terminalCardHtml(t) {
  const isOnline = t.status === "Online";
  const metrics = t.latest_metrics || {};
  const cpu = (metrics.cpu_usage_percent || 0).toFixed(1);
  const memUsed = ((metrics.memory_used_mb || 0) / 1024).toFixed(1);
  const memTotal = ((metrics.memory_total_mb || 1) / 1024).toFixed(1);
  const memPercent = Math.min(
    100,
    Math.round(
      ((metrics.memory_used_mb || 0) / (metrics.memory_total_mb || 1)) * 100,
    ),
  );
  const isHighLoad = isOnline && (Number(cpu) > 75 || memPercent > 80);

  let cpuColor = "fill-emerald";
  if (Number(cpu) > 80) cpuColor = "fill-rose";
  else if (Number(cpu) > 50) cpuColor = "fill-amber";

  let memColor = "fill-emerald";
  if (memPercent > 80) memColor = "fill-rose";
  else if (memPercent > 50) memColor = "fill-amber";

  const displayName = t.custom_name
    ? escapeHtml(t.custom_name)
    : escapeHtml(t.info?.hostname || "Unknown Host");
  const isNamed = !!t.custom_name;
  const hostInfo = escapeHtml(t.info?.hostname || "-");
  const termIdRaw = t.info?.terminal_id || "";
  const termId = escapeHtml(termIdRaw);
  const termIdJs = escapeJsString(termIdRaw);
  const rawNotes = t.notes ? escapeHtml(t.notes) : "";
  const tagsArr = Array.isArray(t.tags) ? t.tags : [];
  const osVersion = escapeHtml(t.info?.os_version || "-");
  const lanIp = escapeHtml(t.info?.lan_ip || "-");
  const username = escapeHtml(t.info?.username || "-");

  return `
          <div class="card">
            <div class="card-header">
              <div style="flex: 1; min-width: 0;">
                <div class="device-name">
                  <span title="${isNamed ? "自定义名称: " + displayName : "设备尚未命名，点击按钮设置别名"}">${displayName}</span>
                  ${!isNamed ? '<span class="unnamed-hint">未命名</span>' : ""}
                  <button class="edit-name-btn" onclick="openEditMetaModal('${termIdJs}')" title="设置别名 / 备注 / 标签">✏️ 命名</button>
                </div>
                <div class="hostname-sub">
                  <span>主机: <b>${hostInfo}</b></span>
                  <span>·</span>
                  <span class="device-id">${termId}</span>
                </div>
                ${rawNotes ? `<div class="note-box">📝 ${rawNotes}</div>` : ""}
                ${
                  tagsArr.length > 0
                    ? `
                  <div class="tags-container">
                    ${tagsArr.map((tg) => `<span class="tag-pill"># ${escapeHtml(tg)}</span>`).join("")}
                  </div>
                `
                    : ""
                }
              </div>
              <div class="badge-wrap" style="flex-shrink: 0;">
                ${isHighLoad ? '<span class="status-badge high-load">⚠️ 负载告警</span>' : ""}
                <span class="status-badge ${isOnline ? "online" : "offline"}">
                  ${isOnline ? "● 在线" : "○ 离线"}
                </span>
                ${!isOnline ? `<button class="btn btn-danger btn-sm" style="padding: 2px 7px; font-size: 11px;" onclick="deleteOfflineTerminal('${termIdJs}')" title="从平台移除此离线设备记录">🗑️ 移除</button>` : ""}
              </div>
            </div>

            <div class="metrics">
              <div>
                <div class="metric-row">
                  <span>CPU 占用</span>
                  <b>${cpu}%</b>
                </div>
                <div class="progress-bar-bg">
                  <div class="progress-bar-fill ${cpuColor}" style="width: ${Math.min(100, Math.max(2, Number(cpu)))}%"></div>
                </div>
              </div>
              <div>
                <div class="metric-row">
                  <span>内存负载</span>
                  <b>${memUsed} GB / ${memTotal} GB (${memPercent}%)</b>
                </div>
                <div class="progress-bar-bg">
                  <div class="progress-bar-fill ${memColor}" style="width: ${memPercent}%"></div>
                </div>
              </div>
            </div>

            <div class="info-list">
              <div class="info-item">系统: <b>${osVersion}</b></div>
              <div class="info-item">内网 IP: <b style="font-family: monospace;">${lanIp}</b></div>
              <div class="info-item">用户: <b>${username}</b></div>
              <div class="info-item">心跳: <b style="color: ${isOnline ? "var(--success)" : "var(--text-dim)"}">${t.last_heartbeat_elapsed_secs > 86400 ? "很久以前" : t.last_heartbeat_elapsed_secs + "s 前"}</b></div>
            </div>

            <div class="card-actions">
              <button class="btn btn-featured btn-sm" onclick="openRemoteDesktop('${termIdJs}')">🖥️ 远程桌面直控</button>
              <button class="btn btn-secondary btn-sm" onclick="runOverview('${termIdJs}')">📊 硬件概况</button>
              <button class="btn btn-secondary btn-sm" onclick="runScreenshot('${termIdJs}')">📸 屏幕截图</button>
              <button class="btn btn-secondary btn-sm" onclick="runProcessManager('${termIdJs}')">🔍 进程管理</button>
              <button class="btn btn-secondary btn-sm" onclick="runServiceManager('${termIdJs}')">⚙️ 系统服务</button>
              <button class="btn btn-secondary btn-sm" onclick="openCommandModal('${termIdJs}')">⚡ 命令排障</button>
            </div>
          </div>
        `;
}

/**
 * @param {string} terminalId
 * @param {any} proc
 * @returns {string}
 */
export function processRowHtml(terminalId, proc) {
  const name = proc.name || "";
  const exePath = proc.exe_path ? escapeHtml(proc.exe_path) : "";
  return `
          <tr class="proc-row" data-name="${escapeHtml(name.toLowerCase())}" data-pid="${escapeHtml(String(proc.pid))}">
            <td><code style="color: var(--primary);">${escapeHtml(String(proc.pid))}</code></td>
            <td><b>${escapeHtml(name)}</b> ${exePath ? `<span style="color: var(--text-dim); font-size: 11px;">(${exePath})</span>` : ""}</td>
            <td><b>${proc.memory_mb}</b> MB</td>
            <td>${(proc.cpu_usage || 0).toFixed(1)}%</td>
            <td style="text-align: right;">
              <button class="btn btn-danger btn-sm" onclick="killTargetProc('${escapeJsString(terminalId)}', ${Number(proc.pid)}, '${escapeJsString(name)}')">终止</button>
            </td>
          </tr>
        `;
}

export function parseTagList(rawTags) {
  const raw = (rawTags || "").trim();
  if (!raw) return [];
  return raw
    .split(/[,，]/)
    .map((s) => s.trim())
    .filter(Boolean);
}

function memoryPercent(metrics) {
  const used = metrics?.memory_used_mb || 0;
  const total = metrics?.memory_total_mb || 1;
  return Math.min(100, Math.round((used / total) * 100));
}

export function isHighLoad(terminal) {
  const cpu = terminal.latest_metrics?.cpu_usage_percent || 0;
  return cpu > 75 || memoryPercent(terminal.latest_metrics) > 80;
}

export function filterTerminals(terminals, { tab = "all", search = "" } = {}) {
  const query = (search || "").toLowerCase().trim();
  return (terminals || []).filter((t) => {
    const online = t.status === "Online";
    if (tab === "online" && !online) return false;
    if (tab === "offline" && online) return false;
    if (tab === "warn" && (!online || !isHighLoad(t))) return false;

    if (!query) return true;
    const hostname = (t.info?.hostname || "").toLowerCase();
    const ip = (t.info?.lan_ip || "").toLowerCase();
    const id = (t.info?.terminal_id || "").toLowerCase();
    const os = (t.info?.os_version || "").toLowerCase();
    const customName = (t.custom_name || "").toLowerCase();
    const notes = (t.notes || "").toLowerCase();
    const tags = (t.tags || []).join(" ").toLowerCase();
    return (
      hostname.includes(query) ||
      ip.includes(query) ||
      id.includes(query) ||
      os.includes(query) ||
      customName.includes(query) ||
      notes.includes(query) ||
      tags.includes(query)
    );
  });
}
