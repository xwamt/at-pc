import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const frontendRoot = join(dirname(fileURLToPath(import.meta.url)), "..");

const DASHBOARD_MODULES = [
  "src/app.js",
  "src/ui.js",
  "src/desktop/session.js",
  "src/terminals/ui.js",
];

test("dashboard modules do not disable TypeScript checking", () => {
  for (const relative of DASHBOARD_MODULES) {
    const source = readFileSync(join(frontendRoot, relative), "utf8");
    assert.equal(
      source.includes("@ts-nocheck"),
      false,
      `${relative} must not contain @ts-nocheck`,
    );
  }
});
