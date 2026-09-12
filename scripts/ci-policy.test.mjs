import assert from "node:assert/strict";
import test from "node:test";

import {
  validateGeneratedApplicationScript,
  validateRepositoryValidationWorkflow,
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
      - run: sh scripts/layered-template-check.sh
      - run: sh scripts/cli-check.sh
  generated-application:
    steps:
      - run: sh scripts/generated-application-check.sh
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
      - uses: EmbarkStudios/cargo-deny-action@v2
      - run: cargo audit --file Cargo.lock
`;

const validGeneratedApplicationScript = `#!/usr/bin/env sh
set -eu
cargo run --locked --quiet -p hegira_cli -- new sqlite-application
cargo run --locked --quiet -p hegira_cli -- new postgres-application
development_root="$staging_parent/sqlite-development-validation"
APP_ENV=sqlite cargo leptos build -p app_server \
  --bin-features ssr,db-sqlite --lib-features hydrate
for database in sqlite postgres; do
  renderer --generated-source "$staging_parent/$database-source"
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
