import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  validateReleaseFiles,
  validateReleaseMetadata,
  validateReleaseWorkflow,
} from "./release-policy.mjs";

function metadata(overrides = {}) {
  const hegira = {
    id: "hegira 0.2.0 (path+file:///workspace/apps/hegira)",
    name: "hegira",
    version: "0.2.0",
  };
  const domain = {
    id: "domain 0.2.0 (path+file:///workspace/crates/domain)",
    name: "domain",
    version: "0.2.0",
  };
  return {
    workspace_members: [hegira.id, domain.id],
    packages: [hegira, domain],
    ...overrides,
  };
}

function releaseFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hegira-release-"));
  fs.mkdirSync(path.join(root, "docs", "releases"), { recursive: true });
  fs.writeFileSync(
    path.join(root, "CHANGELOG.md"),
    "# Changelog\n\n## [0.2.0] - 2026-07-24\n",
  );
  fs.writeFileSync(
    path.join(root, "docs", "releases", "v0.2.0.md"),
    "# Hegira v0.2.0\n",
  );
  return root;
}

const validWorkflow = `
permissions:
  contents: read

jobs:
  validate:
    steps:
      - run: sh scripts/release-policy.sh
      - uses: anchore/sbom-action@v0
        with:
          upload-release-assets: false
  full-stack:
    steps:
      - run: sh scripts/full-stack-build-check.sh
  container:
    steps:
      - run: sh scripts/container-smoke.sh
  publish:
    if: github.event_name == 'push'
    permissions:
      contents: write
    steps:
      - run: gh release create --verify-tag
`;

test("accepts consistent workspace versions", () => {
  assert.deepEqual(validateReleaseMetadata(metadata(), "v0.2.0"), []);
});

test("rejects a non-stable release ref", () => {
  const errors = validateReleaseMetadata(metadata(), "release-0.2");
  assert.ok(errors.some((error) => error.includes("vMAJOR.MINOR.PATCH")));
});

test("rejects a mismatched workspace package version", () => {
  const fixture = metadata();
  fixture.packages[1].version = "0.1.2";
  const errors = validateReleaseMetadata(fixture, "v0.2.0");
  assert.ok(
    errors.some(
      (error) => error.includes("domain") && error.includes("0.1.2"),
    ),
  );
});

test("accepts matching changelog and versioned release notes", (context) => {
  const root = releaseFixture();
  context.after(() => fs.rmSync(root, { recursive: true, force: true }));
  assert.deepEqual(validateReleaseFiles(root, "v0.2.0"), []);
});

test("rejects a missing dated changelog release", (context) => {
  const root = releaseFixture();
  context.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.writeFileSync(path.join(root, "CHANGELOG.md"), "# Changelog\n");
  const errors = validateReleaseFiles(root, "v0.2.0");
  assert.ok(errors.some((error) => error.includes("dated release heading")));
});

test("rejects release notes with the wrong identity", (context) => {
  const root = releaseFixture();
  context.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.writeFileSync(
    path.join(root, "docs", "releases", "v0.2.0.md"),
    "# Hegira v0.1.2\n",
  );
  const errors = validateReleaseFiles(root, "v0.2.0");
  assert.ok(errors.some((error) => error.includes("# Hegira v0.2.0")));
});

test("rejects missing versioned release notes", (context) => {
  const root = releaseFixture();
  context.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.rmSync(path.join(root, "docs", "releases", "v0.2.0.md"));
  const errors = validateReleaseFiles(root, "v0.2.0");
  assert.ok(errors.some((error) => error.includes("missing or empty")));
});

test("accepts the source-first release workflow contract", () => {
  assert.deepEqual(validateReleaseWorkflow(validWorkflow), []);
});

test("rejects obsolete Linux bundle publication", () => {
  const errors = validateReleaseWorkflow(
    `${validWorkflow}\narchive: hegira-v0.2.0-linux-x86_64.tar.gz\n`,
  );
  assert.ok(errors.some((error) => error.includes("Linux application bundle")));
});

test("rejects a missing production container gate", () => {
  const errors = validateReleaseWorkflow(
    validWorkflow.replace("sh scripts/container-smoke.sh", "true"),
  );
  assert.ok(
    errors.some((error) => error.includes("production container validation")),
  );
});

test("rejects publication outside tag push events", () => {
  const errors = validateReleaseWorkflow(
    validWorkflow.replace(
      "if: github.event_name == 'push'",
      "if: github.event_name == 'workflow_dispatch'",
    ),
  );
  assert.ok(
    errors.some((error) => error.includes("restricted to tag push events")),
  );
});
