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
cargo run --locked -p hegira_cli -- inspect --help
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

## Inspect An Existing Application

Run inspection from the application root or any directory below it:

```sh
cargo run --locked --manifest-path /path/to/hegira/Cargo.toml \
  -p hegira_cli -- inspect
```

Without `--application-root`, the CLI searches the real working directory and
its ancestors for `hegira.toml`. More than one candidate is ambiguous and
fails with a conflict; the CLI never chooses between nested applications.
Automation can select one root explicitly:

```sh
cargo run --locked --manifest-path /path/to/hegira/Cargo.toml \
  -p hegira_cli -- inspect --application-root /path/to/my-application --json
```

Human output reports the application identity, resolved root, manifest schema,
framework repository and version, selected components, database, client, and
mutation compatibility. JSON output has `output_schema: 1` and exposes the
same application root, typed manifest when the schema is understood, and a
`mutation_compatibility` value of `compatible`, `incompatible`, or
`unsupported` with the responsible manifest field.

Inspection is read-only. It opens the manifest and required application-owned
`apps/`, `crates/`, and `config/` roots without following symlinks. It does not
read runtime configuration, environment values, user-home state, or secrets,
and it does not write application files.

## Compatibility And Mutation Safety

`inspect` can report an incompatible or unsupported application successfully,
but `generate resource` and `generate migration` fail with conflict exit code
`4` unless the application is compatible with the running CLI. The current
mutation policy requires:

- manifest schema `1` and the canonical Hegira framework repository;
- the exact framework release version compiled into the CLI;
- the `layered-base` and Leptos Identity component composition generated by
  the current application template;
- exactly one supported database (`sqlite` or `postgres`); and
- exactly one supported client (`leptos`).

This exact-version rule prevents a source-built CLI from silently editing an
application generated for another framework release. Use the Hegira source or
release archive matching `framework.version` in the application's
`hegira.toml`.

Every mutation command constructs and validates one ordered change plan.
`--dry-run` reports that same plan without opening the application for writes;
apply rechecks absent-file and SHA-256 content preconditions immediately before
publication. Human and JSON summaries contain paths, operations, and digests,
not generated source, existing file contents, credentials, or runtime values.
An existing generated file, stale source edit, occupied namespace, malformed
managed integration block, concurrent mutation, or symlinked path fails rather
than being overwritten or adopted.

Publication uses the application-owned `.hegira-mutation.lock` recovery marker
and private staging files. Recoverable failures are rolled back. If a crash,
concurrent file change, incomplete rollback, or uncertain cleanup leaves the
marker in place, later mutations stop with a conflict. Inspect the marker,
transaction files, and affected application paths against version-control
state before manual recovery; do not delete the marker blindly. Platforms or
filesystems without the required no-follow and atomic rename behavior fail
before application files are changed.

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

For a resource such as `Order`, the command creates an application-owned
`order.rs` in each selected layer and edits only declared managed integration
points:

| Owner | Generated responsibility |
|---|---|
| `crates/domain` | UUID entity skeleton |
| `crates/application_contracts` | Commands, queries, responses, permissions, and service contract |
| `crates/application` | Repository, authorization, and identifier ports plus use-case service |
| `crates/infrastructure` | Selected SQLx repository, typed composition, and provider migration |
| `crates/presentation` | Bearer HTTP handlers and OpenAPI contribution |
| `apps/server` and `apps/web` | Explicit server composition, Leptos pages, server functions, routes, navigation, and localization |

All of this generated source belongs to the application and may be extended.
Framework primitives and official Identity packages remain pinned dependencies;
their implementations are not copied into the generated application.

The generated application service authorizes every use case before repository
access. Bearer API routes remain separate from cookie-authenticated browser/BFF
routes. Leptos permission gates improve navigation and page behavior only;
client-side or UI visibility checks are never an authorization boundary.

Generation does not infer business invariants or relationships and does not
run formatters, builds, application processes, or migrations. Review the
generated aggregate shape, validation, authorization, transaction needs,
relationships, API behavior, and user experience. Complete and apply the
forward migration, then run the formatting and application checks reported by
the command. The generator automates repeatable integration work; these product
and business decisions remain developer-owned.

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

The generated `Cargo.lock` is part of the release-verified application source.
Keep it in version control and use explicit `cargo update` operations when the
application intentionally adopts a different dependency graph.

```sh
cd ../my-application
npm ci --prefix apps/web/src
APP_ENV=sqlite cargo leptos watch -p app_server \
  --bin-features ssr,db-sqlite --lib-features hydrate \
  --bin-cargo-args=--locked --lib-cargo-args=--locked
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
  --bin-features ssr,db-postgres --lib-features hydrate \
  --bin-cargo-args=--locked --lib-cargo-args=--locked
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

`scripts/cli-check.sh` owns focused inspection, compatibility, dry-run,
conflict, recovery, and generator command contracts. The generated-application
gate creates both provider profiles through the public CLI, applies a generated
resource only to disposable validation copies, and exercises its migration,
authorization, HTTP, UI, and production-container behavior.

See [Architecture](architecture.md), [Configuration](configuration.md), and
[Deployment](deployment.md) before changing providers or production defaults.
