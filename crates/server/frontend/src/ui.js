import { byId } from "./dom.js";

/** @type {null | (() => void)} */
let beforeClose = null;

/**
 * @param {() => void} handler
 */
export function setModalBeforeClose(handler) {
  beforeClose = handler;
}

export function showAuthModal() {
  const modal = byId("authModal");
  if (modal) modal.style.display = "flex";
  const input = byId("authTokenInput");
  if (input) {
    input.focus();
  }
}

export function hideAuthModal() {
  const modal = byId("authModal");
  if (modal) modal.style.display = "none";
}

/**
 * @param {string} title
 * @param {string} htmlContent
 * @param {boolean} [isLg]
 */
export function showModal(title, htmlContent, isLg = false) {
  byId("modalTitle").innerText = title;
  byId("modalContent").innerHTML = htmlContent;
  const modalBox = document.querySelector("#resultModal .modal");
  if (isLg) {
    modalBox?.classList.add("modal-lg");
  } else {
    modalBox?.classList.remove("modal-lg");
  }
  byId("resultModal").style.display = "flex";
}

export function closeModal() {
  beforeClose?.();
  byId("resultModal").style.display = "none";
}
