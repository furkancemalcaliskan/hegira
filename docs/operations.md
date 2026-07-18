# Operations

This guide covers routine production operation after an image has been built
and deployed. Build and topology decisions are documented in
[Deployment](deployment.md).

## Migrations And Seed

Use the dedicated migrator:

```sh
APP_ENV=production cargo run -p db_migrator --release --no-default-features --features ssr,db-postgres -- migrate
```

Production identity seed is disabled by default. `reset` and `recreate` require
`APP_ENV=test` or the explicit `ALLOW_DB_RESET=true` guard and must never be
used against production.

## Health And Readiness

| Endpoint | Meaning |
|---|---|
| `/healthz` | Process is alive; suitable for restart decisions |
| `/readyz` | Required dependencies and active role are ready for traffic |
| `/metrics` | Prometheus output when compiled and enabled |

Do not use readiness failure as an automatic restart signal. A dependency
outage should remove the replica from traffic without creating a restart loop.

Worker-only deployments expose equivalent health endpoints on the operations
listener, default port `9091`, when enabled. Keep this listener private.

## Durable Jobs

Durable jobs use database claims, bounded retries, idempotent handlers, and
stale-lock recovery. Monitor:

- pending, running, retry, and dead-letter counts;
- oldest pending age;
- processing duration and failure rate;
- worker heartbeat freshness.

Run only one SQLite worker process. PostgreSQL supports separately scaled
workers, but job handlers must remain idempotent because at-least-once delivery
is intentional.

## Metrics And Tracing

Compile `metrics-prometheus` before enabling metrics. Protect the scrape route
at the network layer. Compile `otel-otlp` before enabling OTLP export and set a
sampling ratio appropriate for traffic volume.

HTTP requests propagate W3C trace context. Web and worker roles identify
themselves in telemetry resource attributes. Never attach passwords, tokens,
OAuth codes, or mail bodies to logs or spans.

## PostgreSQL Backup And Restore

Create a custom-format backup and verify its table of contents:

```sh
DATABASE_URL=postgres://... scripts/postgres-backup.sh backups
pg_restore --list backups/hegira.dump >/dev/null
```

Encrypt backups, store them outside the database failure domain, and enforce
retention and restore access controls.

Restore into an isolated database first:

```sh
export DATABASE_URL=postgres://.../hegira_restore_test
export ALLOW_DB_RESTORE=true
scripts/postgres-restore.sh backups/hegira.dump
```

Run migrations against the restored database, start an isolated web instance,
and verify login, RBAC, audit continuity, queue state, and representative
application records. Record observed RTO and RPO.

## Search Recovery

PostgreSQL is the source of truth; Meilisearch is rebuildable. After database
recovery:

```sh
APP_ENV=production APP__SEARCH__ENABLED=true \
cargo run -p db_migrator --release --no-default-features \
  --features ssr,db-postgres,search-meilisearch -- reindex-search
```

Monitor task completion before directing search traffic to the rebuilt index.

## Security Checklist

- Replace the default JWT secret through a secret manager.
- Keep production admin seed and OpenAPI disabled.
- Restrict CORS origins and preserve secure HttpOnly cookie settings.
- Terminate TLS at a trusted proxy and forward only trusted client headers.
- Keep database, Redis, worker operations, metrics, and OTLP ports private.
- Use least-privilege database and object-storage credentials.
- Rotate OAuth, SMTP, S3, Redis, and database credentials.
- Alert on authentication failures, readiness failures, dead letters, and stale
  worker heartbeats.

## Trusted Proxy and Client IP Contract

Hegira uses the TCP peer address as the secure default for request rate-limit
identity. It ignores `X-Forwarded-For` from every peer unless that peer belongs
to an explicitly configured `security.trusted_proxies` CIDR.

```yaml
security:
  trusted_proxies:
    - 10.20.0.0/16
    - 2001:db8:20::/48
```

List only networks used by proxies that connect directly to the application;
never use broad public ranges as a convenience. Universal `0.0.0.0/0` and
`::/0` trust entries are rejected. Each trusted proxy must append
the address it received to `X-Forwarded-For` and replace untrusted inbound
forwarding headers at the public edge. Hegira walks the chain from the direct
peer toward the client and stops at the nearest untrusted address. Malformed or
excessively long chains are rejected.

The address seen on Hegira's TCP connection must belong to the expected direct
proxy network. If several trusted proxy layers append to the chain, configure
each trusted hop with the smallest practical CIDR; do not add client or public
Internet ranges. Keep `security.trusted_proxies` empty when Hegira is directly
Internet-facing.

Failure behavior is intentionally fail-closed:

- an untrusted peer's `X-Forwarded-For` is ignored, even when malformed;
- a malformed or overlong chain from a trusted peer returns `400 Bad Request`;
- missing TCP peer information returns an internal error instead of assigning a
  shared fallback identity;
- invalid or universal trusted-proxy CIDRs prevent startup.

The memory rate limiter is local to one web process, so replicas enforce
independent quotas. Select the Redis backend for a quota shared across web
replicas. Both backends use the same trusted-proxy-aware resolved client
identity when constructing their keys.

## Production Integration Tests

Tests that reset PostgreSQL are ignored by default. Run them only against a
disposable database:

```sh
APP_ENV=test \
DATABASE_URL=postgres://postgres:postgres@localhost:5432/hegira_test \
cargo test --workspace --all-features -- --ignored
```

External Redis, Meilisearch, S3, SMTP, and OTLP acceptance tests require their
respective services. A compile-only feature check does not replace a staged
integration test.

## Incident And Rollback

1. Stop or scale down the failing role without destroying durable state.
2. Capture logs, traces, queue depth, and dependency health.
3. Roll application code back only when the current schema remains compatible.
4. Restore data only after confirming corruption or loss; do not use restore as
   a substitute for code rollback.
5. Reindex derived search data and resume workers after the database is stable.
6. Verify readiness and critical workflows before reopening traffic.
