import { authFetch as request } from "./api/index.js";

/** @type {() => void} */
let onUnauthorized = () => {};

export function setUnauthorizedHandler(handler) {
  onUnauthorized = handler;
}

export function authFetch(url, options = {}) {
  return request(url, options, { onUnauthorized });
}
