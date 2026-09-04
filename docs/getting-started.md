# Getting Started

Hegira currently ships framework source, official modules, a canonical layered
application base, and a source-runnable CLI that creates that application
non-interactively. The CLI writes the selected source tree and next-step
instructions; it does not install dependencies, run migrations, initialize a
Git repository, or execute generated code.

In an interactive terminal, the guided form can collect the application name,
destination, and implemented adapter selections:

```sh
cargo run --locked -p hegira_cli -- new
```

It shows defaults and a final summary before writing files. Cancellation leaves
no generated application. Scripts, CI, redirected input, and other non-TTY
execution must provide the name and destination explicitly as shown below.

## Prerequisites

- the Rust toolchain pinned by `rust-toolchain.toml`;
- the `wasm32-unknown-unknown` target;
- `cargo-leptos`;
- Node.js and npm for the Leptos stylesheet toolchain.

From the framework repository root:

```sh
rustup target add wasm32-unknown-unknown
cargo install cargo-leptos
cargo run --locked -p hegira_cli -- new my-application \
  --destination ../my-application
```

The output is an independent Cargo workspace. Its normal dependencies use the
framework repository and release tag declared by the template rather than
paths into the maintainer checkout.

SQLite, Leptos, and Identity are the defaults. For an explicit PostgreSQL
application, use:

```sh
cargo run --locked -p hegira_cli -- new my-application \
  --destination ../my-application \
  --database postgres \
  --client leptos \
  --component identity
```

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
