import test from "node:test";
import assert from "node:assert/strict";
import { createHmac, randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import { fileURLToPath } from "node:url";
import { adminToken, verify } from "./upgraded-application-http.mjs";

test("disposable principal tokens bind the subject and ephemeral signing key", () => {
  const secret = randomBytes(32).toString("hex");
  const token = adminToken(secret);
  const [header, payload, signature] = token.split(".");
  assert.deepEqual(JSON.parse(Buffer.from(header, "base64url")), { alg: "HS256", typ: "JWT" });
  const claims = JSON.parse(Buffer.from(payload, "base64url"));
  assert.equal(claims.sub, "upgrade-admin@example.test");
  assert.equal(claims.exp - claims.iat, 3600);
  assert.match(claims.jti, /^[a-f0-9]{32}$/);
  assert.equal(signature, createHmac("sha256", secret).update(`${header}.${payload}`).digest("base64url"));
  assert.notEqual(adminToken(secret), token);
  assert.throws(() => adminToken(undefined));
});

test("production probes fail closed on missing headers, malformed WASM, or exposed minimal APIs", async t => {
  const application = fileURLToPath(new URL("../templates/applications/layered", import.meta.url));
  let fault;
  const server = createServer((request, response) => {
    const path = request.url;
    if (path === "/") {
      for (const [name, value] of Object.entries({
        "x-content-type-options": "nosniff", "x-frame-options": "DENY",
        "content-security-policy": "default-src 'self'", "strict-transport-security": "max-age=31536000",
        "x-request-id": "probe-fixture",
      })) if (name !== fault) response.setHeader(name, value);
      response.end("<!doctype html><html></html>");
    } else if (["/healthz", "/readyz", "/pkg/app.css", "/pkg/app.js"].includes(path)) {
      response.end("ok");
    } else if (path === "/pkg/app.wasm") {
      response.end(Buffer.from(fault === "wasm" ? "ffffffff" : "0061736d", "hex"));
    } else if (path === "/assets/branding/hegira-logo.png") {
      response.end(readFileSync(`${application}/apps/web/src/public/assets/branding/hegira-logo.png`));
    } else {
      response.statusCode = fault === "exposed-api" ? 200 : 404;
      response.end();
    }
  });
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  t.after(async () => {
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  });
  const base = `http://127.0.0.1:${server.address().port}`;
  await verify(base, "minimal", application);
  for (fault of ["x-frame-options", "content-security-policy", "wasm", "exposed-api"]) {
    await assert.rejects(verify(base, "minimal", application), undefined, fault);
  }
});
