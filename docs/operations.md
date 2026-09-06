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
