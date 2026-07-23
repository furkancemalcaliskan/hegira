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
| `backend` | Targets `develop` or `main` | `develop` or `main` | Yes | No | Validation only |
| `full-stack-build` | Targets `develop` or `main`, with packaging path filters | `develop` or `main`, with packaging path filters | Yes | No | Validation only |
| `production-container-smoke` | Targets `develop` or `main`, with production path filters | `develop` or `main`, with production path filters | Yes | No | Disposable local containers only |
| `release` | No | No | Yes, build only | Push matching `v*` | Manual: validation artifact; tag: GitHub Release |

The `repository-policy` workflow validates every pull request to a protected
integration branch. It checks documentation links, agent adapters, pull request
metadata, issue-branch naming, and the supported Dependabot and release
promotion exceptions. It uses no secrets and has read-only repository
permission.

Add the exact `repository-policy` status check to the required checks for both
`develop` and `main`. The check must pass before merge.

A plain push to an issue branch does not trigger the push-based validation
workflows. Updating an open pull request triggers its `pull_request` checks.
Validation workflows cancel superseded runs for the same pull request or
integration ref.

The backend workflow contains three gates:

- `feature-matrix` compiles SQLite, PostgreSQL, WASM hydration, observability,
  and distributed-provider capability sets;
- `quality` runs formatting, DX, Clippy, provider checks, library tests, and
  ignored PostgreSQL integration tests against a disposable service;
- `supply-chain` runs dependency policy and vulnerability checks.

The full-stack build workflow is path-filtered to the deployable package,
workspace crates, frontend inputs, and packaging configuration. It installs
frontend dependencies from the committed npm lockfile, resolves Tailwind from
the repository-local installation, builds the PostgreSQL server and database
migrator, builds the hydrated frontend, and verifies the server, migrator,
WebAssembly, JavaScript, CSS, and branding outputs. It does not create a
platform archive or perform a deployment.

The container workflow is deliberately path-filtered to production build,
configuration, Rust source, and smoke-test inputs. It builds the default
`ssr,db-postgres` image, migrates a disposable PostgreSQL database, verifies
health, readiness, HTML, CSS, and JavaScript, then removes the stack.

## Local Validation

Validate repository documentation, agent adapters, and policy fixtures:

```sh
sh scripts/repository-policy.sh
```

To reproduce pull request metadata validation with a saved GitHub
`pull_request` event:

```sh
sh scripts/repository-policy.sh --event path/to/pull-request-event.json
```

Run the backend gate without ignored PostgreSQL tests:

```sh
sh scripts/backend-check.sh
```

Verify the full-stack release outputs without creating a release archive:

```sh
sh scripts/full-stack-build-check.sh
```

To match the CI quality job, provide a disposable PostgreSQL database and opt
in to destructive ignored tests:

```sh
APP_ENV=test \
DATABASE_URL=postgres://postgres:postgres@localhost:5432/hegira_test \
WITH_IGNORED_DB_TESTS=true \
sh scripts/backend-check.sh
```

Validate the production container contract with Docker:

```sh
sh scripts/container-smoke.sh
```

Never point the ignored database tests at persistent or production data.

## Release Contract

The `release` workflow supports manual release-candidate validation and
tag-triggered publication. A manual run builds and verifies the bundle,
checksum, SBOM, and release notes, but its publish job is skipped. A push to
`develop` or `main` never creates a release, and a pull request never publishes
an artifact. Before creating a `v*` tag, verify:

- every release-gating milestone issue is closed;
- required CI checks are successful on the release commit;
- `develop` has been promoted to `main` through a pull request;
- `CHANGELOG.md` and affected documentation are current;
- release notes are ready.

Run the workflow manually against the intended release commit before tagging
and require its build job to succeed. This is release-candidate validation, not
publication.

Create and verify a signed annotated tag only from the verified `main` release
commit:

```sh
git tag -s vX.Y.Z -m "Hegira vX.Y.Z"
git tag -v vX.Y.Z
git push origin vX.Y.Z
```

Pushing the tag is the publication boundary. Do not create or push it while a
required check, checksum, SBOM, release note, or milestone gate is incomplete.

The tag workflow builds the minimal PostgreSQL server and hydrated frontend,
then publishes a Linux bundle, SHA-256 checksum, and SPDX JSON SBOM to a GitHub
Release. It does not publish a container image.

## No Preview Deployment

CI and production-container smoke validation are not deployments. No committed
workflow creates a preview application, public preview URL, GitHub Deployment,
or GitHub Environment. Validation jobs use no cloud deployment credentials.
Adding preview infrastructure in the future requires an explicit architectural
and security decision; it must not be inferred from the existing CI contract.
