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
  ["stable quality context", "  quality:"],
  ["stable supply-chain context", "  supply-chain:"],
  ["framework validation", "sh scripts/framework-check.sh"],
  ["official module validation", "sh scripts/official-modules-check.sh"],
  ["template validation", "sh scripts/layered-template-check.sh"],
  ["generated application validation", "sh scripts/generated-application-check.sh"],
  ["explicit disposable PostgreSQL authentication", "POSTGRES_HOST_AUTH_METHOD: trust"],
  ["dependency policy", "EmbarkStudios/cargo-deny-action@v2"],
  ["dependency audit", "cargo audit --file Cargo.lock"],
];

const QUALITY_DEPENDENCIES = [
  "framework",
  "official-modules",
  "templates",
  "generated-application",
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
  if (!/pull_request:\s*\n    branches:\s*\n      - develop\s*\n      - main/m.test(workflow)) {
    errors.push("repository validation must run for pull requests to develop and main");
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
  for (const dependency of QUALITY_DEPENDENCIES) {
    if (!qualityJob.includes(`- ${dependency}`)) {
      errors.push(`quality is missing ownership dependency: ${dependency}`);
    }
  }

  return errors;
}

export function validateCIRepository(root) {
  const errors = [];
  const workflowPath = path.join(root, ".github", "workflows", "backend.yml");
  if (!fs.existsSync(workflowPath)) {
    errors.push("repository validation workflow is missing: .github/workflows/backend.yml");
  } else {
    errors.push(
      ...validateRepositoryValidationWorkflow(
        fs.readFileSync(workflowPath, "utf8"),
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
