import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

test("database matrix rejects invalid selection or missing PostgreSQL authority before setup", () => {
  const env = { ...process.env };
  delete env.ALLOW_DATABASE_OPERATION_DISPOSABLE_TARGETS;
  delete env.DATABASE_OPERATION_POSTGRES_URL;
  for (const args of [[], ["all"], ["postgres"], ["sqlite", "all"]]) {
    const result = spawnSync("sh", [new URL("./database-operation-check.sh", import.meta.url).pathname, ...args], { env, encoding: "utf8" });
    assert.equal(result.status, 2);
    assert.equal(result.stdout, "");
  }
});

test("Cargo artifact selection is closed, streaming, and cannot publish a failed build receipt", t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hegira-db-artifact-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const deps = path.join(root, "debug", "deps");
  fs.mkdirSync(deps, { recursive: true });
  const executable = path.join(deps, "database_operation_contract-test");
  fs.writeFileSync(executable, "controlled artifact; never executed");
  const program = new URL("./database-operation-test-binary.mjs", import.meta.url).pathname;
  const artifact = { reason: "compiler-artifact", target: { name: "database_operation_contract" }, executable };
  const finish = { reason: "build-finished", success: true };
  const input = messages => messages.map(m => JSON.stringify(m)).join("\n") + "\n";
  const run = messages => spawnSync(process.execPath, [program, root], { input: input(messages), encoding: "utf8" });
  const selected = run([artifact, finish]);
  assert.equal(selected.status, 0, selected.stderr);
  const receipt = path.join(root, "database-operation-test-binary.json");
  const before = fs.readFileSync(receipt, "utf8");
  assert.equal(spawnSync(process.execPath, [program, "--read", root], { encoding: "utf8" }).stdout, `${executable}\n`);
  for (const messages of [
    [artifact], [artifact, { ...finish, success: false }],
    [artifact, artifact, finish], [finish],
    [{ ...artifact, executable: process.execPath }, finish],
    [{ ...artifact, executable: `${deps}/../deps/database_operation_contract-test` }, finish],
  ]) {
    assert.notEqual(run(messages).status, 0);
    assert.equal(fs.readFileSync(receipt, "utf8"), before);
  }
});
