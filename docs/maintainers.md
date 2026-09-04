# Maintainer Workflow

This document defines how repository validation, integration, and release
automation are separated. GitHub repository settings should protect `develop`
and `main` consistently with this contract.

## Branch And Pull Request Flow

Use the following path for milestone work:

```text
issue branch -> pull request -> develop -> promotion pull request -> main -> v* tag -> release
```

Every maintainer-authored milestone change starts from an accepted GitHub issue
assigned to one milestone. Move the issue to `In Progress` when its branch is
created, `In Review` when its pull request is ready for review, and `Done` after
merge.

Create each issue branch from the latest `develop` using:

```text
<type>/<issue>-<short-description>
```

Supported types are `feat`, `fix`, `refactor`, `test`, `docs`, `ci`, `release`,
and `chore`. Commit messages begin with:

```text
#<issue> <type>(<scope>): <description>
```

Ordinary issue pull request titles omit the issue number:

```text
<type>(<scope>): <description>
```

Link the issue in the pull request body with `Closes #<issue>`. Ordinary issue
pull requests target `develop` and use squash merge. Set the resulting squash
commit title to the issue-prefixed commit format before merging. Dependabot
pull requests are the standing exception to the issue and branch requirements;
they keep their generated titles but still require review and all checks
required by `develop`.

Promote a completed milestone from `develop` to `main` only after its required
issues and validation gates pass. The promotion pull request uses a merge
commit and the following title:

```text
release: promote hegira vX.Y.Z to main
```

Do not develop directly on `develop` or `main`. Commit, push, pull-request
creation, merge, tag, release, deployment, and other external mutations require
explicit maintainer authorization. The current contribution availability and
authorized change requirements are defined in
[CONTRIBUTING.md](../CONTRIBUTING.md).

## GitHub Actions Contract

| Workflow | Pull request | Branch push | Manual | Tag | Side effect |
|---|---|---|---|---|---|
| `repository-policy` | Targets `develop` or `main` | No | No | No | Validation only |
| `repository-validation` | Targets `develop` or `main` | `develop` or `main` | Yes | No | Validation with disposable databases and containers |
| `release` | No | No | `main` only | Push matching `v*.*.*` | Manual: validation artifacts; tag: source-first GitHub Release |

The `repository-policy` workflow validates every pull request to a protected
integration branch. It checks documentation links, agent adapters, pull request
metadata, issue-branch naming, workspace dependency boundaries, and the
source-first release contract, together with the supported Dependabot and
release promotion exceptions. It uses no secrets and has read-only repository
permission.

Keep these exact status checks required for both `develop` and `main`:

- `repository-policy`
- `quality`
- `supply-chain`
- `feature-matrix (sqlite-server)`
- `feature-matrix (postgres-server)`
- `feature-matrix (wasm-hydrate)`
- `feature-matrix (observability)`
- `feature-matrix (distributed-providers)`

The stable `quality` context is an aggregate gate. It reports failure unless
the `framework`, `official-modules`, `tooling`, and `generated-application`
jobs all succeed. Generated-application database, release-build, production
container, and HTTP/security validation is therefore release-blocking without
requiring a new protected-branch context.

A plain push to an issue branch does not trigger the push-based validation
workflows. Updating an open pull request triggers its `pull_request` checks.
Validation workflows cancel superseded runs for the same pull request or
integration ref.

The repository validation workflow separates these responsibilities:

- `feature-matrix` independently renders the canonical application and compiles
  its SQLite, PostgreSQL, WASM hydration, observability, and distributed-provider
  capability sets as an external framework consumer;
- `framework` validates only application-independent framework packages under
  minimal and all-capability contracts;
- `official-modules` validates the canonical Identity packages directly,
  including ignored SQLx contracts against a disposable PostgreSQL service;
- `tooling` validates the DX baseline, source-runnable CLI, rendering tool,
  component manifests, workspace-external layered application, locked
  dependency boundaries, hydration, and release output;
- `generated-application` validates fresh SQLite and PostgreSQL applications,
  the supported v0.2.0 upgrade, locked application dependency boundaries, and
  the rendered production container;
- `quality` aggregates the four repository ownership gates under the existing
  required status context;
- `supply-chain` runs dependency policy and vulnerability checks.

Disposable PostgreSQL containers use trust authentication only inside isolated
GitHub-hosted runners. They contain disposable test data, expose no repository
secret, and are destroyed after validation.

The former compatibility-host full-stack and production-container workflows and
scripts are retired. The generated-application job is the single integration
owner for those contracts.

## Local Validation

Run the commands in this section from the repository root. The virtual workspace coordinates
framework packages under `crates/`, official module packages under `modules/`, and internal
repository tooling under `tools/`. The canonical application under `templates/` remains an
independent workspace and is rendered into disposable directories for integration validation.

Validate repository documentation, agent adapters, and policy fixtures:

```sh
sh scripts/repository-policy.sh
```

This standard repository check includes the locked-metadata package allowlist,
repository-location ownership boundary, and source-first release policies. Run
those focused contracts directly with:

```sh
sh scripts/architecture-boundaries.sh
sh scripts/release-policy.sh
```

Validate the source-runnable CLI command, diagnostic, and process-outcome
contracts:

```sh
sh scripts/cli-check.sh
```

The CLI tests invoke the compiled `hegira` binary without user-home or global
configuration variables. They verify default and explicit application
selections, independent release-source dependencies, deterministic output,
destination conflicts, interactive default equivalence, supported-choice
mapping, cancellation, non-TTY behavior, and the absence of global
configuration requirements. Prompt tests inject deterministic input and capture
output without relying on a host terminal.

Validate the workspace-external canonical layered application base against the
current framework checkout:

```sh
sh scripts/layered-template-check.sh
```

The check works on a disposable copy. The reusable render core preserves pinned
release-style dependencies and does not write maintainer paths into template
source files. Repository checks explicitly select the separate validation
adapter, which patches declared framework dependencies only in the disposable
output. These local-source options are absent from the normal renderer command.
Before either path plans output, it verifies the canonical package identity,
framework compatibility, declared component set, and locked source digest.
After an intentional package-source change, calculate the replacement digest
with the same library contract used by rendering:

```sh
cargo run --locked -p template_renderer --example package_digest -- \
  --repository-root . --template layered
```

Review the complete package diff before replacing `content_digest` in
`templates/package.toml`.
The check runs
the renderer snapshot and failure-path tests, installs the client package lock,
validates the rendered workspace's direct application and Hegira dependencies,
validates native workspace targets and tests, compiles the hydration target,
and produces the full-stack Cargo Leptos release output.

The renderer is an internal maintainer tool rather than the public Hegira CLI.
To inspect an independently copyable release-style render:

```sh
cargo run --locked -p template_renderer -- render \
  --repository-root . \
  --template layered \
  --output /tmp/hegira-layered
```

The destination must not already exist. Component manifests declare their
requirements, conflicts, source inputs, and repository-validation dependency
patches. They cannot execute shell commands.

Validate a rendered application against both database providers, the supported
v0.2.0 database upgrade, and the production container contract:

```sh
sh scripts/generated-application-check.sh
```

The check renders into a disposable directory, runs SQLite fresh-install and
upgrade tests in memory, and starts an ephemeral PostgreSQL container for the
equivalent PostgreSQL contracts. It then builds the rendered application image,
boots it against the disposable database, and verifies readiness, hydration
assets, security headers, and unauthenticated Bearer API behavior. The check
also validates the rendered workspace's locked direct dependency graph and
rejects retired compatibility packages. It stages a credential-free framework
source view under the disposable render so
the same relative Cargo paths work on the host and inside the Docker build. It
generates runtime-only database and JWT values and removes its containers,
network, database state, and rendered output on exit. It never targets the
maintainer's configured database.

To reproduce pull request metadata validation with a saved GitHub
`pull_request` event:

```sh
sh scripts/repository-policy.sh --event path/to/pull-request-event.json
```

Run the backend gate without ignored PostgreSQL tests:

```sh
sh scripts/backend-check.sh
```

This aggregate runs the framework, official-module, canonical layered
application, and source-runnable CLI tooling checks. Run an ownership gate
directly while iterating when the change is confined to that surface:

```sh
sh scripts/framework-check.sh
sh scripts/official-modules-check.sh
sh scripts/layered-template-check.sh
sh scripts/cli-check.sh
```

To include the framework and official-module gates' ignored PostgreSQL tests locally, provide
a disposable PostgreSQL database and opt in explicitly:

```sh
APP_ENV=test \
DATABASE_URL=postgres://postgres:postgres@localhost:5432/hegira_test \
WITH_IGNORED_DB_TESTS=true \
sh scripts/backend-check.sh
```

Never point the ignored database tests at persistent or production data.

## Release Contract

Hegira is distributed as source. A release consists of an immutable signed
stable SemVer tag, a GitHub Release, versioned release notes, GitHub-provided
source archives, and a source-scoped SPDX JSON SBOM. It does not contain a
platform executable, application bundle, published crate or CLI package,
official container image, or deployment.

The `release` workflow supports manual release-candidate validation from
`main` and publication from a pushed `vMAJOR.MINOR.PATCH` tag. Both paths:

- verify that the release ref, every workspace package version, the dated
  changelog entry, and the versioned release-note identity agree;
- verify that registry publication remains disabled for every workspace
  package;
- generate and verify an SPDX JSON SBOM from a clean checkout before build
  outputs exist;
- validate application-independent framework packages directly;
- validate official Identity module packages, including SQLx contracts against
  a disposable PostgreSQL database;
- validate typed rendering tooling, the independent layered workspace,
  hydration, and release output;
- validate fresh SQLite and PostgreSQL generated applications, supported
  v0.2.0 upgrades, and the rendered production container and HTTP contract.

A manual run uploads the source SBOM as a short-lived workflow artifact but
cannot execute the publication job. A push to `develop` or `main` never creates
a release, and a pull request never publishes an artifact. Before creating a
tag, verify:

- every release-gating milestone issue is closed;
- required CI checks are successful on the release commit;
- `develop` has been promoted to `main` through a pull request;
- `CHANGELOG.md`, every workspace package version, and affected documentation
  are current;
- `docs/releases/vX.Y.Z.md` is ready;
- a manual release workflow run on the intended `main` commit succeeds.

This manual run is release-candidate validation, not publication.

Create and verify a signed annotated tag only from the verified `main` release
commit:

```sh
git tag -s vX.Y.Z -m "Hegira vX.Y.Z"
git tag -v vX.Y.Z
git push origin vX.Y.Z
```

Pushing the tag is the publication boundary. Do not create or push it while a
required check, SBOM, release note, or milestone gate is incomplete. The tag
workflow repeats the release validation and uses `gh release create
--verify-tag` to publish the versioned notes and source SBOM. GitHub
automatically provides `.zip` and `.tar.gz` source archives for the tagged
source.

Enable GitHub's release immutability setting for the repository. Publication
then locks the associated tag and uploaded SBOM asset and automatically creates
a release attestation covering the release tag, commit, and assets. Verify an
immutable published release with `gh release verify vX.Y.Z`, and verify a
downloaded SBOM with `gh release verify-asset vX.Y.Z <path>`. Published tags
and releases must not be deleted, moved, recreated, or replaced. Correct a
released defect with a new patch version. The workflow creates a new release
and never updates an existing one.

Versioned release notes are historical records. The v0.1.x notes continue to
describe the platform bundle artifacts actually published for those versions;
the source-first contract applies beginning with v0.2.0.

## No Preview Deployment

CI and production-container smoke validation are not deployments. No committed
workflow creates a preview application, public preview URL, GitHub Deployment,
or GitHub Environment. Validation jobs use no cloud deployment credentials.
Adding preview infrastructure in the future requires an explicit architectural
and security decision; it must not be inferred from the existing CI contract.
