# Operations

Operational ownership belongs to the generated application. Hegira provides
framework primitives and a canonical composition; it does not operate a shared
repository-hosted application.

## Database Changes

Application migrations live in the generated application's infrastructure
package. Official-module migrations remain module-owned and are combined into
an ordered application migration plan. Preserve every released migration's
identifier, order, and checksum. Add a new migration instead of editing a
released one.

Production keeps automatic migration and Identity seed disabled. Execute the
application-owned migration plan as a one-shot deployment step before rollout.
Destructive reset requires an explicit disposable-database authorization token
and must never target persistent or production data.

### Read-Only Migration Status

`persistence::migrations::MigrationPlan::status(&DatabaseConfig)` inspects an
explicitly selected SQLite or PostgreSQL target using the same composed plan
as migration execution. The schema-1 typed report distinguishes a missing
database, missing `_sqlx_migrations` metadata, and present history; its entries
are ordered by version and contain module identity plus applied/pending state.
Unknown or duplicate history identities, unsuccessful records, and checksum
conflicts return typed errors rather than a repair or an apparently healthy
report. Views and unreadable/malformed metadata fail closed.

Status never calls `Migrator::run`, provisions a database or migration table,
seeds data, or reads runtime configuration on the caller's behalf. Startup
`auto_migrate` settings grant it no write authority. It opens a dedicated
connection rather than reusing a potentially write-capable runtime pool.
PostgreSQL reads through one repeatable-read, read-only transaction, respecting
the configured search path and limiting statements to five seconds. Permission,
connection, and query failures expose no raw driver errors, credentials, SQL,
history descriptions, or checksum bytes, including in Debug/source chains.

SQLite requires a file-backed target, forces read-only/no-create flags and
`query_only`, and does not change the database's journal mode or use `immutable`
to bypass locking. In-memory databases, raw `file:` URIs, and custom VFS choices
are rejected. A missing file remains missing. Data, schema, migration records,
and existing WAL content remain unchanged, but normal SQLite WAL coordination
can create/update `-wal`/`-shm` sidecars. This is not a promise of an untouched
directory. See [SQLite WAL documentation](https://www.sqlite.org/wal.html).

Infrastructure remains responsible for composing application and official-module
migration sources and obtaining separate explicit intent for database access.
This framework API is not a public `hegira` database command. The observed
report is a point-in-time snapshot, not approval to migrate, repair, or deploy.

## Health And Readiness

- `/healthz` reports process liveness.
- `/readyz` reports whether dependencies selected by the application are ready.

Use liveness for process replacement and readiness for traffic admission. A
worker role exposes its operational listener only when configured; keep that
listener private to the deployment network.

## Observability

The framework supplies tracing, request IDs, background-job observation,
worker-heartbeat state, health primitives, and optional Prometheus and OTLP
adapters. The application composition root decides which concrete dependencies
are readiness-critical and which endpoints are exposed.

Do not log credentials, tokens, session cookies, authorization headers,
personal data, or database URLs containing passwords. Treat production logs and
traces as sensitive operational data.

## Backup And Recovery

Backups are deployment-specific. Use the database provider's supported backup
mechanism, encrypt backups, restrict access, and test restore procedures against
an isolated environment. Recovery documentation must record the application
version, migration state, provider version, and verification steps.

Never test reset, migration, backup, or restore commands against a production
database from framework-repository validation.

## Provider Changes

Runtime provider selection must match compiled capabilities. Validate a new
provider combination before rollout, keep configuration validation ahead of
external client initialization, and use staged readiness checks during the
transition. See [Configuration](configuration.md) for the feature-to-provider
contract and [Deployment](deployment.md) for the runtime topology.
