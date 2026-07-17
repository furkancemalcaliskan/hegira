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
- [Security policy](SECURITY.md)
- [Changelog](CHANGELOG.md)

## Quality Gate

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
./scripts/dx-manual-check.sh
```

PostgreSQL tests marked `ignored` require a disposable `DATABASE_URL` because
they reset the target database.

## Project Policy

This project is published as open-source software, but external code
contributions are not accepted. Security vulnerabilities should be reported
privately according to [SECURITY.md](SECURITY.md).

## License

[Apache 2.0](LICENSE.txt)
