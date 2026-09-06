import assert from "node:assert/strict";
import test from "node:test";

import { validateRepositoryValidationWorkflow } from "./ci-policy.mjs";

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
  supply-chain:
    steps:
      - uses: EmbarkStudios/cargo-deny-action@v2
      - run: cargo audit --file Cargo.lock
`;

test("accepts separated repository ownership gates", () => {
  assert.deepEqual(validateRepositoryValidationWorkflow(validWorkflow), []);
});

test("rejects generated application validation outside the quality gate", () => {
  const workflow = validWorkflow.replace(
    "      - generated-application\n  supply-chain:",
    "  supply-chain:",
  );
  const errors = validateRepositoryValidationWorkflow(workflow);
  assert.ok(
    errors.some((error) =>
      error.includes("quality is missing ownership dependency: generated-application"),
    ),
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
