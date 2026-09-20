import { restPaths } from "../api/index.js";
import { byId } from "../dom.js";
import { authFetch } from "../http.js";
import { escapeHtml, escapeJsString } from "../terminals/index.js";
import { copyTextToClipboard, showModal } from "../ui.js";
import { keysTableHtml } from "./index.js";

/**
 * Checks if the current user has the admin role, or can call /api/keys.
 * @returns {Promise<boolean>}
 */
export async function checkAdminRole() {
  try {
    const res = await authFetch(restPaths.authMe());
    if (res.ok) {
      const data = await res.json();
      const role = String(data.role || data.principal?.role || "").toLowerCase();
      if (role === "admin") return true;
    }
  } catch {
    // ignore
  }

  // Fallback: probe GET /api/keys
  try {
    const keysRes = await authFetch(restPaths.keys());
    if (keysRes.ok) return true;
  } catch {
    // ignore
  }
  return false;
}

/**
 * Updates visibility of Keys UI elements based on admin check.
 * @returns {Promise<boolean>}
 */
export async function refreshAdminAccess() {
  const isAdmin = await checkAdminRole();
  const keysBtn = byId("keysBtn");
  if (keysBtn) {
    keysBtn.style.display = isAdmin ? "inline-flex" : "none";
  }
  const keysTabBtn = byId("keysTabBtn");
  if (keysTabBtn) {
    keysTabBtn.style.display = isAdmin ? "inline-block" : "none";
  }
  return isAdmin;
}

/**
 * Opens the Keys management modal and loads key list from GET /api/keys.
 */
export async function showKeysModal() {
  showModal(
    "🔑 API 密钥管理",
    `<div style="padding: 30px 20px; text-align: center; color: var(--text-muted);"><pre>正在获取 API 密钥列表...</pre></div>`,
    true,
  );

  try {
    const res = await authFetch(restPaths.keys());
    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      const msg = err.error || err.message || `HTTP ${res.status}`;
      showModal(
        "🔑 API 密钥管理",
        `<div class="empty-state">
           <h3>无法加载密钥列表</h3>
           <p style="margin-top: 8px; color: var(--danger); font-size: 13px;">${escapeHtml(msg)}</p>
           <p style="margin-top: 4px; font-size: 12px; color: var(--text-dim);">仅管理员 (Admin) 角色可查看与维护服务端密钥库</p>
           <button class="btn btn-secondary btn-sm" style="margin-top: 14px;" onclick="showKeysModal()">重试</button>
         </div>`,
        true,
      );
      return;
    }

    const data = await res.json();
    const tableMarkup = keysTableHtml(data);
    const content = `
      <div class="keys-panel">
        <div class="keys-panel-header">
          <div>
            <div style="font-size: 14px; font-weight: 700; color: #fff;">🔑 密钥库管理 (KeyStore)</div>
            <div style="font-size: 12px; color: var(--text-muted); margin-top: 2px;">
              管理服务端签发的客户端 (sk) 与设备 (dv) API Key。支持生成、平滑过渡轮换及撤销凭据。
            </div>
          </div>
          <div style="display: flex; gap: 8px; align-items: center;">
            <button class="btn btn-secondary btn-sm" onclick="showKeysModal()">🔄 刷新列表</button>
            <button class="btn btn-primary btn-sm" onclick="openCreateKeyModal()">➕ 创建密钥</button>
          </div>
        </div>
        ${tableMarkup}
      </div>
    `;
    showModal("🔑 API 密钥管理", content, true);
  } catch (err) {
    const error = /** @type {Error} */ (err);
    showModal(
      "🔑 API 密钥管理",
      `<div class="empty-state">
         <h3>加载异常</h3>
         <p style="margin-top: 8px; color: var(--danger); font-size: 13px;">${escapeHtml(error.message)}</p>
         <button class="btn btn-secondary btn-sm" style="margin-top: 14px;" onclick="showKeysModal()">重试</button>
       </div>`,
      true,
    );
  }
}

/**
 * Opens modal dialog to create a new API key.
 */
export function openCreateKeyModal() {
  const content = `
    <div style="display: flex; flex-direction: column; gap: 14px;">
      <div class="form-group" style="margin-bottom: 0;">
        <label class="form-label" style="color: #f1f5f9; font-weight: 600;">
          密钥名称 (Name) <span style="color: var(--danger);">*</span>
        </label>
        <input type="text" id="createKeyName" class="form-control" placeholder="例如：ci-pipeline, console-admin, dev-mac" autofocus onkeydown="if(event.key==='Enter') submitCreateKey()">
        <div style="font-size: 11px; color: var(--text-dim); margin-top: 4px;">标识此密钥所有者或用途，便于后续审计与鉴权排查。</div>
      </div>

      <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 12px;">
        <div class="form-group" style="margin-bottom: 0;">
          <label class="form-label" style="color: #f1f5f9; font-weight: 600;">类型 (Kind) <span style="color: var(--danger);">*</span></label>
          <select id="createKeyKind" class="form-control">
            <option value="sk">sk (客户端/用户密钥)</option>
            <option value="dv">dv (设备端注册密钥)</option>
          </select>
        </div>

        <div class="form-group" style="margin-bottom: 0;">
          <label class="form-label" style="color: #f1f5f9; font-weight: 600;">角色 (Role) <span style="color: var(--danger);">*</span></label>
          <select id="createKeyRole" class="form-control">
            <option value="admin">admin (超级管理员 · 包含全部管理与密钥权限)</option>
            <option value="operator">operator (运维操作员 · 允许操作终端与执行诊断)</option>
            <option value="viewer">viewer (只读监控 · 仅允许查看拓扑与状态)</option>
          </select>
        </div>
      </div>

      <div class="form-group" style="margin-bottom: 0;">
        <label class="form-label" style="color: #f1f5f9; font-weight: 600;">权限范围 (Scopes，可选)</label>
        <input type="text" id="createKeyScopes" class="form-control" placeholder="留空默认全部权限，多个范围以逗号分隔，如：console, mcp" onkeydown="if(event.key==='Enter') submitCreateKey()">
        <div style="font-size: 11px; color: var(--text-dim); margin-top: 4px;">在角色权限之上进一步收窄允许调用的能力范围。</div>
      </div>

      <div class="form-group" style="margin-bottom: 0;">
        <label class="form-label" style="color: #f1f5f9; font-weight: 600;">有效期限 (天数，可选)</label>
        <input type="number" id="createKeyExpiresDays" class="form-control" placeholder="留空或输入 0 表示永久有效；如 30, 90, 365" min="0" step="1" onkeydown="if(event.key==='Enter') submitCreateKey()">
      </div>

      <div id="createKeyErrorMsg" style="color: var(--danger); font-size: 12.5px; margin-top: 4px; display: none;"></div>

      <div style="display: flex; justify-content: flex-end; gap: 8px; margin-top: 8px; padding-top: 14px; border-top: 1px solid var(--card-border);">
        <button class="btn btn-secondary btn-sm" onclick="showKeysModal()">返回列表</button>
        <button class="btn btn-primary btn-sm" onclick="submitCreateKey()">确认创建</button>
      </div>
    </div>
  `;
  showModal("➕ 创建 API 密钥", content, false);
}

/**
 * Submits the create key form to POST /api/keys.
 */
export async function submitCreateKey() {
  const nameInput = /** @type {HTMLInputElement | null} */ (byId("createKeyName"));
  const kindSelect = /** @type {HTMLSelectElement | null} */ (byId("createKeyKind"));
  const roleSelect = /** @type {HTMLSelectElement | null} */ (byId("createKeyRole"));
  const scopesInput = /** @type {HTMLInputElement | null} */ (byId("createKeyScopes"));
  const daysInput = /** @type {HTMLInputElement | null} */ (byId("createKeyExpiresDays"));
  const errorEl = byId("createKeyErrorMsg");

  const name = nameInput?.value?.trim();
  if (!name) {
    if (errorEl) {
      errorEl.innerText = "请输入密钥名称";
      errorEl.style.display = "block";
    }
    nameInput?.focus();
    return;
  }

  const kind = kindSelect?.value || "sk";
  const role = roleSelect?.value || "admin";
  const scopesRaw = scopesInput?.value?.trim();
  const daysRaw = daysInput?.value?.trim();

  /** @type {Record<string, any>} */
  const payload = {
    name,
    kind,
    role,
  };

  if (scopesRaw) {
    payload.scopes = scopesRaw
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);
  }
  if (daysRaw) {
    const days = parseInt(daysRaw, 10);
    if (!Number.isNaN(days) && days > 0) {
      payload.expires_in_days = days;
    }
  }

  try {
    const res = await authFetch(restPaths.keys(), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });

    if (res.ok) {
      const data = await res.json();
      const plaintext = data.plaintext || data.key || data.token || "";
      showOneTimeKeyModal(plaintext, "密钥创建成功");
    } else {
      const err = await res.json().catch(() => ({}));
      const msg = err.error || err.message || `创建失败 (HTTP ${res.status})`;
      if (errorEl) {
        errorEl.innerText = msg;
        errorEl.style.display = "block";
      }
    }
  } catch (err) {
    const error = /** @type {Error} */ (err);
    if (errorEl) {
      errorEl.innerText = `请求异常: ${error.message}`;
      errorEl.style.display = "block";
    }
  }
}

/**
 * Displays one-time plaintext key modal with copy button and warning.
 * @param {string} plaintext
 * @param {string} [title]
 */
export function showOneTimeKeyModal(plaintext, title = "密钥生成成功") {
  const content = `
    <div style="display: flex; flex-direction: column; gap: 16px;">
      <div class="warning-banner">
        <div style="font-weight: 700; font-size: 13.5px; margin-bottom: 6px; display: flex; align-items: center; gap: 6px;">
          <span>⚠️ 请立即复制并妥善保管您的 API 密钥！</span>
        </div>
        <div>
          出于最高安全原则，该密钥明文<b>仅在此刻完整展示一次</b>。窗口一旦关闭，服务端绝不存储也不可再次找回该明文！若遗失则需重新创建或轮换。
        </div>
      </div>

      <div class="form-group" style="margin-bottom: 0;">
        <label class="form-label" style="color: #f1f5f9; font-weight: 600;">API 密钥明文 (Plaintext API Key)：</label>
        <div style="display: flex; gap: 8px; align-items: stretch;">
          <input type="text" id="oneTimeKeyInput" class="form-control" value="${escapeHtml(plaintext)}" readonly style="font-size: 13px; font-weight: 600; color: var(--primary); background: #050811; user-select: all; font-family: ui-monospace, monospace;">
          <button id="copyOneTimeKeyBtn" class="btn btn-primary btn-sm" onclick="copyOneTimeKey()" style="white-space: nowrap; padding: 0 16px;">📋 复制</button>
        </div>
      </div>

      <div style="display: flex; justify-content: flex-end; gap: 8px; margin-top: 8px; padding-top: 14px; border-top: 1px solid var(--card-border);">
        <button class="btn btn-success btn-sm" onclick="showKeysModal()">我已妥善保存，返回密钥列表</button>
      </div>
    </div>
  `;
  showModal(`🔑 ${title}`, content, false);
}

/**
 * Copies the one-time key to clipboard.
 */
export function copyOneTimeKey() {
  const input = /** @type {HTMLInputElement | null} */ (byId("oneTimeKeyInput"));
  const text = input?.value || "";
  if (!text) return;
  copyTextToClipboard(text)
    .then(() => {
      const btn = byId("copyOneTimeKeyBtn");
      if (btn) {
        const orig = btn.innerText;
        btn.innerText = "✓ 已复制";
        setTimeout(() => {
          if (btn) btn.innerText = orig;
        }, 1500);
      }
    })
    .catch(() => {});
}

/**
 * Opens confirmation modal before revoking an API key.
 * @param {string} id
 * @param {string} name
 * @param {string} prefix
 */
export function openRevokeKeyModal(id, name, prefix) {
  const displayName = name || prefix || id;
  const content = `
    <div style="display: flex; flex-direction: column; gap: 14px;">
      <p style="font-size: 13.5px; color: #f8fafc; line-height: 1.6;">
        确定要吊销密钥 <b style="color: var(--danger); font-family: monospace;">${escapeHtml(displayName)}</b> 吗？
      </p>

      <div style="background: rgba(244, 63, 94, 0.1); border: 1px solid rgba(244, 63, 94, 0.3); border-radius: 8px; padding: 12px 14px; font-size: 12px; color: #fca5a5; line-height: 1.5;">
        ⚠️ <b>警告</b>：吊销后该密钥将立即失效，所有使用该密钥的客户端及脚本将无法再通过鉴权网关。此操作不可逆！
      </div>

      <div id="revokeErrorMsg" style="color: var(--danger); font-size: 12.5px; display: none;"></div>

      <div style="display: flex; justify-content: flex-end; gap: 8px; margin-top: 8px; padding-top: 12px; border-top: 1px solid var(--card-border);">
        <button class="btn btn-secondary btn-sm" onclick="showKeysModal()">取消</button>
        <button class="btn btn-danger btn-sm" onclick="submitRevokeKey('${escapeJsString(id)}')">确认吊销</button>
      </div>
    </div>
  `;
  showModal("🚫 确认吊销 API 密钥", content, false);
}

/**
 * Sends DELETE /api/keys/:id to revoke key.
 * @param {string} id
 */
export async function submitRevokeKey(id) {
  const errorEl = byId("revokeErrorMsg");
  try {
    const res = await authFetch(restPaths.key(id), {
      method: "DELETE",
    });

    if (res.ok) {
      showKeysModal();
    } else {
      const err = await res.json().catch(() => ({}));
      const msg = err.error || err.message || `吊销失败 (HTTP ${res.status})`;
      if (errorEl) {
        errorEl.innerText = msg;
        errorEl.style.display = "block";
      }
    }
  } catch (err) {
    const error = /** @type {Error} */ (err);
    if (errorEl) {
      errorEl.innerText = `请求异常: ${error.message}`;
      errorEl.style.display = "block";
    }
  }
}

/**
 * Opens dialog asking for grace period before rotating an API key.
 * @param {string} id
 * @param {string} name
 * @param {string} prefix
 */
export function openRotateKeyModal(id, name, prefix) {
  const displayName = name || prefix || id;
  const content = `
    <div style="display: flex; flex-direction: column; gap: 14px;">
      <p style="font-size: 13px; color: var(--text-muted); line-height: 1.5;">
        轮换将为密钥 <b style="color: var(--primary); font-family: monospace;">${escapeHtml(displayName)}</b> 签发一把全新密钥。旧密钥将在设定的过渡缓冲期结束后自动失效。
      </p>

      <div class="form-group" style="margin-bottom: 0;">
        <label class="form-label" style="color: #f1f5f9; font-weight: 600;">
          缓冲过渡期（小时，Grace Period）：
        </label>
        <input type="number" id="rotateGraceHours" class="form-control" value="24" min="0" step="1" style="font-size: 13px;" onkeydown="if(event.key==='Enter') submitRotateKey('${escapeJsString(id)}')">
        <div style="font-size: 11px; color: var(--text-dim); margin-top: 6px;">
          默认 24 小时。旧密钥在此期间仍可用于鉴权平滑过渡；若输入 0 则旧密钥立即失效。
        </div>
      </div>

      <div id="rotateErrorMsg" style="color: var(--danger); font-size: 12.5px; display: none;"></div>

      <div style="display: flex; justify-content: flex-end; gap: 8px; margin-top: 8px; padding-top: 12px; border-top: 1px solid var(--card-border);">
        <button class="btn btn-secondary btn-sm" onclick="showKeysModal()">取消</button>
        <button class="btn btn-primary btn-sm" onclick="submitRotateKey('${escapeJsString(id)}')">确认轮换</button>
      </div>
    </div>
  `;
  showModal(`🔄 轮换密钥 · ${escapeHtml(displayName)}`, content, false);
}

/**
 * Sends POST /api/keys/:id/rotate to rotate key.
 * @param {string} id
 */
export async function submitRotateKey(id) {
  const hoursInput = /** @type {HTMLInputElement | null} */ (byId("rotateGraceHours"));
  const errorEl = byId("rotateErrorMsg");

  let hours = 24;
  if (hoursInput?.value !== "") {
    const parsed = parseInt(hoursInput?.value || "24", 10);
    if (!Number.isNaN(parsed) && parsed >= 0) {
      hours = parsed;
    }
  }

  try {
    const res = await authFetch(restPaths.rotateKey(id), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ grace_hours: hours }),
    });

    if (res.ok) {
      const data = await res.json();
      const plaintext = data.plaintext || data.key || data.token || "";
      showOneTimeKeyModal(plaintext, "密钥轮换成功");
    } else {
      const err = await res.json().catch(() => ({}));
      const msg = err.error || err.message || `轮换失败 (HTTP ${res.status})`;
      if (errorEl) {
        errorEl.innerText = msg;
        errorEl.style.display = "block";
      }
    }
  } catch (err) {
    const error = /** @type {Error} */ (err);
    if (errorEl) {
      errorEl.innerText = `请求异常: ${error.message}`;
      errorEl.style.display = "block";
    }
  }
}
