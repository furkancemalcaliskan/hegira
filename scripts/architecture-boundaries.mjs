import { spawnSync } from "node:child_process";
import crypto from "node:crypto";
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
    "identity_leptos",
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
  audit: [],
  cache: [],
  mail: [],
  search: [],
  security: [],
  settings: [],
  storage: [],
  configuration: [],
  persistence: [],
  background_jobs: [],
  http_support: [],
  leptos_support: [],
  observability: ["background_jobs"],
  test_support: [
    "audit",
    "background_jobs",
    "cache",
    "mail",
    "settings",
    "storage",
  ],
  domain_shared: ["identity_domain_shared"],
  domain: ["identity_domain"],
  application_contracts: ["identity_application_contracts"],
  application: [
    "background_jobs",
    "identity_application",
    "settings",
    "storage",
  ],
  infrastructure: [
    "application",
    "application_contracts",
    "background_jobs",
    "configuration",
    "domain",
    "domain_shared",
    "identity_sqlx",
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
    "identity_domain",
    "identity_domain_shared",
  ],
  identity_application: [
    "audit",
    "cache",
    "identity_application_contracts",
    "identity_domain",
    "identity_domain_shared",
    "mail",
    "search",
    "security",
  ],
  identity_sqlx: [
    "identity_application",
    "identity_application_contracts",
    "identity_domain",
    "identity_domain_shared",
    "persistence",
  ],
  identity_http: [
    "http_support",
    "identity_application",
    "identity_application_contracts",
    "leptos_support",
  ],
  identity_leptos: [
    "application",
    "application_contracts",
    "domain_shared",
    "leptos_support",
    "presentation",
    "web",
  ],
  template_renderer: [],
});

const packageContract = (role, disposition, issues = []) =>
  Object.freeze({ role, disposition, issues: Object.freeze(issues) });

export const WORKSPACE_PACKAGE_POLICY = Object.freeze({
  hegira: packageContract("compatibility", "replace-and-retire", [145, 146]),
  platform_core: packageContract("framework", "retain"),
  audit: packageContract("framework", "retain"),
  cache: packageContract("framework", "retain"),
  mail: packageContract("framework", "retain"),
  search: packageContract("framework", "retain"),
  security: packageContract("framework", "retain"),
  settings: packageContract("framework", "retain"),
  storage: packageContract("framework", "retain"),
  configuration: packageContract("framework", "retain"),
  persistence: packageContract("framework", "retain"),
  background_jobs: packageContract("framework", "retain"),
  http_support: packageContract("framework", "retain"),
  leptos_support: packageContract("framework", "retain"),
  observability: packageContract("framework", "retain"),
  test_support: packageContract("framework", "retain"),
  domain_shared: packageContract(
    "compatibility",
    "extract-and-retire",
    [146],
  ),
  domain: packageContract("compatibility", "extract-and-retire", [146]),
  application_contracts: packageContract(
    "compatibility",
    "extract-and-retire",
    [146],
  ),
  application: packageContract(
    "compatibility",
    "extract-and-retire",
    [146],
  ),
  infrastructure: packageContract(
    "compatibility",
    "extract-and-retire",
    [137, 138, 146],
  ),
  presentation: packageContract(
    "compatibility",
    "extract-and-retire",
    [139, 146],
  ),
  web: packageContract(
    "compatibility",
    "extract-and-retire",
    [135, 141, 146],
  ),
  runtime: packageContract("framework", "retain"),
  db_migrator: packageContract(
    "compatibility",
    "replace-and-retire",
    [142, 146],
  ),
  identity_domain_shared: packageContract("module", "retain"),
  identity_domain: packageContract("module", "retain"),
  identity_application_contracts: packageContract("module", "retain"),
  identity_application: packageContract("module", "retain"),
  identity_sqlx: packageContract("module", "retain"),
  identity_http: packageContract("module", "retain"),
  identity_leptos: packageContract("module", "decouple-and-retain", [135]),
  template_renderer: packageContract("tool", "refactor-and-retain", [148]),
});

export const REPOSITORY_OWNERSHIP_POLICY = Object.freeze({
  framework: Object.freeze(["framework"]),
  module: Object.freeze(["framework", "module"]),
  application: Object.freeze(["framework", "module", "application"]),
  compatibility: Object.freeze(["framework", "module", "compatibility"]),
  tool: Object.freeze(["framework", "module", "tool"]),
});

export const TRANSITIONAL_COMPATIBILITY_EDGES = Object.freeze({
  "identity_leptos -> application": 135,
  "identity_leptos -> application_contracts": 135,
  "identity_leptos -> domain_shared": 135,
  "identity_leptos -> presentation": 135,
  "identity_leptos -> web": 135,
});

const PACKAGE_ROLE_LOCATION_POLICY = Object.freeze({
  framework: Object.freeze(["framework"]),
  module: Object.freeze(["module"]),
  application: Object.freeze(["app"]),
  compatibility: Object.freeze(["app", "framework"]),
  tool: Object.freeze(["tool"]),
});

const PACKAGE_DISPOSITIONS = new Set([
  "retain",
  "decouple-and-retain",
  "canonicalize-and-retain",
  "refactor-and-retain",
  "extract-and-retire",
  "replace-and-retire",
]);

const IDENTITY_SQL_PATTERN =
  /\b(?:select|from|join|insert\s+into|update|delete\s+from|create\s+table|alter\s+table|drop\s+table)\b[^\n]*\b(?:users|sessions|roles|permissions|user_roles|role_permissions|oauth_states|user_oauth_connections|oauth_pending_signups)\b/i;

const HISTORICAL_APPLICATION_IDENTITY_SQL = Object.freeze({
  "crates/infrastructure/src/db/migrations/022_retire_catalog_persistence.sql":
    "36cee0eac99d95ce134bce3bde5efc0cae646403ef1da3dcf020b991a4d0a01c4048e24a9f6916215c7ae7abd484f362",
  "crates/infrastructure/src/db/migrations_sqlite/009_retire_catalog_persistence.sql":
    "c3485487deb6ff20755cce6a76b80de70c2c1129c72075989ea4a2c76b0b3e4634f5985f01fb569d50a0ae3650ad38e7",
});

const HISTORICAL_APPLICATION_IDENTITY_SQL_TESTS = new Set([
  "crates/infrastructure/src/db/retirement_tests.rs",
]);

function sha384(content) {
  return crypto.createHash("sha384").update(content).digest("hex");
}

export function validateIdentitySqlOwnership(files) {
  const errors = [];
  for (const { location, content } of files) {
    if (!location.startsWith("crates/") || !IDENTITY_SQL_PATTERN.test(content)) {
      continue;
    }

    if (HISTORICAL_APPLICATION_IDENTITY_SQL_TESTS.has(location)) {
      continue;
    }

    const historicalChecksum = HISTORICAL_APPLICATION_IDENTITY_SQL[location];
    if (historicalChecksum === undefined) {
      errors.push(
        `Identity SQL must be module-owned under modules/identity/sqlx: ${location}`,
      );
    } else if (sha384(content) !== historicalChecksum) {
      errors.push(
        `historical application migration checksum changed: ${location}`,
      );
    }
  }
  return errors;
}

const IDENTITY_BUSINESS_COMPATIBILITY_ROOTS = [
  "crates/domain_shared/",
  "crates/domain/",
  "crates/application_contracts/",
  "crates/application/",
];

const IDENTITY_BUSINESS_SOURCE_REFERENCE =
  /(?:#\s*\[\s*path\s*=\s*"|include!\s*\(\s*")[^"]*modules\/identity\/(?:domain_shared|domain|application_contracts|application)\//;

export function validateIdentityBusinessSourceOwnership(files) {
  return files
    .filter(
      ({ location, content }) =>
        IDENTITY_BUSINESS_COMPATIBILITY_ROOTS.some((root) =>
          location.startsWith(root),
        ) && IDENTITY_BUSINESS_SOURCE_REFERENCE.test(content),
    )
    .map(
      ({ location }) =>
        `Identity business source must be compiled by its module package, not included by compatibility code: ${location}`,
    );
}

const IDENTITY_SQLX_SOURCE_REFERENCE =
  /(?:#\s*\[\s*path\s*=\s*"|include!\s*\(\s*")[^"]*modules\/identity\/sqlx\//;

export function validateIdentitySqlxSourceOwnership(files) {
  return files
    .filter(
      ({ location, content }) =>
        location.startsWith("crates/infrastructure/") &&
        IDENTITY_SQLX_SOURCE_REFERENCE.test(content),
    )
    .map(
      ({ location }) =>
        `Identity SQLx source must be compiled by identity_sqlx, not included by compatibility infrastructure: ${location}`,
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

function packageLocationOwnership(workspaceRoot, packageMetadata) {
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
  packagePolicy = WORKSPACE_PACKAGE_POLICY,
  transitionalEdges = TRANSITIONAL_COMPATIBILITY_EDGES,
) {
  const errors = [];
  const workspaceRoot = path.resolve(metadata.workspace_root);
  const { packages, edges } = workspaceGraph(metadata);
  const packageNames = new Set(
    packages.map((packageMetadata) => packageMetadata.name),
  );
  const checkedEdges = new Set();

  for (const packageMetadata of packages) {
    const locationOwnership = packageLocationOwnership(
      workspaceRoot,
      packageMetadata,
    );
    if (locationOwnership === undefined) {
      errors.push(
        `workspace package is outside an owned repository location: ${packageMetadata.name} (${packageDirectory(packageMetadata)})`,
      );
    }

    const contract = packagePolicy[packageMetadata.name];
    if (contract === undefined) {
      errors.push(
        `workspace package has no ownership and disposition contract: ${packageMetadata.name}`,
      );
    } else {
      const allowedLocations = PACKAGE_ROLE_LOCATION_POLICY[contract.role];
      if (allowedLocations === undefined) {
        errors.push(
          `workspace package has an invalid ownership role: ${packageMetadata.name} (${contract.role})`,
        );
      } else if (
        locationOwnership !== undefined &&
        !allowedLocations.includes(locationOwnership)
      ) {
        errors.push(
          `workspace package role does not match its repository location: ${packageMetadata.name} (${contract.role} packages may not live under ${locationOwnership})`,
        );
      }

      if (!PACKAGE_DISPOSITIONS.has(contract.disposition)) {
        errors.push(
          `workspace package has an invalid disposition: ${packageMetadata.name} (${contract.disposition})`,
        );
      }
      if (!Array.isArray(contract.issues)) {
        errors.push(
          `workspace package has an invalid follow-up issue list: ${packageMetadata.name}`,
        );
      } else if (
        contract.disposition !== "retain" &&
        (contract.issues.length === 0 ||
          contract.issues.some(
            (issue) => !Number.isInteger(issue) || issue <= 0,
          ))
      ) {
        errors.push(
          `workspace package transition has no accepted issue: ${packageMetadata.name}`,
        );
      } else if (
        contract.disposition === "retain" &&
        contract.issues.length !== 0
      ) {
        errors.push(
          `retained workspace package must not have transition issues: ${packageMetadata.name}`,
        );
      }
    }

    if (!Object.hasOwn(policy, packageMetadata.name)) {
      errors.push(
        `workspace package has no architecture policy entry: ${packageMetadata.name}`,
      );
    }

    if (
      packageMetadata.name === "identity_leptos" &&
      (packageMetadata.dependencies ?? []).some(
        (dependency) => dependency.name === "sqlx",
      )
    ) {
      errors.push(
        "identity_leptos must depend on Identity contracts and application services, not SQLx",
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

  for (const packageName of Object.keys(packagePolicy)) {
    if (!packageNames.has(packageName)) {
      errors.push(
        `ownership and disposition contract references missing workspace package: ${packageName}`,
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

    const fromOwnership = packagePolicy[from]?.role;
    const toOwnership = packagePolicy[to]?.role;
    const allowedOwnerships =
      REPOSITORY_OWNERSHIP_POLICY[fromOwnership] ?? [];
    if (
      toOwnership !== undefined &&
      !allowedOwnerships.includes(toOwnership) &&
      !Object.hasOwn(transitionalEdges, edgeName)
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
  for (const [edgeName, issue] of Object.entries(transitionalEdges)) {
    if (!Number.isInteger(issue) || issue <= 0) {
      errors.push(
        `transitional compatibility edge has no accepted issue: ${edgeName}`,
      );
    }
    if (!checkedEdges.has(edgeName)) {
      errors.push(
        `transitional compatibility policy references missing workspace edge: ${edgeName}`,
      );
    }
    const [from, to] = edgeName.split(" -> ");
    const fromRole = packagePolicy[from]?.role;
    const toRole = packagePolicy[to]?.role;
    if (
      !["framework", "module"].includes(fromRole) ||
      toRole !== "compatibility"
    ) {
      errors.push(
        `transitional compatibility policy may only allow framework or module packages to leave compatibility code: ${edgeName}`,
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

  const sourceFiles = repositorySourceFiles(root);
  const errors = [
    ...validateWorkspaceMetadata(metadata),
    ...validateIdentityBusinessSourceOwnership(sourceFiles),
    ...validateIdentitySqlxSourceOwnership(sourceFiles),
    ...validateIdentitySqlOwnership(sourceFiles),
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
