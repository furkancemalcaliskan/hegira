import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";

import {
  REPOSITORY_OWNERSHIP_POLICY,
  WORKSPACE_DEPENDENCY_POLICY,
  validateIdentitySqlOwnership,
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

  return {
    policy,
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
  return validateWorkspaceMetadata(fixture.metadata, fixture.policy);
}

test("accepts the documented current workspace graph", () => {
  assert.deepEqual(validateWorkspaceMetadata(workspaceMetadata()), []);
});

test("accepts framework isolation and app-owned composition edges", () => {
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
      WORKSPACE_DEPENDENCY_POLICY.observability.includes("background_jobs") &&
      WORKSPACE_DEPENDENCY_POLICY.test_support.includes("application") &&
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
      WORKSPACE_DEPENDENCY_POLICY.identity_sqlx.includes("persistence") &&
      WORKSPACE_DEPENDENCY_POLICY.identity_http.includes("http_support") &&
      WORKSPACE_DEPENDENCY_POLICY.presentation.includes("infrastructure"),
  );
  assert.deepEqual(REPOSITORY_OWNERSHIP_POLICY.framework, ["framework"]);
  assert.deepEqual(REPOSITORY_OWNERSHIP_POLICY.module, [
    "framework",
    "module",
  ]);
});

test("rejects a dependency from a reusable crate to the deployable app", () => {
  const errors = validateWorkspaceMetadata(
    workspaceMetadata([{ from: "web", to: "hegira" }]),
  );
  assert.ok(
    errors.some(
      (error) =>
        error.includes("web -> hegira") &&
        error.includes("framework packages may not depend on app packages"),
    ),
  );
});

test("accepts valid framework module template tool and app directions", () => {
  const packages = [
    { name: "framework_core", location: "crates/framework_core" },
    { name: "framework_http", location: "crates/framework_http" },
    { name: "identity_domain", location: "modules/identity/domain" },
    { name: "identity_http", location: "modules/identity/http" },
    { name: "app_template", location: "templates/app/server" },
    { name: "template_renderer", location: "tools/template_renderer" },
    { name: "example_app", location: "apps/example" },
  ];
  const dependencies = [
    { from: "framework_http", to: "framework_core" },
    { from: "identity_domain", to: "framework_core" },
    { from: "identity_http", to: "identity_domain" },
    { from: "identity_http", to: "framework_http" },
    { from: "app_template", to: "identity_http" },
    { from: "app_template", to: "framework_core" },
    { from: "template_renderer", to: "app_template" },
    { from: "example_app", to: "identity_http" },
    { from: "example_app", to: "framework_http" },
  ];

  assert.deepEqual(validateOwnershipFixture(packages, dependencies), []);
});

for (const target of [
  { ownership: "module", location: "modules/identity" },
  { ownership: "template", location: "templates/app" },
  { ownership: "tool", location: "tools/cli" },
]) {
  test(`rejects a framework dependency on a ${target.ownership} package`, () => {
    const errors = validateOwnershipFixture(
      [
        { name: "framework", location: "crates/framework" },
        { name: "downstream", location: target.location },
      ],
      [{ from: "framework", to: "downstream" }],
    );

    assert.deepEqual(errors, [
      `invalid repository ownership edge: framework -> downstream (framework packages may not depend on ${target.ownership} packages)`,
    ]);
  });
}

for (const target of [
  { ownership: "template", location: "templates/app" },
  { ownership: "tool", location: "tools/cli" },
]) {
  test(`rejects a module dependency on a ${target.ownership} package`, () => {
    const errors = validateOwnershipFixture(
      [
        { name: "identity", location: "modules/identity" },
        { name: "downstream", location: target.location },
      ],
      [{ from: "identity", to: "downstream" }],
    );

    assert.deepEqual(errors, [
      `invalid repository ownership edge: identity -> downstream (module packages may not depend on ${target.ownership} packages)`,
    ]);
  });
}

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
    ]),
    [
      "Identity SQL must be module-owned under modules/identity/sqlx: crates/infrastructure/src/users.rs",
    ],
  );
});
