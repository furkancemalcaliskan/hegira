import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { createInterface } from "node:readline";

// A receipt in the already locked repository-owned Cargo cache, never the app.
const read = process.argv[2] === "--read";
const root = path.resolve(process.argv[read ? 3 : 2]);
const receipt = path.join(root, "database-operation-test-binary.json");
function validate(executable) {
  assert.equal(typeof executable, "string");
  assert.equal(path.resolve(executable), executable);
  assert(executable.startsWith(`${root}/debug/deps/`));
  assert.equal(fs.lstatSync(executable).isFile(), true);
  return executable;
}
if (read) {
  console.log(validate(JSON.parse(fs.readFileSync(receipt, "utf8"))));
} else {
  let finished = false;
  const artifacts = [];
  for await (const line of createInterface({ input: process.stdin, crlfDelay: Infinity })) {
    if (!line) continue;
    const message = JSON.parse(line);
    if (message.reason === "build-finished") finished = message.success === true;
    if (message.reason === "compiler-artifact" &&
        message.target.name === "database_operation_contract" && message.executable) {
      artifacts.push(message.executable);
    }
  }
  assert(finished, "Cargo must finish successfully before a test binary can run");
  assert.equal(artifacts.length, 1);
  fs.writeFileSync(receipt, JSON.stringify(validate(artifacts[0])));
}
