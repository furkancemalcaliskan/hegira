// Runtime-only contracts against the script-owned disposable production container.
import assert from "node:assert/strict";
import { createHmac, randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";

export function adminToken(secret) {
  const now = Math.floor(Date.now() / 1000);
  const encode = value => Buffer.from(JSON.stringify(value)).toString("base64url");
  const input = `${encode({ alg: "HS256", typ: "JWT" })}.${encode({
    sub: "upgrade-admin@example.test", exp: now + 3600, iat: now,
    jti: randomBytes(16).toString("hex"),
  })}`;
  return `${input}.${createHmac("sha256", secret).update(input).digest("base64url")}`;
}

export async function verify(base, composition, application) {
  const request = (path, options = {}) => fetch(`${base}${path}`, {
    ...options, redirect: "manual", signal: AbortSignal.timeout(15_000),
  });
  const json = async (path, status, options = {}) => {
    const response = await request(path, options);
    assert.equal(response.status, status, `${composition}: ${options.method ?? "GET"} ${path}`);
    return response.json();
  };
  for (const path of ["/healthz", "/readyz"]) {
    assert.equal((await request(path)).status, 200, path);
  }
  const index = await request("/");
  assert.equal(index.status, 200);
  assert.match(await index.text(), /<!doctype html|<html/i);
  for (const [name, expected] of [
    ["x-content-type-options", "nosniff"], ["x-frame-options", "DENY"],
  ]) assert.equal(index.headers.get(name), expected);
  for (const name of ["content-security-policy", "strict-transport-security", "x-request-id"])
    assert.ok(index.headers.get(name), name);
  for (const path of ["/pkg/app.css", "/pkg/app.js"]) {
    const response = await request(path);
    assert.equal(response.status, 200, path);
    assert.ok((await response.arrayBuffer()).byteLength > 0, path);
  }
  const wasm = await request("/pkg/app.wasm");
  assert.equal(wasm.status, 200);
  assert.equal(Buffer.from(await wasm.arrayBuffer()).subarray(0, 4).toString("hex"), "0061736d");
  const logo = await request("/assets/branding/hegira-logo.png");
  assert.equal(logo.status, 200);
  assert.deepEqual(Buffer.from(await logo.arrayBuffer()), readFileSync(
    `${application}/apps/web/src/public/assets/branding/hegira-logo.png`,
  ));

  if (composition === "minimal") {
    for (const [path, method] of [
      ["/api/identity/users", "GET"], ["/api/identity/auth/login", "POST"],
      ["/api/upgrade-notes", "GET"],
    ]) assert.equal((await request(path, { method })).status, 404, path);
    console.log("minimal production: health, assets, headers, and absent Identity/resource APIs passed");
    return;
  }

  const token = process.env.HEGIRA_UPGRADE_ADMIN_TOKEN;
  assert.ok(token);
  const headers = { "content-type": "application/json", authorization: `Bearer ${token}` };
  const input = { title: "after-upgrade", active: true, sequence: 8, external_id: null, observed_at: null };
  assert.equal((await json("/api/identity/users", 401)).code, "auth:missing_bearer_token");
  assert.equal((await json("/api/upgrade-notes", 401)).code, "auth:missing_bearer_token");
  assert.equal((await request("/api/upgrade-notes", {
    headers: { cookie: `hegira-session=${token}` },
  })).status, 401, "cookies cannot authenticate Bearer APIs");
  assert.equal((await request("/api/upgrade-notes?offset=0&limit=20", {
    headers: { authorization: "Bearer invalid-token" },
  })).status, 401);
  // These mutations have no Origin; applying BFF CSRF policy to Bearer routes would break them.
  const preserved = await json("/api/upgrade-notes/11111111-1111-1111-1111-111111111111", 200, { headers });
  assert.equal(preserved.title, "before-upgrade");
  const created = await json("/api/upgrade-notes", 201, {
    method: "POST", headers, body: JSON.stringify(input),
  });
  assert.ok(created.id);
  assert.equal((await json(`/api/upgrade-notes/${created.id}`, 200, { headers })).title, input.title);
  const listed = await json("/api/upgrade-notes?offset=0&limit=20", 200, { headers });
  assert.ok(JSON.stringify(listed).includes("after-upgrade"));
  assert.equal((await json(`/api/upgrade-notes/${created.id}`, 200, {
    method: "PUT", headers, body: JSON.stringify({ ...input, title: "updated", sequence: 9 }),
  })).title, "updated");
  assert.equal((await request(`/api/upgrade-notes/${created.id}`, { method: "DELETE", headers })).status, 204);
  assert.equal((await request(`/api/upgrade-notes/${created.id}`, { headers })).status, 404);

  // Create a real unprivileged principal through the protected user-management API.
  // Self-registration requires mail delivery, which this production profile disables.
  const credentials = { username: `upgrade-${randomBytes(8).toString("hex")}@example.test`,
    password: randomBytes(32).toString("base64url") };
  const anonymousHeaders = { "content-type": "application/json" };
  const user = await json("/api/identity/users", 201, {
    method: "POST", headers,
    body: JSON.stringify({ ...credentials, is_verified: true, roles: [] }),
  });
  assert.equal(user.username, credentials.username);
  const invalidLogin = await request("/api/identity/auth/login", {
    method: "POST", headers: anonymousHeaders,
    body: JSON.stringify({ ...credentials, password: randomBytes(32).toString("base64url") }),
  });
  assert.equal(invalidLogin.status, 401);
  const login = await json("/api/identity/auth/login", 200, {
    method: "POST", headers: anonymousHeaders, body: JSON.stringify(credentials),
  });
  assert.ok(login.token);
  assert.equal((await request("/api/upgrade-notes?offset=0&limit=20", {
    headers: { authorization: `Bearer ${login.token}` },
  })).status, 403, "authenticated users without the resource permission must be denied");

  const bffPath = readFileSync(`${application}/.hegira-validation/bff-path`, "utf8");
  assert.ok(bffPath.startsWith("/api/"));
  for (const origin of [undefined, "https://untrusted.example"]) {
    const cookieHeaders = { "content-type": "application/x-www-form-urlencoded", cookie: `hegira-session=${token}` };
    if (origin) cookieHeaders.origin = origin;
    assert.equal((await request(bffPath, {
      method: "POST", headers: cookieHeaders, body: "",
    })).status, 403, "cookie BFF mutation requires the trusted Origin");
  }
  console.log(`${composition} production: preserved data, CRUD, authentication, authorization, and cookie/Bearer isolation passed`);
}

if (process.argv[2] === "token") {
  process.stdout.write(adminToken(process.env.UPGRADE_JWT_SECRET));
} else if (process.argv[2] === "check") {
  await verify(process.argv[3], process.argv[4], process.argv[5]);
}
