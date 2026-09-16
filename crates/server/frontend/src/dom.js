/**
 * @param {string} id
 * @returns {any}
 */
export function byId(id) {
  return document.getElementById(id);
}

/**
 * @param {string} selector
 * @returns {HTMLElement[]}
 */
export function qsAll(selector) {
  return /** @type {HTMLElement[]} */ (Array.from(document.querySelectorAll(selector)));
}
