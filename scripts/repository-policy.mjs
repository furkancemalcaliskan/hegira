import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const CHANGE_TYPES = [
  "feat",
  "fix",
  "refactor",
  "test",
  "docs",
  "ci",
  "release",
  "chore",
];

const TYPE_PATTERN = CHANGE_TYPES.join("|");
const PULL_REQUEST_TITLE = new RegExp(
  `^(?:${TYPE_PATTERN})\\([a-z0-9][a-z0-9._/-]*\\): \\S(?:.*\\S)?$`,
);
const ISSUE_BRANCH = new RegExp(
  `^(?:${TYPE_PATTERN})/([1-9][0-9]*)-[a-z0-9]+(?:[._-][a-z0-9]+)*$`,
);
const RELEASE_TITLE =
  /^release: promote hegira v[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)? to main$/;

const REQUIRED_FILES = [
  "AGENTS.md",
  "CLAUDE.md",
  ".cursor/rules/hegira.mdc",
];

function readText(file) {
  return fs.readFileSync(file, "utf8");
}

function stripFencedCode(markdown) {
  const output = [];
  let fence = null;

  for (const line of markdown.split("\n")) {
    const match = line.match(/^\s{0,3}(`{3,}|~{3,})/);
    if (match) {
      const marker = match[1][0];
      if (fence === null) {
        fence = marker;
      } else if (fence === marker) {
        fence = null;
      }
      continue;
    }

    if (fence === null) {
      output.push(line);
    }
  }

  return output.join("\n");
}

function markdownDestinations(markdown) {
  const content = stripFencedCode(markdown);
  const destinations = [];

  for (const match of content.matchAll(/!?\[[^\]]*]\(([^)]+)\)/g)) {
    destinations.push(match[1]);
  }

  for (const match of content.matchAll(/^\s*\[[^\]]+]:\s*(\S+)/gm)) {
    destinations.push(match[1]);
  }

  return destinations;
}

function normalizedLocalDestination(rawDestination) {
  let destination = rawDestination.trim();

  if (destination.startsWith("<")) {
    const closing = destination.indexOf(">");
    if (closing === -1) {
      return destination;
    }
    destination = destination.slice(1, closing);
  } else {
    destination = destination.split(/\s+/, 1)[0];
  }

  if (
    destination === "" ||
    destination.startsWith("#") ||
    /^[A-Za-z][A-Za-z0-9+.-]*:/.test(destination)
  ) {
    return null;
  }

  const withoutFragment = destination.split("#", 1)[0].split("?", 1)[0];
  if (withoutFragment === "") {
    return null;
  }

  try {
    return decodeURIComponent(withoutFragment);
  } catch {
    return withoutFragment;
  }
}

function markdownFiles(root) {
  const files = [];

  for (const entry of fs.readdirSync(root, { withFileTypes: true })) {
    if (entry.isFile() && entry.name.endsWith(".md")) {
      files.push(path.join(root, entry.name));
    }
  }

  const visit = (directory) => {
    if (!fs.existsSync(directory)) {
      return;
    }

    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const file = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        visit(file);
      } else if (entry.isFile() && entry.name.endsWith(".md")) {
        files.push(file);
      }
    }
  };

  visit(path.join(root, "docs"));

  const pullRequestTemplate = path.join(
    root,
    ".github",
    "PULL_REQUEST_TEMPLATE.md",
  );
  if (fs.existsSync(pullRequestTemplate)) {
    files.push(pullRequestTemplate);
  }

  return [...new Set(files)].sort();
}

export function validateRepository(root) {
  const errors = [];

  for (const required of REQUIRED_FILES) {
    const requiredFile = path.join(root, required);
    if (!fs.existsSync(requiredFile) || !fs.statSync(requiredFile).isFile()) {
      errors.push(`required agent instruction file is missing: ${required}`);
    }
  }

  const claudeFile = path.join(root, "CLAUDE.md");
  if (fs.existsSync(claudeFile)) {
    const firstInstruction = readText(claudeFile)
      .split("\n")
      .find((line) => line.trim() !== "");
    if (firstInstruction?.trim() !== "@AGENTS.md") {
      errors.push("CLAUDE.md must import @AGENTS.md as its first instruction");
    }
  }

  const cursorFile = path.join(root, ".cursor", "rules", "hegira.mdc");
  if (fs.existsSync(cursorFile)) {
    const cursorRule = readText(cursorFile);
    if (!/^alwaysApply:\s*true\s*$/m.test(cursorRule)) {
      errors.push("Cursor rule must set alwaysApply: true");
    }
    if (!/^@AGENTS\.md\s*$/m.test(cursorRule)) {
      errors.push("Cursor rule must reference @AGENTS.md");
    }
  }

  for (const file of markdownFiles(root)) {
    const relativeFile = path.relative(root, file);
    for (const rawDestination of markdownDestinations(readText(file))) {
      const destination = normalizedLocalDestination(rawDestination);
      if (destination === null) {
        continue;
      }

      const resolved = destination.startsWith("/")
        ? path.join(root, destination.slice(1))
        : path.resolve(path.dirname(file), destination);
      const relativeResolved = path.relative(root, resolved);

      if (
        relativeResolved === ".." ||
        relativeResolved.startsWith(`..${path.sep}`) ||
        path.isAbsolute(relativeResolved)
      ) {
        errors.push(
          `${relativeFile} references a path outside the repository: ${destination}`,
        );
        continue;
      }

      if (!fs.existsSync(resolved)) {
        errors.push(
          `${relativeFile} references missing relative path: ${destination}`,
        );
      }
    }
  }

  return errors;
}

function closingIssues(body) {
  return [
    ...body.matchAll(/\bCloses\s+#([1-9][0-9]*)\b/gi),
  ].map((match) => match[1]);
}

export function validatePullRequest(metadata) {
  const title = metadata.title ?? "";
  const body = metadata.body ?? "";
  const head = metadata.head ?? "";
  const base = metadata.base ?? "";
  const actor = metadata.actor ?? "";
  const headRepository = metadata.headRepository ?? "";
  const baseRepository = metadata.baseRepository ?? "";
  const errors = [];

  const dependabot =
    actor === "dependabot[bot]" && head.startsWith("dependabot/");

  if (dependabot) {
    if (base !== "develop") {
      errors.push(
        `Dependabot pull requests must target develop; received base: ${base}`,
      );
    }
    return errors;
  }

  if (base === "main") {
    if (head !== "develop") {
      errors.push(
        `only develop may be promoted to main; received source branch: ${head}`,
      );
    }
    if (
      headRepository !== "" &&
      baseRepository !== "" &&
      headRepository !== baseRepository
    ) {
      errors.push("release promotion must originate from this repository");
    }
    if (!RELEASE_TITLE.test(title)) {
      errors.push(
        "release promotion title must match: release: promote hegira vX.Y.Z to main",
      );
    }
    return errors;
  }

  if (base !== "develop") {
    errors.push(
      `ordinary issue pull requests must target develop; received base: ${base}`,
    );
    return errors;
  }

  if (/^#[1-9][0-9]*\s/.test(title)) {
    errors.push(
      "ordinary pull request titles must not start with an issue number",
    );
  }
  if (!PULL_REQUEST_TITLE.test(title)) {
    errors.push(
      "ordinary pull request title must match: <type>(<scope>): <description>",
    );
  }

  const branch = head.match(ISSUE_BRANCH);
  if (branch === null) {
    errors.push(
      "ordinary issue branch must match: <type>/<issue>-<short-description>",
    );
  }

  const issues = closingIssues(body);
  if (issues.length !== 1) {
    errors.push(
      "ordinary pull request body must contain exactly one: Closes #<issue>",
    );
  } else if (branch !== null && issues[0] !== branch[1]) {
    errors.push(
      `pull request closes #${issues[0]} but source branch belongs to #${branch[1]}`,
    );
  }

  return errors;
}

export function pullRequestMetadata(event) {
  if (event.pull_request === undefined) {
    throw new Error("event does not contain pull_request metadata");
  }

  return {
    title: event.pull_request.title,
    body: event.pull_request.body,
    head: event.pull_request.head?.ref,
    base: event.pull_request.base?.ref,
    actor: event.pull_request.user?.login ?? event.sender?.login,
    headRepository: event.pull_request.head?.repo?.full_name,
    baseRepository: event.pull_request.base?.repo?.full_name,
  };
}

function optionValue(argumentsList, option) {
  const index = argumentsList.indexOf(option);
  if (index === -1 || index + 1 >= argumentsList.length) {
    return null;
  }
  return argumentsList[index + 1];
}

function report(name, errors) {
  if (errors.length === 0) {
    process.stdout.write(`${name}: ok\n`);
    return;
  }

  for (const error of errors) {
    process.stderr.write(`${name} violation: ${error}\n`);
  }
  process.exitCode = 1;
}

function run(argumentsList) {
  const command = argumentsList[0];

  if (command === "repository") {
    const root = path.resolve(optionValue(argumentsList, "--root") ?? ".");
    report("repository policy", validateRepository(root));
    return;
  }

  if (command === "pull-request") {
    const eventPath = optionValue(argumentsList, "--event");
    if (eventPath === null) {
      throw new Error("pull-request requires --event <path>");
    }

    const event = JSON.parse(readText(path.resolve(eventPath)));
    report(
      "pull request policy",
      validatePullRequest(pullRequestMetadata(event)),
    );
    return;
  }

  throw new Error(
    "usage: repository-policy.mjs <repository|pull-request> [options]",
  );
}

const invokedFile = process.argv[1]
  ? fs.realpathSync(path.resolve(process.argv[1]))
  : null;
const currentFile = fs.realpathSync(fileURLToPath(import.meta.url));

if (invokedFile === currentFile) {
  try {
    run(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`repository policy error: ${error.message}\n`);
    process.exitCode = 1;
  }
}
