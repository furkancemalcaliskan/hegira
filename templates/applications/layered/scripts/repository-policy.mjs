import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const types = "feat|fix|refactor|test|docs|ci|release|chore";
const branchPattern = new RegExp(`^(?:${types})/([1-9][0-9]*)-[a-z0-9]+(?:[._-][a-z0-9]+)*$`);
const titlePattern = new RegExp(`^(?:${types})\\([a-z0-9][a-z0-9._/-]*\\): \\S(?:[^\\r\\n]*\\S)?$`);
const commitPattern = new RegExp(`^#([1-9][0-9]*) ((?:${types})\\([a-z0-9][a-z0-9._/-]*\\): \\S(?:[^\\r\\n]*\\S)?)$`);
const versionPattern = "v(?:0|[1-9][0-9]*)\\.(?:0|[1-9][0-9]*)\\.(?:0|[1-9][0-9]*)(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?";
const requiredFiles = [
  "AGENTS.md", "CLAUDE.md", ".cursor/rules/application.mdc", "README.md",
  "docs/architecture.md", "docs/development.md", "docs/ownership.md", "docs/repository.md",
  "hegira.toml", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".node-version",
  ".github/repository-policy.json", ".github/PULL_REQUEST_TEMPLATE.md",
  ".github/workflows/repository-policy.yml", "scripts/repository-policy.sh",
  "scripts/repository-policy.mjs", "scripts/repository-policy.test.mjs",
];

export function validateConfiguration(policy) {
  const errors = [];
  if (!policy || typeof policy !== "object" || Array.isArray(policy)) return ["policy must be an object"];
  if (Object.keys(policy).sort().join(",") !== "application,dependabot,maintenance,schema") errors.push("unexpected policy keys");
  if (policy.schema !== 1) errors.push("policy schema must be 1");
  if (typeof policy.application !== "string" || !/^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/.test(policy.application)) errors.push("invalid application identity");
  if (typeof policy.dependabot !== "boolean") errors.push("dependabot must be a boolean");
  if (!Array.isArray(policy.maintenance)) return [...errors, "maintenance must be an array"];
  const branches = new Set();
  for (const entry of policy.maintenance) {
    if (!entry || typeof entry !== "object" || Array.isArray(entry)) { errors.push("invalid maintenance entry"); continue; }
    if (Object.keys(entry).sort().join(",") !== "base,branch,title") errors.push("unexpected maintenance keys");
    if (typeof entry.branch !== "string" || !/^chore\/[a-z0-9]+(?:-[a-z0-9]+)*$/.test(entry.branch)
        || branchPattern.test(entry.branch) || branches.has(entry.branch)) errors.push("maintenance needs a unique, exact non-issue chore branch");
    branches.add(entry.branch);
    if (!["develop", "main"].includes(entry.base)) errors.push("invalid maintenance base");
    if (typeof entry.title !== "string" || !titlePattern.test(entry.title) || !entry.title.startsWith("chore(")) errors.push("maintenance needs an exact chore title");
  }
  return errors;
}

function loadPolicy(root) {
  const policy = JSON.parse(fs.readFileSync(path.join(root, ".github/repository-policy.json"), "utf8"));
  const errors = validateConfiguration(policy);
  if (errors.length) throw new Error(errors.join("; "));
  return policy;
}

function closingIssues(body) {
  // Ignore examples/comments: only standalone, visible closing lines count.
  const visible = body.replace(/<!--[\s\S]*?-->/g, "").replace(/^\s*(`{3,}|~{3,})[^\n]*\n[\s\S]*?^\s*\1[^\n]*$/gm, "");
  const standalone = [...visible.matchAll(/^\s*Closes #([1-9][0-9]*)\s*$/gim)].map(match => match[1]);
  const references = [...visible.matchAll(/\b(?:close[sd]?|fix(?:es|ed)?|resolve[sd]?)\s+#([1-9][0-9]*)\b/gi)];
  // Reject extra GitHub closing syntax even when one valid line is present.
  return references.length === standalone.length ? standalone : [...standalone, "invalid closing syntax"];
}

function sameRepository(metadata) {
  return metadata.headRepository !== "" && metadata.headRepository === metadata.baseRepository;
}

function kind(metadata, policy) {
  if (policy.maintenance.some(entry => entry.branch === metadata.head)) return "maintenance";
  if (policy.dependabot && metadata.actor === "dependabot[bot]" && metadata.head.startsWith("dependabot/")) return "dependabot";
  if (metadata.base === "main") return "promotion";
  return "issue";
}

export function validatePullRequest(metadata, policy) {
  const errors = validateConfiguration(policy);
  if (errors.length) return errors;
  if (!metadata || ["title", "body", "head", "base", "actor", "headRepository", "baseRepository"].some(key => typeof metadata[key] !== "string")) return ["incomplete pull request metadata"];
  const issues = closingIssues(metadata.body);
  switch (kind(metadata, policy)) {
    case "maintenance": {
      const entry = policy.maintenance.find(entry => entry.branch === metadata.head);
      if (metadata.base !== entry.base || metadata.title !== entry.title || !sameRepository(metadata) || issues.length) errors.push("maintenance must match its exact branch/base/title, originate locally, and close no issue");
      break;
    }
    case "dependabot":
      if (metadata.base !== "develop" || !sameRepository(metadata) || !metadata.title.trim()) errors.push("Dependabot must originate locally and target develop with a nonempty title");
      break;
    case "promotion": {
      const promotion = new RegExp(`^release: promote ${policy.application} ${versionPattern} to main$`);
      if (metadata.head !== "develop" || !sameRepository(metadata) || !promotion.test(metadata.title) || issues.length) errors.push("promotion requires local develop -> main, the configured application release title, and no closing issue");
      break;
    }
    default: {
      const branch = metadata.head.match(branchPattern);
      if (metadata.base !== "develop") errors.push("ordinary PRs must target develop");
      if (!titlePattern.test(metadata.title)) errors.push("PR title must be issue-free: <type>(<scope>): <description>");
      if (!branch) errors.push("branch must match <type>/<issue>-<short-description>");
      if (issues.length !== 1 || (branch && issues[0] !== branch[1])) errors.push("body must contain exactly one standalone Closes #<issue> matching the branch");
    }
  }
  return errors;
}

export function validateCommit(title, metadata, policy) {
  const errors = validatePullRequest(metadata, policy);
  if (errors.length) return errors;
  if (typeof title !== "string" || !title.trim() || /[\r\n]/.test(title)) return ["invalid commit title"];
  switch (kind(metadata, policy)) {
    case "dependabot": return [];
    case "maintenance": return title === metadata.title ? [] : ["maintenance commit must use the configured title"];
    case "promotion": return title === metadata.title ? [] : ["promotion merge commit must use the release title"];
    default: {
      const commit = title.match(commitPattern);
      return commit && commit[1] === metadata.head.match(branchPattern)[1] ? [] : ["commit must start #<branch issue> <type>(<scope>): <description>"];
    }
  }
}

export function pullRequestMetadata(event) {
  const pr = event?.pull_request;
  if (!pr || [pr.title, pr.head?.ref, pr.base?.ref, pr.user?.login, pr.head?.repo?.full_name, pr.base?.repo?.full_name].some(value => typeof value !== "string" || value === "") || (pr.body !== null && typeof pr.body !== "string")) throw new Error("invalid pull_request event");
  return { title: pr.title, body: pr.body ?? "", head: pr.head.ref, base: pr.base.ref,
    actor: pr.user.login, headRepository: pr.head.repo.full_name, baseRepository: pr.base.repo.full_name };
}

export function validateEventCommits(event, root, policy) {
  const metadata = pullRequestMetadata(event);
  const errors = validatePullRequest(metadata, policy);
  if (errors.length) return errors;
  for (const sha of [event.pull_request.base.sha, event.pull_request.head.sha]) {
    if (typeof sha !== "string" || !/^[0-9a-f]{40}$/.test(sha)) throw new Error("event must provide exact base/head commit SHAs");
    execFileSync("git", ["cat-file", "-e", `${sha}^{commit}`], { cwd: root, stdio: "pipe" });
  }
  // develop's integrated history was checked in its issue PRs. A future
  // promotion merge commit does not exist yet and remains an owner review.
  if (kind(metadata, policy) === "promotion") return [];
  const subjects = execFileSync("git", ["log", "--format=%s%x00", `${event.pull_request.base.sha}..${event.pull_request.head.sha}`], { cwd: root, encoding: "utf8", maxBuffer: 1024 * 1024 }).split("\0\n").filter(Boolean);
  if (!subjects.length) return ["pull request contains no commits"];
  return subjects.flatMap(subject => validateCommit(subject.trimEnd(), metadata, policy));
}

export function validateRepository(root) {
  const errors = [];
  for (const file of requiredFiles) {
    if (!fs.existsSync(path.join(root, file)) || !fs.lstatSync(path.join(root, file)).isFile()) errors.push(`missing regular application file: ${file}`);
  }
  try { loadPolicy(root); } catch (error) { errors.push(error.message); }
  const workflowFile = path.join(root, ".github/workflows/repository-policy.yml");
  if (fs.existsSync(workflowFile)) errors.push(...validateWorkflow(fs.readFileSync(workflowFile, "utf8")));
  if (fs.existsSync(path.join(root, "CLAUDE.md")) && !fs.readFileSync(path.join(root, "CLAUDE.md"), "utf8").startsWith("@AGENTS.md\n")) errors.push("Claude must import canonical AGENTS.md first");
  if (fs.existsSync(path.join(root, ".cursor/rules/application.mdc"))) {
    const cursor = fs.readFileSync(path.join(root, ".cursor/rules/application.mdc"), "utf8");
    if (!/^alwaysApply: true$/m.test(cursor) || !/^@AGENTS\.md$/m.test(cursor)) errors.push("Cursor must always apply canonical AGENTS.md");
  }
  return errors;
}

export function validateWorkflow(source) {
  const errors = [];
  for (const required of ["  pull_request:\n", "  push:\n", "  workflow_dispatch:\n", "branches: [develop, main]",
    "types: [opened, synchronize, reopened, edited, ready_for_review]", "  contents: read\n",
    "    timeout-minutes: 10\n", "fetch-depth: 0", "persist-credentials: false", "node-version-file: .node-version",
    "run: sh scripts/repository-policy.sh\n", "if: github.event_name == 'pull_request'",
    'run: sh scripts/repository-policy.sh --event "$GITHUB_EVENT_PATH"']) {
    if (!source.includes(required)) errors.push(`workflow is missing required contract: ${required.trim()}`);
  }
  if (/pull_request_target|workflow_run|secrets\.|:\s*write\b|\$\{\{/.test(source)) errors.push("policy workflow must use read-only events without secrets or expression interpolation");
  const actions = [...source.matchAll(/uses:\s*(\S+)/g)].map(match => match[1]);
  if (actions.length !== 2 || !actions[0].match(/^actions\/checkout@[0-9a-f]{40}$/) || !actions[1].match(/^actions\/setup-node@[0-9a-f]{40}$/)) errors.push("workflow must use exactly immutable checkout/setup-node actions");
  return errors;
}

function run(args) {
  const command = args.shift();
  const options = {};
  while (args.length) {
    const key = args.shift();
    if (!["--root", "--event", "--title"].includes(key) || !args.length || key in options) throw new Error("invalid policy arguments");
    options[key] = args.shift();
  }
  const root = path.resolve(options["--root"] ?? ".");
  let errors;
  if (command === "repository" && !options["--event"] && !options["--title"]) errors = validateRepository(root);
  else if (["pull-request", "commit"].includes(command) && options["--event"] && (command === "commit") === (options["--title"] !== undefined)) {
    const policy = loadPolicy(root);
    const event = JSON.parse(fs.readFileSync(options["--event"], "utf8"));
    errors = command === "commit" ? validateCommit(options["--title"], pullRequestMetadata(event), policy) : validateEventCommits(event, root, policy);
  } else throw new Error("usage: repository-policy.mjs repository --root <path> | pull-request --root <path> --event <path> | commit --root <path> --event <path> --title <title>");
  if (errors.length) { for (const error of errors) console.error(`policy violation: ${error}`); process.exitCode = 1; }
  else console.log("application repository policy: ok");
}

if (process.argv[1] && fs.realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { run(process.argv.slice(2)); } catch (error) { console.error(`policy error: ${error.message}`); process.exitCode = 1; }
}
