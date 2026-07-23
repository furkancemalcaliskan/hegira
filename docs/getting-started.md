# Getting Started

This guide starts the complete Leptos and Axum application locally. Use SQLite
for a zero-service development environment or PostgreSQL when database-specific
behavior must match production.

Run the commands in this guide from the repository root. It is the Cargo
workspace entry point; the deployable package is located at `apps/hegira`.

## Prerequisites

- Current stable Rust toolchain
- `wasm32-unknown-unknown` Rust target
- `cargo-leptos`
- Node.js and npm for Tailwind assets

```sh
rustup target add wasm32-unknown-unknown
cargo install cargo-leptos
cd crates/web/src && npm ci && cd ../../..
```

## SQLite Development

SQLite needs no container or external service. Migrate the local database and
start the Leptos development server:

```sh
APP_ENV=sqlite cargo run -p db_migrator --no-default-features --features ssr,db-sqlite -- migrate
APP_ENV=sqlite cargo leptos watch -p hegira --bin-features ssr,db-sqlite --lib-features hydrate
```

Open `http://127.0.0.1:3000` and sign in with the admin credentials configured
under `seed` in `config/sqlite.yaml`. Change these credentials in any shared
development environment.

Useful migrator commands:

```sh
APP_ENV=sqlite cargo run -p db_migrator --no-default-features --features ssr,db-sqlite -- seed
ALLOW_DB_RESET=true APP_ENV=sqlite cargo run -p db_migrator --no-default-features --features ssr,db-sqlite -- recreate
```

`reset` and `recreate` are guarded by `APP_ENV=test` or
`ALLOW_DB_RESET=true`.

## PostgreSQL Development

Create a PostgreSQL database and override its URL when necessary:

```sh
export APP__DATABASE__URL=postgres://postgres:postgres@localhost:5432/hegira
cargo run -p db_migrator --no-default-features --features ssr,db-postgres -- migrate
cargo leptos watch -p hegira --bin-features ssr,db-postgres --lib-features hydrate
```

PostgreSQL and SQLite migrations are intentionally separate. Add and test both
when a schema change belongs to a provider-independent feature.

## Development URLs

| URL | Purpose |
|---|---|
| `/` | Leptos application |
| `/catalog/products` | Reference CRUD page |
| `/healthz` | Process liveness |
| `/readyz` | Dependency readiness |
| `/swagger-ui` | OpenAPI UI when compiled and enabled |

Compile OpenAPI explicitly:

```sh
APP__OPENAPI__ENABLED=true \
cargo run -p hegira --no-default-features --features ssr,db-sqlite,openapi
```

## Verify The Workspace

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

The default test matrix runs without external Redis, Meilisearch, S3, or SMTP.
See [Configuration](configuration.md) before enabling optional capabilities and
[Deployment](deployment.md) before producing a release image.
