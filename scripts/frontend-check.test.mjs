import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

function fixture(t, status = 0) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hegira-frontend-contract-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.mkdirSync(path.join(root, "scripts"));
  fs.copyFileSync(new URL("./frontend-check.sh", import.meta.url), path.join(root, "scripts/frontend-check.sh"));
  for (const template of ["layered", "layered-minimal"]) {
    const frontend = path.join(root, "templates/applications", template, "apps/web/src");
    fs.mkdirSync(frontend, { recursive: true });
    fs.writeFileSync(path.join(frontend, "package.json"), JSON.stringify({
      overrides: { "@tailwindcss/cli": { "@parcel/watcher": "2.6.0" } },
    }));
    fs.writeFileSync(path.join(frontend, "package-lock.json"), JSON.stringify({
      packages: { "node_modules/@parcel/watcher": { version: "2.6.0" } },
    }));
  }
  fs.mkdirSync(path.join(root, "bin"));
  fs.symlinkSync(process.execPath, path.join(root, "bin/node"));
  fs.writeFileSync(path.join(root, "bin/npm"), `#!/bin/sh\nprintf '%s\\n' "$*" >>"$AUDIT_CALLS"\nexit ${status}\n`, { mode: 0o700 });
  const calls = path.join(root, "calls");
  return {
    root,
    run: (...args) => spawnSync("/bin/sh", [path.join(root, "scripts/frontend-check.sh"), ...args], {
      encoding: "utf8", env: { PATH: path.join(root, "bin") + ":/usr/bin:/bin", AUDIT_CALLS: calls },
    }),
    calls: () => fs.existsSync(calls) ? fs.readFileSync(calls, "utf8").trim().split("\n") : [],
  };
}

test("audits both canonical locks without installing or omitting dependency classes", t => {
  const check = fixture(t);
  assert.equal(check.run().status, 0);
  assert.equal(check.calls().length, 2);
  for (const call of check.calls()) {
    assert.match(call, /^audit --audit-level=high --include=dev --include=optional --include=peer --prefix /);
  }
  assert.match(check.calls()[0], /layered\/apps\/web\/src$/);
  assert.match(check.calls()[1], /layered-minimal\/apps\/web\/src$/);
});

test("audit failure propagates and prevents candidate sign-off", t => {
  const check = fixture(t, 23);
  assert.equal(check.run().status, 23);
  assert.equal(check.calls().length, 1);
});

test("missing scoped override and vulnerable nested dependencies fail closed", t => {
  for (const broken of ["override", "graph"]) {
    const check = fixture(t);
    const frontend = path.join(check.root, "templates/applications/layered/apps/web/src");
    if (broken === "override") fs.writeFileSync(path.join(frontend, "package.json"), "{}");
    else {
      const file = path.join(frontend, "package-lock.json");
      const lock = JSON.parse(fs.readFileSync(file, "utf8"));
      lock.packages["node_modules/other/node_modules/braces"] = { version: "3.0.3" };
      fs.writeFileSync(file, JSON.stringify(lock));
    }
    assert.notEqual(check.run().status, 0);
    assert.equal(check.calls().length, 0);
  }
});

test("unsupported audit options cannot weaken the gate", t => {
  const check = fixture(t);
  assert.equal(check.run("--omit=dev").status, 2);
  assert.equal(check.calls().length, 0);
});
