# Hegira

A production-oriented Full-Stack Rust web application template built with Axum, Leptos, SQLx,
and an ABP-inspired layered architecture.

The template supports a containers-free SQLite development profile and a
PostgreSQL deployment profile. Redis, Meilisearch, S3, SMTP, Prometheus, and
OpenTelemetry remain optional compile-time capabilities.

## Stack

- Axum HTTP API and middleware
- Leptos SSR with client hydration
- SQLx with PostgreSQL and SQLite adapters
- DDD-oriented domain and application layers
- RBAC, audit logging, OAuth, TOTP, and secure web sessions
- Durable jobs, scheduler, health checks, metrics, and tracing
- Single-process or separated web/worker deployment

## Quick Start

Install the Rust WASM target and `cargo-leptos`:

```sh
rustup target add wasm32-unknown-unknown
cargo install cargo-leptos
```

Start with SQLite and no external services:

```sh
APP_ENV=sqlite cargo run -p db_migrator --no-default-features --features ssr,db-sqlite -- migrate
APP_ENV=sqlite cargo leptos watch --bin-features ssr,db-sqlite --lib-features hydrate
```

Open `http://127.0.0.1:3000`. The SQLite profile seeds the development admin
configured in `config/sqlite.yaml`.

## Documentation

- [Getting started](docs/getting-started.md)
- [Architecture](docs/architecture.md)
- [CRUD tutorial](docs/crud-tutorial.md)
- [Configuration](docs/configuration.md)
- [Deployment](docs/deployment.md)
- [Operations](docs/operations.md)
- [Maintainer workflow](docs/maintainers.md)
- [Security policy](SECURITY.md)
- [Changelog](CHANGELOG.md)

## Quality Gate

Run the same local backend gate used by CI:

```sh
sh scripts/backend-check.sh
```

The CI quality job also sets `WITH_IGNORED_DB_TESTS=true` and supplies a
disposable PostgreSQL `DATABASE_URL`. Verify the production container contract
separately when Docker is available:

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

The backend workflow runs for pull requests targeting `develop` or `main`,
post-merge pushes to those branches, and explicit manual dispatches. The
production container smoke workflow uses the same integration boundaries plus
production-relevant path filters. Plain feature-branch pushes do not run these
gates unless they update an open pull request. Superseded runs for the same
pull request or integration ref are cancelled.

Release automation runs only for `v*` tags. CI validates repository changes;
it does not deploy them. No workflow creates a preview application, public
preview URL, or GitHub Environment. See the [maintainer workflow](docs/maintainers.md)
for the complete trigger and release contract.

## Project Policy

This project is published as open-source software, but external code
contributions are not accepted. Security vulnerabilities should be reported
privately according to [SECURITY.md](SECURITY.md).

## License

[Apache 2.0](LICENSE.txt)
