# Getting Started

Hegira currently ships framework source, official modules, and a canonical
layered application base. The public CLI is not implemented yet. The internal
renderer below exists for source validation and can materialize the current
template, but its command-line interface is not a stable public contract.

## Prerequisites

- the Rust toolchain pinned by `rust-toolchain.toml`;
- the `wasm32-unknown-unknown` target;
- `cargo-leptos`;
- Node.js and npm for the Leptos stylesheet toolchain.

From the framework repository root:

```sh
rustup target add wasm32-unknown-unknown
cargo install cargo-leptos
cargo run --locked -p template_renderer -- render \
  --repository-root . \
  --template layered \
  --output ../my-application
```

The output is an independent Cargo workspace. Its normal dependencies use the
framework repository and release tag declared by the template rather than
paths into the maintainer checkout.

## Run With SQLite

```sh
cd ../my-application
npm ci --prefix apps/web/src
APP_ENV=sqlite cargo leptos watch -p app_server \
  --bin-features ssr,db-sqlite --lib-features hydrate
```

Open `http://127.0.0.1:3000`. The SQLite development profile creates its local
database, runs the application-owned migration plan, and applies configured
Identity seed behavior at startup. Review `config/sqlite.yaml` before using the
application in a shared environment.

## Run With PostgreSQL

Start a disposable local database from the generated application:

```sh
POSTGRES_PASSWORD=local-development-only docker compose up -d database
APP_ENV=development \
APP__DATABASE__URL=postgres://postgres:local-development-only@localhost:5432/application \
cargo leptos watch -p app_server \
  --bin-features ssr,db-postgres --lib-features hydrate
```

The development profile may run migrations and seed data automatically.
Production intentionally disables both behaviors; deployment automation must
execute the application-owned migration plan before rollout.

## Validate Framework Source

Run framework-repository checks from the Hegira repository root:

```sh
sh scripts/repository-policy.sh
sh scripts/backend-check.sh
```

The complete generated application contract requires Docker and uses only
disposable state:

```sh
sh scripts/generated-application-check.sh
```

See [Architecture](architecture.md), [Configuration](configuration.md), and
[Deployment](deployment.md) before changing providers or production defaults.
