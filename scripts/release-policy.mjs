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
  [
    "generated application validation",
    "run: sh scripts/generated-application-check.sh\n",
  ],
  [
    "component lifecycle validation",
    "scripts/generated-application-check.sh identity-added",
  ],
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
  ["duplicate component lifecycle job", "\n  component-lifecycle:"],
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

function releaseCandidate(metadata) {
  return metadata.metadata?.hegira?.release_candidate ?? null;
}

function stableReleaseParts(releaseRef) {
  const match = releaseRef.match(STABLE_RELEASE_REF);
  return match === null ? null : match.slice(1).map(Number);
}

function compareStableReleases(left, right) {
  for (let index = 0; index < left.length; index += 1) {
    if (left[index] !== right[index]) {
      return left[index] - right[index];
    }
  }
  return 0;
}

export function validateReleaseCandidateMetadata(metadata, releaseRef) {
  const candidate = releaseCandidate(metadata);
  if (
    candidate === null ||
    typeof candidate !== "object" ||
    Array.isArray(candidate)
  ) {
    return ["release candidate metadata is missing or invalid"];
  }

  const errors = [];
  const keys = Object.keys(candidate).sort();
  if (keys.join(",") !== "base,issue,target") {
    errors.push(
      "release candidate metadata may contain only base, issue, and target",
    );
  }

  const baseParts =
    typeof candidate.base === "string"
      ? stableReleaseParts(candidate.base)
      : null;
  const targetParts =
    typeof candidate.target === "string"
      ? stableReleaseParts(candidate.target)
      : null;
  if (baseParts === null) {
    errors.push("release candidate base must be a stable SemVer tag");
  }
  if (targetParts === null) {
    errors.push("release candidate target must be a stable SemVer tag");
  } else if (candidate.target !== releaseRef) {
    errors.push("release candidate target must match the candidate release ref");
  }
  if (
    baseParts !== null &&
    targetParts !== null &&
    compareStableReleases(baseParts, targetParts) >= 0
  ) {
    errors.push("release candidate version must be newer than its base");
  }

  if (!Number.isSafeInteger(candidate.issue) || candidate.issue <= 0) {
    errors.push("release candidate issue must be a positive integer");
  }

  return errors;
}

function canonicalReleasePackageNames(root, releaseRef) {
  const packagePath = path.join(root, "templates", "package.toml");
  const lockPath = path.join(
    root,
    "templates",
    "applications",
    "layered",
    "Cargo.lock",
  );
  if (!fs.existsSync(packagePath) || !fs.existsSync(lockPath)) {
    return new Set();
  }

  const packageManifest = fs.readFileSync(packagePath, "utf8");
  const frameworkSection =
    packageManifest.match(/(?:^|\n)\[framework\]\n([\s\S]*?)(?=\n\[|$)/)?.[1] ??
    "";
  const repository = frameworkSection.match(/^repository = "([^"]+)"$/m)?.[1];
  if (!repository) {
    return new Set();
  }

  const lockfile = fs.readFileSync(lockPath, "utf8");
  const source = `source = "git+${repository}?tag=${releaseRef}#`;
  const names = new Set();
  for (const block of lockfile.split(/\n(?=\[\[package\]\]\n)/)) {
    if (!block.includes(source)) {
      continue;
    }
    const name = block.match(/^name = "([^"]+)"$/m)?.[1];
    if (name) {
      names.add(name);
    }
  }
  return names;
}

function validateCandidateWorkspaceMetadata(root, metadata, candidate) {
  const errors = [];
  const baseVersion = releaseVersion(candidate.base);
  const targetVersion = releaseVersion(candidate.target);
  if (baseVersion === null || targetVersion === null) {
    return errors;
  }

  const releasePackages = canonicalReleasePackageNames(root, candidate.base);
  if (releasePackages.size === 0) {
    errors.push(
      "release candidate base lockfile contains no framework source packages",
    );
    return errors;
  }

  const packages = workspacePackages(metadata);
  const workspaceNames = new Set(
    packages.map((packageMetadata) => packageMetadata.name),
  );
  for (const packageName of releasePackages) {
    if (!workspaceNames.has(packageName)) {
      errors.push(
        `release candidate source package is absent from the workspace: ${packageName}`,
      );
    }
  }

  for (const packageMetadata of packages) {
    const expectedVersion = releasePackages.has(packageMetadata.name)
      ? targetVersion
      : baseVersion;
    if (packageMetadata.version !== expectedVersion) {
      const group = releasePackages.has(packageMetadata.name)
        ? "release source"
        : "repository-only";
      errors.push(
        `release candidate ${group} package version mismatch: ${packageMetadata.name} is ${packageMetadata.version}, expected ${expectedVersion}`,
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

function escapeRegularExpression(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

export function validateCanonicalApplicationLock(root, releaseRef) {
  const errors = [];
  const packagePath = path.join(root, "templates", "package.toml");
  const lockPath = path.join(
    root,
    "templates",
    "applications",
    "layered",
    "Cargo.lock",
  );
  if (!fs.existsSync(packagePath)) {
    return ["canonical component package manifest is missing"];
  }
  if (!fs.existsSync(lockPath)) {
    return ["canonical application lockfile is missing"];
  }

  const packageManifest = fs.readFileSync(packagePath, "utf8");
  const frameworkSection =
    packageManifest.match(/(?:^|\n)\[framework\]\n([\s\S]*?)(?=\n\[|$)/)?.[1] ??
    "";
  const repository = frameworkSection.match(/^repository = "([^"]+)"$/m)?.[1];
  const version = frameworkSection.match(/^version = "([^"]+)"$/m)?.[1];
  if (!repository || !version) {
    return ["canonical component package framework identity is invalid"];
  }
  if (version !== releaseRef) {
    errors.push(
      `canonical component package framework version is ${version}, expected ${releaseRef}`,
    );
  }

  const lockfile = fs.readFileSync(lockPath, "utf8");
  const sourcePattern = new RegExp(
    `^source = "git\\+${escapeRegularExpression(repository)}\\?tag=${escapeRegularExpression(releaseRef)}#[0-9a-f]{40}"$`,
    "gm",
  );
  const expectedSources = lockfile.match(sourcePattern) ?? [];
  const repositorySources = lockfile
    .split("\n")
    .filter((line) => line.startsWith(`source = "git+${repository}`));
  if (expectedSources.length === 0) {
    errors.push(
      "canonical application lockfile contains no framework package resolved from the release tag",
    );
  }
  if (expectedSources.length !== repositorySources.length) {
    errors.push(
      "canonical application lockfile contains a framework source outside the release tag",
    );
  }
  if (new Set(expectedSources).size > 1) {
    errors.push(
      "canonical application lockfile resolves framework packages to multiple revisions",
    );
  }
  return errors;
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
  errors.push(...validateCanonicalApplicationLock(root, releaseRef));

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

export function validateReleaseRepository(
  root,
  releaseRef,
  metadata,
  { allowCandidate = false } = {},
) {
  const workflowPath = path.join(root, ".github", "workflows", "release.yml");
  const candidate = releaseCandidate(metadata);
  const candidateErrors =
    candidate === null
      ? []
      : validateReleaseCandidateMetadata(metadata, releaseRef);
  const candidateBaseIsValid =
    candidate !== null &&
    typeof candidate.base === "string" &&
    stableReleaseParts(candidate.base) !== null;
  const errors = [];

  if (candidate !== null && allowCandidate) {
    errors.push(...candidateErrors);
    if (candidateBaseIsValid) {
      errors.push(...validateReleaseFiles(root, candidate.base));
    }
    if (candidateErrors.length === 0) {
      errors.push(
        ...validateCandidateWorkspaceMetadata(root, metadata, candidate),
      );
    }
  } else {
    errors.push(...validateReleaseMetadata(metadata, releaseRef));
    errors.push(...validateReleaseFiles(root, releaseRef));
    if (candidate !== null) {
      errors.push(
        "release candidate metadata must be removed before explicit release validation",
      );
    }
  }

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
  const requestedReleaseRef = optionValue(args, "--release-ref");
  const candidate = releaseCandidate(metadata);
  const releaseRef =
    requestedReleaseRef ?? candidate?.target ?? currentReleaseRef(metadata);
  const errors = validateReleaseRepository(root, releaseRef, metadata, {
    allowCandidate: requestedReleaseRef === null,
  });

  if (errors.length > 0) {
    for (const error of errors) {
      process.stderr.write(`release policy violation: ${error}\n`);
    }
    process.exitCode = 1;
    return;
  }

  const mode =
    candidate === null ? "release" : `candidate from ${candidate.base}`;
  process.stdout.write(
    `release policy: ok (${releaseRef}, ${workspacePackages(metadata).length} packages, ${mode})\n`,
  );
}

const invokedPath = process.argv[1]
  ? pathToFileURL(path.resolve(process.argv[1])).href
  : "";
if (import.meta.url === invokedPath) {
  runCli();
}
