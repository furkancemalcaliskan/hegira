# Hegira Repository Instructions

This file is the canonical working contract for coding agents in this
repository. Tool-specific instruction files must delegate here instead of
copying these rules.

## Sources Of Truth

Read the files relevant to the task before making changes:

- [README.md](README.md) for the project overview and supported capabilities.
- [Architecture](docs/architecture.md) for current crate responsibilities,
  dependency direction, request boundaries, persistence, and runtime roles.
- [Configuration](docs/configuration.md) for compile-time capabilities and
  runtime validation.
- [Contributing](CONTRIBUTING.md) for contribution availability and the
  issue/branch/pull-request contract.
- [Maintainer workflow](docs/maintainers.md) for CI, integration, and release
  behavior.
- [Security policy](SECURITY.md) for private vulnerability reporting.

Source code and committed configuration are authoritative for behavior. Update
affected documentation in the same change when current behavior, paths, or
commands change. Never describe planned work as implemented.

## Current Repository Shape

- The repository root is a virtual Cargo workspace and the documented command
  entry point.
- `crates/` contains the application-manifest contract and
  application-independent platform core, audit, cache, mail,
  search, security, settings, storage, configuration, persistence,
  background-work, HTTP, Leptos, observability, test-support, and runtime
  packages. These packages are application-independent framework source.
- `modules/identity/` contains the canonical Identity Domain Shared, Domain,
  Application Contracts, Application, SQLx, Axum HTTP, and Leptos adapter
  packages.
  The HTTP adapter contributes Identity routes, OpenAPI, Bearer extraction,
  secure session-cookie handling, and explicit cookie/Bearer security policies
  to the host. The Leptos adapter owns Identity pages, server functions, and
  explicit route and navigation contributions. Historical application-owned
  retirement migrations retain their original identities and checksums in the
  canonical application template outside the Identity SQLx migration source.
- `templates/applications/layered/` contains the workspace-external,
  layered full-stack application base. Its brand-neutral server composition
  depends on application-owned layers, framework primitives, and official
  Identity Application, SQLx, HTTP, and Leptos adapters without using legacy
  framework-layer packages. `apps/web` owns the default Leptos shell,
  application navigation and localization, and explicitly selects the Identity
  Leptos adapter directly. It
  consumes framework packages through a pinned release source; repository
  validation rewrites those dependencies only in a disposable staging copy.
  Its Infrastructure operation surface owns startup migration and Identity
  seed composition plus explicitly authorized disposable-database reset for
  validation; it does not use compatibility migration helpers or `db_migrator`.
  Each render owns a validated `hegira.toml` containing only application
  identity, framework source/version, selected components, and database/client
  adapters. Runtime configuration and secrets do not belong in that manifest.
- `templates/package.toml` identifies the versioned canonical, data-only
  component package, its compatible framework release source, contained
  templates/components, and locked source digest. `templates/components/`
  contains that package's typed component manifests. Package-controlled
  framework source variables cannot be overridden by a normal render.
- `tools/template_renderer/` contains the reusable deterministic and atomic
  render core plus an explicitly separate repository-validation adapter. The
  normal render contract preserves pinned release sources; only disposable
  maintainer checks may select the adapter that rewrites them. It is not the
  public Hegira CLI.
- `tools/hegira_cli/` contains the source-runnable `hegira` command shell,
  deterministic non-interactive layered application creation, stable process
  outcomes, and user-facing diagnostic contract.
- `scripts/` contains local validation and release helpers.
- `.github/workflows/` contains validation and release automation.

Do not create future-facing directories, manifests, modules, tools, clients, or
capabilities unless an accepted issue explicitly requires a working
implementation.

## Setup And Run

Create the current canonical application with the source-runnable CLI, then
install its development prerequisites:

```sh
cargo run --locked -p hegira_cli -- new my-application \
  --destination ../my-application
cd ../my-application
rustup target add wasm32-unknown-unknown
cargo install cargo-leptos
npm ci --prefix apps/web/src
```

Start the current SQLite profile:

```sh
APP_ENV=sqlite cargo leptos watch -p app_server \
  --bin-features ssr,db-sqlite --lib-features hydrate
```

The application listens on `http://127.0.0.1:3000`. See
`docs/getting-started.md` before selecting PostgreSQL or enabling another
capability.

## Before Editing

For implementation work:

1. Verify the accepted GitHub issue, milestone, scope, dependencies, and
   acceptance criteria.
2. Inspect `git status`, the current branch, recent history, and existing user
   changes.
3. Preserve unrelated tracked, untracked, ignored, and stashed work.
4. Create the issue branch from the latest `develop`; never implement directly
   on `develop` or `main`.
5. Keep the change limited to the accepted issue.

Use issue branches in this form:

```text
<type>/<issue>-<short-description>
```

Supported types are `feat`, `fix`, `refactor`, `test`, `docs`, `ci`, `release`,
and `chore`.

## Architecture Boundaries

Preserve the dependency direction and responsibilities documented in
`docs/architecture.md`.

- Keep domain and application rules independent from Axum, Leptos, SQLx, and
  vendor SDKs.
- Keep controllers and Leptos server functions transport-focused; delegate
  business validation and authorization to application services.
- Treat UI permission checks as presentation only. Protected operations require
  application-layer authorization.
- Keep cookie-authenticated browser/BFF policy separate from Bearer API policy.
- Keep transactions at use-case boundaries rather than HTTP-request scope.
- Prefer explicit typed composition over service locators, runtime reflection,
  or automatic endpoint publication.
- Keep provider-specific SQL and migrations explicit.
- Validate configuration and compiled capabilities before initializing external
  dependencies.

Do not bypass an architectural boundary merely to make a check pass. If the
accepted issue requires changing a boundary, update the implementation,
enforcement, and current architecture documentation together.

## Validation

Use the smallest relevant checks while iterating, then run the checks required
by the affected contract.

Standard repository and backend validation:

```sh
sh scripts/repository-policy.sh
sh scripts/backend-check.sh
```

Focused framework and official-module validation:

```sh
sh scripts/framework-check.sh
sh scripts/official-modules-check.sh
```

Focused source-runnable CLI validation:

```sh
sh scripts/cli-check.sh
```

Focused workspace dependency-boundary validation:

```sh
sh scripts/architecture-boundaries.sh
```

Focused canonical layered application validation:

```sh
sh scripts/layered-template-check.sh
```

Rendered provider, upgrade, and production-container validation:

```sh
sh scripts/generated-application-check.sh
```

Focused release identity and source-first workflow validation:

```sh
sh scripts/release-policy.sh
```

Basic focused commands include:

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo run --locked -p template_renderer -- render \
  --repository-root . --template layered --output /tmp/hegira-layered
```

PostgreSQL tests marked `ignored` reset their target database. Run them only
with the explicit opt-in and a disposable database as documented in
`docs/maintainers.md`. Never point tests, migrations, reset commands, or smoke
checks at persistent or production data.

For documentation-only changes, validate formatting, relative links, and the
changed contract. Do not run destructive or unrelated checks solely for
ceremony.

## Change Discipline

- Make the smallest coherent change that satisfies the issue.
- Do not delete, rewrite, or revert unrelated user work.
- Do not weaken tests, security controls, production defaults, or quality gates
  to obtain a green result.
- Do not add dependencies, abstractions, providers, or compatibility layers
  without a current use case in issue scope.
- Do not silently change public behavior or claim support that is not tested.
- Use repository scripts and established patterns before adding new tooling.
- Keep generated artifacts, credentials, local databases, and build outputs out
  of commits.

If a required action is destructive, changes external state, expands issue
scope, or needs a product decision not present in the repository, stop and
request maintainer direction.

## Git And GitHub Authority

Commit messages begin with:

```text
#<issue> <type>(<scope>): <description>
```

Ordinary issue pull request titles omit the issue number and use
`<type>(<scope>): <description>`. Link the issue in the body with
`Closes #<issue>`. Ordinary issue pull requests target `develop` and use squash
merge; set the resulting squash commit title to the issue-prefixed commit
format. The verified milestone is promoted from `develop` to `main` with the
release title and merge method documented in `CONTRIBUTING.md`.

Preparing local changes does not authorize publication. Do not commit, push,
create or modify a pull request, merge, tag, release, deploy, change repository
settings, or perform another external mutation without explicit maintainer
authority for that action.

## Security

- Never expose or commit credentials, tokens, private keys, personal data,
  production logs, database dumps, or secret configuration.
- Do not trust request metadata, client-side checks, or forwarded headers
  outside the documented security policy.
- Preserve least-privilege workflow permissions and keep untrusted pull-request
  execution free of secrets.
- Report suspected vulnerabilities through GitHub private vulnerability
  reporting as required by `SECURITY.md`.

## Completion

Before handing work back:

1. Review the complete diff and repository status.
2. Confirm the issue scope and acceptance criteria are satisfied.
3. Run and report the relevant checks, including anything not run.
4. Summarize changed files, behavior impact, risks, and remaining decisions.
5. Provide separate issue-prefixed commit and issue-free pull-request titles
   when requested, but leave external mutations to the maintainer unless
   explicitly authorized.

These instructions guide agent behavior. Deterministic repository rules remain
enforced by scripts, tests, CI, and protected-branch settings where available.
