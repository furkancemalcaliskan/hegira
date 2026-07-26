# Deployment

Hegira can run as one process or as separately scaled web and worker processes.
Choose the smallest topology that meets the workload.

## Topologies

### Single Process

Use `APP__RUNTIME__ROLE=all`. The process serves HTTP and runs scheduler and
durable job loops. This is the default for SQLite and small PostgreSQL systems.

### Separate Web And Worker

Use the same compiled binary with different configuration:

```text
web replicas:    APP__RUNTIME__ROLE=web
worker replicas: APP__RUNTIME__ROLE=worker
```

Web replicas expose the application port. Worker replicas may expose the
isolated operations listener when `worker_operations.enabled=true`. This
topology requires PostgreSQL; enable distributed locking where scheduled work
must have one active executor.

## Release Build

The committed production profile is the minimal PostgreSQL contract: database
sessions, in-process rate limiting, and disabled optional external providers.
It matches the Dockerfile's default `ssr,db-postgres` server feature set.

Run release commands from the repository root. Cargo selects the deployable
package at `apps/hegira` by its package name, `hegira`; Cargo-Leptos reads its
build metadata there while writing outputs to the workspace-level `target`
directory.

Build the minimal profile with:

```sh
cargo leptos build -p hegira --release \
  --bin-features ssr,db-postgres \
  --lib-features hydrate
```

Compile optional integrations explicitly for a distributed profile:

```sh
cargo leptos build -p hegira --release \
  --bin-features ssr,db-postgres,cache-redis,mailer-smtp,storage-s3,search-meilisearch,metrics-prometheus,otel-otlp \
  --lib-features hydrate
```

OpenAPI is intentionally omitted from production builds.
Compiling a capability does not enable it at runtime. Override the matching
configuration backend and credentials when enabling Redis, SMTP, S3,
Meilisearch, Prometheus, or OTLP.

The two supported production capability contracts are:

| Contract | Server features | Runtime expectation |
|---|---|---|
| Minimal | `ssr,db-postgres` | Database sessions, in-process rate limiting, optional providers disabled |
| Distributed | `ssr,db-postgres,cache-redis,mailer-smtp,storage-s3,search-meilisearch,metrics-prometheus,otel-otlp` | Enable only the external providers that are provisioned and configured |

The distributed build makes adapters available; it does not turn them on.
Runtime configuration that selects an adapter missing from the binary fails
capability preflight before dependency initialization.

Both commands produce the server at `target/release/hegira` and the hydrated
site under `target/site`. Frontend source, styles, and public assets remain
under `crates/web`; they are inputs to the application-owned Cargo-Leptos
package rather than independent deployment units.

## Source Distribution

Hegira is released as a source template rather than as a compiled application.
Each stable release is identified by a signed `vMAJOR.MINOR.PATCH` tag and a
GitHub Release. GitHub provides `.zip` and `.tar.gz` archives of the tagged
source; the only custom release asset is an SPDX JSON SBOM generated from that
source checkout.

The release workflow still verifies the minimal PostgreSQL full-stack build and
production container. Those are validation contracts for users building and
deploying their own application, not published binaries or images. Hegira does
not publish a platform-specific executable, Linux application bundle, official
container image, preview application, or production deployment.

## Database Release Step

Run migrations once before rolling out application replicas:

```sh
APP_ENV=production \
cargo run -p db_migrator --release --no-default-features \
  --features ssr,db-postgres -- migrate
```

Do not rely on multiple web replicas racing to auto-migrate. Backward-compatible
schema changes should be deployed before code that requires them.

## Docker

The Dockerfile defaults to `ssr,db-postgres` and provides three targets:

| Target | Purpose |
|---|---|
| `final` | Default web image |
| `web-runtime` | Explicit web role with Leptos assets |
| `worker-runtime` | Worker role with operations port `9091` |

Build a normal web image:

```sh
docker build -t hegira:latest .
```

This image matches the committed minimal production profile and does not
require Redis, SMTP, S3, Meilisearch, Prometheus, or an OTLP collector.

### Production Container Smoke Test

Run the release smoke test locally with:

```sh
sh scripts/container-smoke.sh
```

The test builds the default `ssr,db-postgres` production image, starts a
disposable PostgreSQL database, applies migrations as a one-shot task, and
verifies `/healthz`, `/readyz`, the application page, and generated CSS and
JavaScript assets. Failure logs are printed before the complete stack and its
volumes are removed.

The smoke workflow validates the container contract only. It does not create a
preview application, publish a public URL, use a GitHub Environment, or require
deployment credentials.

Build explicit role images with the same capability set:

```sh
docker build --target web-runtime \
  --build-arg SERVER_FEATURES=ssr,db-postgres,cache-redis,mailer-smtp \
  -t hegira-web:latest .

docker build --target worker-runtime \
  --build-arg SERVER_FEATURES=ssr,db-postgres,cache-redis,mailer-smtp \
  -t hegira-worker:latest .
```

Pass configuration as environment variables or mounted secret-provider output.
Do not bake credentials into the image.

## Rollout Contract

1. Back up the production database.
2. Run the migrator as a one-shot release task.
3. Deploy worker-compatible schema before new workers.
4. Roll out web replicas and wait for `/readyz`.
5. Roll out workers and wait for their operations `/readyz`.
6. Verify metrics, traces, queue depth, and representative user workflows.

Use rolling replacement only for backward-compatible changes. Destructive
schema changes need an expand/migrate/contract sequence across releases.

## Required Production Overrides

At minimum set:

```text
APP_ENV=production
APP__APPLICATION__PUBLIC_URL
APP__DATABASE__URL
APP__SECURITY__JWT_SECRET
APP__SECURITY__CORS__ALLOWED_ORIGINS
APP__RUNTIME__ROLE
```

Add provider credentials only for capabilities compiled into the image. See
[Configuration](configuration.md) for the feature matrix and
[Operations](operations.md) for post-deployment checks.
