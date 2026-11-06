// Explicit disposable CI boot, not a database URL/reset interface.
import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import { mkdtemp, realpath, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { setTimeout as delay } from "node:timers/promises";

export const provider = "{{database_adapter}}";
export const profile = "{{development_profile}}";
export const postgresImage = "postgres:17-alpine@sha256:aa90e97ee862e558111d34cfb8b2c4bec768c2b039fb791341686928560263b3";

export function docker(command, args, env = process.env) {
  const result = spawnSync(command, args, {
    env, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"],
    timeout: 30_000, maxBuffer: 64 * 1024,
  });
  assert.equal(result.status, 0, "Disposable container operation failed; raw output is suppressed");
  return result.stdout.trim();
}

async function unusedPort() {
  const listener = createServer();
  await new Promise((done, fail) => {
    listener.once("error", fail);
    listener.listen(0, "127.0.0.1", done);
  });
  const port = listener.address().port;
  await new Promise((done, fail) => listener.close(error => error ? fail(error) : done()));
  return port;
}

function signalGroup(child, signal) {
  if (!child?.pid || child.exitCode !== null || child.signalCode !== null) return;
  try { process.kill(-child.pid, signal); }
  catch (error) { if (error.code !== "ESRCH") throw new Error("Cannot stop the owned CI server"); }
}

export async function main() {
  assert.equal(process.argv.length, 2, "No integration arguments or database URLs are accepted");
  assert.equal(process.env.APPLICATION_CI_DISPOSABLE, "true", "Set APPLICATION_CI_DISPOSABLE=true only for the disposable CI boot");
  assert.equal(process.platform, "linux", "Disposable CI boot requires Linux");
  assert.equal(process.arch, "x64", "Pinned CI tools and PostgreSQL image require x64");
  assert.ok(["sqlite", "postgres"].includes(provider), "Unsupported rendered provider");
  const root = await realpath(join(dirname(fileURLToPath(import.meta.url)), ".."));
  assert.ok(isAbsolute(process.env.RUNNER_TEMP ?? ""), "Select an absolute disposable runner directory");
  const runner = await realpath(process.env.RUNNER_TEMP);
  const outside = relative(root, runner);
  assert.ok(outside === ".." || outside.startsWith("../"), "Runner state must be outside the application");
  const directory = await mkdtemp(join(runner, "application-integration-"));
  let container;
  let child;
  let closed;
  let interrupted = false;
  const cancel = () => { interrupted = true; signalGroup(child, "SIGTERM"); };
  process.on("SIGINT", cancel);
  process.on("SIGTERM", cancel);
  try {
    let url;
    const sqlite = join(directory, "application.sqlite3");
    if (provider === "sqlite") {
      await writeFile(sqlite, "", { flag: "wx", mode: 0o600 });
      url = `sqlite://${sqlite}`;
    } else {
      const password = randomBytes(32).toString("hex");
      const created = docker("docker", [
        "run", "--detach", "--rm", "--label", "application-ci=disposable",
        "--read-only", "--tmpfs", "/var/lib/postgresql/data",
        "--tmpfs", "/var/run/postgresql", "--publish", "127.0.0.1::5432",
        "--env", "POSTGRES_USER=postgres", "--env", "POSTGRES_DB=application_ci",
        "--env", "POSTGRES_PASSWORD", postgresImage,
      ], { ...process.env, POSTGRES_PASSWORD: password });
      assert.match(created, /^[a-f0-9]{64}$/, "Unexpected owned container identity");
      container = created;
      const binding = docker("docker", ["port", container, "5432/tcp"]);
      assert.match(binding, /^127\.0\.0\.1:[0-9]+$/, "Container must bind only a random localhost port");
      url = `postgres://postgres:${password}@${binding}/application_ci`;
      let ready = false;
      for (let attempt = 0; attempt < 60 && !interrupted; attempt++) {
        // Initialization also starts a socket-only temporary server. Require
        // TCP readiness before starting the application against the host port.
        const probe = spawnSync("docker", ["exec", container, "pg_isready", "-h", "127.0.0.1", "-U", "postgres", "-d", "application_ci"], { stdio: "ignore", timeout: 5000 });
        if (probe.status === 0) { ready = true; break; }
        await delay(1000);
      }
      assert.ok(ready, "Owned PostgreSQL did not become ready");
    }
    const port = await unusedPort();
    const name = `ci-${randomBytes(12).toString("hex")}`;
    const env = {
      ...process.env,
      APP_ENV: profile,
      DATABASE_URL: url,
      APP__DATABASE__URL: url,
      APP__DATABASE__BACKEND: provider,
      APP__DATABASE__AUTO_MIGRATE: "true",
      APP__STARTUP__ENSURE_DATABASE: "false",
      APP__STARTUP__SEED_IDENTITY: "false",
      APP__STARTUP__SCHEDULER: "false",
      APP__STARTUP__DURABLE_JOBS: "false",
      APP__SEED__SEED_ADMIN: "false",
      APP__RUNTIME__ROLE: "web",
      APP__SECURITY__JWT_SECRET: randomBytes(32).toString("hex"),
      APP__APPLICATION__NAME: name,
      APP__SERVER__ADDR: `127.0.0.1:${port}`,
      LEPTOS_SITE_ADDR: `127.0.0.1:${port}`,
      LEPTOS_SITE_ROOT: join(root, "target/hegira/validation/build/site"),
      LEPTOS_SITE_PKG_DIR: "pkg",
      APP__LOGGING__FILTER: "off",
    };
    assert.ok(!interrupted, "Integration was interrupted");
    child = spawn(join(root, "target/hegira/validation/build/release/app_server"), [], {
      cwd: root, env, detached: true, stdio: "ignore",
    });
    let failed = false;
    child.once("error", () => { failed = true; });
    closed = new Promise(done => child.once("close", done));
    let ready = false;
    for (let attempt = 0; attempt < 60 && !interrupted; attempt++) {
      assert.ok(!failed && child.exitCode === null && child.signalCode === null, "Owned CI server exited before readiness; logs are suppressed");
      try {
        const response = await fetch(`http://127.0.0.1:${port}/readyz`, { signal: AbortSignal.timeout(2000) });
        if (response.ok) { ready = true; break; }
      } catch { /* Startup is bounded; raw network/configuration errors stay private. */ }
      await delay(1000);
    }
    assert.ok(ready && !interrupted, "Owned CI server did not become ready");
    const health = await fetch(`http://127.0.0.1:${port}/healthz`, { signal: AbortSignal.timeout(2000) });
    assert.ok(health.ok && (await health.text()).includes(name), "Health response did not identify this owned CI server");
    const page = await fetch(`http://127.0.0.1:${port}/`, { signal: AbortSignal.timeout(5000) });
    assert.ok(page.ok && (await page.text()).includes("<html"), "Application SSR shell failed");
    if (provider === "sqlite") {
      const { DatabaseSync } = await import("node:sqlite");
      const database = new DatabaseSync(sqlite, { readOnly: true });
      try {
        assert.equal(database.prepare("SELECT count(*) AS failures FROM _sqlx_migrations WHERE NOT success").get().failures, 0);
      } finally { database.close(); }
    } else {
      assert.equal(docker("docker", ["exec", container, "psql", "-U", "postgres", "-d", "application_ci", "-tAc", "SELECT count(*) FROM _sqlx_migrations WHERE NOT success"]), "0");
    }
    console.log(`Disposable ${provider} migrations, readiness and SSR boot: ok`);
  } finally {
    try {
      if (child) {
        signalGroup(child, "SIGTERM");
        await Promise.race([closed, delay(3000, undefined, { ref: false })]);
        signalGroup(child, "SIGKILL");
        await closed;
      }
      if (container) docker("docker", ["rm", "--force", "--volumes", container]);
    } finally {
      process.off("SIGINT", cancel);
      process.off("SIGTERM", cancel);
      await rm(directory, { recursive: true, force: true });
    }
  }
  assert.ok(!interrupted, "Integration was interrupted");
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(() => {
    console.error("Disposable application CI integration failed; raw logs, credentials and runtime settings are suppressed.");
    process.exitCode = 1;
  });
}
