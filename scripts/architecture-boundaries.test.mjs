import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import test from "node:test";

import {
  REPOSITORY_OWNERSHIP_POLICY,
  TRANSITIONAL_COMPATIBILITY_EDGES,
  WORKSPACE_DEPENDENCY_POLICY,
  WORKSPACE_PACKAGE_POLICY,
  validateIdentityBusinessSourceOwnership,
  validateIdentitySqlOwnership,
  validateIdentitySqlxSourceOwnership,
  validateWorkspaceMetadata,
} from "./architecture-boundaries.mjs";

const PACKAGE_LOCATIONS = Object.freeze({
  hegira: "apps/hegira",
  identity_domain_shared: "modules/identity/domain_shared",
  identity_domain: "modules/identity/domain",
  identity_application_contracts: "modules/identity/application_contracts",
  identity_application: "modules/identity/application",
  identity_sqlx: "modules/identity/sqlx",
  identity_http: "modules/identity/http",
  identity_leptos: "modules/identity/leptos",
  template_renderer: "tools/template_renderer",
});

function packageLocation(name) {
  return PACKAGE_LOCATIONS[name] ?? `crates/${name}`;
}

function workspaceMetadata(extraEdges = []) {
  const root = path.resolve("/workspace");
  const packageNames = Object.keys(WORKSPACE_DEPENDENCY_POLICY);
  const edges = [
    ...Object.entries(WORKSPACE_DEPENDENCY_POLICY).flatMap(
      ([from, targets]) => targets.map((to) => ({ from, to })),
    ),
    ...extraEdges,
  ];

  return {
    workspace_root: root,
    workspace_members: packageNames.map((name) => `workspace#${name}`),
    packages: packageNames.map((name) => {
      const directory = path.join(root, packageLocation(name));
      return {
        id: `workspace#${name}`,
        name,
        manifest_path: path.join(directory, "Cargo.toml"),
        dependencies: edges
          .filter((edge) => edge.from === name)
          .map((edge) => ({
            name: edge.to,
            path: path.join(root, packageLocation(edge.to)),
          })),
      };
    }),
  };
}

function ownershipFixture(packageLocations, dependencies) {
  const root = path.resolve("/workspace");
  const policy = Object.fromEntries(
    packageLocations.map(({ name }) => [
      name,
      [
        ...new Set(
          dependencies
            .filter((dependency) => dependency.from === name)
            .map((dependency) => dependency.to),
        ),
      ],
    ]),
  );
  const locationByName = new Map(
    packageLocations.map(({ name, location }) => [name, location]),
  );
  const packagePolicy = Object.fromEntries(
    packageLocations.map(({ name, location, role }) => {
      let inferredRole = role;
      if (inferredRole === undefined) {
        if (location.startsWith("crates/")) inferredRole = "framework";
        else if (location.startsWith("modules/")) inferredRole = "module";
        else if (location.startsWith("tools/")) inferredRole = "tool";
        else if (location.startsWith("apps/")) inferredRole = "compatibility";
        else inferredRole = "invalid";
      }
      return [name, { role: inferredRole, disposition: "retain", issues: [] }];
    }),
  );

  return {
    policy,
    packagePolicy,
    metadata: {
      workspace_root: root,
      workspace_members: packageLocations.map(
        ({ name }) => `workspace#${name}`,
      ),
      packages: packageLocations.map(({ name, location }) => ({
        id: `workspace#${name}`,
        name,
        manifest_path: path.join(root, location, "Cargo.toml"),
        dependencies: dependencies
          .filter((dependency) => dependency.from === name)
          .map((dependency) => ({
            name: dependency.to,
            path: path.join(root, locationByName.get(dependency.to)),
            kind: dependency.kind ?? null,
            optional: dependency.optional ?? false,
          })),
      })),
    },
  };
}

function validateOwnershipFixture(packageLocations, dependencies) {
  const fixture = ownershipFixture(packageLocations, dependencies);
  return validateWorkspaceMetadata(
    fixture.metadata,
    fixture.policy,
    fixture.packagePolicy,
    {},
  );
}

test("accepts the documented current workspace graph", () => {
  assert.deepEqual(validateWorkspaceMetadata(workspaceMetadata()), []);
});

test("rejects SQLx from the Identity Leptos adapter", () => {
  const metadata = workspaceMetadata();
  metadata.packages
    .find((packageMetadata) => packageMetadata.name === "identity_leptos")
    .dependencies.push({ name: "sqlx", path: null });

  assert.deepEqual(validateWorkspaceMetadata(metadata), [
    "identity_leptos must depend on Identity contracts and application services, not SQLx",
  ]);
});

test("accepts the documented ownership-class dependency directions", () => {
  const metadata = workspaceMetadata();
  const errors = validateWorkspaceMetadata(metadata);
  assert.deepEqual(errors, []);
  assert.ok(
    WORKSPACE_DEPENDENCY_POLICY.runtime.length === 0 &&
      WORKSPACE_DEPENDENCY_POLICY.configuration.length === 0 &&
      WORKSPACE_DEPENDENCY_POLICY.persistence.length === 0 &&
      WORKSPACE_DEPENDENCY_POLICY.background_jobs.length === 0 &&
      WORKSPACE_DEPENDENCY_POLICY.http_support.length === 0 &&
      WORKSPACE_DEPENDENCY_POLICY.leptos_support.length === 0 &&
      WORKSPACE_DEPENDENCY_POLICY.audit.length === 0 &&
      WORKSPACE_DEPENDENCY_POLICY.cache.length === 0 &&
      WORKSPACE_DEPENDENCY_POLICY.mail.length === 0 &&
      WORKSPACE_DEPENDENCY_POLICY.search.length === 0 &&
      WORKSPACE_DEPENDENCY_POLICY.security.length === 0 &&
      WORKSPACE_DEPENDENCY_POLICY.settings.length === 0 &&
      WORKSPACE_DEPENDENCY_POLICY.storage.length === 0 &&
      WORKSPACE_DEPENDENCY_POLICY.observability.includes("background_jobs") &&
      WORKSPACE_DEPENDENCY_POLICY.test_support.includes("audit") &&
      !WORKSPACE_DEPENDENCY_POLICY.test_support.includes("application") &&
      WORKSPACE_DEPENDENCY_POLICY.hegira.includes("infrastructure") &&
      WORKSPACE_DEPENDENCY_POLICY.hegira.includes("persistence") &&
      WORKSPACE_DEPENDENCY_POLICY.db_migrator.includes("persistence") &&
      WORKSPACE_DEPENDENCY_POLICY.identity_domain.includes(
        "identity_domain_shared",
      ) &&
      WORKSPACE_DEPENDENCY_POLICY.identity_application_contracts.includes(
        "identity_domain",
      ) &&
      WORKSPACE_DEPENDENCY_POLICY.identity_application.includes(
        "identity_application_contracts",
      ) &&
      WORKSPACE_DEPENDENCY_POLICY.infrastructure.includes("identity_sqlx") &&
      WORKSPACE_DEPENDENCY_POLICY.identity_sqlx.includes("persistence") &&
      WORKSPACE_DEPENDENCY_POLICY.identity_http.includes("http_support") &&
      WORKSPACE_DEPENDENCY_POLICY.identity_leptos.includes("web") &&
      !WORKSPACE_DEPENDENCY_POLICY.identity_leptos.includes("persistence") &&
      WORKSPACE_DEPENDENCY_POLICY.presentation.includes("infrastructure"),
  );
  assert.deepEqual(REPOSITORY_OWNERSHIP_POLICY.framework, ["framework"]);
  assert.deepEqual(REPOSITORY_OWNERSHIP_POLICY.module, [
    "framework",
    "module",
  ]);
  assert.deepEqual(REPOSITORY_OWNERSHIP_POLICY.compatibility, [
    "framework",
    "module",
    "compatibility",
  ]);
  assert.deepEqual(REPOSITORY_OWNERSHIP_POLICY.application, [
    "framework",
    "module",
    "application",
  ]);
});

test("classifies every current package and records each transition issue", () => {
  assert.deepEqual(
    new Set(Object.keys(WORKSPACE_PACKAGE_POLICY)),
    new Set(Object.keys(WORKSPACE_DEPENDENCY_POLICY)),
  );

  for (const [name, contract] of Object.entries(WORKSPACE_PACKAGE_POLICY)) {
    assert.ok(contract.role, `${name} must have a role`);
    assert.ok(contract.disposition, `${name} must have a disposition`);
    if (contract.disposition !== "retain") {
      assert.ok(contract.issues.length > 0, `${name} must have an issue`);
      assert.ok(
        contract.issues.every((issue) => Number.isInteger(issue)),
        `${name} must have valid issues`,
      );
    }
  }
});

test("keeps transitional compatibility edges explicit and issue-bound", () => {
  assert.equal(
    TRANSITIONAL_COMPATIBILITY_EDGES["test_support -> application"],
    undefined,
  );
  assert.equal(
    TRANSITIONAL_COMPATIBILITY_EDGES["identity_http -> presentation"],
    134,
  );
  assert.equal(
    TRANSITIONAL_COMPATIBILITY_EDGES["identity_leptos -> web"],
    135,
  );
});

test("framework policy exposes no dependency on an Identity module package", () => {
  const identityPackages = new Set(
    Object.entries(PACKAGE_LOCATIONS)
      .filter(([, location]) => location.startsWith("modules/identity/"))
      .map(([name]) => name),
  );
  const violations = Object.entries(WORKSPACE_DEPENDENCY_POLICY)
    .filter(([name]) => WORKSPACE_PACKAGE_POLICY[name].role === "framework")
    .flatMap(([name, dependencies]) =>
      dependencies
        .filter((dependency) => identityPackages.has(dependency))
        .map((dependency) => `${name} -> ${dependency}`),
    );

  assert.deepEqual(violations, []);
});

test("Identity business and persistence packages expose no compatibility dependency", () => {
  for (const name of [
    "identity_domain_shared",
    "identity_domain",
    "identity_application_contracts",
    "identity_application",
    "identity_sqlx",
  ]) {
    const compatibilityDependencies = WORKSPACE_DEPENDENCY_POLICY[name].filter(
      (dependency) =>
        WORKSPACE_PACKAGE_POLICY[dependency].role === "compatibility",
    );
    assert.deepEqual(
      compatibilityDependencies,
      [],
      `${name} must not depend on compatibility code`,
    );
  }
});

test("rejects a dependency from a framework package to the compatibility host", () => {
  const errors = validateWorkspaceMetadata(
    workspaceMetadata([{ from: "runtime", to: "hegira" }]),
  );
  assert.ok(
    errors.some(
      (error) =>
        error.includes("runtime -> hegira") &&
        error.includes(
          "framework packages may not depend on compatibility packages",
        ),
    ),
  );
});

test("accepts valid framework module tool and compatibility directions", () => {
  const packages = [
    { name: "framework_core", location: "crates/framework_core" },
    { name: "framework_http", location: "crates/framework_http" },
    { name: "identity_domain", location: "modules/identity/domain" },
    { name: "identity_http", location: "modules/identity/http" },
    { name: "template_renderer", location: "tools/template_renderer" },
    { name: "compatibility_host", location: "apps/example" },
  ];
  const dependencies = [
    { from: "framework_http", to: "framework_core" },
    { from: "identity_domain", to: "framework_core" },
    { from: "identity_http", to: "identity_domain" },
    { from: "identity_http", to: "framework_http" },
    { from: "template_renderer", to: "identity_http" },
    { from: "compatibility_host", to: "identity_http" },
    { from: "compatibility_host", to: "framework_http" },
  ];

  assert.deepEqual(validateOwnershipFixture(packages, dependencies), []);
});

for (const target of [
  { ownership: "module", location: "modules/identity" },
  {
    ownership: "application",
    location: "apps/application",
    role: "application",
  },
  { ownership: "tool", location: "tools/cli" },
]) {
  test(`rejects a framework dependency on ${target.ownership} code`, () => {
    const errors = validateOwnershipFixture(
      [
        { name: "framework", location: "crates/framework" },
        { name: "downstream", location: target.location, role: target.role },
      ],
      [{ from: "framework", to: "downstream" }],
    );

    assert.deepEqual(errors, [
      `invalid repository ownership edge: framework -> downstream (framework packages may not depend on ${target.ownership} packages)`,
    ]);
  });
}

for (const target of [
  {
    ownership: "application",
    location: "apps/application",
    role: "application",
  },
  {
    ownership: "compatibility",
    location: "crates/compatibility",
    role: "compatibility",
  },
  { ownership: "tool", location: "tools/cli" },
]) {
  test(`rejects a module dependency on ${target.ownership} code`, () => {
    const errors = validateOwnershipFixture(
      [
        { name: "identity", location: "modules/identity" },
        { name: "downstream", location: target.location, role: target.role },
      ],
      [{ from: "identity", to: "downstream" }],
    );

    assert.deepEqual(errors, [
      `invalid repository ownership edge: identity -> downstream (module packages may not depend on ${target.ownership} packages)`,
    ]);
  });
}

test("rejects application-template packages in the framework workspace", () => {
  const errors = validateOwnershipFixture(
    [
      {
        name: "generated_server",
        location: "templates/applications/layered/apps/server",
        role: "application",
      },
    ],
    [],
  );

  assert.ok(
    errors.some((error) =>
      error.includes("workspace package role does not match its repository location"),
    ),
  );
});

test("accepts an explicitly owned application composition package", () => {
  const errors = validateOwnershipFixture(
    [
      { name: "framework", location: "crates/framework" },
      { name: "module", location: "modules/module" },
      { name: "application", location: "apps/application", role: "application" },
    ],
    [
      { from: "application", to: "framework" },
      { from: "application", to: "module" },
    ],
  );

  assert.deepEqual(errors, []);
});

test("rejects an unapproved framework dependency on compatibility code", () => {
  const errors = validateOwnershipFixture(
    [
      { name: "framework", location: "crates/framework" },
      {
        name: "legacy",
        location: "crates/legacy",
        role: "compatibility",
      },
    ],
    [{ from: "framework", to: "legacy" }],
  );

  assert.deepEqual(errors, [
    "invalid repository ownership edge: framework -> legacy (framework packages may not depend on compatibility packages)",
  ]);
});

test("requires a transition disposition to reference an accepted issue", () => {
  const fixture = ownershipFixture(
    [{ name: "legacy", location: "crates/legacy", role: "compatibility" }],
    [],
  );
  fixture.packagePolicy.legacy = {
    role: "compatibility",
    disposition: "extract-and-retire",
    issues: [],
  };

  assert.deepEqual(
    validateWorkspaceMetadata(
      fixture.metadata,
      fixture.policy,
      fixture.packagePolicy,
      {},
    ),
    ["workspace package transition has no accepted issue: legacy"],
  );
});

test("rejects a stale transitional compatibility exception", () => {
  const fixture = ownershipFixture(
    [
      { name: "framework", location: "crates/framework" },
      {
        name: "legacy",
        location: "crates/legacy",
        role: "compatibility",
      },
    ],
    [],
  );

  const errors = validateWorkspaceMetadata(
    fixture.metadata,
    fixture.policy,
    fixture.packagePolicy,
    { "framework -> legacy": 999 },
  );

  assert.ok(
    errors.includes(
      "transitional compatibility policy references missing workspace edge: framework -> legacy",
    ),
  );
});

for (const dependency of [
  { label: "normal", kind: null, optional: false },
  { label: "optional", kind: null, optional: true },
  { label: "development", kind: "dev", optional: false },
  { label: "build", kind: "build", optional: false },
]) {
  test(`rejects a forbidden ${dependency.label} dependency`, () => {
    const errors = validateOwnershipFixture(
      [
        { name: "framework", location: "crates/framework" },
        { name: "identity", location: "modules/identity" },
      ],
      [{ from: "framework", to: "identity", ...dependency }],
    );

    assert.deepEqual(errors, [
      "invalid repository ownership edge: framework -> identity (framework packages may not depend on module packages)",
    ]);
  });
}

test("rejects a workspace package outside an owned repository location", () => {
  const errors = validateOwnershipFixture(
    [{ name: "unknown", location: "vendor/unknown" }],
    [],
  );

  assert.deepEqual(errors, [
    "workspace package is outside an owned repository location: unknown (/workspace/vendor/unknown)",
    "workspace package has an invalid ownership role: unknown (invalid)",
  ]);
});

test("rejects an outward dependency from the domain layer", () => {
  const errors = validateWorkspaceMetadata(
    workspaceMetadata([{ from: "domain", to: "infrastructure" }]),
  );
  assert.ok(
    errors.some(
      (error) =>
        error.includes("domain -> infrastructure") &&
        error.includes("not permitted"),
    ),
  );
});

test("rejects an outward dependency from the application layer", () => {
  const errors = validateWorkspaceMetadata(
    workspaceMetadata([{ from: "application", to: "presentation" }]),
  );
  assert.ok(
    errors.some(
      (error) =>
        error.includes("application -> presentation") &&
        error.includes("not permitted"),
    ),
  );
});

test("requires every workspace package to have a policy entry", () => {
  const metadata = workspaceMetadata();
  metadata.workspace_members.push("workspace#unregistered");
  metadata.packages.push({
    id: "workspace#unregistered",
    name: "unregistered",
    manifest_path: path.join(
      metadata.workspace_root,
      "crates",
      "unregistered",
      "Cargo.toml",
    ),
    dependencies: [],
  });

  const errors = validateWorkspaceMetadata(metadata);
  assert.ok(
    errors.some((error) =>
      error.includes("workspace package has no architecture policy entry"),
    ),
  );
  assert.ok(
    errors.some((error) =>
      error.includes("workspace package has no ownership and disposition contract"),
    ),
  );
});

test("rejects a local dependency target outside the workspace", () => {
  const metadata = workspaceMetadata();
  const domain = metadata.packages.find(
    (packageMetadata) => packageMetadata.name === "domain",
  );
  domain.dependencies.push({
    name: "unregistered",
    path: path.join(metadata.workspace_root, "vendor", "unregistered"),
  });

  const errors = validateWorkspaceMetadata(metadata);
  assert.ok(
    errors.some((error) =>
      error.includes("domain -> unregistered"),
    ),
  );
});

test("requires Identity SQL to remain module-owned", () => {
  assert.deepEqual(
    validateIdentitySqlOwnership([
      {
        location: "crates/infrastructure/src/users.rs",
        content: "SELECT id FROM users WHERE deleted_at IS NULL",
      },
      {
        location: "modules/identity/sqlx/src/users.rs",
        content: "SELECT id FROM users WHERE deleted_at IS NULL",
      },
      {
        location: "crates/storage/src/path.rs",
        content: 'StoragePath::from_segments(["identity", "users"])',
      },
      {
        location:
          "crates/infrastructure/src/db/migrations/022_retire_catalog_persistence.sql",
        content: fs.readFileSync(
          path.join(
            process.cwd(),
            "crates/infrastructure/src/db/migrations/022_retire_catalog_persistence.sql",
          ),
          "utf8",
        ),
      },
      {
        location: "crates/infrastructure/src/db/retirement_tests.rs",
        content: "SELECT name FROM permissions WHERE name LIKE 'Catalog.%'",
      },
    ]),
    [
      "Identity SQL must be module-owned under modules/identity/sqlx: crates/infrastructure/src/users.rs",
    ],
  );
});

test("rejects changes to historical application-owned Identity SQL", () => {
  assert.deepEqual(
    validateIdentitySqlOwnership([
      {
        location:
          "crates/infrastructure/src/db/migrations/022_retire_catalog_persistence.sql",
        content: "DELETE FROM permissions WHERE name LIKE 'Catalog.%';",
      },
    ]),
    [
      "historical application migration checksum changed: crates/infrastructure/src/db/migrations/022_retire_catalog_persistence.sql",
    ],
  );
});

test("requires Identity business source to compile from module packages", () => {
  assert.deepEqual(
    validateIdentityBusinessSourceOwnership([
      {
        location: "crates/application/src/lib.rs",
        content:
          '#[path = "../../../modules/identity/application/src/identity/mod.rs"]',
      },
      {
        location: "modules/identity/application/src/lib.rs",
        content: "pub mod identity;",
      },
      {
        location: "crates/domain/src/lib.rs",
        content:
          'include!("../../../modules/identity/domain/src/identity/mod.rs");',
      },
      {
        location: "crates/presentation/src/lib.rs",
        content: '#[path = "../../../modules/identity/http/src/lib.rs"]',
      },
    ]),
    [
      "Identity business source must be compiled by its module package, not included by compatibility code: crates/application/src/lib.rs",
      "Identity business source must be compiled by its module package, not included by compatibility code: crates/domain/src/lib.rs",
    ],
  );
});

test("requires Identity SQLx source to compile from the module adapter", () => {
  assert.deepEqual(
    validateIdentitySqlxSourceOwnership([
      {
        location: "crates/infrastructure/src/identity/mod.rs",
        content:
          '#[path = "../../../../modules/identity/sqlx/src/identity/provider.rs"]',
      },
      {
        location: "modules/identity/sqlx/src/identity/mod.rs",
        content: "pub mod provider;",
      },
    ]),
    [
      "Identity SQLx source must be compiled by identity_sqlx, not included by compatibility infrastructure: crates/infrastructure/src/identity/mod.rs",
    ],
  );
});
