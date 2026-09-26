import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";

const REQUIRED_CONTRACTS = [
  ["stable feature-matrix context", "name: feature-matrix (${{ matrix.name }})"],
  ["SQLite capability context", "name: sqlite-server"],
  ["PostgreSQL capability context", "name: postgres-server"],
  ["hydration capability context", "name: wasm-hydrate"],
  ["observability capability context", "name: observability"],
  ["distributed-provider capability context", "name: distributed-providers"],
  ["generated feature consumer", "sh scripts/generated-feature-check.sh"],
  ["stable quality context", "  quality:"],
  ["stable supply-chain context", "  supply-chain:"],
  ["framework validation", "sh scripts/framework-check.sh"],
  ["official module validation", "sh scripts/official-modules-check.sh"],
  ["tooling validation", "sh scripts/layered-template-check.sh"],
  ["CLI validation", "sh scripts/cli-check.sh"],
  ["pinned Node selection", "node-version-file: .node-version"],
  ["pinned Cargo Leptos selection", "cargo-leptos@0.3.7"],
  [
    "parallel generated lifecycle context",
    "name: generated-application (${{ matrix.lifecycle }})",
  ],
  [
    "matrix lifecycle validation",
    'run: sh scripts/generated-application-check.sh "${{ matrix.lifecycle }}"',
  ],
  ["explicit disposable PostgreSQL authentication", "POSTGRES_HOST_AUTH_METHOD: trust"],
  ["dependency policy", "EmbarkStudios/cargo-deny-action@v2"],
  ["dependency audit", "cargo audit --file Cargo.lock"],
];

const QUALITY_DEPENDENCIES = [
  ["framework", "FRAMEWORK_RESULT"],
  ["official-modules", "MODULES_RESULT"],
  ["tooling", "TOOLING_RESULT"],
  ["generated-application", "GENERATED_APPLICATION_RESULT"],
];

const GENERATED_APPLICATION_CONTRACTS = [
  [
    "early generated tooling preparation",
    'generated_tool_bin=$(sh "$repo_root/scripts/generated-toolchain.sh" prepare',
  ],
  [
    "container tooling preflight",
    '"$canonical_lock" --container',
  ],
  [
    "resolved generated application tooling",
    'generated-toolchain.sh" application',
  ],
  ["public SQLite application creation", "-- new sqlite-application"],
  ["public PostgreSQL application creation", "-- new postgres-application"],
  ["canonical application lockfile", 'test -f "$staging_parent/sqlite-source/Cargo.lock"'],
  [
    "byte-identical provider lockfiles",
    'cmp "$staging_parent/sqlite-source/Cargo.lock" "$staging_parent/postgres-source/Cargo.lock"',
  ],
  ["both selected database profiles", "for database in sqlite postgres; do"],
  ["verified public CLI source", '--generated-source "$source"'],
  ["installed Identity CLI source", '--identity-added-source "$source"'],
  ["minimal application selection", '--composition minimal --database "$database"'],
  ["public Identity installation", "-- component add identity"],
  ["selected source staging", 'stage_application "$staging_parent/$database-source"'],
  ["public resource mutation", "-- generate resource"],
  ["resource dry-run", '--application-root "$validation_root" --dry-run --json'],
  ["resource apply", '--application-root "$validation_root" --json'],
  ["pristine public output check", 'test ! -e "$staging_parent/$database-source/$generated_resource_path"'],
  ["disposable development validation", 'development_root="$staging_parent/sqlite-development-validation"'],
  ["documented development build", "APP_ENV=sqlite cargo leptos build -p app_server"],
  ["documented development features", "--bin-features ssr,db-sqlite --lib-features hydrate"],
  ["locked Cargo Leptos server build", "--bin-cargo-args=--locked"],
  ["locked Cargo Leptos client build", "--lib-cargo-args=--locked"],
  ["locked generated workspace tests", "cargo test --locked --workspace"],
  ["generated hydration build", "--features hydrate"],
  ["production container build", 'docker build --tag "$GENERATED_APP_IMAGE" "$generated_root"'],
  ["production readiness probe", '"$base_url/readyz"'],
  ["generated resource HTTP contract", '"$base_url/api/validation-records"'],
  ["major phase timing", 'phase_begin "public application creation"'],
  ["provider phase timing", 'phase_begin "$database provider lifecycle"'],
  ["job summary timing output", '"$GITHUB_STEP_SUMMARY"'],
  ["bounded cache footprint output", "generated-application cache footprint"],
  ["default lifecycle HTTP port", "default_http_port=38081"],
  ["Identity-added lifecycle HTTP port", "default_http_port=38082"],
  ["default lifecycle PostgreSQL port", "default_postgres_port=35432"],
  ["Identity-added lifecycle PostgreSQL port", "default_postgres_port=35433"],
  ["lifecycle-bound runtime credentials", 'GENERATED_APP_DB_PASSWORD="generated-$mode-'],
];

const GENERATED_JOB_CONTRACTS = [
  ["non-cancelling lifecycle matrix", "fail-fast: false"],
  ["default lifecycle matrix entry", "- lifecycle: default"],
  ["Identity-added lifecycle matrix entry", "- lifecycle: identity-added"],
  ["default bounded cache identity", "cache_name: generated-application-check"],
  [
    "Identity-added bounded cache identity",
    "cache_name: identity-added-application-check",
  ],
  [
    "isolated bounded cache workspace",
    "target/validation/build/${{ matrix.cache_name }}",
  ],
  ["immutable source tree identity", "git rev-parse 'HEAD^{tree}'"],
  ["lifecycle-bound cache identity", "generated-${{ matrix.lifecycle }}"],
  [
    "source-bound cache identity",
    "source-${{ steps.source-identity.outputs.tree }}",
  ],
  ["lockfile cache identity", "hashFiles('Cargo.lock', 'templates/applications/layered/Cargo.lock')"],
  ["native and WASM target cache identity", "targets-native-wasm32"],
  ["development, test, and release profile cache identity", "profiles-dev-test-release"],
  ["SQLite and PostgreSQL cache identity", "providers-sqlite-postgres"],
  ["compiled feature cache identity", "features-ssr-db-sqlite-db-postgres-hydrate"],
  ["failure-safe cache publication", "cache-on-failure: false"],
  ["cache effectiveness diagnostic", "steps.generated-cache.outputs.cache-hit"],
];

const COMPATIBILITY_HOST_CONTRACTS = [
  "--package hegira",
  "-p hegira",
  "compatibility host",
  "host composition",
];

export function validateRepositoryValidationWorkflow(workflow) {
  const errors = [];

  if (!workflow.startsWith("name: repository-validation\n")) {
    errors.push("repository validation workflow has an unexpected identity");
  }
  if (!/^permissions:\s*\n  contents: read\s*$/m.test(workflow)) {
    errors.push("repository validation workflow must use read-only contents permission");
  }
  for (const permission of ["contents: write", "deployments: write", "id-token: write"]) {
    if (workflow.includes(permission)) {
      errors.push(`untrusted repository validation may not grant ${permission}`);
    }
  }
  if (workflow.includes("pull_request_target")) {
    errors.push("repository validation may not execute through pull_request_target");
  }
  if (workflow.includes("POSTGRES_PASSWORD")) {
    errors.push("disposable repository validation must not embed PostgreSQL passwords");
  }
  if (workflow.includes("\n  component-lifecycle:")) {
    errors.push(
      "component lifecycle validation must remain in the existing generated-application job",
    );
  }

  const generatedApplicationJob = workflow.match(
    /^  generated-application:\s*$([\s\S]*?)(?=^  [a-zA-Z0-9_-]+:\s*$|(?![\s\S]))/m,
  )?.[1] ?? "";
  for (const [description, contract] of GENERATED_JOB_CONTRACTS) {
    if (!generatedApplicationJob.includes(contract)) {
      errors.push(`generated application job is missing ${description}: ${contract}`);
    }
  }
  if (!/pull_request:\s*\n    branches:\s*\n      - develop\s*\n      - main/m.test(workflow)) {
    errors.push("repository validation must run for pull requests to develop and main");
  }
  if (!/push:\s*\n    branches:\s*\n      - develop\s*\n      - main/m.test(workflow)) {
    errors.push(
      "repository validation pushes must be restricted to develop and main",
    );
  }
  if (
    !/concurrency:\s*\n  group: repository-validation-.*\n  cancel-in-progress: true/m.test(
      workflow,
    )
  ) {
    errors.push("repository validation must cancel superseded branch and pull-request runs");
  }

  for (const contract of COMPATIBILITY_HOST_CONTRACTS) {
    if (workflow.includes(contract)) {
      errors.push(
        `required repository validation references the compatibility host: ${contract}`,
      );
    }
  }

  for (const [description, contract] of REQUIRED_CONTRACTS) {
    if (!workflow.includes(contract)) {
      errors.push(`repository validation is missing ${description}: ${contract}`);
    }
  }

  const qualityJob = workflow.match(
    /^  quality:\s*$([\s\S]*?)(?=^  [a-zA-Z0-9_-]+:\s*$|(?![\s\S]))/m,
  )?.[1] ?? "";
  if (!qualityJob.includes("if: always()")) {
    errors.push("quality must report even when an ownership gate fails");
  }
  for (const [dependency, resultVariable] of QUALITY_DEPENDENCIES) {
    if (!qualityJob.includes(`- ${dependency}`)) {
      errors.push(`quality is missing ownership dependency: ${dependency}`);
    }
    const resultExpression = "${{ needs." + dependency + ".result }}";
    if (!qualityJob.includes(`${resultVariable}: ${resultExpression}`)) {
      errors.push(`quality does not capture ownership result: ${dependency}`);
    }
    if (!qualityJob.includes(`test "$${resultVariable}" = success`)) {
      errors.push(`quality does not require ownership success: ${dependency}`);
    }
  }

  return errors;
}

export function validateGeneratedApplicationScript(script) {
  const errors = [];
  if (!script.startsWith("#!/usr/bin/env sh\nset -eu\n")) {
    errors.push("generated application validation must fail closed");
  }
  for (const [description, contract] of GENERATED_APPLICATION_CONTRACTS) {
    if (!script.includes(contract)) {
      errors.push(
        `generated application validation is missing ${description}: ${contract}`,
      );
    }
  }
  const developmentBuild = script.match(
    /APP_ENV=sqlite cargo leptos build[\s\S]*?--lib-features hydrate/,
  )?.[0];
  if (developmentBuild?.includes("--release")) {
    errors.push(
      "generated application validation must exercise the non-release development build",
    );
  }
  for (const secretReference of ["${{ secrets.", "GH_TOKEN", "GITHUB_TOKEN"]) {
    if (script.includes(secretReference)) {
      errors.push(
        `generated application validation may not consume repository secrets: ${secretReference}`,
      );
    }
  }
  return errors;
}

export function validateCIRepository(root) {
  const errors = [];
  const workflowPath = path.join(root, ".github", "workflows", "backend.yml");
  const generatedApplicationPath = path.join(
    root,
    "scripts",
    "generated-application-check.sh",
  );
  if (!fs.existsSync(workflowPath)) {
    errors.push("repository validation workflow is missing: .github/workflows/backend.yml");
  } else {
    errors.push(
      ...validateRepositoryValidationWorkflow(
        fs.readFileSync(workflowPath, "utf8"),
      ),
    );
  }

  if (!fs.existsSync(generatedApplicationPath)) {
    errors.push(
      "generated application validation is missing: scripts/generated-application-check.sh",
    );
  } else {
    errors.push(
      ...validateGeneratedApplicationScript(
        fs.readFileSync(generatedApplicationPath, "utf8"),
      ),
    );
  }

  for (const obsolete of ["full-stack-build.yml", "container-smoke.yml"]) {
    if (fs.existsSync(path.join(root, ".github", "workflows", obsolete))) {
      errors.push(`redundant pull-request workflow remains: ${obsolete}`);
    }
  }
  return errors;
}

function main() {
  if (process.argv.length !== 3) {
    throw new Error("usage: node scripts/ci-policy.mjs <repository-root>");
  }
  const errors = validateCIRepository(path.resolve(process.argv[2]));
  if (errors.length > 0) {
    for (const error of errors) {
      console.error(`ci policy: ${error}`);
    }
    process.exitCode = 1;
    return;
  }
  console.log("ci policy: ok");
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
