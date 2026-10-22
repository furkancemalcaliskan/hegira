// Repository-only composed-contract fixture. It never renders or changes source.
import assert from "node:assert/strict";
import { readFileSync, realpathSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { spawnSync } from "node:child_process";

const root = realpathSync(process.argv[2]);
const { manifest, plans, server } = JSON.parse(readFileSync(0, "utf8"));
const read = path => readFileSync(join(root, path), "utf8");
const load = path => import(pathToFileURL(join(root, path)).href);
const policy = await load("scripts/repository-policy.mjs");
const integration = await load("scripts/ci-integration.mjs");
const configuration = JSON.parse(read(".github/repository-policy.json"));
assert.equal(configuration.application, manifest.application);
assert.deepEqual(policy.validateRepository(root), []);

const documents = ["README.md", "AGENTS.md", "CLAUDE.md", ".cursor/rules/application.mdc",
  "docs/architecture.md", "docs/development.md", "docs/ownership.md", "docs/repository.md",
  ".github/PULL_REQUEST_TEMPLATE.md"];
const headingId = heading => heading.toLowerCase().replace(/[^\p{L}\p{N}_\s-]/gu, "").replace(/ /g, "-");
for (const document of documents) {
  const content = read(document);
  assert.ok(!/scripts\/(?:backend|framework|official-modules|cli|layered-template)-check\.sh|tools\/template_renderer|modules\/identity\//.test(content), `${document}: framework-only entry point`);
  for (const [, destination] of content.matchAll(/\]\(([^)]+)\)/g)) {
    if (/^(?:https?:|mailto:)/.test(destination)) continue;
    const [path, anchor] = destination.split("#");
    const target = realpathSync(resolve(dirname(join(root, document)), path));
    const local = relative(root, target);
    assert.ok(local !== ".." && !local.startsWith("../"), `${document}: escaping link ${destination}`);
    if (anchor) {
      const headings = [...readFileSync(target, "utf8").matchAll(/^#{1,6} (.+)$/gm)].map(match => headingId(match[1]));
      assert.ok(headings.includes(anchor), `${document}: missing anchor ${destination}`);
    }
  }
}
assert.ok(read("CLAUDE.md").startsWith("@AGENTS.md\n"));
for (const document of ["CLAUDE.md", ".cursor/rules/application.mdc"]) {
  assert.ok(read(document).includes("This adapter adds no separate project rules."));
  assert.ok(!/--execute|squash|#<issue>|Domain/.test(read(document)), `${document}: duplicated policy`);
}

// Use the actual examples in canonical instructions as inputs to the delivered
// validator, rather than reproducing its branch/title/closing-line regexes.
const agents = read("AGENTS.md").replace(/\s+/g, " ");
const repository = read("docs/repository.md").replace(/\s+/g, " ");
const example = agents.match(/branch `([^`]+)` uses commit `([^`]+)`, PR title `([^`]+)`, and body `([^`]+)`/);
assert.ok(example, "canonical ordinary issue example");
const [, head, commit, title, body] = example;
const metadata = { head, title, body, base: "develop", actor: "fixture",
  headRepository: "fork/application", baseRepository: "owner/application" };
assert.deepEqual(policy.validatePullRequest(metadata, configuration), []);
assert.deepEqual(policy.validateCommit(commit, metadata, configuration), []);
assert.notDeepEqual(policy.validateCommit(title, metadata, configuration), []);
for (const document of [agents, repository]) {
  for (const convention of ["<type>/<issue>-<short-description>", "#<issue> <type>(<scope>): <description>", "Closes #<issue>"]) {
    assert.ok(document.includes(convention), `missing shared convention ${convention}`);
  }
  const release = document.match(/`(release: promote [^`]+)`/);
  assert.ok(release);
  const promotion = { ...metadata, head: "develop", base: "main", body: "Verified milestone.",
    headRepository: metadata.baseRepository, title: release[1].replace("vX.Y.Z", "v1.2.3") };
  assert.deepEqual(policy.validatePullRequest(promotion, configuration), []);
  assert.deepEqual(policy.validateCommit(promotion.title, promotion, configuration), []);
}
assert.ok(read(".github/PULL_REQUEST_TEMPLATE.md").includes("\nCloses #<issue>\n"));

const [database] = manifest.selection.databases;
assert.equal(manifest.selection.databases.length, 1);
assert.deepEqual(manifest.selection.clients, ["leptos"]);
const profile = database === "sqlite" ? "sqlite" : "development";
assert.equal(integration.provider, database);
assert.equal(integration.profile, profile);
assert.deepEqual(server.features.default, [`db-${database}`]);
const development = read("docs/development.md");
assert.ok(development.includes(`config/${profile}.yaml`));
assert.ok(development.includes(`ssr,db-${database}`));
assert.ok(!development.includes(`ssr,db-${database === "sqlite" ? "postgres" : "sqlite"}`));
assert.ok(read(`config/${profile}.yaml`).includes(`backend: ${database}`));
assert.ok(agents.includes(`selected provider is \`${database}\``));
assert.ok(read(".github/workflows/quality.yml").includes(`Locked ${database} native and WASM checks`));
for (const [operation, plan] of Object.entries(plans)) {
  assert.equal(plan.database, database);
  assert.deepEqual(plan.modules, manifest.composition.modules ?? []);
  assert.deepEqual(plan.capabilities, manifest.composition.capabilities);
  const native = plan.steps[0];
  const features = native.arguments[native.arguments.indexOf(operation === "dev" || operation === "build" ? "--bin-features" : "--features") + 1];
  assert.ok(features.includes(`db-${database}`), `${operation}: selected native provider`);
  assert.ok(native.arguments.some(argument => argument.includes("--locked")), `${operation}: locked source`);
  if (operation === "check" || operation === "test") {
    assert.ok(native.arguments.includes("--no-default-features"));
    assert.ok(plan.steps[1].arguments.includes("hydrate"));
    assert.ok(plan.steps[1].arguments.includes("wasm32-unknown-unknown"));
  } else {
    assert.equal(native.arguments[native.arguments.indexOf("--lib-features") + 1], "hydrate");
  }
}

// Required context names in hosting instructions must match the emitted jobs.
const workflow = read(".github/workflows/quality.yml");
const aggregate = workflow.split("\n  quality:\n")[1];
assert.ok(aggregate, "stable aggregate job");
const qualityName = aggregate.match(/^    name: (.+)$/m)[1];
const policyName = read(".github/workflows/repository-policy.yml").match(/^    name: (.+)$/m)[1];
assert.ok(repository.includes(`Require \`${policyName}\` and \`${qualityName}\``));
const checks = [...aggregate.matchAll(/^          (\w+_RESULT): \$\{\{ needs\.([\w-]+)\.result \}\}$/gm)];
assert.equal(checks.length, 4);
const needs = aggregate.match(/^    needs: \[([^\]]+)\]/m)[1].split(", ");
assert.deepEqual(checks.map(([, , job]) => job).sort(), needs.sort());
assert.ok(aggregate.includes("    if: always()\n"));
const shell = aggregate.split("        run: |\n")[1].split("\n")
  .filter(line => line.trim()).map(line => line.replace(/^          /, "")).join("\n");
const successful = Object.fromEntries(checks.map(([, variable]) => [variable, "success"]));
assert.equal(spawnSync("sh", ["-ec", shell], { env: successful }).status, 0);
for (const [, variable] of checks) {
  for (const result of ["failure", "cancelled", "skipped", ""]) {
    assert.notEqual(spawnSync("sh", ["-ec", shell], { env: { ...successful, [variable]: result } }).status, 0, `${variable}=${result}`);
  }
}
console.log("composed application governance: ok");
