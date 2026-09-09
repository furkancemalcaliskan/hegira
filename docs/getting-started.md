# Getting Started

Hegira currently ships framework source, official modules, a canonical layered
application base, and a source-runnable CLI with guided and non-interactive
application creation. The CLI writes the selected source tree and next-step
instructions; it does not install dependencies, run migrations, initialize a
Git repository, or execute generated code.

## Build And Invoke The CLI

Use a complete Hegira source checkout or extracted source archive and the Rust
toolchain pinned by its `rust-toolchain.toml`. Releases remain source-only:
there is no published CLI executable or supported crates.io installation.
From the framework repository root:

```sh
cargo build --locked -p hegira_cli
cargo run --locked -p hegira_cli -- --help
cargo run --locked -p hegira_cli -- new --help
cargo run --locked -p hegira_cli -- generate resource --help
cargo run --locked -p hegira_cli -- generate migration --help
```

`cargo run` builds and invokes the `hegira` binary. Its canonical package is
loaded from the source tree recorded at compilation, not downloaded from a
registry. Keep that tree available at its original location; copying the binary
alone is not a standalone installation. Rebuild after relocating the source.
Node, Docker, and `cargo-leptos` are not needed just to generate files.

## Guided Creation

In an interactive terminal, the guided form can collect the application name,
destination, and implemented adapter selections:

```sh
cargo run --locked -p hegira_cli -- new
```

It shows defaults and a final summary before writing files. Cancellation leaves
no generated application. Scripts, CI, redirected input, and other non-TTY
execution must provide the name and destination explicitly as shown below.

Supplying both the name and destination skips prompts and confirmation, even
in a terminal. Omitted adapter flags then use defaults. If either required
input is missing, the guided flow collects missing values and confirms the
selection; explicit flags are retained.

## Application Prerequisites And Non-Interactive Creation

- the Rust toolchain pinned by `rust-toolchain.toml`;
- the `wasm32-unknown-unknown` target;
- `cargo-leptos` (CI validates version `0.3.7`);
- Node.js and npm for the Leptos stylesheet toolchain (CI uses Node.js 22);
- Docker Compose when using the local PostgreSQL service or container checks.

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

Use source from a compatible release for an independently buildable application.
Generation itself does not fetch or build those pinned dependencies. An
unreleased checkout can contain changes absent from its declared release tag;
successful generation alone does not prove that tag contains the required
packages. Maintainer checks validate current source in disposable copies, not
by silently changing the application's release pin.

## Identity And Destination Safety

Application identity uses 1–64 lowercase ASCII letters, digits, and single
internal hyphens, starting with a letter. Rust keywords, Cargo's `target`, and
portable device names such as `con`, `aux`, and `com1` are reserved. This same
identity is shown in the CLI summary and stored in `hegira.toml`; application
crate names remain the template's brand-neutral `app_*` names.

The destination's final directory name accepts 1–64 ASCII letters, digits,
hyphens, and underscores, starting with a letter or digit. Reserved names are
rejected case-insensitively. Its parent must already exist without symlinks.
Absolute destinations and leading `../` for sibling locations are supported;
traversal through a named directory (`child/../application`) is rejected.
Existing files, empty directories, non-empty directories, and dangling symlinks
are all conflicts. There is no overwrite or force mode.

Safe publication uses directory handles and an exclusive atomic rename on
Linux/Android and Apple platforms. Linux is covered by repository validation;
other platforms and filesystems without this primitive fail closed. On systems
where an ancestor is an alias (for example `/tmp` on macOS), use its real path.
Generated files and directories start with owner-only permissions (0600/0700).

## Supported Selections

| Flag | Accepted values | Default |
|---|---|---|
| `--database` | `sqlite`, `postgres` | `sqlite` |
| `--client` | `leptos` | `leptos` |
| `--component` | `identity` | `identity` |

Each invocation selects one database. Identity resolves to `layered-base` and
`layered-leptos-identity`; the CLI does not provide an empty or Identity-free
composition. Database selection sets the generated default Cargo feature and
recommended startup profile, not database credentials or provisioning.

For an explicit PostgreSQL
application, use:

```sh
cargo run --locked -p hegira_cli -- new my-application \
  --destination ../my-application \
  --database postgres \
  --client leptos \
  --component identity
```

This is an alternative to the SQLite example, not a second command to run
against the same destination.

## Generated Ownership And `hegira.toml`

The generated directory belongs to your application. Its `apps/` composition
roots, layered `crates/`, configuration, migration composition, and deployment
files are editable application source. Official Identity and framework packages
remain dependencies, not copied module implementations. See the
[architecture ownership contract](architecture.md#canonical-generated-application)
for the layer boundaries.

The generated root `hegira.toml` records generation state:

| Field | Meaning |
|---|---|
| `schema` | Manifest format version, currently `1` |
| `application` | Validated project identity; does not rename the `app_*` crates |
| `framework.repository` | Package-controlled HTTPS framework source |
| `framework.version` | Package-controlled stable SemVer release tag |
| `selection.components` | Resolved canonical component identities |
| `selection.databases` | Selected database adapter |
| `selection.clients` | Selected client adapter |

The renderer validates and writes this manifest during creation. Editing it
does not regenerate files, change Cargo dependencies, switch the running
database, or upgrade an application. Keep it consistent with application source.
Runtime settings belong in `config/{APP_ENV}.yaml` and environment overrides;
credentials never belong in `hegira.toml`. See
[Configuration](configuration.md) for the separate runtime contract.

The CLI currently exposes application creation, read-only inspection, complete
layered resource generation, and application-owned migration scaffold
generation. It does not provide module management, migration execution or
rollback, automatic upgrades, remote component installation, or additional
client templates. Optional runtime providers are configured explicitly in the
application; they are not extra `new` selections.

Successful creation and help use stdout; diagnostics use stderr. Exit codes
are `0` (success, including guided cancellation), `1` (internal error),
`2` (usage error), `3` (validation failure), and `4` (destination conflict).

## Generate A Layered Resource

Run the source-built CLI from a generated application and provide each field
as `name:type`; append `?` to the type for a nullable field:

```sh
cargo run --locked --manifest-path /path/to/hegira/Cargo.toml \
  -p hegira_cli -- generate resource Order \
  --field name:string --field fulfilled_at:datetime?
```

The supported scalar set is `string`, `bool`, `i64`, `uuid`, and `datetime`.
Use `--plural <NAME>` for an irregular UpperCamelCase plural. The command reads
the database, client, and components from `hegira.toml`, validates all names
and integration points, and publishes one atomic plan covering Domain,
Application Contracts, Application, SQLx persistence and migration, Axum and
OpenAPI, and the selected Leptos client. `--dry-run` previews the identical
content-redacted plan; `--json` emits its deterministic machine form.

Generation does not infer business invariants or relationships and does not
run formatters, builds, application processes, or migrations. Review the
generated authorization and validation rules, complete and apply the forward
migration, then run the application checks reported by the command.

## Generate An Application Migration

Run the source-built CLI from a generated application to create a migration
for the database adapter selected in its `hegira.toml`:

```sh
cargo run --locked --manifest-path /path/to/hegira/Cargo.toml \
  -p hegira_cli -- generate migration add_orders
```

Alternatively, pass `--application-root <path>` when the working directory is
outside the application. Migration identities use 1–64 bytes of lowercase
ASCII snake_case. The command rejects reserved identities, an identity already
present in the selected provider history, malformed histories, and unsafe or
incompatible application roots before publication.

The generated SQL file is an intentionally empty, provider-labelled scaffold
under `crates/infrastructure/migrations/sqlite/` or
`crates/infrastructure/migrations/postgres/`. Add the forward-only SQL required
by the application after generation. The command never connects to a database,
executes or reverts migrations, translates SQL, or changes an existing
migration.

`--dry-run` reports the exact typed plan used by apply without writing the
application. `--json` emits its deterministic, versioned, content-redacted
machine representation. Alongside the SQL file, the command maintains
`crates/infrastructure/migrations/.hegira-generator.toml`. This application-
owned coordination record reserves the next migration version through the
same failure-safe mutation transaction; it contains no runtime configuration,
credentials, or SQL content. Concurrent or repeated stale plans fail as
conflicts instead of silently replacing migration history.

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

From the generated application's root, install its frontend dependencies with
`npm ci --prefix apps/web/src` if not already done. Start a disposable local
database:

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
