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
| `crates/` | Application-independent framework primitives, layered compatibility packages, providers, and runtime support |
| `modules/identity/` | Canonical source for the official layered Identity module and its SQLx, Axum, and Leptos adapters |
| `templates/applications/layered/` | Workspace-external, brand-neutral layered application base with application-owned server, web, configuration, migration composition, and deployment files |
| `templates/components/` | Typed component manifests that define the canonical application composition |
| `tools/template_renderer/` | Internal deterministic renderer used by repository validation; it is not a public CLI |
| `apps/hegira/` | Current deployable compatibility host used to validate framework integration |

The canonical rendered application is an independent Cargo workspace and consumes framework
packages from a pinned release source. Releases are source-first, and the repository does not
currently implement a public application-generation command.

## Quick Start

Run the following commands from the repository root to start the current compatibility host.
The root coordinates the Cargo workspace, while the deployable package lives at
`apps/hegira`. See [Architecture](docs/architecture.md) for the separate canonical generated
application contract.

Install the Rust WASM target, `cargo-leptos`, and lockfile-pinned frontend
tooling:

```sh
rustup target add wasm32-unknown-unknown
cargo install cargo-leptos
npm ci --prefix crates/web/src
```

Start with SQLite and no external services:

```sh
APP_ENV=sqlite cargo run -p db_migrator --no-default-features --features ssr,db-sqlite -- migrate
APP_ENV=sqlite cargo leptos watch -p hegira --bin-features ssr,db-sqlite --lib-features hydrate
```

Open `http://127.0.0.1:3000`. The SQLite profile seeds the development admin
configured in `config/sqlite.yaml`.

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

Run the aggregate backend gate for framework, official-module, and template ownership:

```sh
sh scripts/backend-check.sh
```

Validate the canonical rendered application, including its fresh and upgrade migration paths,
production image, and HTTP contract, with disposable Docker state:

```sh
sh scripts/generated-application-check.sh
```

The CI framework and official-module jobs also set `WITH_IGNORED_DB_TESTS=true` and supply
disposable PostgreSQL databases. Verify the compatibility host's full-stack release outputs
without creating a platform archive:

```sh
sh scripts/full-stack-build-check.sh
```

Verify the production container contract separately when Docker is available:

```sh
sh scripts/container-smoke.sh
```

PostgreSQL tests marked `ignored` require a disposable `DATABASE_URL` because
they reset the target database.

## Integration Workflow

`develop` is the integration branch for milestone work and `main` contains
release-ready history. Create issue branches from `develop`, open pull requests
back to `develop`, and promote a verified milestone from `develop` to `main`
through a pull request before tagging a release.

The repository validation workflow runs for pull requests targeting `develop`
or `main`, post-merge pushes to those branches, and explicit manual dispatches.
It separates framework, official-module, template, generated-application,
capability-matrix, and supply-chain responsibilities. The stable `quality`
status succeeds only when every repository ownership gate passes. Plain
feature-branch pushes do not run these gates unless they update an open pull
request. Superseded runs for the same pull request or integration ref are
cancelled.

Release automation publishes source-first GitHub Releases only for stable
`vMAJOR.MINOR.PATCH` tags. GitHub provides the source archives, and the
source-scoped SPDX SBOM is the only custom release asset. Release automation validates the
framework, official modules, canonical template, and generated application; it does not publish
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
