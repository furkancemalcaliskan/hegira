# Contributing to Hegira

Hegira is open-source software under active architectural development. The
project is not currently accepting unsolicited external code contributions.

Do not open a pull request unless the maintainer has explicitly requested or
approved the contribution in advance. Unsolicited pull requests may be closed
without review. This restriction is temporary and can be reconsidered when the
project's public architecture and extension contracts are stable enough to
support external development safely.

Bug reports, technical feedback, and architectural discussions remain welcome
through the repository's GitHub issues. Suspected security vulnerabilities must
be reported privately as described in the [security policy](SECURITY.md), never
through a public issue, discussion, or pull request.

## Authorized Changes

Every maintainer-authored code or documentation change must start from an
accepted GitHub issue assigned to one milestone. The issue must be ready for
development before its branch is created.

Use this integration path:

```text
issue -> issue branch -> pull request -> develop
      -> release promotion pull request -> main -> signed tag -> release
```

Create issue branches from the latest `develop`. Use one of these forms:

```text
feat/<issue>-<short-description>
fix/<issue>-<short-description>
refactor/<issue>-<short-description>
test/<issue>-<short-description>
docs/<issue>-<short-description>
ci/<issue>-<short-description>
release/<issue>-<short-description>
chore/<issue>-<short-description>
```

Commit messages must begin with the related issue number:

```text
#<issue> <type>(<scope>): <description>
```

For example:

```text
#53 docs(governance): define the repository operating contract
```

Ordinary issue pull request titles omit the issue number and use:

```text
<type>(<scope>): <description>
```

The pull request body links the issue with `Closes #<issue>`. Dependabot pull
requests keep their generated titles. A release promotion from `develop` to
`main` uses:

```text
release: promote hegira vX.Y.Z to main
```

Dependabot updates are the only standing exception to the issue and branch
requirements. They still require review and all checks required by their target
branch.

## Pull Requests

Ordinary issue pull requests target `develop` and use squash merge. The squash
commit title must be set to the issue-prefixed commit format even though the
pull request title omits the issue number.

Only the completed and verified milestone is promoted from `develop` to `main`.
That promotion uses a pull request and a merge commit so the integration
history remains explicit. Do not develop directly on `develop` or `main`.

An authorized pull request must:

- stay within the accepted issue scope;
- preserve unrelated work already present in the repository;
- describe the result, validation, risks, and security impact;
- update affected documentation when current behavior or paths change;
- avoid presenting planned capabilities as implemented;
- pass all checks required by the target branch.

Use the repository pull request template and resolve every review conversation
before merge.

## Validation

Run validation appropriate to the change. The standard repository policy check
is:

```sh
sh scripts/repository-policy.sh
```

The aggregate backend gate validates framework, official-module, and canonical template
ownership:

```sh
sh scripts/backend-check.sh
```

Changes to the layered template, renderer, migrations, or generated runtime contract must also
pass the generated-application gate with disposable Docker state:

```sh
sh scripts/generated-application-check.sh
```

Never point ignored integration tests at persistent or production data. See the
[maintainer workflow](docs/maintainers.md) for the current CI and release
contract.

## Authority

Creating a branch and preparing changes does not grant publication authority.
Commit, push, pull-request creation, merge, tag, release, deployment, and other
external mutations require explicit maintainer authorization.
