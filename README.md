<p align="center">
  <img src="assets/branding/hegira-logo.png" width="320" alt="Hegira logo">
</p>

<h1 align="center">Hegira</h1>

<p align="center"><strong>Your destination. A proven path.</strong></p>

<p align="center">
  An opinionated full-stack application template for the journey from idea to production.
</p>

Hegira is a production-oriented, opinionated application foundation built around Axum,
Leptos, SQLx, an ABP-inspired layered architecture, and DDD-oriented layers. Its canonical
full-stack template provides more than an empty starter: authentication, authorization,
persistence, observability, background work, deployment, and security are approached through
sensible defaults and explicit conventions.

The product and its destination remain yours. Hegira provides a proven path through recurring
architectural and infrastructure decisions so that development can focus on the application
itself rather than rebuilding the same foundation.

## The Hegira Philosophy

Every application begins as a journey into uncertain territory. Before developers can build
what makes their product unique, they must repeatedly solve the same problems: architecture,
authentication, authorization, persistence, observability, background work, deployment, and
security.

Hegira exists so that this journey does not have to begin from an empty map.

Its philosophy is inspired by the caravan: a group of travelers following a route shaped by
accumulated experience. A caravan does not choose the travelers' destination, nor does it
prevent them from taking another path. It carries shared knowledge, reduces avoidable risk,
and allows each traveler to focus on the purpose of the journey rather than the mechanics of
survival.

Hegira applies the same principle to software development. It provides a production-oriented
path, sensible defaults, and explicit conventions for recurring architectural decisions. These
conventions are guides, not walls. Applications remain free to evolve, replace components, and
leave the established path whenever their requirements demand it.

The destination belongs to the developer. Hegira simply makes the road clearer.

**Your destination. A proven path.**

## Stack

- Axum HTTP API and middleware
- Leptos SSR with client hydration
- SQLx with PostgreSQL and SQLite adapters
- ABP-inspired layered architecture with DDD-oriented domain and application layers
- RBAC, audit logging, OAuth, TOTP, and secure web sessions
- Durable jobs, scheduler, health checks, metrics, and tracing
- Single-process or separated web/worker deployment

The template supports a containers-free SQLite development profile and a PostgreSQL deployment
profile. Redis, Meilisearch, S3, SMTP, Prometheus, and OpenTelemetry remain optional compile-time
capabilities.

## Repository Model

Hegira separates reusable framework code from official modules and generated application
ownership:

| Surface | Responsibility |
|---|---|
| `crates/` | Application-independent framework primitives, application-manifest contract, providers, and runtime support |
| `modules/identity/` | Canonical source for the official layered Identity module and its SQLx, Axum, and Leptos adapters |
| `templates/applications/layered/` | Workspace-external, brand-neutral layered application base with application-owned server, web, configuration, migration composition, and deployment files |
| `templates/package.toml` | Versioned canonical component-package identity, framework compatibility, component graph, and source digest |
| `templates/components/` | Typed data-only component manifests that define the canonical application composition |
| `tools/hegira_cli/` | Source-runnable `hegira new` with guided and non-interactive application creation, stable diagnostics, and exit outcomes |
| `tools/template_renderer/` | Reusable deterministic render core with a separate disposable repository-validation adapter; it is not a public CLI |

The canonical rendered application is an independent Cargo workspace, consumes framework
packages from a pinned release source, and records its generation identity and selected
components in a validated `hegira.toml`. Runtime configuration and secrets remain outside that
manifest. The canonical package locks its source inputs with a deterministic SHA-256 digest so
repository-local or untracked files cannot silently enter generated output. Releases remain
source-only; the CLI is not distributed through crates.io or as a standalone
executable. Keep the source tree available when running the source-built CLI.

The source-runnable CLI can create the canonical layered application non-interactively with
explicit, automation-friendly inputs:

```sh
cargo run --locked -p hegira_cli -- --help
cargo run --locked -p hegira_cli -- new my-application \
  --destination ../my-application
```

See [Building and invoking the CLI](docs/getting-started.md#build-and-invoke-the-cli)
for source requirements and the distinction between released and unreleased generation inputs.

Run `cargo run --locked -p hegira_cli -- new` in an interactive terminal for a guided workflow.
The prompts show the implemented choices and defaults, summarize the resulting application, and
ask for confirmation before any files are written. Non-interactive terminals never wait for
prompt input and require the application name and destination explicitly.
Supplying both inputs skips prompts and confirmation. Only SQLite/PostgreSQL,
Leptos, and Identity selections are supported; generation does not provide
module management, code generators, or automatic upgrades.

Its stable process outcomes are success (`0`), internal error (`1`), usage
error (`2`), validation error (`3`), and conflict (`4`). Human-readable help
and version output use standard output; diagnostics use standard error.

## Quick Start

Create the default layered application with SQLite, Leptos, and the official Identity
component. Generation writes files only; it does not install tools, run migrations, initialize
Git, or execute generated code:

```sh
cargo run --locked -p hegira_cli -- new my-application \
  --destination ../my-application
cd ../my-application
```

Pass `--database postgres` to make PostgreSQL the generated application's selected and default
database adapter. `--client leptos` and `--component identity` may be supplied explicitly for
automation; they are the currently supported client and official component selections.

Use a new destination under an existing real parent directory. Existing entries
are never overwritten. See [Getting started](docs/getting-started.md) for identity,
path, and safe-publication platform requirements.

Install the Rust WASM target, `cargo-leptos`, and lockfile-pinned frontend
tooling:

```sh
rustup target add wasm32-unknown-unknown
cargo install cargo-leptos
npm ci --prefix apps/web/src
```

Start with SQLite and no external services:

```sh
APP_ENV=sqlite cargo leptos watch -p app_server \
  --bin-features ssr,db-sqlite --lib-features hydrate
```

Open `http://127.0.0.1:3000`. The SQLite profile seeds the development admin
configured in the generated application's `config/sqlite.yaml`. Development startup owns its
SQLite database creation, migrations, and configured seed behavior.

## Documentation

- [Getting started](docs/getting-started.md)
- [Architecture](docs/architecture.md)
- [Configuration](docs/configuration.md)
- [Deployment](docs/deployment.md)
- [Operations](docs/operations.md)
- [Maintainer workflow](docs/maintainers.md)
- [Contribution policy](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
- [Changelog](CHANGELOG.md)

## Quality Gate

Validate repository governance and workspace dependency boundaries:

```sh
sh scripts/repository-policy.sh
```

Run the aggregate backend gate for framework, official-module, template, and
source-runnable CLI ownership:

```sh
sh scripts/backend-check.sh
```

Validate CLI-generated SQLite and PostgreSQL applications, including their fresh and upgrade migration paths,
production image, and HTTP contract, with disposable Docker state:

```sh
sh scripts/generated-application-check.sh
```

The CI official-module job sets `WITH_IGNORED_DB_TESTS=true` and supplies a
disposable PostgreSQL database. The generated-application job is the sole owner of application
database, provider, upgrade, container, hydration, and HTTP integration coverage.

PostgreSQL tests marked `ignored` require a disposable `DATABASE_URL` because
they reset the target database.

## Integration Workflow

`develop` is the integration branch for milestone work and `main` contains
release-ready history. Create issue branches from `develop`, open pull requests
back to `develop`, and promote a verified milestone from `develop` to `main`
through a pull request before tagging a release.

The repository validation workflow runs for pull requests targeting `develop`
or `main`, post-merge pushes to those branches, and explicit manual dispatches.
It separates framework, official-module, tooling, generated-application,
capability-matrix, and supply-chain responsibilities. Every capability-matrix
entry renders and compiles the canonical application as an external framework
consumer. The stable `quality`
status succeeds only when every repository ownership gate passes. Plain
feature-branch pushes do not run these gates unless they update an open pull
request. Superseded runs for the same pull request or integration ref are
cancelled.

Release automation publishes source-first GitHub Releases only for stable
`vMAJOR.MINOR.PATCH` tags. GitHub provides the source archives, and the
source-scoped SPDX SBOM is the only custom release asset. Release automation validates the
framework, official modules, rendering tooling, and generated application; it does not publish
an executable, crate, CLI package, container image, preview application, public URL, or GitHub
Environment. Immutable GitHub Releases receive GitHub's automatic release attestation. See
the [maintainer workflow](docs/maintainers.md) for the complete trigger and
release contract.

## Project Policy

This project is published as open-source software, but external code
contributions are not currently accepted unless explicitly requested or
approved by the maintainer. See [CONTRIBUTING.md](CONTRIBUTING.md) for the
current policy. Security vulnerabilities should be reported privately
according to [SECURITY.md](SECURITY.md).

## License

[Apache 2.0](LICENSE.txt)
