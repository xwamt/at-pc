import { escapeHtml, escapeJsString } from "../terminals/index.js";

/**
 * @param {any} key
 * @param {number} [now]
 * @returns {{ status: "Active" | "Revoked" | "Expired", label: string, badgeClass: string }}
 */
export function getKeyStatus(key, now = Date.now()) {
  if (key.revoked_at || key.revoked === true) {
    return {
      status: "Revoked",
      label: "已吊销 (Revoked)",
      badgeClass: "status-badge offline",
    };
  }
  if (key.expires_at) {
    const exp = new Date(key.expires_at).getTime();
    if (!Number.isNaN(exp) && exp <= now) {
      return {
        status: "Expired",
        label: "已过期 (Expired)",
        badgeClass: "status-badge high-load",
      };
    }
  }
  return {
    status: "Active",
    label: "有效 (Active)",
    badgeClass: "status-badge online",
  };
}

/**
 * @param {string | null | undefined} iso
 * @returns {string}
 */
export function formatDateTime(iso) {
  if (!iso) return "-";
  try {
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return String(iso);
    const pad = (/** @type {number} */ n) => String(n).padStart(2, "0");
    const y = d.getFullYear();
    const m = pad(d.getMonth() + 1);
    const day = pad(d.getDate());
    const h = pad(d.getHours());
    const min = pad(d.getMinutes());
    const s = pad(d.getSeconds());
    return `${y}-${m}-${day} ${h}:${min}:${s}`;
  } catch {
    return String(iso);
  }
}

/**
 * @param {string[] | string | null | undefined} scopes
 * @returns {string}
 */
export function formatScopes(scopes) {
  if (Array.isArray(scopes) && scopes.length > 0) {
    return scopes
      .map((s) => `<span class="tag-scope">${escapeHtml(s)}</span>`)
      .join(" ");
  }
  if (typeof scopes === "string" && scopes.trim()) {
    return `<span class="tag-scope">${escapeHtml(scopes.trim())}</span>`;
  }
  return `<span style="color: var(--text-dim); font-size: 11px;">* (全部)</span>`;
}

/**
 * @param {any} key
 * @returns {string}
 */
export function keyRowHtml(key) {
  const id = key.id || "";
  const name = key.name || "-";
  const prefix = key.prefix || (key.id ? `...${key.id}` : "-");
  const kind = key.kind || "sk";
  const role = String(key.role || "viewer").toLowerCase();
  const statusInfo = getKeyStatus(key);
  const createdAtStr = formatDateTime(key.created_at);
  const expiresAtStr = key.expires_at ? formatDateTime(key.expires_at) : "永不过期";

  const kindBadge =
    kind === "sk"
      ? `<span class="tag-kind tag-kind-sk">sk (客户端)</span>`
      : kind === "dv"
      ? `<span class="tag-kind tag-kind-dv">dv (设备)</span>`
      : `<span class="tag-kind">${escapeHtml(kind)}</span>`;

  let roleBadgeClass = "tag-role-viewer";
  if (role === "admin") roleBadgeClass = "tag-role-admin";
  else if (role === "operator") roleBadgeClass = "tag-role-operator";
  const roleBadge = `<span class="tag-role ${roleBadgeClass}">${escapeHtml(role)}</span>`;

  let actionsHtml = "";
  if (statusInfo.status === "Active") {
    actionsHtml = `
      <div style="display: flex; gap: 6px;">
        <button class="btn btn-secondary btn-sm" onclick="openRotateKeyModal('${escapeJsString(id)}', '${escapeJsString(name)}', '${escapeJsString(prefix)}')" title="轮换此密钥">🔄 轮换</button>
        <button class="btn btn-danger btn-sm" onclick="openRevokeKeyModal('${escapeJsString(id)}', '${escapeJsString(name)}', '${escapeJsString(prefix)}')" title="吊销此密钥">🚫 吊销</button>
      </div>
    `;
  } else if (statusInfo.status === "Expired") {
    actionsHtml = `
      <div style="display: flex; gap: 6px;">
        <button class="btn btn-danger btn-sm" onclick="openRevokeKeyModal('${escapeJsString(id)}', '${escapeJsString(name)}', '${escapeJsString(prefix)}')" title="吊销此密钥">🚫 吊销</button>
      </div>
    `;
  } else {
    actionsHtml = `<span style="color: var(--text-dim); font-size: 11px;">已失效</span>`;
  }

  return `
    <tr>
      <td style="font-weight: 600; color: #f8fafc;">${escapeHtml(name)}</td>
      <td><code class="key-prefix">${escapeHtml(prefix)}</code></td>
      <td>${kindBadge}</td>
      <td>${roleBadge}</td>
      <td>${formatScopes(key.scopes)}</td>
      <td style="font-size: 11.5px; color: var(--text-muted);" title="创建于: ${createdAtStr}&#10;过期时间: ${expiresAtStr}">${createdAtStr}</td>
      <td><span class="${statusInfo.badgeClass}">${statusInfo.label}</span></td>
      <td>${actionsHtml}</td>
    </tr>
  `;
}

/**
 * @param {any} keys
 * @returns {string}
 */
export function keysTableHtml(keys) {
  const list = Array.isArray(keys) ? keys : (keys?.keys || []);
  if (list.length === 0) {
    return `
      <div class="empty-state" style="padding: 40px 20px;">
        <h3>暂无 API 密钥记录</h3>
        <p style="margin-top: 8px; font-size: 13px;">点击右上角「➕ 创建密钥」为客户端或设备签发新密钥</p>
      </div>
    `;
  }

  const rows = list.map((k) => keyRowHtml(k)).join("");
  return `
    <div style="overflow-x: auto; max-height: 60vh;">
      <table class="data-table keys-table">
        <thead>
          <tr>
            <th style="min-width: 110px;">密钥名称</th>
            <th style="min-width: 140px;">前缀 (Prefix)</th>
            <th style="min-width: 90px;">类型</th>
            <th style="min-width: 80px;">角色</th>
            <th style="min-width: 120px;">权限范围</th>
            <th style="min-width: 140px;">创建时间</th>
            <th style="min-width: 110px;">状态</th>
            <th style="min-width: 130px;">操作</th>
          </tr>
        </thead>
        <tbody>
          ${rows}
        </tbody>
      </table>
    </div>
  `;
}
