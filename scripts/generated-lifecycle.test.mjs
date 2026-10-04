import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hegira-lifecycle-dispatch-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const scripts = path.join(root, "scripts");
  fs.mkdirSync(scripts);
  fs.copyFileSync(new URL("./generated-application-check.sh", import.meta.url),
    path.join(scripts, "generated-application-check.sh"));
  fs.writeFileSync(path.join(scripts, "validation-cache.sh"), "");
  fs.writeFileSync(path.join(scripts, "upgraded-application-check.sh"),
    '#!/bin/sh\nprintf "%s\\n" "$#" "$1"\nexit "${TEST_EXIT:-0}"\n');
  return path.join(scripts, "generated-application-check.sh");
}

test("upgrade cells dispatch exactly once and preserve failure/interruption outcomes", t => {
  const script = fixture(t);
  for (const composition of ["default", "minimal", "identity-added"]) {
    for (const status of [0, 1, 2, 130, 143]) {
      const result = spawnSync("sh", [script, `upgrade-${composition}`], {
        encoding: "utf8", env: { ...process.env, TEST_EXIT: String(status) },
      });
      assert.equal(result.status, status);
      assert.equal(result.stdout, `1\n${composition}\n`);
    }
  }
});

test("invalid lifecycle selections or extra arguments never dispatch", t => {
  const script = fixture(t);
  for (const args of [
    ["upgrade-other"], ["upgrade-default", "minimal"],
    ["default", "upgrade-default"],
  ]) {
    const result = spawnSync("sh", [script, ...args], { encoding: "utf8" });
    assert.equal(result.status, 2);
    assert.equal(result.stdout, "");
  }
});
