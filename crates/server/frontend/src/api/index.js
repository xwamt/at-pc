const TOKEN_KEY = "at_pc_auth_token";

function defaultSearchParams() {
  return new URLSearchParams(globalThis.location?.search ?? "");
}

function defaultStorage() {
  return globalThis.sessionStorage;
}

/**
 * @param {{ searchParams?: URLSearchParams, storage?: { getItem: (key: string) => string | null, setItem: (key: string, value: string) => void, removeItem: (key: string) => void } }} [opts]
 */
export function readAuthToken(opts = {}) {
  const params = opts.searchParams ?? defaultSearchParams();
  const store = opts.storage ?? defaultStorage();
  const urlToken = params.get("token") || params.get("pin");
  if (urlToken) {
    store.setItem(TOKEN_KEY, urlToken);
    return urlToken;
  }
  return store.getItem(TOKEN_KEY) || "";
}

export function writeAuthToken(token, storage = defaultStorage()) {
  if (token) {
    storage.setItem(TOKEN_KEY, token);
  } else {
    storage.removeItem(TOKEN_KEY);
  }
}

export function getAuthToken() {
  return readAuthToken();
}

export function setAuthToken(token) {
  writeAuthToken(token);
}

export function applyAuthHeader(headers, token) {
  if (!headers || !token) {
    return headers;
  }
  if (headers instanceof Headers) {
    if (!headers.has("Authorization")) {
      headers.set("Authorization", `Bearer ${token}`);
    }
  } else if (!headers.Authorization) {
    headers.Authorization = `Bearer ${token}`;
  }
  return headers;
}

export const restPaths = {
  terminals: () => "/api/terminals",
  calls: () => "/api/calls",
  audit: () => "/api/audit",
  auditExport: () => "/api/audit/export",
  terminal: (id) => `/api/terminals/${encodeURIComponent(id)}`,
  terminalMeta: (id) => `/api/terminals/${encodeURIComponent(id)}/meta`,
  invoke: (id) => `/api/terminals/${encodeURIComponent(id)}/invoke`,
  terminalCalls: (id) => `/api/terminals/${encodeURIComponent(id)}/calls`,
  cancelCall: (callId) => `/api/calls/${encodeURIComponent(callId)}/cancel`,
  desktopStream: (id) => `/api/terminals/${encodeURIComponent(id)}/desktop/stream`,
  desktopStop: (id) => `/api/terminals/${encodeURIComponent(id)}/desktop/stop`,
  desktopFrame: (id) => `/api/terminals/${encodeURIComponent(id)}/desktop/frame`,
  desktopFrameRaw: (id) => `/api/terminals/${encodeURIComponent(id)}/desktop/frame.jpg`,
  desktopInput: (id) => `/api/terminals/${encodeURIComponent(id)}/desktop/input`,
};

/**
 * @param {string} url
 * @param {RequestInit} [options]
 * @param {{ fetchImpl?: typeof fetch, getToken?: () => string, onUnauthorized?: () => void }} [deps]
 */
export async function authFetch(url, options = {}, deps = {}) {
  const fetchImpl = deps.fetchImpl ?? globalThis.fetch.bind(globalThis);
  const getToken = deps.getToken ?? getAuthToken;
  const request = { ...options, headers: options.headers || {} };
  applyAuthHeader(request.headers, getToken());
  const res = await fetchImpl(url, request);
  if (res.status === 401) {
    deps.onUnauthorized?.();
  }
  return res;
}
