import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  currentReleaseRef,
  validateCanonicalApplicationLock,
  validateReleaseCandidateMetadata,
  validateReleaseFiles,
  validateReleaseMetadata,
  validateReleaseRepository,
  validateReleaseWorkflow,
} from "./release-policy.mjs";

function metadata(overrides = {}) {
  const framework = {
    id: "runtime 0.2.0 (path+file:///workspace/crates/runtime)",
    name: "runtime",
    version: "0.2.0",
    publish: [],
  };
  const domain = {
    id: "domain 0.2.0 (path+file:///workspace/crates/domain)",
    name: "domain",
    version: "0.2.0",
    publish: [],
  };
  return {
    workspace_members: [framework.id, domain.id],
    packages: [framework, domain],
    ...overrides,
  };
}

function releaseFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hegira-release-"));
  fs.mkdirSync(path.join(root, "docs", "releases"), { recursive: true });
  fs.mkdirSync(path.join(root, "templates", "applications", "layered"), {
    recursive: true,
  });
  fs.writeFileSync(
    path.join(root, "CHANGELOG.md"),
    "# Changelog\n\n## [0.2.0] - 2026-07-24\n",
  );
  fs.writeFileSync(
    path.join(root, "docs", "releases", "v0.2.0.md"),
    "# Hegira v0.2.0\n",
  );
  fs.writeFileSync(
    path.join(root, "templates", "package.toml"),
    '[framework]\nrepository = "https://github.com/example/hegira.git"\nversion = "v0.2.0"\n',
  );
  fs.writeFileSync(
    path.join(root, "templates", "applications", "layered", "Cargo.lock"),
    '[[package]]\nname = "runtime"\nversion = "0.2.0"\nsource = "git+https://github.com/example/hegira.git?tag=v0.2.0#0123456789abcdef0123456789abcdef01234567"\n',
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
  framework:
    steps:
      - run: sh scripts/framework-check.sh
  official-modules:
    steps:
      - run: sh scripts/official-modules-check.sh
  tooling:
    steps:
      - run: sh scripts/layered-template-check.sh
      - run: sh scripts/cli-check.sh
  generated-application:
    name: generated-application (\${{ matrix.lifecycle }})
    strategy:
      fail-fast: false
      matrix:
        include:
          - lifecycle: default
            cache_name: generated-application-check
          - lifecycle: identity-added
            cache_name: identity-added-application-check
    steps:
      - id: source-identity
        run: echo "tree=$(git rev-parse 'HEAD^{tree}')"
      - id: generated-cache
        with:
          workspaces: . -> target/validation/build/\${{ matrix.cache_name }}
          cache-on-failure: false
          key: generated-\${{ matrix.lifecycle }}-targets-native-wasm32-profiles-dev-test-release-providers-sqlite-postgres-features-ssr-db-sqlite-db-postgres-hydrate-lock-\${{ hashFiles('Cargo.lock', 'templates/applications/layered/Cargo.lock') }}-source-\${{ steps.source-identity.outputs.tree }}
      - run: echo "\${{ steps.generated-cache.outputs.cache-hit }}"
      - run: sh scripts/generated-application-check.sh "\${{ matrix.lifecycle }}"
  publish:
    if: github.event_name == 'push'
    needs:
      - validate
      - framework
      - official-modules
      - tooling
      - generated-application
    permissions:
      contents: write
    steps:
      - run: >-
          gh release create
          "dist/hegira-$RELEASE_REF.spdx.json#Source SPDX SBOM"
          --verify-tag --title "$RELEASE_REF"
`;

test("accepts consistent workspace versions", () => {
  assert.deepEqual(validateReleaseMetadata(metadata(), "v0.2.0"), []);
});

test("derives the release ref without a compatibility host package", () => {
  assert.equal(currentReleaseRef(metadata()), "v0.2.0");
});

test("accepts a newer issue-bound release candidate", () => {
  const fixture = metadata({
    metadata: {
      hegira: {
        release_candidate: {
          base: "v0.1.2",
          issue: 307,
          target: "v0.2.0",
        },
      },
    },
  });
  assert.deepEqual(validateReleaseCandidateMetadata(fixture, "v0.2.0"), []);
});

test("validates base release files during a candidate transition", (context) => {
  const root = releaseFixture();
  context.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.mkdirSync(path.join(root, ".github", "workflows"), { recursive: true });
  fs.writeFileSync(
    path.join(root, ".github", "workflows", "release.yml"),
    validWorkflow,
  );
  const fixture = metadata({
    metadata: {
      hegira: {
        release_candidate: {
          base: "v0.2.0",
          issue: 307,
          target: "v0.3.0",
        },
      },
    },
  });
  fixture.packages[0].version = "0.3.0";
  assert.deepEqual(
    validateReleaseRepository(root, "v0.3.0", fixture, {
      allowCandidate: true,
    }),
    [],
  );
});

test("rejects candidate versions outside canonical release-source packages", (context) => {
  const root = releaseFixture();
  context.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.mkdirSync(path.join(root, ".github", "workflows"), { recursive: true });
  fs.writeFileSync(
    path.join(root, ".github", "workflows", "release.yml"),
    validWorkflow,
  );
  const fixture = metadata({
    metadata: {
      hegira: {
        release_candidate: {
          base: "v0.2.0",
          issue: 307,
          target: "v0.3.0",
        },
      },
    },
  });
  for (const packageMetadata of fixture.packages) {
    packageMetadata.version = "0.3.0";
  }
  const errors = validateReleaseRepository(root, "v0.3.0", fixture, {
    allowCandidate: true,
  });
  assert.ok(
    errors.some(
      (error) =>
        error.includes("repository-only package version mismatch") &&
        error.includes("domain"),
    ),
  );
});

test("rejects malformed and non-increasing release candidates", () => {
  const fixture = metadata({
    metadata: {
      hegira: {
        release_candidate: {
          base: "v0.2.0",
          issue: 0,
          target: "v0.2.0",
          bypass: true,
        },
      },
    },
  });
  const errors = validateReleaseCandidateMetadata(fixture, "v0.2.0");
  assert.ok(
    errors.some((error) => error.includes("only base, issue, and target")),
  );
  assert.ok(errors.some((error) => error.includes("must be newer")));
  assert.ok(errors.some((error) => error.includes("positive integer")));
});

test("explicit release validation rejects candidate metadata", (context) => {
  const root = releaseFixture();
  context.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const fixture = metadata({
    metadata: {
      hegira: {
        release_candidate: {
          base: "v0.1.2",
          issue: 307,
          target: "v0.2.0",
        },
      },
    },
  });
  const errors = validateReleaseRepository(root, "v0.2.0", fixture);
  assert.ok(
    errors.some((error) => error.includes("must be removed before explicit")),
  );
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

test("rejects a publishable workspace package", () => {
  const fixture = metadata();
  fixture.packages[1].publish = null;
  const errors = validateReleaseMetadata(fixture, "v0.2.0");
  assert.ok(
    errors.some((error) =>
      error.includes("registry publication is not disabled"),
    ),
  );
});

test("accepts matching changelog and versioned release notes", (context) => {
  const root = releaseFixture();
  context.after(() => fs.rmSync(root, { recursive: true, force: true }));
  assert.deepEqual(validateReleaseFiles(root, "v0.2.0"), []);
});

test("accepts a canonical application lock resolved from one release revision", (context) => {
  const root = releaseFixture();
  context.after(() => fs.rmSync(root, { recursive: true, force: true }));
  assert.deepEqual(validateCanonicalApplicationLock(root, "v0.2.0"), []);
});

test("rejects a missing canonical application lock", (context) => {
  const root = releaseFixture();
  context.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.rmSync(path.join(root, "templates", "applications", "layered", "Cargo.lock"));
  const errors = validateCanonicalApplicationLock(root, "v0.2.0");
  assert.ok(errors.some((error) => error.includes("lockfile is missing")));
});

test("rejects a canonical application lock resolved outside the release tag", (context) => {
  const root = releaseFixture();
  context.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const lockPath = path.join(root, "templates", "applications", "layered", "Cargo.lock");
  fs.writeFileSync(
    lockPath,
    fs.readFileSync(lockPath, "utf8").replace("?tag=v0.2.0", "?branch=main"),
  );
  const errors = validateCanonicalApplicationLock(root, "v0.2.0");
  assert.ok(errors.some((error) => error.includes("outside the release tag")));
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

test("accepts the source-only framework release workflow contract", () => {
  assert.deepEqual(validateReleaseWorkflow(validWorkflow), []);
});

test("rejects a prefixed GitHub Release title", () => {
  const errors = validateReleaseWorkflow(
    validWorkflow.replace(
      '--title "$RELEASE_REF"',
      '--title "Hegira $RELEASE_REF"',
    ),
  );
  assert.ok(
    errors.some((error) =>
      error.includes("canonical SemVer release title"),
    ),
  );
});

test("rejects a missing source SBOM release asset", () => {
  const errors = validateReleaseWorkflow(
    validWorkflow.replace(
      '"dist/hegira-$RELEASE_REF.spdx.json#Source SPDX SBOM"',
      '"dist/unrelated.txt"',
    ),
  );
  assert.ok(errors.some((error) => error.includes("source SBOM release asset")));
});

test("rejects obsolete Linux bundle publication", () => {
  const errors = validateReleaseWorkflow(
    `${validWorkflow}\narchive: hegira-v0.2.0-linux-x86_64.tar.gz\n`,
  );
  assert.ok(errors.some((error) => error.includes("Linux application bundle")));
});

test("rejects a missing generated application gate", () => {
  const errors = validateReleaseWorkflow(
    validWorkflow.replace("sh scripts/generated-application-check.sh", "true"),
  );
  assert.ok(
    errors.some((error) => error.includes("matrix lifecycle validation")),
  );
});

test("rejects a missing component lifecycle gate", () => {
  const errors = validateReleaseWorkflow(
    validWorkflow.replace(
      "- lifecycle: identity-added",
      "- lifecycle: unrelated",
    ),
  );
  assert.ok(
    errors.some((error) => error.includes("Identity-added lifecycle matrix entry")),
  );
});

test("rejects a separate component lifecycle release job", () => {
  const errors = validateReleaseWorkflow(
    `${validWorkflow}\n  component-lifecycle:\n    steps:\n      - run: true\n`,
  );
  assert.ok(
    errors.some((error) => error.includes("duplicate component lifecycle job")),
  );
});

test("rejects a missing CLI validation gate", () => {
  const errors = validateReleaseWorkflow(
    validWorkflow.replace("sh scripts/cli-check.sh", "true"),
  );
  assert.ok(errors.some((error) => error.includes("CLI validation")));
});

test("rejects compatibility-host release validation", () => {
  const errors = validateReleaseWorkflow(
    `${validWorkflow}\nrun: sh scripts/full-stack-build-check.sh\n`,
  );
  assert.ok(
    errors.some((error) =>
      error.includes("compatibility-host full-stack validation"),
    ),
  );
});

test("rejects compatibility-host package validation", () => {
  const errors = validateReleaseWorkflow(
    `${validWorkflow}\nrun: cargo check -p hegira\n`,
  );
  assert.ok(
    errors.some((error) =>
      error.includes("compatibility-host package validation"),
    ),
  );
});

test("rejects Cargo registry publication", () => {
  const errors = validateReleaseWorkflow(
    `${validWorkflow}\nrun: cargo publish --workspace\n`,
  );
  assert.ok(
    errors.some((error) => error.includes("Cargo registry publication")),
  );
});

test("rejects registry write permission", () => {
  const errors = validateReleaseWorkflow(
    `${validWorkflow}\npermissions:\n  packages: write\n`,
  );
  assert.ok(errors.some((error) => error.includes("registry write permission")));
});

test("rejects publication that bypasses a repository ownership gate", () => {
  const errors = validateReleaseWorkflow(
    validWorkflow.replace("      - generated-application\n", ""),
  );
  assert.ok(
    errors.some((error) =>
      error.includes("validation dependency: generated-application"),
    ),
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
