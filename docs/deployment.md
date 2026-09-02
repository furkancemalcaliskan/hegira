# Deployment

Deployment belongs to each generated application. The Hegira framework
repository does not ship a deployable compatibility host, official application
image, or root runtime configuration.

## Build Contract

From a rendered application workspace, install the lockfile-pinned frontend
dependencies and build the default PostgreSQL production profile:

```sh
npm ci --prefix apps/web/src
PATH="$PWD/apps/web/src/node_modules/.bin:$PATH" \
  cargo leptos build -p app_server --release \
  --bin-features ssr,db-postgres --lib-features hydrate
```


The output contains the `app_server` executable and the hydrated site under
`target/site`. Optional Redis, SMTP, S3, Meilisearch, Prometheus, and OTLP
adapters must be selected explicitly at compile time and configured at runtime.

## Container Contract

The rendered application owns its Dockerfile. It builds the server and hydrated
client from the application workspace, copies the application-owned production
profile, exposes port `3000`, and starts `app_server` with `APP_ENV=production`.

```sh
docker build -t my-application .
docker run --rm -p 3000:3000 \
  -e APP__DATABASE__URL \
  -e APP__SECURITY__JWT_SECRET \
  my-application
```

Do not place real credentials in the image, Compose file, repository, or shell
history. Supply them through the deployment platform's secret mechanism.

## Database Lifecycle

The generated infrastructure layer composes application migrations with the
migrations of selected official modules. Production defaults keep
`database.auto_migrate=false` and `startup.seed_identity=false`. Deployment
automation must execute the application-owned migration plan as a one-shot step
before new application instances receive traffic. Do not enable destructive
reset outside explicitly disposable validation databases.

## Runtime Topology

`runtime.role=all` runs HTTP and background work in one process.
`runtime.role=web` serves HTTP traffic, while `runtime.role=worker` runs
background work and its configured operational endpoint. Independently scaled
SQLite web and worker processes are rejected; use PostgreSQL for split roles.

Operational endpoints include liveness and readiness checks. Expose metrics and
worker operational ports only to trusted infrastructure. Configure
`security.trusted_proxies` with only the CIDRs that connect directly to the
application.

## Repository Validation

Hegira releases validate the rendered application rather than a repository
host:

```sh
sh scripts/generated-application-check.sh
```

That gate verifies fresh and upgraded databases, the release build, production
container startup, hydration assets, readiness, security headers, and Bearer
API behavior using disposable state.
