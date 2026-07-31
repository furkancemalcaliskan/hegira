# Configuration

The current compatibility host loads configuration from the repository root at
`config/{APP_ENV}.yaml` and then applies environment-variable overrides. A canonical rendered
application owns the equivalent `config/` directory inside its independent application
workspace; it does not read profiles from the framework repository. Nested keys use double
underscores:

```sh
APP_ENV=production
APP__DATABASE__URL=postgres://user:password@db:5432/hegira
APP__SECURITY__JWT_SECRET=replace-with-a-secret
APP__RUNTIME__ROLE=web
```

Profiles are `development`, `sqlite`, `test`, and `production`. Treat committed
YAML values as defaults, not a secret store.

Startup validates configuration before initializing telemetry, the database,
or another external dependency. Validation runs in this order:

1. Structural invariants that apply to every environment, such as lifetimes,
   positive timeouts, provider settings, and URL schemes.
2. Runtime selections against capabilities compiled into the binary.
3. Production-only policy, including HTTPS public URLs and origins, a strong
   JWT secret, disabled admin seed and OpenAPI, and supported database topology.

Each failure names the relevant configuration key. Development and test
profiles may use intentionally relaxed policy values, but they do not bypass
structural validation.

Capability validation aggregates all missing compiled providers into one
startup error. For example, selecting Redis sessions and SMTP with a minimal
binary produces an error shaped like:

```text
invalid application capabilities: runtime configuration requires capabilities not present in the binary:
- sessions.backend=redis requires the cache-redis Cargo feature
- mailer.backend=smtp requires the mailer-smtp Cargo feature
```

This preflight runs before telemetry, database pools, or other external clients
are initialized. A bad capability selection therefore fails deterministically
instead of appearing later as a connection or missing-provider error. Disabled
optional providers do not require their Cargo features.

## Compile-Time Features

Cargo features decide which providers enter the binary. Runtime settings only
select among providers that were compiled. Startup validates this contract
before connecting to the database or any other external dependency and reports
every missing enabled capability together.

| Capability | Cargo feature | Runtime selection | External service |
|---|---|---|---|
| PostgreSQL | `db-postgres` | `database.backend=postgres` | PostgreSQL |
| SQLite | `db-sqlite` | `database.backend=sqlite` | None |
| OpenAPI UI | `openapi` | `openapi.enabled=true` | None |
| Redis cache/session/rate limit | `cache-redis` | `cache.backend=redis`, `sessions.backend=redis`, or enabled `security.rate_limit.backend=redis` | Redis |
| SMTP | `mailer-smtp` | `mailer.backend=smtp` | SMTP server |
| S3 storage | `storage-s3` | `storage.backend=s3` | S3-compatible service |
| Meilisearch | `search-meilisearch` | `search.backend=meilisearch` | Meilisearch |
| Prometheus | `metrics-prometheus` | `metrics.enabled=true` | Scraper only |
| OTLP tracing | `otel-otlp` | `telemetry.enabled=true` | OTLP collector |
| WASM route splitting | `wasm-split` | release build behavior | None |

Use `--no-default-features` in release commands to avoid compiling both
database providers accidentally.

## Core Settings

| Section | Important keys |
|---|---|
| `application` | `name`, `public_url` |
| `server` | `addr`, request timeout, body limit |
| `runtime` | `role`: `all`, `web`, or `worker` |
| `startup` | database ensure, identity seed, scheduler, durable jobs |
| `database` | backend, URL, pool size, auto migration |
| `security` | JWT secret, trusted proxy CIDRs, CORS, rate limiting |
| `sessions` | database/Redis backend and lifetime limits |
| `oauth` | provider credentials, callbacks, scopes, state TTL |
| `mailer` | null/log/SMTP backend and sender |
| `cache` | null/memory/Redis backend and authorization TTL |
| `storage` | null/local/S3 backend |
| `search` | null/Meilisearch backend and task timeout |
| `jobs.durable` | polling, batch, and lock timeout |
| `metrics` | enable flag and endpoint path |
| `telemetry` | OTLP protocol, endpoint, timeout, sampling |

## Recommended Profiles

Small local application:

```text
db-sqlite + runtime.role=all + database sessions + log mailer
```

Minimal production application (the committed `production` profile):

```text
db-postgres + runtime.role=web + database sessions + memory rate limiting
optional providers disabled
```

Distributed production application:

```text
db-postgres + separate web/worker roles + Redis + SMTP
optional Meilisearch, S3, Prometheus, and OTLP
```

SQLite is intended for local development, tests, and single-process small
deployments. Do not run independently scaled SQLite web and worker replicas.

The minimal profile is deliberately compatible with a binary compiled using
only `ssr,db-postgres`. To select Redis, SMTP, S3, Meilisearch, Prometheus, or
OTLP at runtime, first compile the corresponding Cargo feature and then
override the provider configuration. Runtime configuration never adds a
provider that was omitted from the binary.

Compile-time availability and runtime enablement are deliberately independent:

- compiling a feature makes its adapter available but does not enable it;
- selecting an adapter at runtime requires its feature to be present;
- enabling several unavailable adapters reports all missing features together;
- structural and production-policy validation still applies when every
  requested capability is compiled.

## Production Validation

Production startup rejects unsafe policy values, including the default JWT
secret, admin seed, enabled OpenAPI, non-HTTPS public URLs or CORS origins, and
split web/worker roles backed by a local SQLite database. Structural provider,
URL, metric, lifetime, and worker-operation checks apply in every environment.

Keep `database.auto_migrate=false` and `startup.seed_identity=false` in
production. Run `db_migrator` as an explicit release step.

`security.trusted_proxies` is empty by default. Add only the IPv4 and IPv6
CIDRs of reverse proxies that connect directly to Hegira. Direct clients cannot
select their rate-limit identity with `X-Forwarded-For`; the header is evaluated
only when the TCP peer is trusted.

See [Deployment](deployment.md) for build profiles and [Operations](operations.md)
for health, migrations, backup, and recovery.
