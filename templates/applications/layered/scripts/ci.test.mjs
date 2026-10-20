import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync, mkdirSync, chmodSync, copyFileSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { provider, profile, postgresImage } from "./ci-integration.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const workflow = readFileSync(join(root, ".github/workflows/quality.yml"), "utf8");
const tools = readFileSync(join(root, ".github/actions/application-tools/action.yml"), "utf8");
const script = readFileSync(join(root, "scripts/ci.sh"), "utf8");

export function qualityErrors(source, setup = tools, commands = script) {
  const errors = [];
  const require = (condition, label) => { if (!condition) errors.push(label); };
  for (const forbidden of [/pull_request_target/, /paths(?:-ignore)?:/, /continue-on-error/, /secrets\s*[.[]/, /permissions:\s*write-all/, /(?:contents|actions|packages|id-token):\s*write/]) {
    require(!forbidden.test(source + setup), `unsafe workflow: ${forbidden}`);
  }
  require(/permissions:\s*\n  contents: read/.test(source), "read-only token");
  require(/cancel-in-progress: true/.test(source) && /group: application-quality-/.test(source), "bounded concurrency");
  require(/pull_request:\s*\n    branches: \[develop, main\]/.test(source), "unfiltered protected-branch PRs");
  require(/push:\s*\n    branches: \[develop, main\]/.test(source) && /workflow_dispatch:/.test(source), "integration and manual entry points");
  for (const uses of (source + setup).matchAll(/uses: ([^\s]+)/g)) {
    require(uses[1] === "./application/.github/actions/application-tools" || /@[a-f0-9]{40}$/.test(uses[1]), "immutable action source");
  }
  require((source.match(/persist-credentials: false/g) ?? []).length === 4 && /persist-credentials: false/.test(setup), "no persisted checkout credentials");
  require(/ref: b869b3ae3b60da7052ca91c500547081f9fc9f54/.test(setup), "reviewed compatible CLI source");
  for (const pin of ["toolchain: 1.96.1", "targets: wasm32-unknown-unknown", "cargo-leptos@0.3.7", "cargo-audit@0.22.2", "cargo-deny@0.20.2"]) {
    require(setup.includes(pin), `tool pin ${pin}`);
  }
  for (const [job, step] of [["source", "source"], ["native", "check"], ["release", "build"], ["supply-chain", "security"]]) {
    const block = source.match(new RegExp(`^  ${job}:\\n([\\s\\S]*?)(?=^  [a-z-]+:|(?![\\s\\S]))`, "m"))?.[1] ?? "";
    require(block.includes(`sh scripts/ci.sh ${step}`) && /timeout-minutes:/.test(block), `${job} entry point/deadline`);
    if (job !== "source") require(/needs: source/.test(block), `${job} ordering`);
  }
  require(/run: sh scripts\/ci.sh test/.test(source), "native tests");
  require(/APPLICATION_CI_DISPOSABLE: 'true'/.test(source) && /run: sh scripts\/ci.sh integration/.test(source), "explicit disposable integration");
  require((source.match(/if: always\(\)/g) ?? []).length === 3 && (source.match(/run: sh scripts\/ci.sh cleanup/g) ?? []).length === 2, "failure cleanup");
  const quality = source.split("  quality:\n")[1] ?? "";
  require(/if: always\(\)/.test(quality) && /needs: \[source, native, release, supply-chain\]/.test(quality), "unconditional complete aggregate");
  require(/permissions: \{\}/.test(quality) && /working-directory: \./.test(quality), "aggregate needs no source checkout");
  for (const name of ["SOURCE", "NATIVE", "RELEASE", "SUPPLY_CHAIN"]) {
    require(quality.includes(`test "$${name}_RESULT" = success`), `aggregate propagates ${name}`);
  }
  require(!/if:/.test(quality.split("    steps:")[1] ?? ""), "aggregate step cannot be skipped");
  require(/--validation-cache/.test(commands) && /artifacts prune --cache-budget-mib 1 --execute/.test(commands), "CLI-owned budget and reclamation");
  require(!/cargo (?:clean|generate-lockfile|update)|npm (?:install|update)|rm -rf.*target/.test(commands), "no developer-cache cleanup or unlocked dependency rewrite");
  require(commands.includes("cargo tree --locked --target all --all-features --invert rsa@0.9.10") && /\[ -z "\$graph" \]/.test(commands), "conditional RSA exception proof");
  require(commands.includes("cargo deny --locked --all-features check") && commands.includes("cargo audit --file Cargo.lock"), "Rust supply-chain enforcement");
  require(commands.includes("npm audit --audit-level=high --include=dev --include=optional --include=peer"), "all frontend dependency classes audited");
  require(commands.includes("sha256sum --check --status"), "dependency locks retained on every phase outcome");
  require(!/\|\|\s*true/.test(commands), "no tolerated audit or operation failures");
  return errors;
}

test("generated workflow selects this provider and preserves reviewed security/ordering contracts", () => {
  assert.ok(["sqlite", "postgres"].includes(provider));
  assert.equal(profile, provider === "sqlite" ? "sqlite" : "development");
  assert.ok(workflow.includes(`Locked ${provider} native and WASM checks`));
  assert.match(postgresImage, /^postgres:17-alpine@sha256:[a-f0-9]{64}$/);
  assert.deepEqual(qualityErrors(workflow), []);
  const setupScript = readFileSync(join(root, "scripts/ci-tools.sh"), "utf8");
  assert.match(setupScript, /e959f2170af4c20c552e9de3a0253704d6a9d2766e8fdb88e4d6ac4bae9388fe/);
  assert.match(setupScript, /wasm-opt version 123/);
  assert.ok(!setupScript.includes("cargo generate-lockfile"));
});

test("workflow mutations cannot hide constituent failure or broaden fork privileges", () => {
  for (const [from, to] of [
    ["contents: read", "contents: write"],
    ["cancel-in-progress: true", "cancel-in-progress: false"],
    ["  pull_request:", "  pull_request_target:"],
    ["    branches: [develop, main]", "    paths: ['docs/**']"],
    ["    needs: source", "    needs: []"],
    ["needs: [source, native, release, supply-chain]", "needs: [native]"],
    ['test "$RELEASE_RESULT" = success', "true"],
    ["    if: always()\n    needs:", "    if: success()\n    needs:"],
  ]) {
    assert.notEqual(workflow.replace(from, to), workflow, from);
    assert.ok(qualityErrors(workflow.replace(from, to)).length > 0, from);
  }
  assert.ok(qualityErrors(workflow.replace(/@([a-f0-9]{40})/g, "@v7")).length > 0);
  assert.ok(qualityErrors(workflow, tools.replace("cargo-leptos@0.3.7", "cargo-leptos@latest")).length > 0);
  assert.ok(qualityErrors(workflow, tools, script.replace('graph=$(cargo tree', 'graph=$(false || cargo tree').replace(' --invert rsa@0.9.10)', ' --invert rsa@0.9.10) || true')).length > 0);
});

test("the real aggregate shell fails for every unsuccessful or missing constituent", () => {
  const aggregate = workflow.split("  quality:\n")[1].split("        run: |\n")[1]
    .split("\n").filter(line => line.trim()).map(line => line.replace(/^          /, "")).join("\n");
  const results = { SOURCE_RESULT: "success", NATIVE_RESULT: "success", RELEASE_RESULT: "success", SUPPLY_CHAIN_RESULT: "success" };
  assert.equal(spawnSync("sh", ["-ec", aggregate], { env: results }).status, 0);
  for (const variable of Object.keys(results)) {
    for (const outcome of ["failure", "cancelled", "skipped", ""]) {
      assert.notEqual(spawnSync("sh", ["-ec", aggregate], { env: { ...results, [variable]: outcome } }).status, 0, `${variable}=${outcome}`);
    }
  }
});

function fixture() {
  const parent = mkdtempSync(join(tmpdir(), "application-ci-contract-"));
  const application = join(parent, "application ; shell bytes");
  const runner = join(parent, "runner");
  const bin = join(parent, "tools");
  for (const directory of [join(application, "scripts"), join(application, "apps/web/src"), join(runner, "hegira-ci-bootstrap/debug"), bin]) mkdirSync(directory, { recursive: true });
  copyFileSync(join(root, "scripts/ci.sh"), join(application, "scripts/ci.sh"));
  copyFileSync(join(root, "scripts/ci-integration.mjs"), join(application, "scripts/ci-integration.mjs"));
  writeFileSync(join(application, "Cargo.lock"), "reviewed cargo lock\n");
  writeFileSync(join(application, "apps/web/src/package-lock.json"), "reviewed frontend lock\n");
  const log = join(parent, "commands.jsonl");
  writeFileSync(log, "");
  const helper = `#!${process.execPath}\nimport fs from 'node:fs';\nconst args=process.argv.slice(2);\nfs.appendFileSync(process.env.CI_CONTRACT_LOG,JSON.stringify(args)+'\\n');\nif(process.env.CI_CONTRACT_REWRITE==='true')fs.writeFileSync('Cargo.lock','unreviewed rewrite');\nif(args[0]==='tree' && process.env.CI_CONTRACT_RSA==='true')console.log('rsa v0.9.10');\nif(process.env.CI_CONTRACT_FAIL===args[0] && (args.includes('--execute')||['deny','audit','tree'].includes(args[0])))process.exit(23);\n`;
  for (const file of [join(runner, "hegira-ci-bootstrap/debug/hegira"), join(bin, "cargo"), join(bin, "npm")]) {
    writeFileSync(file, helper); chmodSync(file, 0o700);
  }
  const rustup = join(bin, "rustup");
  writeFileSync(rustup, '#!/bin/sh\nprintf "%s\\n" "$CI_CONTRACT_CARGO"\n'); chmodSync(rustup, 0o700);
  const env = { ...process.env, PATH: `${bin}:${process.env.PATH}`, RUNNER_TEMP: runner, CI_CONTRACT_CARGO: join(bin, "cargo"), CI_CONTRACT_LOG: log };
  return {
    application, runner, bin, parent, env, log,
    run(action, extra = {}) { return spawnSync("sh", ["scripts/ci.sh", action], { cwd: application, env: { ...env, ...extra }, encoding: "utf8", timeout: 20_000 }); },
    commands() { return readFileSync(log, "utf8").trim().split("\n").filter(Boolean).map(JSON.parse); },
    close() { rmSync(parent, { recursive: true, force: true }); },
  };
}

test("public CLI invocations use preview/execution trust, bounded caches and exact installed tools", () => {
  for (const action of ["check", "test", "build"]) {
    const f = fixture();
    try {
      const output = f.run(action);
      assert.equal(output.status, 0, output.stderr);
      const commands = f.commands();
      const [preview, execute] = commands;
      assert.equal(preview[0], action);
      assert.ok(preview.includes("--dry-run") && !preview.includes("--execute"));
      for (const flag of ["--validation-cache", "--cache-budget-mib", "--execute", "--trust-application", "--cargo", "--tool-directory"]) assert.ok(execute.includes(flag), flag);
      assert.ok(!execute.includes("--ignored") && !execute.includes("--all-features"));
      if (action === "build") {
        assert.ok(execute.includes("--release"));
        assert.equal(execute[execute.indexOf("--wasm-bindgen") + 1], join(f.runner, "hegira-ci-wasm/bin/wasm-bindgen"));
        assert.equal(execute[execute.indexOf("--wasm-opt") + 1], join(f.runner, "hegira-ci-binaryen/bin/wasm-opt"));
      }
      assert.equal(commands.at(-1)[1], "cache-status");
      assert.equal(readFileSync(join(f.application, "Cargo.lock"), "utf8"), "reviewed cargo lock\n");
    } finally { f.close(); }
  }
});

test("operation, audit and lock mutation failures propagate instead of becoming green", () => {
  for (const [action, env] of [
    ["check", { CI_CONTRACT_FAIL: "check" }],
    ["security", { CI_CONTRACT_FAIL: "tree" }],
    ["security", { CI_CONTRACT_RSA: "true" }],
    ["security", { CI_CONTRACT_FAIL: "deny" }],
    ["security", { CI_CONTRACT_FAIL: "audit" }],
    ["check", { CI_CONTRACT_REWRITE: "true" }],
  ]) {
    const f = fixture();
    try { assert.notEqual(f.run(action, env).status, 0); }
    finally { f.close(); }
  }
  const f = fixture();
  try {
    assert.equal(f.run("security").status, 0);
    assert.deepEqual(f.commands().map(args => args[0]), ["tree", "deny", "audit", "audit"]);
    assert.equal(f.run("arbitrary-operation").status, 2);
    assert.equal(f.run("check", { APPLICATION_CI_BUDGET_MIB: "0" }).status, 2);
    assert.equal(f.run("cleanup").status, 0);
    assert.ok(f.commands().some(args => args[0] === "artifacts" && args.includes("--execute")));
  } finally { f.close(); }
});

test("integration requires explicit disposable consent and boots only newly owned local state", () => {
  const f = fixture();
  try {
    const rejected = f.run("integration", { APPLICATION_CI_DISPOSABLE: "false" });
    assert.notEqual(rejected.status, 0);
    assert.deepEqual(readdirSync(f.runner).sort(), ["hegira-ci-bootstrap"]);
    const binary = join(f.application, "target/hegira/validation/build/release/app_server");
    mkdirSync(dirname(binary), { recursive: true });
    writeFileSync(binary, `#!${process.execPath}\nimport http from 'node:http';import fs from 'node:fs';\nconst e=process.env;\nif(e.APP__STARTUP__SEED_IDENTITY!=='false'||e.APP__STARTUP__ENSURE_DATABASE!=='false')process.exit(5);\nif(e.APP__DATABASE__BACKEND==='sqlite'){const {DatabaseSync}=await import('node:sqlite');const db=new DatabaseSync(e.APP__DATABASE__URL.slice('sqlite://'.length));db.exec('CREATE TABLE _sqlx_migrations(success BOOLEAN)');db.close();}\nfs.writeFileSync(e.CI_OBSERVATION,JSON.stringify({profile:e.APP_ENV,provider:e.APP__DATABASE__BACKEND,url:e.APP__DATABASE__URL}));\nconst [host,port]=e.APP__SERVER__ADDR.split(':');http.createServer((req,res)=>res.end(req.url==='/'?'<html>owned shell</html>':e.APP__APPLICATION__NAME)).listen(Number(port),host);\n`);
    chmodSync(binary, 0o700);
    const fakeDocker = join(f.bin, "docker");
    writeFileSync(fakeDocker, `#!${process.execPath}\nimport fs from 'node:fs';const a=process.argv.slice(2);fs.appendFileSync(process.env.CI_CONTRACT_LOG,JSON.stringify(a)+'\\n');if(a[0]==='run'){if(!process.env.POSTGRES_PASSWORD)process.exit(5);console.log(process.env.CI_CONTRACT_DOCKER_ID ?? 'c'.repeat(64));}else if(a[0]==='port')console.log('127.0.0.1:54329');else if(a.includes('psql'))console.log('0');\n`);
    chmodSync(fakeDocker, 0o700);
    const observation = join(f.parent, "runtime.json");
    const output = f.run("integration", { APPLICATION_CI_DISPOSABLE: "true", CI_OBSERVATION: observation, APP__DATABASE__URL: "private-inherited-production-url" });
    assert.equal(output.status, 0, output.stderr);
    assert.ok(!output.stdout.includes("private-inherited") && !output.stderr.includes("private-inherited"));
    const runtime = JSON.parse(readFileSync(observation, "utf8"));
    assert.equal(runtime.provider, provider);
    assert.equal(runtime.profile, profile);
    assert.ok(!runtime.url.includes("private-inherited"));
    if (provider === "sqlite") {
      assert.ok(runtime.url.startsWith(`sqlite://${f.runner}/application-integration-`));
      assert.equal(readFileSync(f.log, "utf8"), "");
    } else {
      const commands = f.commands();
      assert.ok(commands[0].includes(postgresImage));
      assert.ok(commands.some(args => args[2] === "pg_isready" && args[3] === "-h" && args[4] === "127.0.0.1"));
      assert.deepEqual(commands.at(-1), ["rm", "--force", "--volumes", "c".repeat(64)]);
      writeFileSync(f.log, "");
      const invalid = f.run("integration", { APPLICATION_CI_DISPOSABLE: "true", CI_CONTRACT_DOCKER_ID: "unowned-container" });
      assert.notEqual(invalid.status, 0);
      assert.deepEqual(f.commands().map(args => args[0]), ["run"], "invalid identities grant no inspection or cleanup authority");
    }
    assert.deepEqual(readdirSync(f.runner).sort(), ["hegira-ci-bootstrap"]);
  } finally { f.close(); }
});
