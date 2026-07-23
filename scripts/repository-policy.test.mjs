import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  pullRequestMetadata,
  validatePullRequest,
  validateRepository,
} from "./repository-policy.mjs";

function ordinaryPullRequest(overrides = {}) {
  return {
    title: "ci(policy): enforce repository contracts",
    body: "## Related Issue\n\nCloses #55\n",
    head: "ci/55-repository-policy",
    base: "develop",
    actor: "maintainer",
    headRepository: "furkancemalcaliskan/hegira",
    baseRepository: "furkancemalcaliskan/hegira",
    ...overrides,
  };
}

test("accepts a valid ordinary issue pull request", () => {
  assert.deepEqual(validatePullRequest(ordinaryPullRequest()), []);
});

test("rejects an issue-prefixed pull request title", () => {
  const errors = validatePullRequest(
    ordinaryPullRequest({
      title: "#55 ci(policy): enforce repository contracts",
    }),
  );
  assert.ok(errors.some((error) => error.includes("must not start")));
});

test("rejects an invalid Conventional Commit title", () => {
  const errors = validatePullRequest(
    ordinaryPullRequest({ title: "Repository policy" }),
  );
  assert.ok(errors.some((error) => error.includes("<type>(<scope>)")));
});

test("rejects an unsupported issue branch type", () => {
  const errors = validatePullRequest(
    ordinaryPullRequest({ head: "feature/55-repository-policy" }),
  );
  assert.ok(errors.some((error) => error.includes("issue branch must match")));
});

test("rejects a missing closing issue", () => {
  const errors = validatePullRequest(ordinaryPullRequest({ body: "Summary" }));
  assert.ok(errors.some((error) => error.includes("exactly one")));
});

test("rejects a closing issue that differs from the branch issue", () => {
  const errors = validatePullRequest(
    ordinaryPullRequest({ body: "Closes #56" }),
  );
  assert.ok(errors.some((error) => error.includes("#56")));
});

test("rejects an ordinary issue pull request targeting main", () => {
  const errors = validatePullRequest(ordinaryPullRequest({ base: "main" }));
  assert.ok(errors.some((error) => error.includes("only develop")));
});

test("accepts a Dependabot pull request to develop", () => {
  const errors = validatePullRequest({
    title: "build(deps): bump dependencies",
    body: "",
    head: "dependabot/cargo/rust-dependencies",
    base: "develop",
    actor: "dependabot[bot]",
    headRepository: "furkancemalcaliskan/hegira",
    baseRepository: "furkancemalcaliskan/hegira",
  });
  assert.deepEqual(errors, []);
});

test("rejects a Dependabot pull request to main", () => {
  const errors = validatePullRequest({
    title: "build(deps): bump dependencies",
    body: "",
    head: "dependabot/cargo/rust-dependencies",
    base: "main",
    actor: "dependabot[bot]",
    headRepository: "furkancemalcaliskan/hegira",
    baseRepository: "furkancemalcaliskan/hegira",
  });
  assert.ok(errors.some((error) => error.includes("must target develop")));
});

test("accepts a valid release promotion", () => {
  const errors = validatePullRequest({
    title: "release: promote hegira v0.2.0 to main",
    body: "",
    head: "develop",
    base: "main",
    actor: "maintainer",
    headRepository: "furkancemalcaliskan/hegira",
    baseRepository: "furkancemalcaliskan/hegira",
  });
  assert.deepEqual(errors, []);
});

test("rejects an invalid release promotion source branch", () => {
  const errors = validatePullRequest({
    title: "release: promote hegira v0.2.0 to main",
    body: "",
    head: "release/61-v0.2.0",
    base: "main",
    actor: "maintainer",
    headRepository: "furkancemalcaliskan/hegira",
    baseRepository: "furkancemalcaliskan/hegira",
  });
  assert.ok(errors.some((error) => error.includes("only develop")));
});

test("rejects an invalid release promotion title", () => {
  const errors = validatePullRequest({
    title: "release: v0.2.0",
    body: "",
    head: "develop",
    base: "main",
    actor: "maintainer",
    headRepository: "furkancemalcaliskan/hegira",
    baseRepository: "furkancemalcaliskan/hegira",
  });
  assert.ok(errors.some((error) => error.includes("release promotion title")));
});

test("rejects a release promotion from a fork branch named develop", () => {
  const errors = validatePullRequest({
    title: "release: promote hegira v0.2.0 to main",
    body: "",
    head: "develop",
    base: "main",
    actor: "contributor",
    headRepository: "contributor/hegira",
    baseRepository: "furkancemalcaliskan/hegira",
  });
  assert.ok(errors.some((error) => error.includes("this repository")));
});

test("extracts pull request metadata from an event object", () => {
  assert.deepEqual(
    pullRequestMetadata({
      pull_request: {
        title: "ci(policy): enforce repository contracts",
        body: "## Related Issue\n\nCloses #55\n",
        head: {
          ref: "ci/55-repository-policy",
          repo: { full_name: "furkancemalcaliskan/hegira" },
        },
        base: {
          ref: "develop",
          repo: { full_name: "furkancemalcaliskan/hegira" },
        },
        user: { login: "maintainer" },
      },
    }),
    ordinaryPullRequest(),
  );
});

function createRepositoryFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hegira-policy-"));
  fs.mkdirSync(path.join(root, ".cursor", "rules"), { recursive: true });
  fs.mkdirSync(path.join(root, ".github"), { recursive: true });
  fs.mkdirSync(path.join(root, "docs"), { recursive: true });

  fs.writeFileSync(
    path.join(root, "AGENTS.md"),
    [
      "# Instructions",
      "",
      "[Current documentation](docs/current.md)",
      "[Section](#instructions)",
      "[External](https://example.com/docs)",
      "",
    ].join("\n"),
  );
  fs.writeFileSync(path.join(root, "CLAUDE.md"), "@AGENTS.md\n");
  fs.writeFileSync(
    path.join(root, ".cursor", "rules", "hegira.mdc"),
    "---\nalwaysApply: true\n---\n\n@AGENTS.md\n",
  );
  fs.writeFileSync(
    path.join(root, ".github", "PULL_REQUEST_TEMPLATE.md"),
    "# Pull Request\n",
  );
  fs.writeFileSync(path.join(root, "docs", "current.md"), "# Current\n");

  return root;
}

test("accepts valid repository documentation and agent adapters", (context) => {
  const root = createRepositoryFixture();
  context.after(() => fs.rmSync(root, { recursive: true, force: true }));
  assert.deepEqual(validateRepository(root), []);
});

test("rejects a missing relative Markdown destination", (context) => {
  const root = createRepositoryFixture();
  context.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.appendFileSync(path.join(root, "docs", "current.md"), "[Missing](nope.md)\n");
  const errors = validateRepository(root);
  assert.ok(errors.some((error) => error.includes("nope.md")));
});

test("rejects a Markdown destination outside the repository", (context) => {
  const root = createRepositoryFixture();
  context.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.appendFileSync(
    path.join(root, "docs", "current.md"),
    "[Escape](../../outside.md)\n",
  );
  const errors = validateRepository(root);
  assert.ok(errors.some((error) => error.includes("outside the repository")));
});

test("rejects a missing agent instruction entry point", (context) => {
  const root = createRepositoryFixture();
  context.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.rmSync(path.join(root, "AGENTS.md"));
  const errors = validateRepository(root);
  assert.ok(errors.some((error) => error.includes("AGENTS.md")));
});

test("rejects a broken Claude adapter", (context) => {
  const root = createRepositoryFixture();
  context.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.writeFileSync(path.join(root, "CLAUDE.md"), "# Instructions\n");
  const errors = validateRepository(root);
  assert.ok(errors.some((error) => error.includes("CLAUDE.md")));
});

test("rejects a broken Cursor adapter", (context) => {
  const root = createRepositoryFixture();
  context.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.writeFileSync(
    path.join(root, ".cursor", "rules", "hegira.mdc"),
    "---\nalwaysApply: false\n---\n",
  );
  const errors = validateRepository(root);
  assert.ok(errors.some((error) => error.includes("Cursor")));
});
