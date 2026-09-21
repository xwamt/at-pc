import { apiLogin, getAuthToken, restPaths, setAuthToken } from "./api/index.js";
import {
  bindDesktopDocumentListeners,
  checkModifierCursor,
  desktopState,
  injectSpecialKey,
  openRemoteDesktop,
  releaseAllInputs,
  setCursorMode,
  setScaleMode,
  stopAndCloseDesktop,
  switchStreamDisplay,
  toggleCanvasFullscreen,
} from "./desktop/session.js";
import { setUnauthorizedHandler, authFetch } from "./http.js";
import {
  cancelRunningCall,
  cancelTerminalCmd,
  deleteOfflineTerminal,
  executeCommand,
  executeSvcAction,
  fetchTerminals,
  filterProcTable,
  killTargetProc,
  openCommandModal,
  openEditMetaModal,
  querySvcStatus,
  renderCards,
  runOverview,
  runProcessManager,
  runScreenshot,
  runServiceManager,
  saveTerminalMeta,
  setFilterTab,
  showRunningCallsModal,
} from "./terminals/ui.js";
import { terminalsState } from "./terminals/state.js";
import {
  closeModal,
  copyTextToClipboard,
  hideAuthModal,
  setModalBeforeClose,
  showAuthModal,
  showModal,
} from "./ui.js";
import { byId } from "./dom.js";
import {
  checkAdminRole,
  copyOneTimeKey,
  openCreateKeyModal,
  openRevokeKeyModal,
  openRotateKeyModal,
  refreshAdminAccess,
  showKeysModal,
  showOneTimeKeyModal,
  submitCreateKey,
  submitRevokeKey,
  submitRotateKey,
} from "./keys/ui.js";

export { copyTextToClipboard };

let isAuthRequired = false;

function handleUnauthorized() {
  isAuthRequired = true;
  terminalsState.terminalsData = [];
  const grid = byId("terminalGrid");
  if (grid) {
    grid.innerHTML = `
            <div class="empty-state">
              <h3>🔐 请先输入 API Key</h3>
              <p style="margin-top: 8px; font-size: 13px;">服务端已开启鉴权保护，验证通过后方可加载拓扑设备</p>
              <button class="btn btn-primary btn-sm" style="margin-top: 14px;" onclick="showAuthModal()">输入 API Key</button>
            </div>
          `;
  }
  const keysBtn = byId("keysBtn");
  if (keysBtn) keysBtn.style.display = "none";
  const keysTabBtn = byId("keysTabBtn");
  if (keysTabBtn) keysTabBtn.style.display = "none";
  promptAuth();
}

function promptAuth() {
  const input = byId("authTokenInput");
  if (input) {
    input.value = getAuthToken();
  }
  const authError = byId("authErrorMsg");
  if (authError) {
    authError.style.display = "none";
    authError.textContent = "认证失败：API Key 无效或已过期";
  }
  showAuthModal();
}

export async function submitAuthToken() {
  const tokenEl = byId("authTokenInput");
  const token = tokenEl?.value?.trim();
  if (!token) return;

  const authError = byId("authErrorMsg");
  try {
    const loginRes = await apiLogin(token);
    if (loginRes.ok) {
      if (authError) authError.style.display = "none";
      setAuthToken(token);
      const res = await authFetch(restPaths.terminals());
      if (res.ok) {
        isAuthRequired = false;
        hideAuthModal();
        terminalsState.terminalsData = await res.json();
        renderCards();
        refreshAdminAccess();
      } else {
        if (authError) {
          authError.textContent = "认证失败：API Key 无效或已过期";
          authError.style.display = "block";
        }
      }
      return;
    }

    if (loginRes.status === 429) {
      let msg = "请求过于频繁，请稍后再试 (429)";
      try {
        const body = await loginRes.json();
        if (body?.error) {
          msg = `请求过于频繁：${body.error}`;
        }
      } catch {
        // ignore
      }
      if (authError) {
        authError.textContent = msg;
        authError.style.display = "block";
      }
      return;
    }

    if (loginRes.status === 401) {
      if (authError) {
        authError.textContent = "认证失败：API Key 无效或已过期";
        authError.style.display = "block";
      }
      return;
    }

    // Fallback for servers without /api/auth/login endpoint (404)
    if (loginRes.status === 404) {
      setAuthToken(token);
      const res = await authFetch(restPaths.terminals());
      if (res.ok) {
        isAuthRequired = false;
        if (authError) authError.style.display = "none";
        hideAuthModal();
        terminalsState.terminalsData = await res.json();
        renderCards();
        refreshAdminAccess();
        return;
      }
    }

    if (authError) {
      authError.textContent = "认证失败：API Key 无效或已过期";
      authError.style.display = "block";
    }
  } catch {
    if (authError) {
      authError.textContent = "认证请求失败，请检查网络连接";
      authError.style.display = "block";
    }
  }
}

export function showMcpConfigModal() {
  const host = window.location.host;
  const sseUrl = `http://${host}/sse`;
  const configJson = JSON.stringify(
    {
      mcpServers: {
        "at-pc-central": {
          url: sseUrl,
        },
      },
    },
    null,
    2,
  );

  copyTextToClipboard(configJson)
    .then(() => {
      const btn = byId("mcpConfigBtn");
      if (btn) {
        const orig = btn.innerText;
        btn.innerText = "✓ 已复制配置";
        setTimeout(() => {
          btn.innerText = orig;
        }, 1500);
      }
    })
    .catch(() => {});

  showModal(
    "AI Agent MCP 配置 JSON",
    `<p style="margin-bottom: 10px; font-size: 12.5px; color: var(--text-muted);">已尝试写入剪贴板。若浏览器禁止直接访问剪贴板，请全选复制以下配置至 Cursor 或 Claude 的 mcp.json：</p><pre style="user-select: all;">${configJson}</pre>`,
  );
}

function exposeGlobals() {
  const api = {
    showAuthModal: promptAuth,
    submitAuthToken,
    showMcpConfigModal,
    closeModal,
    fetchTerminals,
    renderCards,
    setFilterTab,
    showRunningCallsModal,
    cancelRunningCall,
    openEditMetaModal,
    saveTerminalMeta,
    deleteOfflineTerminal,
    runOverview,
    runScreenshot,
    runProcessManager,
    filterProcTable,
    killTargetProc,
    runServiceManager,
    querySvcStatus,
    executeSvcAction,
    openCommandModal,
    cancelTerminalCmd,
    executeCommand,
    openRemoteDesktop,
    setCursorMode,
    setScaleMode,
    releaseAllInputs,
    injectSpecialKey,
    switchStreamDisplay,
    toggleCanvasFullscreen,
    stopAndCloseDesktop,
    checkModifierCursor,
    showKeysModal,
    openCreateKeyModal,
    submitCreateKey,
    openRevokeKeyModal,
    submitRevokeKey,
    openRotateKeyModal,
    submitRotateKey,
    showOneTimeKeyModal,
    copyOneTimeKey,
    checkAdminRole,
    refreshAdminAccess,
  };
  Object.assign(window, api);
}

export function boot() {
  setUnauthorizedHandler(handleUnauthorized);
  setModalBeforeClose(() => {
    if (desktopState.currentStreamTid) {
      stopAndCloseDesktop();
    }
  });
  bindDesktopDocumentListeners();
  exposeGlobals();
  window.onclick = function (event) {
    const target = /** @type {HTMLElement | null} */ (event.target);
    if (target?.id === "resultModal") closeModal();
  };
  fetchTerminals();
  refreshAdminAccess();
  setInterval(() => {
    if (!isAuthRequired) {
      fetchTerminals();
    }
  }, 2000);
  setInterval(() => {
    if (!isAuthRequired) {
      refreshAdminAccess();
    }
  }, 10000);
}
