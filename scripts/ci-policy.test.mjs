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
  framework:
    services:
      postgres:
        env:
          POSTGRES_HOST_AUTH_METHOD: trust
    steps:
      - run: sh scripts/framework-check.sh
  official-modules:
    steps:
      - run: sh scripts/official-modules-check.sh
  templates:
    steps:
      - run: sh scripts/layered-template-check.sh
  generated-application:
    steps:
      - run: sh scripts/generated-application-check.sh
  quality:
    if: always()
    needs:
      - framework
      - official-modules
      - templates
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

test("rejects a missing template gate", () => {
  const errors = validateRepositoryValidationWorkflow(
    validWorkflow.replace("sh scripts/layered-template-check.sh", "true"),
  );
  assert.ok(errors.some((error) => error.includes("template validation")));
});
