import assert from "node:assert/strict";
import { afterEach, mock, test } from "node:test";

import {
  apiLogin,
  apiLogout,
  applyAuthHeader,
  authFetch,
  readAuthToken,
  restPaths,
  writeAuthToken,
} from "../src/api/index.js";
import { submitAuthToken } from "../src/app.js";

function memoryStorage(initial = {}) {
  const data = { ...initial };
  return {
    getItem(key) {
      return Object.prototype.hasOwnProperty.call(data, key) ? data[key] : null;
    },
    setItem(key, value) {
      data[key] = String(value);
    },
    removeItem(key) {
      delete data[key];
    },
  };
}

afterEach(() => {
  mock.restoreAll();
  delete globalThis.window;
  delete globalThis.document;
});

test("readAuthToken prefers URL token or pin and persists it", () => {
  const calls = [];
  globalThis.window = {
    history: {
      replaceState(state, title, url) {
        calls.push({ state, title, url });
      },
    },
    location: { pathname: "/dashboard" },
  };
  globalThis.document = { title: "AT-PC Management" };

  const storage = memoryStorage();
  const fromToken = readAuthToken({
    searchParams: new URLSearchParams("token=url-token"),
    storage,
  });
  assert.equal(fromToken, "url-token");
  assert.equal(storage.getItem("at_pc_auth_token"), "url-token");
  assert.equal(calls.length, 1);
  assert.deepEqual(calls[0], {
    state: {},
    title: "AT-PC Management",
    url: "/dashboard",
  });

  const fromPin = readAuthToken({
    searchParams: new URLSearchParams("pin=url-pin"),
    storage: memoryStorage(),
  });
  assert.equal(fromPin, "url-pin");
  assert.equal(calls.length, 2);
  assert.deepEqual(calls[1], {
    state: {},
    title: "AT-PC Management",
    url: "/dashboard",
  });
});

test("readAuthToken falls back to session storage when URL has no token", () => {
  const calls = [];
  globalThis.window = {
    history: {
      replaceState(state, title, url) {
        calls.push({ state, title, url });
      },
    },
    location: { pathname: "/dashboard" },
  };
  globalThis.document = { title: "AT-PC Management" };

  const storage = memoryStorage({ at_pc_auth_token: "stored-token" });
  const token = readAuthToken({
    searchParams: new URLSearchParams(""),
    storage,
  });
  assert.equal(token, "stored-token");
  assert.equal(calls.length, 0);
});

test("writeAuthToken stores or clears the session token", () => {
  const storage = memoryStorage({ at_pc_auth_token: "old" });
  writeAuthToken("next", storage);
  assert.equal(storage.getItem("at_pc_auth_token"), "next");
  writeAuthToken("", storage);
  assert.equal(storage.getItem("at_pc_auth_token"), null);
});

test("applyAuthHeader sets Bearer on Headers and plain objects without clobbering", () => {
  const headers = new Headers();
  applyAuthHeader(headers, "abc");
  assert.equal(headers.get("Authorization"), "Bearer abc");
  applyAuthHeader(headers, "other");
  assert.equal(headers.get("Authorization"), "Bearer abc");

  const plain = {};
  applyAuthHeader(plain, "abc");
  assert.equal(plain.Authorization, "Bearer abc");
  applyAuthHeader(plain, "other");
  assert.equal(plain.Authorization, "Bearer abc");
});

test("restPaths encode terminal and call identifiers", () => {
  assert.equal(restPaths.terminals(), "/api/terminals");
  assert.equal(restPaths.calls(), "/api/calls");
  assert.equal(restPaths.audit(), "/api/audit");
  assert.equal(restPaths.auditExport(), "/api/audit/export");
  assert.equal(restPaths.terminal("a/b"), "/api/terminals/a%2Fb");
  assert.equal(restPaths.terminalMeta("id 1"), "/api/terminals/id%201/meta");
  assert.equal(restPaths.invoke("t1"), "/api/terminals/t1/invoke");
  assert.equal(restPaths.terminalCalls("t1"), "/api/terminals/t1/calls");
  assert.equal(restPaths.cancelCall("c1"), "/api/calls/c1/cancel");
  assert.equal(
    restPaths.desktopStream("t1"),
    "/api/terminals/t1/desktop/stream",
  );
  assert.equal(restPaths.desktopStop("t1"), "/api/terminals/t1/desktop/stop");
  assert.equal(restPaths.desktopFrame("t1"), "/api/terminals/t1/desktop/frame");
  assert.equal(
    restPaths.desktopFrameRaw("t1"),
    "/api/terminals/t1/desktop/frame.jpg",
  );
  assert.equal(restPaths.desktopInput("t1"), "/api/terminals/t1/desktop/input");
  assert.equal(restPaths.authMe(), "/api/auth/me");
  assert.equal(restPaths.login(), "/api/auth/login");
  assert.equal(restPaths.logout(), "/api/auth/logout");
  assert.equal(restPaths.keys(), "/api/keys");
  assert.equal(restPaths.key("k1"), "/api/keys/k1");
  assert.equal(restPaths.key("k/1"), "/api/keys/k%2F1");
  assert.equal(restPaths.rotateKey("k1"), "/api/keys/k1/rotate");
  assert.equal(restPaths.rotateKey("k/1"), "/api/keys/k%2F1/rotate");
});

test("authFetch injects Authorization and reports 401 without throwing", async () => {
  const calls = [];
  const fetchImpl = async (url, options) => {
    calls.push({ url, options });
    return { status: 401, ok: false };
  };
  let unauthorized = 0;
  const response = await authFetch(
    "/api/terminals",
    {},
    {
      fetchImpl,
      getToken: () => "secret",
      onUnauthorized: () => {
        unauthorized += 1;
      },
    },
  );
  assert.equal(response.status, 401);
  assert.equal(unauthorized, 1);
  assert.equal(calls[0].options.headers.Authorization, "Bearer secret");
  assert.equal(calls[0].options.credentials, "same-origin");
});

test("authFetch sets same-origin credentials by default and respects custom credentials", async () => {
  const calls = [];
  const fetchImpl = async (url, options) => {
    calls.push({ url, options });
    return { status: 200, ok: true };
  };

  await authFetch("/api/terminals", {}, { fetchImpl, getToken: () => "" });
  assert.equal(calls[0].options.credentials, "same-origin");

  await authFetch(
    "/api/terminals",
    { credentials: "include" },
    { fetchImpl, getToken: () => "" },
  );
  assert.equal(calls[1].options.credentials, "include");

  await authFetch(
    "/api/terminals",
    { credentials: "omit" },
    { fetchImpl, getToken: () => "" },
  );
  assert.equal(calls[2].options.credentials, "omit");
});

test("apiLogin sends POST to /api/auth/login with same-origin credentials and JSON body", async () => {
  const calls = [];
  const fakeResponse = { ok: true, status: 200 };
  const fetchImpl = async (url, options) => {
    calls.push({ url, options });
    return fakeResponse;
  };

  const response = await apiLogin("atpc_sk_test_key", fetchImpl);
  assert.equal(response, fakeResponse);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].url, "/api/auth/login");
  assert.equal(calls[0].options.method, "POST");
  assert.equal(calls[0].options.credentials, "same-origin");
  assert.deepEqual(calls[0].options.headers, {
    "Content-Type": "application/json",
  });
  assert.equal(
    calls[0].options.body,
    JSON.stringify({ key: "atpc_sk_test_key" }),
  );
});

test("apiLogout sends POST to /api/auth/logout with same-origin credentials", async () => {
  const calls = [];
  const fakeResponse = { ok: true, status: 200 };
  const fetchImpl = async (url, options) => {
    calls.push({ url, options });
    return fakeResponse;
  };

  const response = await apiLogout(fetchImpl);
  assert.equal(response, fakeResponse);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].url, "/api/auth/logout");
  assert.equal(calls[0].options.method, "POST");
  assert.equal(calls[0].options.credentials, "same-origin");
});

test("submitAuthToken displays appropriate error message on 401 and 429", async () => {
  const elements = {
    authTokenInput: { value: "invalid-key" },
    authErrorMsg: { style: { display: "none" }, textContent: "" },
  };
  globalThis.document = {
    getElementById: (id) => elements[id] || null,
  };

  // Test 401
  globalThis.fetch = async () => ({
    ok: false,
    status: 401,
    json: async () => ({ error: "Unauthorized" }),
  });
  await submitAuthToken();
  assert.equal(elements.authErrorMsg.style.display, "block");
  assert.match(elements.authErrorMsg.textContent, /认证失败/);

  // Test 429
  globalThis.fetch = async () => ({
    ok: false,
    status: 429,
    json: async () => ({ error: "Rate limit exceeded" }),
  });
  await submitAuthToken();
  assert.equal(elements.authErrorMsg.style.display, "block");
  assert.match(elements.authErrorMsg.textContent, /429|请求过于频繁/);
});
