import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";

export const WORKSPACE_DEPENDENCY_POLICY = Object.freeze({
  hegira: [
    "application",
    "application_contracts",
    "background_jobs",
    "configuration",
    "domain",
    "domain_shared",
    "http_support",
    "identity_http",
    "infrastructure",
    "observability",
    "persistence",
    "platform_core",
    "presentation",
    "runtime",
    "test_support",
    "web",
  ],
  platform_core: [],
  configuration: [],
  persistence: [],
  background_jobs: [],
  http_support: [],
  leptos_support: [],
  observability: ["background_jobs"],
  test_support: ["application"],
  domain_shared: [],
  domain: ["domain_shared"],
  application_contracts: ["domain", "domain_shared"],
  application: [
    "application_contracts",
    "background_jobs",
    "domain",
    "domain_shared",
  ],
  infrastructure: [
    "application",
    "application_contracts",
    "background_jobs",
    "configuration",
    "domain",
    "domain_shared",
    "persistence",
    "platform_core",
    "runtime",
  ],
  presentation: [
    "application",
    "application_contracts",
    "domain_shared",
    "infrastructure",
    "leptos_support",
    "observability",
  ],
  web: [
    "application",
    "application_contracts",
    "domain_shared",
    "leptos_support",
    "presentation",
  ],
  runtime: [],
  db_migrator: ["infrastructure", "persistence"],
  identity_domain_shared: [],
  identity_domain: ["identity_domain_shared"],
  identity_application_contracts: [
    "domain_shared",
    "identity_domain",
    "identity_domain_shared",
  ],
  identity_application: [
    "domain_shared",
    "identity_application_contracts",
    "identity_domain",
    "identity_domain_shared",
  ],
  identity_sqlx: [
    "identity_application",
    "identity_application_contracts",
    "identity_domain",
    "identity_domain_shared",
    "persistence",
  ],
  identity_http: [
    "application",
    "application_contracts",
    "domain_shared",
    "http_support",
    "leptos_support",
    "presentation",
  ],
});

export const REPOSITORY_OWNERSHIP_POLICY = Object.freeze({
  app: Object.freeze(["app", "framework", "module"]),
  framework: Object.freeze(["framework"]),
  module: Object.freeze(["framework", "module"]),
  template: Object.freeze(["framework", "module", "template"]),
  tool: Object.freeze(["framework", "module", "template", "tool"]),
});

const IDENTITY_SQL_PATTERN =
  /\b(?:select|from|join|insert\s+into|update|delete\s+from|create\s+table|alter\s+table|drop\s+table)\b[^\n]*\b(?:users|sessions|roles|permissions|user_roles|role_permissions|oauth_states|user_oauth_connections|oauth_pending_signups)\b/i;

export function validateIdentitySqlOwnership(files) {
  return files
    .filter(
      ({ location, content }) =>
        location.startsWith("crates/") && IDENTITY_SQL_PATTERN.test(content),
    )
    .map(
      ({ location }) =>
        `Identity SQL must be module-owned under modules/identity/sqlx: ${location}`,
    );
}

function repositorySourceFiles(root) {
  const files = [];
  const visit = (directory) => {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const location = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        visit(location);
      } else if (entry.isFile() && /\.(?:rs|sql)$/.test(entry.name)) {
        files.push({
          location: path.relative(root, location).split(path.sep).join("/"),
          content: fs.readFileSync(location, "utf8"),
        });
      }
    }
  };

  visit(path.join(root, "crates"));
  return files;
}

const REPOSITORY_PACKAGE_ROOTS = Object.freeze([
  Object.freeze({ directory: "apps", ownership: "app" }),
  Object.freeze({ directory: "crates", ownership: "framework" }),
  Object.freeze({ directory: "modules", ownership: "module" }),
  Object.freeze({ directory: "templates", ownership: "template" }),
  Object.freeze({ directory: "tools", ownership: "tool" }),
]);

function isInside(parent, candidate) {
  const relative = path.relative(parent, candidate);
  return (
    relative === "" ||
    (relative !== ".." &&
      !relative.startsWith(`..${path.sep}`) &&
      !path.isAbsolute(relative))
  );
}

function packageDirectory(packageMetadata) {
  return path.dirname(path.resolve(packageMetadata.manifest_path));
}

function packageOwnership(workspaceRoot, packageMetadata) {
  const directory = packageDirectory(packageMetadata);

  for (const root of REPOSITORY_PACKAGE_ROOTS) {
    if (isInside(path.join(workspaceRoot, root.directory), directory)) {
      return root.ownership;
    }
  }

  return undefined;
}

function workspaceGraph(metadata) {
  const workspaceIds = new Set(metadata.workspace_members ?? []);
  const packages = (metadata.packages ?? []).filter((packageMetadata) =>
    workspaceIds.has(packageMetadata.id),
  );
  const byDirectory = new Map(
    packages.map((packageMetadata) => [
      packageDirectory(packageMetadata),
      packageMetadata,
    ]),
  );
  const edges = [];

  for (const packageMetadata of packages) {
    for (const dependency of packageMetadata.dependencies ?? []) {
      if (dependency.path === null || dependency.path === undefined) {
        continue;
      }

      const dependencyDirectory = path.resolve(dependency.path);
      const target = byDirectory.get(dependencyDirectory);
      edges.push({
        from: packageMetadata,
        target,
        dependencyName: dependency.name,
        dependencyDirectory,
      });
    }
  }

  return { packages, edges };
}

export function validateWorkspaceMetadata(
  metadata,
  policy = WORKSPACE_DEPENDENCY_POLICY,
) {
  const errors = [];
  const workspaceRoot = path.resolve(metadata.workspace_root);
  const { packages, edges } = workspaceGraph(metadata);
  const packageNames = new Set(
    packages.map((packageMetadata) => packageMetadata.name),
  );
  const checkedEdges = new Set();

  for (const packageMetadata of packages) {
    const ownership = packageOwnership(workspaceRoot, packageMetadata);
    if (ownership === undefined) {
      errors.push(
        `workspace package is outside an owned repository location: ${packageMetadata.name} (${packageDirectory(packageMetadata)})`,
      );
    }

    if (!Object.hasOwn(policy, packageMetadata.name)) {
      errors.push(
        `workspace package has no architecture policy entry: ${packageMetadata.name}`,
      );
    }
  }

  for (const packageName of Object.keys(policy)) {
    if (!packageNames.has(packageName)) {
      errors.push(
        `architecture policy references missing workspace package: ${packageName}`,
      );
    }
  }

  for (const [from, allowedTargets] of Object.entries(policy)) {
    for (const target of allowedTargets) {
      if (!Object.hasOwn(policy, target)) {
        errors.push(
          `architecture policy edge references unknown package: ${from} -> ${target}`,
        );
      }
    }
  }

  for (const edge of edges) {
    const from = edge.from.name;
    if (edge.target === undefined) {
      errors.push(
        `local dependency target is not a workspace member: ${from} -> ${edge.dependencyName}`,
      );
      continue;
    }

    const to = edge.target.name;
    const edgeName = `${from} -> ${to}`;
    if (checkedEdges.has(edgeName)) {
      continue;
    }
    checkedEdges.add(edgeName);

    const fromOwnership = packageOwnership(workspaceRoot, edge.from);
    const toOwnership = packageOwnership(workspaceRoot, edge.target);
    const allowedOwnerships =
      REPOSITORY_OWNERSHIP_POLICY[fromOwnership] ?? [];
    if (
      toOwnership !== undefined &&
      !allowedOwnerships.includes(toOwnership)
    ) {
      errors.push(
        `invalid repository ownership edge: ${edgeName} (${fromOwnership ?? "unknown"} packages may not depend on ${toOwnership} packages)`,
      );
      continue;
    }

    const allowedTargets = policy[from] ?? [];
    if (!allowedTargets.includes(to)) {
      errors.push(
        `invalid workspace dependency edge: ${edgeName} (not permitted by the documented workspace boundary policy)`,
      );
    }
  }

  return errors;
}

function readWorkspaceMetadata(root) {
  const result = spawnSync(
    "cargo",
    ["metadata", "--locked", "--format-version", "1"],
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

function runCli() {
  const args = process.argv.slice(2);
  if (args.length !== 3 || args[0] !== "check" || args[1] !== "--root") {
    console.error(
      "usage: node scripts/architecture-boundaries.mjs check --root <repository-root>",
    );
    process.exitCode = 2;
    return;
  }

  const root = path.resolve(args[2]);
  let metadata;
  try {
    metadata = readWorkspaceMetadata(root);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
    return;
  }

  const errors = [
    ...validateWorkspaceMetadata(metadata),
    ...validateIdentitySqlOwnership(repositorySourceFiles(root)),
  ];
  if (errors.length > 0) {
    for (const error of errors) {
      console.error(`architecture boundary violation: ${error}`);
    }
    process.exitCode = 1;
    return;
  }

  const { packages, edges } = workspaceGraph(metadata);
  const uniqueEdges = new Set(
    edges
      .filter((edge) => edge.target !== undefined)
      .map((edge) => `${edge.from.name} -> ${edge.target.name}`),
  );
  console.log(
    `workspace dependency boundaries: ok (${packages.length} packages, ${uniqueEdges.size} edges)`,
  );
}

const invokedPath = process.argv[1]
  ? pathToFileURL(path.resolve(process.argv[1])).href
  : "";
if (import.meta.url === invokedPath) {
  runCli();
}
