import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { spawnSync } from "node:child_process";
import { pathToFileURL } from "node:url";

const STABLE_RELEASE_REF =
  /^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/;

const REQUIRED_WORKFLOW_CONTRACTS = [
  ["release identity validation", "scripts/release-policy.sh"],
  ["framework validation", "scripts/framework-check.sh"],
  ["official module validation", "scripts/official-modules-check.sh"],
  ["tooling validation", "scripts/layered-template-check.sh"],
  ["CLI validation", "scripts/cli-check.sh"],
  ["generated application validation", "scripts/generated-application-check.sh"],
  ["source SBOM generation", "anchore/sbom-action@v0"],
  ["disabled implicit SBOM publication", "upload-release-assets: false"],
  [
    "source SBOM release asset",
    '"dist/hegira-$RELEASE_REF.spdx.json#Source SPDX SBOM"',
  ],
  ["existing-tag verification", "--verify-tag"],
  ["GitHub Release publication", "gh release create"],
  ["canonical SemVer release title", '--title "$RELEASE_REF"'],
];

const OBSOLETE_WORKFLOW_CONTRACTS = [
  ["Linux application bundle", "linux-x86_64"],
  ["obsolete bundle script", "release-bundle.sh"],
  ["bundle checksum", "sha256sum"],
  ["updatable release action", "softprops/action-gh-release"],
  ["compatibility-host full-stack validation", "scripts/full-stack-build-check.sh"],
  ["compatibility-host container validation", "scripts/container-smoke.sh"],
  ["compatibility-host package validation", "-p hegira"],
  ["compatibility-host package validation", "--package hegira"],
  ["Cargo registry publication", "cargo publish"],
  ["Cargo registry credential", "CARGO_REGISTRY_TOKEN"],
  ["crates.io publication", "crates.io"],
  ["registry write permission", "packages: write"],
  ["container registry publication", "docker push"],
];

function workspacePackages(metadata) {
  const workspaceMembers = new Set(metadata.workspace_members ?? []);
  return (metadata.packages ?? []).filter((packageMetadata) =>
    workspaceMembers.has(packageMetadata.id),
  );
}

function releaseVersion(releaseRef) {
  const match = releaseRef.match(STABLE_RELEASE_REF);
  return match === null ? null : releaseRef.slice(1);
}

function readWorkspaceMetadata(root) {
  const result = spawnSync(
    "cargo",
    ["metadata", "--locked", "--no-deps", "--format-version", "1"],
    {
      cwd: root,
      encoding: "utf8",
      maxBuffer: 32 * 1024 * 1024,
      stdio: ["ignore", "pipe", "pipe"],
    },
  );

  if (result.error) {
    throw new Error(`failed to run cargo metadata: ${result.error.message}`);
  }
  if (result.status !== 0) {
    throw new Error(
      `cargo metadata failed with exit code ${result.status}:\n${result.stderr.trim()}`,
    );
  }

  try {
    return JSON.parse(result.stdout);
  } catch (error) {
    throw new Error(`cargo metadata returned invalid JSON: ${error.message}`);
  }
}

export function currentReleaseRef(metadata) {
  const packages = workspacePackages(metadata);
  if (packages.length === 0) {
    throw new Error("Cargo metadata contains no workspace packages");
  }
  const versions = new Set(
    packages.map((packageMetadata) => packageMetadata.version),
  );
  if (versions.size !== 1) {
    throw new Error(
      "workspace package versions must agree before deriving the release ref",
    );
  }
  return `v${packages[0].version}`;
}

export function validateReleaseMetadata(metadata, releaseRef) {
  const errors = [];
  const version = releaseVersion(releaseRef);
  if (version === null) {
    return [
      `release ref must match a stable SemVer tag: vMAJOR.MINOR.PATCH; received: ${releaseRef}`,
    ];
  }

  const packages = workspacePackages(metadata);
  if (packages.length === 0) {
    errors.push("Cargo metadata contains no workspace packages");
    return errors;
  }

  for (const packageMetadata of packages) {
    if (packageMetadata.version !== version) {
      errors.push(
        `workspace package version mismatch: ${packageMetadata.name} is ${packageMetadata.version}, expected ${version} for ${releaseRef}`,
      );
    }
    if (
      !Array.isArray(packageMetadata.publish) ||
      packageMetadata.publish.length !== 0
    ) {
      errors.push(
        `workspace package registry publication is not disabled: ${packageMetadata.name}`,
      );
    }
  }

  return errors;
}

export function validateReleaseFiles(root, releaseRef) {
  const errors = [];
  const version = releaseVersion(releaseRef);
  if (version === null) {
    return errors;
  }

  const changelogPath = path.join(root, "CHANGELOG.md");
  if (!fs.existsSync(changelogPath)) {
    errors.push("release changelog is missing: CHANGELOG.md");
  } else {
    const changelog = fs.readFileSync(changelogPath, "utf8");
    const escapedVersion = version.replaceAll(".", "\\.");
    const releaseHeading = new RegExp(
      `^## \\[${escapedVersion}\\] - [0-9]{4}-[0-9]{2}-[0-9]{2}$`,
      "m",
    );
    if (!releaseHeading.test(changelog)) {
      errors.push(
        `CHANGELOG.md has no dated release heading for version ${version}`,
      );
    }
  }

  const releaseNotesPath = path.join(
    root,
    "docs",
    "releases",
    `${releaseRef}.md`,
  );
  if (
    !fs.existsSync(releaseNotesPath) ||
    !fs.statSync(releaseNotesPath).isFile() ||
    fs.statSync(releaseNotesPath).size === 0
  ) {
    errors.push(
      `versioned release notes are missing or empty: docs/releases/${releaseRef}.md`,
    );
  } else {
    const firstContentLine = fs
      .readFileSync(releaseNotesPath, "utf8")
      .split("\n")
      .find((line) => line.trim() !== "");
    if (firstContentLine?.trim() !== `# Hegira ${releaseRef}`) {
      errors.push(
        `release notes must begin with: # Hegira ${releaseRef}`,
      );
    }
  }

  return errors;
}

export function validateReleaseWorkflow(workflow) {
  const errors = [];

  for (const [description, contract] of REQUIRED_WORKFLOW_CONTRACTS) {
    if (!workflow.includes(contract)) {
      errors.push(`release workflow is missing ${description}: ${contract}`);
    }
  }

  for (const [description, contract] of OBSOLETE_WORKFLOW_CONTRACTS) {
    if (workflow.includes(contract)) {
      errors.push(`release workflow retains ${description}: ${contract}`);
    }
  }

  if (
    !/publish:\s*\n(?:[ \t].*\n)*?[ \t]+if:\s*github\.event_name == 'push'/m.test(
      workflow,
    )
  ) {
    errors.push(
      "release publication must be restricted to tag push events",
    );
  }

  if (!/^permissions:\s*\n  contents: read\s*$/m.test(workflow)) {
    errors.push("release workflow must default to contents: read");
  }

  const publishStart = workflow.search(/^  publish:\s*$/m);
  if (publishStart !== -1) {
    const remainingWorkflow = workflow.slice(publishStart + 1);
    const nextJob = remainingWorkflow.search(/^  [a-zA-Z0-9_-]+:\s*$/m);
    const publishJob =
      nextJob === -1
        ? workflow.slice(publishStart)
        : workflow.slice(publishStart, publishStart + 1 + nextJob);
    const publishPreamble = publishJob.split(/^    steps:\s*$/m, 1)[0];
    for (const dependency of [
      "validate",
      "framework",
      "official-modules",
      "tooling",
      "generated-application",
    ]) {
      if (!publishPreamble.includes(`- ${dependency}`)) {
        errors.push(
          `release publication is missing validation dependency: ${dependency}`,
        );
      }
    }
  }

  const writePermissions = workflow.match(/contents: write/g) ?? [];
  if (
    writePermissions.length !== 1 ||
    !/publish:[\s\S]*?\n    permissions:\s*\n      contents: write\s*$/m.test(
      workflow,
    )
  ) {
    errors.push(
      "only the release publication job may receive contents: write",
    );
  }

  return errors;
}

export function validateReleaseRepository(root, releaseRef, metadata) {
  const workflowPath = path.join(root, ".github", "workflows", "release.yml");
  const errors = [
    ...validateReleaseMetadata(metadata, releaseRef),
    ...validateReleaseFiles(root, releaseRef),
  ];

  if (!fs.existsSync(workflowPath)) {
    errors.push("release workflow is missing: .github/workflows/release.yml");
  } else {
    errors.push(
      ...validateReleaseWorkflow(fs.readFileSync(workflowPath, "utf8")),
    );
  }

  return errors;
}

function optionValue(argumentsList, option) {
  const index = argumentsList.indexOf(option);
  if (index === -1 || index + 1 >= argumentsList.length) {
    return null;
  }
  return argumentsList[index + 1];
}

function runCli() {
  const args = process.argv.slice(2);
  if (args[0] !== "check") {
    throw new Error(
      "usage: node scripts/release-policy.mjs check --root <repository-root> [--release-ref <vX.Y.Z>]",
    );
  }

  const root = path.resolve(optionValue(args, "--root") ?? ".");
  const metadata = readWorkspaceMetadata(root);
  const releaseRef =
    optionValue(args, "--release-ref") ?? currentReleaseRef(metadata);
  const errors = validateReleaseRepository(root, releaseRef, metadata);

  if (errors.length > 0) {
    for (const error of errors) {
      process.stderr.write(`release policy violation: ${error}\n`);
    }
    process.exitCode = 1;
    return;
  }

  process.stdout.write(
    `release policy: ok (${releaseRef}, ${workspacePackages(metadata).length} packages)\n`,
  );
}

const invokedPath = process.argv[1]
  ? pathToFileURL(path.resolve(process.argv[1])).href
  : "";
if (import.meta.url === invokedPath) {
  runCli();
}
