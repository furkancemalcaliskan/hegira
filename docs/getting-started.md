# Getting Started

This guide starts the repository's current Axum and Leptos compatibility host locally. It is
the deployable integration surface used while framework packages, the official Identity
module, and the canonical layered application template are validated independently. Use SQLite
for a zero-service development environment or PostgreSQL when database-specific behavior must
match production.

Run the commands in this guide from the repository root. It is the Cargo
workspace entry point; the compatibility host package is located at `apps/hegira`. The
repository does not currently provide a public application-generation command.

## Repository Surfaces

| Location | Current role |
|---|---|
| `apps/hegira/` | Deployable compatibility host started by this guide |
| `crates/` | Framework primitives, providers, runtime support, and compatibility packages |
| `modules/identity/` | Canonical official Identity module source and adapters |
| `templates/applications/layered/` | Independent layered application source rendered during validation |
| `tools/template_renderer/` | Internal validation renderer, not a public CLI |

See [Architecture](architecture.md) for package ownership and dependency direction.

## Prerequisites

- Current stable Rust toolchain
- `wasm32-unknown-unknown` Rust target
- `cargo-leptos`
- Node.js and npm for Tailwind assets

```sh
rustup target add wasm32-unknown-unknown
cargo install cargo-leptos
npm ci --prefix crates/web/src
```

The npm command installs the lockfile-pinned frontend tooling used by
Cargo-Leptos. Keep the working directory at the repository root for the
remaining commands.

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
| `/dashboard` | Authenticated application dashboard |
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
sh scripts/repository-policy.sh
sh scripts/backend-check.sh
```

Template or generated-application changes additionally require:

```sh
sh scripts/layered-template-check.sh
sh scripts/generated-application-check.sh
```

The generated-application check requires Docker and creates only disposable rendered output,
containers, and databases. PostgreSQL tests marked `ignored` reset their target database and
must never point at persistent or production data.

The default test matrix runs without external Redis, Meilisearch, S3, or SMTP.
See [Configuration](configuration.md) before enabling optional capabilities and
[Deployment](deployment.md) before producing a release image.
