import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { validateConfiguration, validatePullRequest, validateCommit,
  pullRequestMetadata, validateEventCommits, validateRepository, validateWorkflow } from "./repository-policy.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const configuration = JSON.parse(fs.readFileSync(path.join(root, ".github/repository-policy.json"), "utf8"));
// Owner opt-ins must not change the fixtures' deliberately closed default policy.
const policy = { schema: 1, application: configuration.application, dependabot: false, maintenance: [] };
const ordinary = { title: "feat(domain): add order rules", body: "Closes #123", head: "feat/123-order-rules",
  base: "develop", actor: "contributor", headRepository: "contributor/application", baseRepository: "owner/application" };
const promotion = { ...ordinary, head: "develop", base: "main", body: "Verified milestone.",
  title: `release: promote ${policy.application} v1.2.3 to main`, headRepository: ordinary.baseRepository };
const exceptionPolicy = { ...policy, dependabot: true, maintenance: [
  { branch: "chore/update-repository-metadata-develop", base: "develop", title: "chore(repository): update repository metadata" },
  { branch: "chore/update-repository-metadata-main", base: "main", title: "chore(repository): update repository metadata" },
] };
const accepts = (metadata, configuration = policy) => assert.deepEqual(validatePullRequest(metadata, configuration), []);
const rejects = (metadata, configuration = policy) => assert.ok(validatePullRequest(metadata, configuration).length);

test("ordinary fork PRs have issue-free titles and one matching closing line", () => {
  accepts(ordinary);
  accepts({ ...ordinary, body: "Result\n\nCloses #123\n\nValidation" });
  for (const patch of [
    { title: "#123 feat(domain): add order rules" }, { title: "feat: add order rules" },
    { title: "feat(domain): add order rules\nforged" }, { head: "feat/0123-orders" },
    { head: "feature/orders" }, { body: "" }, { body: "Closes #124" },
    { body: "Closes #123\nCloses #123" }, { body: "<!-- Closes #123 -->" },
    { body: "```text\nCloses #123\n```" }, { body: "Closes #123, Closes #124" },
    { body: "Closes #123\nAlso fixes #124" }, { body: "Closes #123\nCloses #124 inline" },
    { base: "main" }, { base: "staging" },
  ]) rejects({ ...ordinary, ...patch });
});

test("branch commits and future squash commit use the branch issue", () => {
  assert.deepEqual(validateCommit("#123 feat(domain): add order rules", ordinary, policy), []);
  for (const title of [ordinary.title, "#124 feat(domain): add order rules", "#0123 feat(domain): add order rules",
    "#123 feat: add order rules", "#123 feat(domain): ", "#123 feat(domain): rules\nforged"]) {
    assert.ok(validateCommit(title, ordinary, policy).length);
  }
});

test("promotion is bound to application identity and same-repository develop", () => {
  accepts(promotion);
  accepts({ ...promotion, title: `release: promote ${policy.application} v1.2.3-rc.1 to main` });
  assert.deepEqual(validateCommit(promotion.title, promotion, policy), []);
  assert.ok(validateCommit(ordinary.title, promotion, policy).length);
  for (const patch of [{ head: "release/123-next" }, { headRepository: "fork/application" },
    { headRepository: "" }, { title: "release: promote another-application v1.2.3 to main" },
    { title: `release: promote ${policy.application} v01.2.3 to main` }, { body: "Closes #123" }]) {
    rejects({ ...promotion, ...patch });
  }
});

test("maintenance exemptions are opt-in exact local branch/base/title tuples", () => {
  for (const entry of exceptionPolicy.maintenance) {
    const metadata = { ...ordinary, head: entry.branch, base: entry.base, title: entry.title, body: "Owner-authorized metadata update.", headRepository: ordinary.baseRepository };
    accepts(metadata, exceptionPolicy);
    rejects(metadata);
    assert.deepEqual(validateCommit(entry.title, metadata, exceptionPolicy), []);
    assert.ok(validateCommit("chore(repository): unrelated", metadata, exceptionPolicy).length);
    for (const patch of [{ base: "staging" }, { title: "chore(repository): anything else" },
      { headRepository: "fork/application" }, { body: "Closes #123" }]) rejects({ ...metadata, ...patch }, exceptionPolicy);
  }
});

test("Dependabot requires explicit opt-in, bot author, local origin and develop", () => {
  const bot = { ...ordinary, head: "dependabot/cargo/dependency-1", actor: "dependabot[bot]",
    title: "Bump dependency from 1 to 2", body: "", headRepository: ordinary.baseRepository };
  accepts(bot, exceptionPolicy);
  rejects(bot);
  assert.deepEqual(validateCommit(bot.title, bot, exceptionPolicy), []);
  for (const patch of [{ actor: "contributor" }, { headRepository: "fork/application" }, { base: "main" }]) rejects({ ...bot, ...patch }, exceptionPolicy);
});

test("configuration rejects broad, malformed and overlapping exemptions", () => {
  assert.deepEqual(validateConfiguration(policy), []);
  assert.deepEqual(validateConfiguration(exceptionPolicy), []);
  for (const invalid of [null, [], { ...policy, schema: 2 }, { ...policy, extra: true },
    { ...policy, application: ".*" }, { ...policy, dependabot: "true" }, { ...policy, maintenance: {} },
    ...[{ branch: "chore/*" }, { branch: "feat/123-orders" }, { base: "staging" }, { title: "anything" }, { actor: "any" }].map(patch => ({ ...policy, maintenance: [{ ...exceptionPolicy.maintenance[0], ...patch }] })),
    { ...policy, maintenance: [exceptionPolicy.maintenance[0], exceptionPolicy.maintenance[0]] }]) {
    assert.ok(validateConfiguration(invalid).length);
  }
});

function eventFor(metadata, baseSha, headSha) {
  return { sender: { login: "irrelevant-editor" }, pull_request: { title: metadata.title, body: metadata.body,
    user: { login: metadata.actor }, head: { ref: metadata.head, sha: headSha, repo: { full_name: metadata.headRepository } },
    base: { ref: metadata.base, sha: baseSha, repo: { full_name: metadata.baseRepository } } } };
}

test("GitHub event parsing uses the PR author and fails closed", () => {
  assert.deepEqual(pullRequestMetadata(eventFor(ordinary)), ordinary);
  assert.equal(pullRequestMetadata(eventFor({ ...ordinary, body: null })).body, "");
  for (const event of [{}, null, { pull_request: {} }, eventFor({ ...ordinary, headRepository: undefined }), eventFor({ ...ordinary, title: {} })]) {
    assert.throws(() => pullRequestMetadata(event));
  }
});

test("real event commit ranges include every branch commit and reject forged SHAs", () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "application-policy-"));
  const git = (...args) => execFileSync("git", args, { cwd: directory, encoding: "utf8", env: { ...process.env,
    GIT_CONFIG_GLOBAL: "/dev/null", GIT_CONFIG_SYSTEM: "/dev/null", GIT_AUTHOR_NAME: "Policy fixture", GIT_AUTHOR_EMAIL: "policy@example.test",
    GIT_COMMITTER_NAME: "Policy fixture", GIT_COMMITTER_EMAIL: "policy@example.test" } }).trim();
  try {
    git("init", "--quiet");
    git("-c", "commit.gpgsign=false", "commit", "--quiet", "--allow-empty", "-m", "fixture base");
    const base = git("rev-parse", "HEAD");
    git("-c", "commit.gpgsign=false", "commit", "--quiet", "--allow-empty", "-m", "#123 feat(domain): order rules");
    const head = git("rev-parse", "HEAD");
    const event = eventFor(ordinary, base, head);
    assert.deepEqual(validateEventCommits(event, directory, policy), []);
    const eventFile = path.join(directory, "event.json");
    fs.writeFileSync(eventFile, JSON.stringify(event));
    fs.mkdirSync(path.join(directory, ".github"));
    fs.writeFileSync(path.join(directory, ".github/repository-policy.json"), JSON.stringify(policy));
    const result = spawnSync(process.execPath, [path.join(root, "scripts/repository-policy.mjs"), "pull-request", "--root", directory, "--event", eventFile], { encoding: "utf8", cwd: directory });
    assert.equal(result.status, 0, result.stderr);
    git("-c", "commit.gpgsign=false", "commit", "--quiet", "--allow-empty", "-m", "#124 fix(domain): wrong issue");
    assert.ok(validateEventCommits(eventFor(ordinary, base, git("rev-parse", "HEAD")), directory, policy).length);
    assert.ok(validateEventCommits(eventFor(ordinary, base, base), directory, policy).length);
    assert.throws(() => validateEventCommits(eventFor(ordinary, base, "--all"), directory, policy));
    assert.throws(() => validateEventCommits(eventFor(ordinary, base, "0".repeat(40)), directory, policy));
    assert.deepEqual(validateEventCommits(eventFor(promotion, base, git("rev-parse", "HEAD")), directory, policy), []);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

test("application repository checks need no framework workspace or Git metadata", () => {
  assert.deepEqual(validateRepository(root), []);
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "application-repository-"));
  try {
    assert.ok(validateRepository(directory).some(error => error.includes("AGENTS.md")));
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

test("workflow preserves fork-safe events, immutable actions and metadata parsing", () => {
  const workflow = fs.readFileSync(path.join(root, ".github/workflows/repository-policy.yml"), "utf8");
  assert.deepEqual(validateWorkflow(workflow), []);
  for (const changed of [workflow.replace("pull_request:", "pull_request_target:"),
    workflow.replace("contents: read", "contents: write"), workflow.replace("persist-credentials: false", "persist-credentials: true"),
    workflow.replace("fetch-depth: 0", "fetch-depth: 1"), workflow.replace("edited, ", ""),
    workflow.replace(/actions\/checkout@[0-9a-f]+/, "actions/checkout@v7"),
    workflow + "\n# secrets.DATABASE_URL\n"]) assert.ok(validateWorkflow(changed).length);
});
