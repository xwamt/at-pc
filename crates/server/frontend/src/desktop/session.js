import { restPaths } from "../api/index.js";
import { byId, qsAll } from "../dom.js";
import { getNormalizedCoords, pointerToDesktopEvent } from "./index.js";
import { authFetch } from "../http.js";
import { showModal } from "../ui.js";

export const desktopState = {
  /** @type {string | null} */
  currentStreamTid: null,
  /** @type {ReturnType<typeof setTimeout> | null} */
  streamPollTimer: null,
  rdActualW: 1920,
  rdActualH: 1080,
  lastFrameTime: Date.now(),
  frameCount: 0,
  /** @type {ReturnType<typeof setInterval> | null} */
  fpsTimer: null,
  isSendingInput: false,
  /** @type {any} */
  pendingMouseMove: null,
  currentCursorMode: "default",
  currentScaleMode: "fit",
  currentStreamDisplayIndex: 0,
  /** @type {any[]} */
  activeMonitorsList: [],
  isFlushingMove: false,
  inputQueue: Promise.resolve(),
  isLocalMouseDown: false,
  localPressedButton: 0,
  /** @type {((event: MouseEvent) => void) | null} */
  globalMouseUpHandler: null,
  /** @type {(() => void) | null} */
  windowBlurHandler: null,
};

export function setCursorMode(mode, btn) {
  desktopState.currentCursorMode = mode;
  qsAll(".cursor-tool-btn").forEach((item) => {
    item.classList.remove("active");
  });
  if (btn) btn.classList.add("active");
  const canvas = byId("rdCanvas");
  if (canvas) {
    canvas.style.cursor = mode;
  }
}

export function setScaleMode(mode, btn) {
  desktopState.currentScaleMode = mode;
  qsAll(".scale-tool-btn").forEach((item) => {
    item.classList.remove("active");
  });
  if (btn) btn.classList.add("active");
  const canvas = byId("rdCanvas");
  if (canvas) {
    if (mode === "stretch") {
      canvas.style.objectFit = "fill";
      canvas.style.width = "100%";
      canvas.style.height = "100%";
    } else if (mode === "original") {
      canvas.style.objectFit = "none";
      canvas.style.width = "auto";
      canvas.style.height = "auto";
    } else {
      canvas.style.objectFit = "contain";
      canvas.style.width = "100%";
      canvas.style.height = "100%";
    }
  }
}

export function checkModifierCursor(event) {
  if (desktopState.currentCursorMode !== "default") return;
  const canvas = byId("rdCanvas");
  if (!canvas) return;
  if (event.altKey && event.shiftKey) {
    canvas.style.cursor = "nwse-resize";
  } else if (event.altKey) {
    canvas.style.cursor = "ew-resize";
  } else if (event.shiftKey) {
    canvas.style.cursor = "ns-resize";
  } else if (event.ctrlKey) {
    canvas.style.cursor = "move";
  } else {
    canvas.style.cursor = "default";
  }
}

export function sendCanvasPointerMove(terminalId, canvas, event) {
  const coords = getNormalizedCoords(
    canvas,
    event,
    desktopState.currentScaleMode,
  );
  const payload = pointerToDesktopEvent({
    coords,
    monitors: desktopState.activeMonitorsList,
    displayIndex: desktopState.currentStreamDisplayIndex,
  });
  sendDesktopInput(terminalId, payload);
}

export function sendDesktopInput(terminalId, event) {
  if (!desktopState.currentStreamTid) return;

  if (event.action === "MouseMove" || event.action === "MouseMovePixel") {
    desktopState.pendingMouseMove = event;
    flushMouseMove(terminalId);
    return;
  }

  desktopState.inputQueue = desktopState.inputQueue.then(async () => {
    if (desktopState.pendingMouseMove) {
      const moveEvt = desktopState.pendingMouseMove;
      desktopState.pendingMouseMove = null;
      try {
        await authFetch(restPaths.desktopInput(terminalId), {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(moveEvt),
        });
      } catch {
        /* ignore move flush errors */
      }
    }
    try {
      await authFetch(restPaths.desktopInput(terminalId), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(event),
      });
    } catch (err) {
      console.warn("Failed to send input event:", err);
    }
  });
}

export function flushMouseMove(terminalId) {
  if (
    desktopState.isFlushingMove ||
    !desktopState.pendingMouseMove ||
    !desktopState.currentStreamTid
  ) {
    return;
  }

  desktopState.isFlushingMove = true;
  const event = desktopState.pendingMouseMove;
  desktopState.pendingMouseMove = null;

  authFetch(restPaths.desktopInput(terminalId), {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(event),
  })
    .catch((err) => console.warn("MouseMove send error:", err))
    .finally(() => {
      desktopState.isFlushingMove = false;
      if (desktopState.pendingMouseMove && desktopState.currentStreamTid) {
        setTimeout(() => flushMouseMove(terminalId), 20);
      }
    });
}

export function releaseAllInputs(terminalId) {
  if (!terminalId) terminalId = desktopState.currentStreamTid;
  if (!terminalId) return;
  desktopState.isLocalMouseDown = false;
  sendDesktopInput(terminalId, { action: "MouseUp", data: { button: 0 } });
  sendDesktopInput(terminalId, { action: "MouseUp", data: { button: 2 } });
  sendDesktopInput(terminalId, { action: "MouseUp", data: { button: 1 } });
  sendDesktopInput(terminalId, {
    action: "KeyUp",
    data: { key_code: 16, key: "Shift" },
  });
  sendDesktopInput(terminalId, {
    action: "KeyUp",
    data: { key_code: 17, key: "Control" },
  });
  sendDesktopInput(terminalId, {
    action: "KeyUp",
    data: { key_code: 18, key: "Alt" },
  });
}

export function injectSpecialKey(terminalId, actionType) {
  if (!desktopState.currentStreamTid) return;
  if (actionType === "ctrl_alt_del") {
    sendDesktopInput(terminalId, {
      action: "KeyDown",
      data: { key_code: 17, key: "Control" },
    });
    sendDesktopInput(terminalId, {
      action: "KeyDown",
      data: { key_code: 18, key: "Alt" },
    });
    sendDesktopInput(terminalId, {
      action: "KeyDown",
      data: { key_code: 46, key: "Delete" },
    });
    setTimeout(() => {
      sendDesktopInput(terminalId, {
        action: "KeyUp",
        data: { key_code: 46, key: "Delete" },
      });
      sendDesktopInput(terminalId, {
        action: "KeyUp",
        data: { key_code: 18, key: "Alt" },
      });
      sendDesktopInput(terminalId, {
        action: "KeyUp",
        data: { key_code: 17, key: "Control" },
      });
    }, 80);
  } else if (actionType === "win_r") {
    sendDesktopInput(terminalId, {
      action: "KeyDown",
      data: { key_code: 91, key: "Meta" },
    });
    sendDesktopInput(terminalId, {
      action: "KeyDown",
      data: { key_code: 82, key: "r" },
    });
    setTimeout(() => {
      sendDesktopInput(terminalId, {
        action: "KeyUp",
        data: { key_code: 82, key: "r" },
      });
      sendDesktopInput(terminalId, {
        action: "KeyUp",
        data: { key_code: 91, key: "Meta" },
      });
    }, 80);
  } else if (actionType === "taskmgr") {
    sendDesktopInput(terminalId, {
      action: "KeyDown",
      data: { key_code: 17, key: "Control" },
    });
    sendDesktopInput(terminalId, {
      action: "KeyDown",
      data: { key_code: 16, key: "Shift" },
    });
    sendDesktopInput(terminalId, {
      action: "KeyDown",
      data: { key_code: 27, key: "Escape" },
    });
    setTimeout(() => {
      sendDesktopInput(terminalId, {
        action: "KeyUp",
        data: { key_code: 27, key: "Escape" },
      });
      sendDesktopInput(terminalId, {
        action: "KeyUp",
        data: { key_code: 16, key: "Shift" },
      });
      sendDesktopInput(terminalId, {
        action: "KeyUp",
        data: { key_code: 17, key: "Control" },
      });
    }, 80);
  } else if (actionType === "esc") {
    sendDesktopInput(terminalId, {
      action: "KeyDown",
      data: { key_code: 27, key: "Escape" },
    });
    setTimeout(() => {
      sendDesktopInput(terminalId, {
        action: "KeyUp",
        data: { key_code: 27, key: "Escape" },
      });
    }, 50);
  }
}

export async function openRemoteDesktop(terminalId) {
  if (desktopState.currentStreamTid) {
    await stopAndCloseDesktop();
  }
  desktopState.currentStreamTid = terminalId;
  desktopState.currentStreamDisplayIndex = 0;
  desktopState.activeMonitorsList = [];
  desktopState.frameCount = 0;
  desktopState.lastFrameTime = Date.now();
  desktopState.pendingMouseMove = null;
  desktopState.isSendingInput = false;
  desktopState.currentCursorMode = "default";
  desktopState.currentScaleMode = "fit";
  desktopState.isLocalMouseDown = false;
  desktopState.inputQueue = Promise.resolve();

  const html = `
        <div id="rdMainWrapper" class="rd-main-wrapper">
          <div class="rd-hud" id="rdHud">
            <div class="rd-hud-section">
              <span class="tag" style="background: #080c14; padding: 4px 8px;"><div class="dot pulse"></div> <span id="rdStatus">建立安全推流...</span></span>
              <span id="rdResolution" style="color: var(--text-muted); font-family: monospace;">-</span>
              <span id="rdFps" style="color: var(--primary); font-weight: 700; font-family: monospace;">- FPS</span>
            </div>

            <div class="rd-hud-section" id="rdMonitorsSection" style="display: none;">
              <span style="color: var(--text-dim); font-size: 11px;">显示器:</span>
              <div id="rdMonitorButtons" style="display: flex; gap: 4px;"></div>
            </div>

            <div class="rd-hud-section">
              <span style="color: var(--text-dim); font-size: 11px;">鼠标手势:</span>
              <button class="cursor-tool-btn active" data-cursor="default" onclick="setCursorMode('default', this)" title="普通鼠标箭头（可随时按住 Alt 键触发拉宽）">↖ 默认</button>
              <button class="cursor-tool-btn" data-cursor="ew-resize" onclick="setCursorMode('ew-resize', this)" title="水平拉宽模式（拖动窗口左右边框调整宽度）">↔ 水平拉宽</button>
              <button class="cursor-tool-btn" data-cursor="ns-resize" onclick="setCursorMode('ns-resize', this)" title="垂直拉长模式（拖动窗口上下边框）">↕ 垂直拉长</button>
              <button class="cursor-tool-btn" data-cursor="nwse-resize" onclick="setCursorMode('nwse-resize', this)" title="对角缩放模式（拖动窗口四角）">⤡ 对角缩放</button>
              <button class="cursor-tool-btn" data-cursor="text" onclick="setCursorMode('text', this)" title="文本选择工字光标">ꕯ 文本</button>
            </div>

            <div class="rd-hud-section">
              <span style="color: var(--text-dim); font-size: 11px;">画面显示:</span>
              <button class="scale-tool-btn active" onclick="setScaleMode('fit', this)" title="保持比例自适应填满全屏">📐 自适应全屏</button>
              <button class="scale-tool-btn" onclick="setScaleMode('stretch', this)" title="强制铺满屏幕（无黑边）">↔️ 强制铺满</button>
              <button class="scale-tool-btn" onclick="setScaleMode('original', this)" title="1:1 原生像素点对点显示">🔍 1:1</button>
            </div>

            <div class="rd-hud-section">
              <span style="color: var(--text-dim); font-size: 11px;">快捷操作:</span>
              <button class="key-btn" style="color: var(--primary); font-weight: 600;" onclick="releaseAllInputs('${terminalId}')" title="紧急复位并释放所有鼠标与键盘按键">🔓 释放按键</button>
              <button class="key-btn" onclick="injectSpecialKey('${terminalId}', 'ctrl_alt_del')">Ctrl+Alt+Del</button>
              <button class="key-btn" onclick="injectSpecialKey('${terminalId}', 'win_r')">Win+R</button>
              <button class="key-btn" onclick="injectSpecialKey('${terminalId}', 'taskmgr')">任务管理器</button>
              <button class="key-btn" onclick="injectSpecialKey('${terminalId}', 'esc')">Esc</button>
            </div>

            <div class="rd-hud-section">
              <button id="rdFsBtn" class="btn btn-secondary btn-sm" onclick="toggleCanvasFullscreen()">⛶ 网页全屏</button>
              <button class="btn btn-danger btn-sm" onclick="stopAndCloseDesktop()">⏹ 断开推流</button>
            </div>
          </div>

          <div id="rdCanvasContainer" class="rd-canvas-container">
            <canvas id="rdCanvas" class="rd-canvas" tabindex="0"></canvas>
            <div id="rdLoadingOverlay" style="position: absolute; color: #94a3b8; font-size: 13px; display: flex; flex-direction: column; align-items: center; gap: 8px;">
              <div class="dot pulse" style="width: 12px; height: 12px;"></div>
              正在缓冲远端屏幕画面...
            </div>
          </div>

          <div class="rd-bottom-bar" style="margin-top: 8px; font-size: 12px; color: var(--text-muted); display: flex; justify-content: space-between; align-items: center;">
            <span>💡 <b>交互指引</b>: 点击画面捕获鼠标焦点。已升级为标准鼠标箭头，<b>按住 Alt 键移动可临时触发 [↔ 水平拉宽] 手势</b>；若按键粘连可点击顶部 [🔓 释放按键]。</span>
            <span style="color: var(--primary); font-family: monospace;">纯 Rust 原生抓帧 · 智能自适应全屏</span>
          </div>
        </div>
      `;
  showModal(`🖥️ 远程桌面直控 - [${terminalId}]`, html, true);

  try {
    await authFetch(restPaths.desktopStream(terminalId), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        fps: 15,
        quality: 55,
        display_index: desktopState.currentStreamDisplayIndex,
      }),
    });
  } catch (err) {
    console.error("Start stream failed", err);
  }

  authFetch(restPaths.invoke(terminalId), {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ tool: "list_monitors", arguments: {} }),
  })
    .then((res) => res.json())
    .then((data) => {
      if (data && data.success && Array.isArray(data.result) && data.result.length > 0) {
        desktopState.activeMonitorsList = data.result;
        const container = byId("rdMonitorButtons");
        const section = byId("rdMonitorsSection");
        if (container && section) {
          container.innerHTML = desktopState.activeMonitorsList
            .map(
              (monitor) => `
              <button class="scale-tool-btn ${monitor.display_index === desktopState.currentStreamDisplayIndex ? "active" : ""}" data-display-index="${monitor.display_index}" onclick="switchStreamDisplay('${terminalId}', ${monitor.display_index})" title="${monitor.name} (${monitor.width}x${monitor.height})">🖥️ 屏幕 ${monitor.display_index}${monitor.is_primary ? " (主屏)" : ""}</button>
            `,
            )
            .join("");
          if (desktopState.activeMonitorsList.length >= 1) {
            section.style.display = "flex";
          }
        }
      }
    })
    .catch((err) => console.warn("Failed to fetch monitors:", err));

  setupRemoteDesktopInteractions(terminalId);
}

export async function switchStreamDisplay(terminalId, displayIndex) {
  if (desktopState.currentStreamDisplayIndex === displayIndex) return;
  desktopState.currentStreamDisplayIndex = displayIndex;

  const container = byId("rdMonitorButtons");
  if (container) {
    container.querySelectorAll("button").forEach((btn) => {
      if (btn.getAttribute("data-display-index") === String(displayIndex)) {
        btn.classList.add("active");
      } else {
        btn.classList.remove("active");
      }
    });
  }

  const statusEl = byId("rdStatus");
  if (statusEl) {
    statusEl.innerText = `切换至屏幕 ${displayIndex}...`;
  }

  try {
    await authFetch(restPaths.desktopStream(terminalId), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ fps: 15, quality: 55, display_index: displayIndex }),
    });
  } catch (err) {
    console.error(`Failed to switch to display ${displayIndex}:`, err);
  }
}

export function setupRemoteDesktopInteractions(terminalId) {
  const canvas = byId("rdCanvas");
  if (!canvas) return;
  const ctx = canvas.getContext("2d");
  const overlay = byId("rdLoadingOverlay");
  const statusEl = byId("rdStatus");
  const resEl = byId("rdResolution");
  const fpsEl = byId("rdFps");

  let lastMoveTime = 0;
  canvas.addEventListener("mousemove", (event) => {
    checkModifierCursor(event);
    const now = Date.now();
    if (now - lastMoveTime < 35) return;
    lastMoveTime = now;
    sendCanvasPointerMove(terminalId, canvas, event);
  });

  canvas.addEventListener("mousedown", (event) => {
    canvas.focus();
    checkModifierCursor(event);
    desktopState.isLocalMouseDown = true;
    desktopState.localPressedButton = event.button;
    sendCanvasPointerMove(terminalId, canvas, event);
    sendDesktopInput(terminalId, {
      action: "MouseDown",
      data: { button: event.button },
    });
  });

  desktopState.globalMouseUpHandler = (event) => {
    if (!desktopState.isLocalMouseDown) return;
    desktopState.isLocalMouseDown = false;
    checkModifierCursor(event);
    sendCanvasPointerMove(terminalId, canvas, event);
    sendDesktopInput(terminalId, {
      action: "MouseUp",
      data: { button: desktopState.localPressedButton },
    });
  };
  window.addEventListener("mouseup", desktopState.globalMouseUpHandler);

  desktopState.windowBlurHandler = () => {
    if (desktopState.isLocalMouseDown) {
      releaseAllInputs(terminalId);
    }
  };
  window.addEventListener("blur", desktopState.windowBlurHandler);

  canvas.addEventListener("contextmenu", (event) => {
    event.preventDefault();
  });

  canvas.addEventListener("wheel", (event) => {
    event.preventDefault();
    const delta = event.deltaY > 0 ? -120 : 120;
    sendDesktopInput(terminalId, {
      action: "MouseWheel",
      data: { delta_y: delta },
    });
  });

  canvas.addEventListener("keydown", (event) => {
    event.preventDefault();
    checkModifierCursor(event);
    sendDesktopInput(terminalId, {
      action: "KeyDown",
      data: { key_code: event.keyCode, key: event.key },
    });
  });

  canvas.addEventListener("keyup", (event) => {
    event.preventDefault();
    checkModifierCursor(event);
    sendDesktopInput(terminalId, {
      action: "KeyUp",
      data: { key_code: event.keyCode, key: event.key },
    });
  });

  window.addEventListener("keydown", checkModifierCursor);
  window.addEventListener("keyup", checkModifierCursor);

  let lastTimestamp = 0;
  async function pollFrameLoop(tid) {
    while (desktopState.currentStreamTid === tid) {
      try {
        const res = await authFetch(restPaths.desktopFrameRaw(tid));
        if (res.ok) {
          const tsHeader = res.headers.get("X-Frame-Timestamp");
          const timestamp = tsHeader ? parseInt(tsHeader, 10) : 0;
          if (!timestamp || timestamp !== lastTimestamp) {
            if (timestamp) lastTimestamp = timestamp;
            const blob = await res.blob();
            const headerW = parseInt(res.headers.get("X-Frame-Width") || "0", 10);
            const headerH = parseInt(res.headers.get("X-Frame-Height") || "0", 10);

            if (typeof createImageBitmap === "function") {
              const bitmap = await createImageBitmap(blob);
              if (desktopState.currentStreamTid === tid) {
                if (overlay) overlay.style.display = "none";
                if (statusEl) statusEl.innerText = "实时推流中";
                desktopState.rdActualW = headerW || bitmap.width;
                desktopState.rdActualH = headerH || bitmap.height;
                if (
                  canvas.width !== desktopState.rdActualW ||
                  canvas.height !== desktopState.rdActualH
                ) {
                  canvas.width = desktopState.rdActualW;
                  canvas.height = desktopState.rdActualH;
                  if (resEl) {
                    resEl.innerText = `${desktopState.rdActualW} × ${desktopState.rdActualH}`;
                  }
                }
                ctx.drawImage(bitmap, 0, 0);
                desktopState.frameCount++;
              }
              bitmap.close?.();
            } else {
              await new Promise((resolve) => {
                const url = URL.createObjectURL(blob);
                const img = new Image();
                img.onload = () => {
                  URL.revokeObjectURL(url);
                  if (desktopState.currentStreamTid !== tid) return resolve();
                  if (overlay) overlay.style.display = "none";
                  if (statusEl) statusEl.innerText = "实时推流中";
                  desktopState.rdActualW = headerW || img.width;
                  desktopState.rdActualH = headerH || img.height;
                  if (
                    canvas.width !== desktopState.rdActualW ||
                    canvas.height !== desktopState.rdActualH
                  ) {
                    canvas.width = desktopState.rdActualW;
                    canvas.height = desktopState.rdActualH;
                    if (resEl) {
                      resEl.innerText = `${desktopState.rdActualW} × ${desktopState.rdActualH}`;
                    }
                  }
                  ctx.drawImage(img, 0, 0);
                  desktopState.frameCount++;
                  resolve();
                };
                img.onerror = () => {
                  URL.revokeObjectURL(url);
                  resolve();
                };
                img.src = url;
              });
            }
          }
        }
      } catch {
        /* transient poll errors are ignored */
      }
      await new Promise((resolve) => setTimeout(resolve, 35));
    }
  }

  pollFrameLoop(terminalId);

  desktopState.fpsTimer = setInterval(() => {
    const now = Date.now();
    const elapsed = (now - desktopState.lastFrameTime) / 1000;
    if (elapsed > 0) {
      const currentFps = Math.round(desktopState.frameCount / elapsed);
      if (fpsEl) fpsEl.innerText = `${currentFps} FPS`;
    }
    desktopState.frameCount = 0;
    desktopState.lastFrameTime = now;
  }, 1000);
}

export function toggleCanvasFullscreen() {
  const wrapper = byId("rdMainWrapper");
  if (!wrapper) return;
  if (!document.fullscreenElement) {
    wrapper.requestFullscreen().catch((err) => alert(`无法全屏: ${err.message}`));
  } else {
    document.exitFullscreen();
  }
}

export function bindDesktopDocumentListeners() {
  document.addEventListener("fullscreenchange", () => {
    const isFs = !!document.fullscreenElement;
    const fsBtn = byId("rdFsBtn");
    if (fsBtn) {
      fsBtn.innerText = isFs ? "⛶ 退出全屏" : "⛶ 网页全屏";
    }
  });
}

export async function stopAndCloseDesktop() {
  if (desktopState.fpsTimer) {
    clearInterval(desktopState.fpsTimer);
    desktopState.fpsTimer = null;
  }
  window.removeEventListener("keydown", checkModifierCursor);
  window.removeEventListener("keyup", checkModifierCursor);
  if (desktopState.globalMouseUpHandler) {
    window.removeEventListener("mouseup", desktopState.globalMouseUpHandler);
    desktopState.globalMouseUpHandler = null;
  }
  if (desktopState.windowBlurHandler) {
    window.removeEventListener("blur", desktopState.windowBlurHandler);
    desktopState.windowBlurHandler = null;
  }

  if (document.fullscreenElement) {
    try {
      await document.exitFullscreen();
    } catch {
      /* ignore fullscreen exit errors */
    }
  }

  const tid = desktopState.currentStreamTid;
  if (tid && desktopState.isLocalMouseDown) {
    releaseAllInputs(tid);
  }
  desktopState.currentStreamTid = null;
  desktopState.currentStreamDisplayIndex = 0;
  desktopState.activeMonitorsList = [];
  desktopState.pendingMouseMove = null;
  desktopState.isFlushingMove = false;
  desktopState.isLocalMouseDown = false;

  if (tid) {
    try {
      await authFetch(restPaths.desktopStop(tid), { method: "POST" });
    } catch {
      /* ignore stop errors */
    }
  }
  const modal = byId("resultModal");
  if (modal) modal.style.display = "none";
}
