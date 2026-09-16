import { getAuthToken, restPaths, setAuthToken } from "./api/index.js";
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
import { closeModal, hideAuthModal, setModalBeforeClose, showAuthModal, showModal } from "./ui.js";
import { byId } from "./dom.js";

function handleUnauthorized() {
  terminalsState.terminalsData = [];
  const grid = byId("terminalGrid");
  if (grid) {
    grid.innerHTML = `
            <div class="empty-state">
              <h3>🔐 请先输入访问凭据</h3>
              <p style="margin-top: 8px; font-size: 13px;">服务端已开启鉴权保护，验证通过后方可加载拓扑设备</p>
              <button class="btn btn-primary btn-sm" style="margin-top: 14px;" onclick="showAuthModal()">输入访问凭据</button>
            </div>
          `;
  }
  promptAuth();
}

function promptAuth() {
  const input = byId("authTokenInput");
  if (input) {
    input.value = getAuthToken();
  }
  showAuthModal();
}

export async function submitAuthToken() {
  const tokenEl = byId("authTokenInput");
  const token = tokenEl?.value?.trim();
  if (!token) return;
  setAuthToken(token);
  const res = await authFetch(restPaths.terminals());
  if (res.ok) {
    const authError = byId("authErrorMsg");
    if (authError) authError.style.display = "none";
    hideAuthModal();
    terminalsState.terminalsData = await res.json();
    renderCards();
  } else {
    const authError = byId("authErrorMsg");
    if (authError) authError.style.display = "block";
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

  navigator.clipboard
    .writeText(configJson)
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
    "AI Agent MCP 配置 JSON (已复制剪贴板)",
    `<p style="margin-bottom: 10px; font-size: 12.5px; color: var(--text-muted);">已将配置写入剪贴板，可直接粘贴至 Cursor 或 Claude 的 mcp.json：</p><pre>${configJson}</pre>`,
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
  setInterval(fetchTerminals, 2000);
}
