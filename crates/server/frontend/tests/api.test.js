import assert from "node:assert/strict";
import { afterEach, mock, test } from "node:test";

import {
  applyAuthHeader,
  authFetch,
  readAuthToken,
  restPaths,
  writeAuthToken,
} from "../src/api/index.js";

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
});

test("readAuthToken prefers URL token or pin and persists it", () => {
  const storage = memoryStorage();
  const fromToken = readAuthToken({
    searchParams: new URLSearchParams("token=url-token"),
    storage,
  });
  assert.equal(fromToken, "url-token");
  assert.equal(storage.getItem("at_pc_auth_token"), "url-token");

  const fromPin = readAuthToken({
    searchParams: new URLSearchParams("pin=url-pin"),
    storage: memoryStorage(),
  });
  assert.equal(fromPin, "url-pin");
});

test("readAuthToken falls back to session storage when URL has no token", () => {
  const storage = memoryStorage({ at_pc_auth_token: "stored-token" });
  const token = readAuthToken({
    searchParams: new URLSearchParams(""),
    storage,
  });
  assert.equal(token, "stored-token");
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
});
