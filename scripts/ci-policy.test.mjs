import assert from "node:assert/strict";
import test from "node:test";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

import {
  validateGeneratedApplicationScript,
  validateRepositoryValidationWorkflow,
  validateUpgradeScripts,
} from "./ci-policy.mjs";

const validWorkflow = `name: repository-validation

on:
  push:
    branches:
      - develop
      - main
  pull_request:
    branches:
      - develop
      - main

concurrency:
  group: repository-validation-\${{ github.event.pull_request.number || github.ref }}
  cancel-in-progress: true

permissions:
  contents: read

jobs:
  feature-matrix:
    name: feature-matrix (\${{ matrix.name }})
    strategy:
      matrix:
        include:
          - name: sqlite-server
          - name: postgres-server
          - name: wasm-hydrate
          - name: observability
          - name: distributed-providers
    steps:
      - run: sh scripts/generated-feature-check.sh
  framework:
    steps:
      - run: sh scripts/framework-check.sh
  official-modules:
    services:
      postgres:
        env:
          POSTGRES_HOST_AUTH_METHOD: trust
    steps:
      - run: sh scripts/official-modules-check.sh
  tooling:
    steps:
      - uses: taiki-e/install-action@v2
        with:
          tool: cargo-leptos@0.3.7
      - uses: actions/setup-node@v7
        with:
          node-version-file: .node-version
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
          - lifecycle: upgrade-default
            cache_name: upgraded-application-default
          - lifecycle: upgrade-minimal
            cache_name: upgraded-application-minimal
          - lifecycle: upgrade-identity-added
            cache_name: upgraded-application-identity-added
    steps:
      - id: source-identity
        run: echo "tree=$(git rev-parse 'HEAD^{tree}')"
      - id: generated-cache
        uses: Swatinem/rust-cache@v2
        with:
          workspaces: . -> target/validation/build/\${{ matrix.cache_name }}
          cache-on-failure: false
          key: generated-\${{ matrix.lifecycle }}-targets-native-wasm32-profiles-dev-test-release-providers-sqlite-postgres-features-ssr-db-sqlite-db-postgres-hydrate-lock-\${{ hashFiles('Cargo.lock', 'templates/applications/layered/Cargo.lock') }}-source-\${{ steps.source-identity.outputs.tree }}
      - run: echo "\${{ steps.generated-cache.outputs.cache-hit }}"
      - run: sh scripts/generated-application-check.sh "\${{ matrix.lifecycle }}"
  quality:
    if: always()
    needs:
      - framework
      - official-modules
      - tooling
      - generated-application
    steps:
      - env:
          FRAMEWORK_RESULT: \${{ needs.framework.result }}
          MODULES_RESULT: \${{ needs.official-modules.result }}
          TOOLING_RESULT: \${{ needs.tooling.result }}
          GENERATED_APPLICATION_RESULT: \${{ needs.generated-application.result }}
        run: |
          test "$FRAMEWORK_RESULT" = success
          test "$MODULES_RESULT" = success
          test "$TOOLING_RESULT" = success
          test "$GENERATED_APPLICATION_RESULT" = success
  supply-chain:
    steps:
      - uses: actions/setup-node@v7
        with:
          node-version-file: .node-version
      - run: sh scripts/frontend-check.sh
      - uses: EmbarkStudios/cargo-deny-action@v2
      - run: cargo audit --file Cargo.lock
`;

const validGeneratedApplicationScript = `#!/usr/bin/env sh
set -eu
phase_begin "public application creation"
phase_begin "$database provider lifecycle"
echo "$GITHUB_STEP_SUMMARY"
echo "generated-application cache footprint"
default_http_port=38081
default_http_port=38082
default_postgres_port=35432
default_postgres_port=35433
GENERATED_APP_DB_PASSWORD="generated-$mode-ephemeral"
canonical_lock="$repo_root/templates/applications/layered/Cargo.lock"
generated_tool_bin=$(sh "$repo_root/scripts/generated-toolchain.sh" prepare "$canonical_lock" --container)
sh "$repo_root/scripts/generated-toolchain.sh" application "$staging_parent/sqlite-validation"
cargo run --locked --quiet -p hegira_cli -- new sqlite-application
cargo run --locked --quiet -p hegira_cli -- new postgres-application
test -f "$staging_parent/sqlite-source/Cargo.lock"
cmp "$staging_parent/sqlite-source/Cargo.lock" "$staging_parent/postgres-source/Cargo.lock"
development_root="$staging_parent/sqlite-development-validation"
APP_ENV=sqlite cargo leptos build -p app_server \
  --bin-features ssr,db-sqlite --lib-features hydrate \
  --bin-cargo-args=--locked --lib-cargo-args=--locked
for database in sqlite postgres; do
  renderer --generated-source "$source"
  renderer --identity-added-source "$source"
  hegira -- new minimal-application --composition minimal --database "$database"
  hegira -- component add identity
  stage_application "$staging_parent/$database-source"
  hegira -- generate resource --application-root "$validation_root" --dry-run --json
  hegira -- generate resource --application-root "$validation_root" --json
  test ! -e "$staging_parent/$database-source/$generated_resource_path"
  cargo test --locked --workspace
  cargo check --features hydrate
done
docker build --tag "$GENERATED_APP_IMAGE" "$generated_root"
curl "$base_url/readyz"
curl "$base_url/api/validation-records"
`;

test("accepts separated repository ownership gates", () => {
  assert.deepEqual(validateRepositoryValidationWorkflow(validWorkflow), []);
});

test("frontend audit cannot be removed, skipped or tolerated in supply-chain", () => {
  for (const replacement of ["true", "sh scripts/frontend-check.sh || true"]) {
    assert.ok(validateRepositoryValidationWorkflow(validWorkflow.replace(
      "sh scripts/frontend-check.sh", replacement,
    )).some(error => error.includes("frontend audit")));
  }
  for (const condition of ["    if: false\n", "    continue-on-error: true\n"]) {
    assert.ok(validateRepositoryValidationWorkflow(validWorkflow.replace(
      "  supply-chain:\n", `  supply-chain:\n${condition}`,
    )).some(error => error.includes("frontend audit")));
  }
});

for (const composition of ["default", "minimal", "identity-added"]) {
  test(`rejects a missing upgrade lifecycle or wrong cache: ${composition}`, () => {
    for (const replacement of ["unrelated", `upgrade-${composition}\n            cache_name: unrelated`]) {
      const errors = validateRepositoryValidationWorkflow(validWorkflow.replace(
        `upgrade-${composition}\n            cache_name: upgraded-application-${composition}`,
        replacement,
      ));
      assert.ok(errors.some(error => error.includes(`missing: ${composition}`)));
    }
  });
}

test("rejects skipped, tolerated, secret-bearing, or path-filtered upgrade validation", () => {
  for (const addition of ["    if: false\n", "    continue-on-error: true\n",
    "    env:\n      GH_TOKEN: token\n"]) {
    const errors = validateRepositoryValidationWorkflow(
      validWorkflow.replace("  generated-application:\n", `  generated-application:\n${addition}`),
    );
    assert.ok(errors.some(error => error.includes("upgrade lifecycle validation")));
  }
  assert.ok(validateRepositoryValidationWorkflow(validWorkflow.replace(
    "  pull_request:\n", "  pull_request:\n    paths: [docs/**]\n",
  )).some(error => error.includes("path filters")));
  assert.ok(validateRepositoryValidationWorkflow(validWorkflow.replace(
    "  generated-application:\n",
    "  generated-application:\n    strategy:\n      matrix:\n        exclude: []\n",
  )).some(error => error.includes("exclude required")));
});

test("the actual quality result fails on any unsuccessful lifecycle matrix", () => {
  const workflow = fs.readFileSync(new URL("../.github/workflows/backend.yml", import.meta.url), "utf8");
  const quality = workflow.match(/^  quality:\s*$([\s\S]*?)(?=^  [a-zA-Z0-9_-]+:\s*$)/m)[1];
  const commands = quality.match(/        run: \|\n((?:          .*\n)+)/)[1]
    .split("\n").map(line => line.slice(10)).join("\n");
  const env = { ...process.env, FRAMEWORK_RESULT: "success", MODULES_RESULT: "success",
    TOOLING_RESULT: "success", GENERATED_APPLICATION_RESULT: "success" };
  assert.equal(spawnSync("sh", ["-eu", "-c", commands], { env }).status, 0);
  for (const status of ["failure", "cancelled", "skipped", ""]) {
    assert.notEqual(spawnSync("sh", ["-eu", "-c", commands], {
      env: { ...env, GENERATED_APPLICATION_RESULT: status },
    }).status, 0);
  }
});

test("requires upgrade source, dispatcher, and focused owner commands", t => {
  const repository = fileURLToPath(new URL("..", import.meta.url));
  assert.deepEqual(validateUpgradeScripts(repository), []);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hegira-upgrade-policy-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.cpSync(path.join(repository, "scripts"), path.join(root, "scripts"), { recursive: true });
  fs.cpSync(path.join(repository, "test-fixtures/upgrade-lifecycle"),
    path.join(root, "test-fixtures/upgrade-lifecycle"), { recursive: true });
  assert.deepEqual(validateUpgradeScripts(root), []);
  for (const [file, contract] of [
    ["scripts/generated-application-check.sh", 'exec sh "$repo_root/scripts/upgraded-application-check.sh"'],
    ["scripts/framework-check.sh", "cargo test --locked -p application_manifest"],
    ["scripts/layered-template-check.sh", "cargo test --locked -p template_renderer"],
    ["scripts/cli-check.sh", "cargo test --locked -p application_mutator"],
    ["scripts/cli-check.sh", "cargo test --locked -p hegira_cli"],
    ["scripts/upgraded-application-check.sh", "for database in sqlite postgres; do"],
    ["scripts/upgraded-application-check.sh", "validation_cache_release"],
  ]) {
    const location = path.join(root, file);
    const source = fs.readFileSync(location, "utf8");
    fs.writeFileSync(location, source.replace(contract, "true"));
    assert.ok(validateUpgradeScripts(root).length > 0, file);
    fs.writeFileSync(location, source);
  }
  fs.rmSync(path.join(root, "scripts/upgraded-application-http.mjs"));
  assert.ok(validateUpgradeScripts(root).some(error => error.includes("source is missing")));
});

test("rejects a filtered focused upgrade suite", t => {
  const repository = fileURLToPath(new URL("..", import.meta.url));
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hegira-upgrade-filter-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.cpSync(path.join(repository, "scripts"), path.join(root, "scripts"), { recursive: true });
  fs.cpSync(path.join(repository, "test-fixtures/upgrade-lifecycle"),
    path.join(root, "test-fixtures/upgrade-lifecycle"), { recursive: true });
  const file = path.join(root, "scripts/cli-check.sh");
  fs.writeFileSync(file, fs.readFileSync(file, "utf8").replace(
    "cargo test --locked -p hegira_cli\n",
    "cargo test --locked -p hegira_cli -- --skip upgrade\n",
  ));
  assert.ok(validateUpgradeScripts(root).some(error => error.includes("contract missing")));
});

test("rejects generated application validation outside the quality gate", () => {
  const workflow = validWorkflow.replace(
    "      - generated-application\n",
    "",
  );
  const errors = validateRepositoryValidationWorkflow(workflow);
  assert.ok(
    errors.some((error) =>
      error.includes("quality is missing ownership dependency: generated-application"),
    ),
  );
});

test("rejects a missing component lifecycle gate", () => {
  const errors = validateRepositoryValidationWorkflow(
    validWorkflow.replace(
      "- lifecycle: identity-added",
      "- lifecycle: unrelated",
    ),
  );
  assert.ok(
    errors.some((error) => error.includes("Identity-added lifecycle matrix entry")),
  );
});

test("rejects a lifecycle cache without immutable source identity", () => {
  const errors = validateRepositoryValidationWorkflow(
    validWorkflow.replace("git rev-parse 'HEAD^{tree}'", "git rev-parse HEAD"),
  );
  assert.ok(
    errors.some((error) => error.includes("immutable source tree identity")),
  );
});

test("rejects a cache key not bound to the immutable source identity", () => {
  const errors = validateRepositoryValidationWorkflow(
    validWorkflow.replace(
      "-source-\${{ steps.source-identity.outputs.tree }}",
      "-source-unbound",
    ),
  );
  assert.ok(
    errors.some((error) => error.includes("source-bound cache identity")),
  );
});

test("rejects a separate component lifecycle status context", () => {
  const errors = validateRepositoryValidationWorkflow(
    `${validWorkflow}\n  component-lifecycle:\n    steps:\n      - run: true\n`,
  );
  assert.ok(
    errors.some((error) => error.includes("existing generated-application job")),
  );
});

test("rejects a quality gate that ignores generated application failure", () => {
  const errors = validateRepositoryValidationWorkflow(
    validWorkflow.replace(
      'test "$GENERATED_APPLICATION_RESULT" = success',
      "true",
    ),
  );
  assert.ok(
    errors.some((error) =>
      error.includes("quality does not require ownership success: generated-application"),
    ),
  );
});

test("accepts the generated and mutated application contract", () => {
  assert.deepEqual(
    validateGeneratedApplicationScript(validGeneratedApplicationScript),
    [],
  );
});

test("rejects lifecycle validation without the installed Identity source", () => {
  const errors = validateGeneratedApplicationScript(
    validGeneratedApplicationScript.replace('--identity-added-source "$source"', ""),
  );
  assert.ok(
    errors.some((error) => error.includes("installed Identity CLI source")),
  );
});

test("rejects a missing documented development build", () => {
  const errors = validateGeneratedApplicationScript(
    validGeneratedApplicationScript.replace(
      "APP_ENV=sqlite cargo leptos build -p app_server",
      "true",
    ),
  );
  assert.ok(
    errors.some((error) => error.includes("documented development build")),
  );
});

test("rejects replacing the development build with a release build", () => {
  const errors = validateGeneratedApplicationScript(
    validGeneratedApplicationScript.replace(
      "cargo leptos build -p app_server",
      "cargo leptos build --release -p app_server",
    ),
  );
  assert.ok(
    errors.some((error) => error.includes("non-release development build")),
  );
});

test("rejects generated applications without the canonical Cargo lock", () => {
  const errors = validateGeneratedApplicationScript(
    validGeneratedApplicationScript.replace(
      'test -f "$staging_parent/sqlite-source/Cargo.lock"',
      "true",
    ),
  );
  assert.ok(
    errors.some((error) => error.includes("canonical application lockfile")),
  );
});

test("rejects generated applications without byte-identical provider locks", () => {
  const errors = validateGeneratedApplicationScript(
    validGeneratedApplicationScript.replace(
      'cmp "$staging_parent/sqlite-source/Cargo.lock" "$staging_parent/postgres-source/Cargo.lock"',
      "true",
    ),
  );
  assert.ok(
    errors.some((error) => error.includes("byte-identical provider lockfiles")),
  );
});

test("rejects unlocked Cargo Leptos builds", () => {
  const errors = validateGeneratedApplicationScript(
    validGeneratedApplicationScript.replace(
      "--bin-cargo-args=--locked --lib-cargo-args=--locked",
      "",
    ),
  );
  assert.ok(
    errors.some((error) => error.includes("locked Cargo Leptos")),
  );
});

test("rejects a generated application gate without public resource mutation", () => {
  const errors = validateGeneratedApplicationScript(
    validGeneratedApplicationScript.replaceAll("-- generate resource", "-- inspect"),
  );
  assert.ok(
    errors.some((error) => error.includes("public resource mutation")),
  );
});

test("rejects repository secrets in generated application validation", () => {
  const errors = validateGeneratedApplicationScript(
    `${validGeneratedApplicationScript}\nTOKEN=\${{ secrets.PRODUCTION_TOKEN }}\n`,
  );
  assert.ok(
    errors.some((error) => error.includes("may not consume repository secrets")),
  );
});

test("rejects write permission in untrusted repository validation", () => {
  const errors = validateRepositoryValidationWorkflow(
    validWorkflow.replace("contents: read", "contents: write"),
  );
  assert.ok(errors.some((error) => error.includes("read-only contents")));
  assert.ok(errors.some((error) => error.includes("contents: write")));
});

test("rejects an embedded disposable PostgreSQL password", () => {
  const errors = validateRepositoryValidationWorkflow(
    `${validWorkflow}\nPOSTGRES_PASSWORD: scanner-trigger\n`,
  );
  assert.ok(errors.some((error) => error.includes("PostgreSQL passwords")));
});

test("rejects pull_request_target execution", () => {
  const errors = validateRepositoryValidationWorkflow(
    validWorkflow.replace("pull_request:", "pull_request_target:"),
  );
  assert.ok(errors.some((error) => error.includes("pull_request_target")));
});

test("rejects a missing tooling gate", () => {
  const errors = validateRepositoryValidationWorkflow(
    validWorkflow.replace("sh scripts/layered-template-check.sh", "true"),
  );
  assert.ok(errors.some((error) => error.includes("tooling validation")));
});

test("rejects a missing CLI gate", () => {
  const errors = validateRepositoryValidationWorkflow(
    validWorkflow.replace("sh scripts/cli-check.sh", "true"),
  );
  assert.ok(errors.some((error) => error.includes("CLI validation")));
});

test("rejects compatibility-host feature validation", () => {
  const errors = validateRepositoryValidationWorkflow(
    validWorkflow.replace(
      "sh scripts/generated-feature-check.sh",
      "cargo check --package hegira",
    ),
  );
  assert.ok(
    errors.some((error) => error.includes("compatibility host")),
  );
});

test("rejects unrestricted feature-branch pushes", () => {
  const errors = validateRepositoryValidationWorkflow(
    validWorkflow.replace(
      "  push:\n    branches:\n      - develop\n      - main",
      "  push:",
    ),
  );
  assert.ok(
    errors.some((error) => error.includes("restricted to develop and main")),
  );
});

test("rejects disabled concurrency cancellation", () => {
  const errors = validateRepositoryValidationWorkflow(
    validWorkflow.replace("cancel-in-progress: true", "cancel-in-progress: false"),
  );
  assert.ok(
    errors.some((error) => error.includes("cancel superseded")),
  );
});
