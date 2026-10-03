import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";

if (process.argv.length !== 3) {
  throw new Error("usage: node scripts/frontend-watcher-smoke.mjs <installed-frontend>");
}
const frontend = path.resolve(process.argv[2]);
const require = createRequire(path.join(frontend, "node_modules/@tailwindcss/cli/package.json"));
const watcher = require("@parcel/watcher");
assert.equal(require("@parcel/watcher/package.json").version, "2.6.0");
const root = await fs.mkdtemp(path.join(os.tmpdir(), "hegira-watcher-smoke-"));
const visible = path.join(root, "visible.css");
const ignored = path.join(root, "ignored", "hidden.css");
const seen = new Set();
let subscription;
let timeout;
try {
  await fs.mkdir(path.dirname(ignored));
  let onVisible;
  let onError;
  const observed = new Promise((resolve, reject) => {
    onVisible = resolve;
    onError = reject;
    timeout = setTimeout(() => reject(new Error("Watcher did not observe CSS creation")), 5000);
  });
  subscription = await watcher.subscribe(root, (error, events) => {
    if (error) return onError(error);
    for (const event of events) seen.add(event.path);
    if (seen.has(visible)) onVisible();
  }, { ignore: ["**/ignored/**"] });
  await fs.writeFile(ignored, ".hidden { color: red; }\n");
  await fs.writeFile(visible, ".visible { color: blue; }\n");
  await observed;
  clearTimeout(timeout);
  await new Promise(resolve => setTimeout(resolve, 200));
  assert(!seen.has(ignored), "Watcher must preserve glob ignore behavior");
  console.log("frontend watcher smoke: ok (CSS creation and ignored glob)");
} finally {
  clearTimeout(timeout);
  try { await subscription?.unsubscribe(); }
  finally { await fs.rm(root, { recursive: true, force: true }); }
}
