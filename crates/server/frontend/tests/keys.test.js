import assert from "node:assert/strict";
import { test } from "node:test";

import {
  formatDateTime,
  formatScopes,
  getKeyStatus,
  keyRowHtml,
  keysTableHtml,
} from "../src/keys/index.js";

test("getKeyStatus marks unrevoked keys without expiry as Active", () => {
  const status = getKeyStatus({
    id: "k1",
    revoked_at: null,
    expires_at: null,
  });
  assert.equal(status.status, "Active");
  assert.equal(status.label, "有效 (Active)");
  assert.equal(status.badgeClass, "status-badge online");
});

test("getKeyStatus marks future expiry keys as Active", () => {
  const future = new Date(Date.now() + 86400000).toISOString();
  const status = getKeyStatus({
    id: "k2",
    revoked_at: null,
    expires_at: future,
  });
  assert.equal(status.status, "Active");
});

test("getKeyStatus marks past expiry keys as Expired", () => {
  const past = new Date(Date.now() - 86400000).toISOString();
  const status = getKeyStatus({
    id: "k3",
    revoked_at: null,
    expires_at: past,
  });
  assert.equal(status.status, "Expired");
  assert.equal(status.label, "已过期 (Expired)");
  assert.equal(status.badgeClass, "status-badge high-load");
});

test("getKeyStatus marks revoked keys as Revoked even if expires_at is future", () => {
  const future = new Date(Date.now() + 86400000).toISOString();
  const status = getKeyStatus({
    id: "k4",
    revoked_at: "2026-09-20T10:00:00Z",
    expires_at: future,
  });
  assert.equal(status.status, "Revoked");
  assert.equal(status.label, "已吊销 (Revoked)");
  assert.equal(status.badgeClass, "status-badge offline");

  const statusBool = getKeyStatus({
    id: "k4-bool",
    revoked: true,
  });
  assert.equal(statusBool.status, "Revoked");
});

test("formatDateTime formats ISO strings into readable local dates", () => {
  assert.equal(formatDateTime(null), "-");
  assert.equal(formatDateTime(""), "-");
  const formatted = formatDateTime("2026-09-20T14:30:00Z");
  assert.match(formatted, /^2026-09-20/);
});

test("formatScopes formats array, string or wildcard", () => {
  assert.match(formatScopes(["console", "mcp"]), /tag-scope.*console.*tag-scope.*mcp/);
  assert.match(formatScopes("terminals:read"), /tag-scope.*terminals:read/);
  assert.match(formatScopes([]), /\* \(全部\)/);
  assert.match(formatScopes(null), /\* \(全部\)/);
});

test("keyRowHtml escapes input and generates badges and action buttons", () => {
  const row = keyRowHtml({
    id: "k1",
    name: "Admin <script>alert(1)</script>",
    prefix: "atpc_sk_k1",
    kind: "sk",
    role: "admin",
    scopes: ["console"],
    created_at: "2026-09-20T12:00:00Z",
    expires_at: null,
    revoked_at: null,
  });

  assert.ok(!row.includes("<script>"));
  assert.ok(row.includes("&lt;script&gt;alert(1)&lt;/script&gt;"));
  assert.ok(row.includes("tag-kind-sk"));
  assert.ok(row.includes("tag-role-admin"));
  assert.ok(row.includes("openRotateKeyModal"));
  assert.ok(row.includes("openRevokeKeyModal"));
  assert.ok(row.includes("有效 (Active)"));
});

test("keyRowHtml for revoked keys hides rotate button and shows disabled text", () => {
  const row = keyRowHtml({
    id: "k2",
    name: "Revoked Key",
    prefix: "atpc_sk_k2",
    kind: "dv",
    role: "operator",
    scopes: [],
    created_at: "2026-09-20T12:00:00Z",
    expires_at: null,
    revoked_at: "2026-09-20T13:00:00Z",
  });

  assert.ok(row.includes("tag-kind-dv"));
  assert.ok(row.includes("tag-role-operator"));
  assert.ok(!row.includes("openRotateKeyModal"));
  assert.ok(!row.includes("openRevokeKeyModal"));
  assert.ok(row.includes("已失效"));
  assert.ok(row.includes("已吊销 (Revoked)"));
});

test("keysTableHtml returns empty state for empty array", () => {
  const emptyHtml = keysTableHtml([]);
  assert.ok(emptyHtml.includes("empty-state"));
  assert.ok(emptyHtml.includes("暂无 API 密钥记录"));
});

test("keysTableHtml returns table with header and rows", () => {
  const tableHtml = keysTableHtml([
    {
      id: "k1",
      name: "Root",
      prefix: "atpc_sk_root",
      kind: "sk",
      role: "admin",
      scopes: [],
      created_at: "2026-09-20T12:00:00Z",
    },
  ]);
  assert.ok(tableHtml.includes("<table class=\"data-table keys-table\">"));
  assert.ok(tableHtml.includes("atpc_sk_root"));
});
