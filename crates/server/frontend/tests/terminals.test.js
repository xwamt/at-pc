import assert from "node:assert/strict";
import { test } from "node:test";

import {
  escapeHtml,
  filterTerminals,
  parseTagList,
} from "../src/terminals/index.js";

const sample = [
  {
    status: "Online",
    custom_name: "财务主机",
    notes: "3楼机房",
    tags: ["财务", "Win11"],
    info: {
      hostname: "acc-pc",
      lan_ip: "10.0.0.8",
      terminal_id: "term-1",
      os_version: "Windows 11",
    },
    latest_metrics: {
      cpu_usage_percent: 20,
      memory_used_mb: 1024,
      memory_total_mb: 8192,
    },
  },
  {
    status: "Online",
    custom_name: "",
    notes: "",
    tags: [],
    info: {
      hostname: "hot-box",
      lan_ip: "10.0.0.9",
      terminal_id: "term-hot",
      os_version: "Windows 10",
    },
    latest_metrics: {
      cpu_usage_percent: 90,
      memory_used_mb: 7000,
      memory_total_mb: 8192,
    },
  },
  {
    status: "Offline",
    custom_name: "",
    notes: "archived",
    tags: ["spare"],
    info: {
      hostname: "old-pc",
      lan_ip: "10.0.0.10",
      terminal_id: "term-off",
      os_version: "Windows 7",
    },
    latest_metrics: null,
  },
];

test("filterTerminals keeps all terminals for the all tab without search", () => {
  const filtered = filterTerminals(sample, { tab: "all", search: "" });
  assert.equal(filtered.length, 3);
});

test("filterTerminals online tab hides offline terminals", () => {
  const filtered = filterTerminals(sample, { tab: "online", search: "" });
  assert.deepEqual(
    filtered.map((t) => t.info.terminal_id),
    ["term-1", "term-hot"],
  );
});

test("filterTerminals warn tab keeps only online high-load terminals", () => {
  const filtered = filterTerminals(sample, { tab: "warn", search: "" });
  assert.deepEqual(
    filtered.map((t) => t.info.terminal_id),
    ["term-hot"],
  );
});

test("filterTerminals search matches name, ip, id, os, notes, and tags", () => {
  assert.equal(
    filterTerminals(sample, { tab: "all", search: "10.0.0.8" })[0].info
      .terminal_id,
    "term-1",
  );
  assert.equal(
    filterTerminals(sample, { tab: "all", search: "term-off" })[0].info
      .terminal_id,
    "term-off",
  );
  assert.equal(
    filterTerminals(sample, { tab: "all", search: "财务" })[0].info.terminal_id,
    "term-1",
  );
  assert.equal(
    filterTerminals(sample, { tab: "all", search: "archived" })[0].info
      .terminal_id,
    "term-off",
  );
  assert.equal(
    filterTerminals(sample, { tab: "all", search: "spare" })[0].info.terminal_id,
    "term-off",
  );
  assert.equal(
    filterTerminals(sample, { tab: "all", search: "no-such-host" }).length,
    0,
  );
});

test("parseTagList splits English and Chinese commas and drops blanks", () => {
  assert.deepEqual(parseTagList("财务, Win11， 关键资产 , ,"), [
    "财务",
    "Win11",
    "关键资产",
  ]);
  assert.deepEqual(parseTagList("   "), []);
});

test("escapeHtml encodes markup characters used in terminal cards", () => {
  assert.equal(escapeHtml(""), "");
  assert.equal(
    escapeHtml(`<img src="x" onerror='alert(1)'>`),
    "&lt;img src=&quot;x&quot; onerror=&#039;alert(1)&#039;&gt;",
  );
});

test("terminalCardHtml escapes os_version, lan_ip, and username so tags cannot inject", async () => {
  const { terminalCardHtml } = await import("../src/terminals/index.js");
  const html = terminalCardHtml({
    status: "Online",
    custom_name: "",
    notes: "",
    tags: [],
    last_heartbeat_elapsed_secs: 3,
    info: {
      hostname: "safe-host",
      terminal_id: "term-safe",
      os_version: `<img src=x onerror=alert('os')>`,
      lan_ip: `<script>alert('ip')</script>`,
      username: `</b><img src=x onerror=alert('user')>`,
    },
    latest_metrics: {
      cpu_usage_percent: 1,
      memory_used_mb: 100,
      memory_total_mb: 1024,
    },
  });
  assert.equal(html.includes("<img"), false, "raw img tags must not appear");
  assert.equal(html.includes("<script"), false, "raw script tags must not appear");
  assert.equal(html.includes("</b><img"), false, "username must not break out of the <b> wrapper");
  assert.ok(html.includes("&lt;img"), "os_version/username markup must be escaped");
  assert.ok(html.includes("&lt;script"), "lan_ip markup must be escaped");
  assert.ok(html.includes("&lt;/b&gt;"), "username closing-tag payload must be escaped");
});

test("processRowHtml escapes exe_path/name and JS-escapes onclick args", async () => {
  const { processRowHtml } = await import("../src/terminals/index.js");
  const html = processRowHtml("term-safe", {
    pid: 42,
    name: `');alert('name');//`,
    exe_path: `<img src=x onerror=alert('exe')>`,
    memory_mb: 12,
    cpu_usage: 1.5,
  });
  assert.equal(html.includes("<img"), false, "raw img tags must not appear in exe_path");
  assert.ok(html.includes("&lt;img"), "exe_path markup must be escaped");
  assert.equal(
    html.includes("alert('name')"),
    false,
    "process name must not break out of the onclick JS string",
  );
  assert.match(html, /onclick="killTargetProc\(/);
});
