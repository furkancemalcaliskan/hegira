import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";

import {
  WORKSPACE_DEPENDENCY_POLICY,
  validateWorkspaceMetadata,
} from "./architecture-boundaries.mjs";

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
      const directory =
        name === "hegira"
          ? path.join(root, "apps", name)
          : path.join(root, "crates", name);
      return {
        id: `workspace#${name}`,
        name,
        manifest_path: path.join(directory, "Cargo.toml"),
        dependencies: edges
          .filter((edge) => edge.from === name)
          .map((edge) => ({
            name: edge.to,
            path:
              edge.to === "hegira"
                ? path.join(root, "apps", edge.to)
                : path.join(root, "crates", edge.to),
          })),
      };
    }),
  };
}

test("accepts the documented current workspace graph", () => {
  assert.deepEqual(validateWorkspaceMetadata(workspaceMetadata()), []);
});

test("accepts existing runtime and presentation composition edges", () => {
  const metadata = workspaceMetadata();
  const errors = validateWorkspaceMetadata(metadata);
  assert.deepEqual(errors, []);
  assert.ok(
    WORKSPACE_DEPENDENCY_POLICY.runtime.includes("web") &&
      WORKSPACE_DEPENDENCY_POLICY.presentation.includes("infrastructure"),
  );
});

test("rejects a dependency from a reusable crate to the deployable app", () => {
  const errors = validateWorkspaceMetadata(
    workspaceMetadata([{ from: "web", to: "hegira" }]),
  );
  assert.ok(
    errors.some(
      (error) =>
        error.includes("web -> hegira") &&
        error.includes("must not depend on deployable packages"),
    ),
  );
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
