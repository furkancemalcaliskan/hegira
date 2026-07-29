# Architecture

Hegira uses an ABP-inspired layered design adapted to Rust, Axum, Leptos, and
explicit compile-time composition. It avoids runtime reflection, assembly
scanning, ambient request transactions, and a universal repository.

## Dependency Direction

The deployable package selects and composes reusable workspace packages. Within
the reusable packages, dependencies point toward business rules. The direct
dependency allowlist below is the normative workspace contract.

```text
apps/hegira
  -> platform_core, configuration, persistence, background_jobs, http_support,
     identity_http, identity_leptos, observability, runtime, test_support, web,
     presentation, infrastructure, application, application_contracts,
     domain, domain_shared

platform_core, configuration, persistence, background_jobs, http_support,
leptos_support, runtime
  -> no local application packages

observability
  -> background_jobs

test_support
  -> application

web
  -> leptos_support, presentation, application, application_contracts,
     domain_shared

presentation
  -> leptos_support, observability, infrastructure, application,
     application_contracts, domain_shared

infrastructure
  -> platform_core, configuration, persistence, background_jobs, runtime,
     application, application_contracts, domain, domain_shared

application
  -> background_jobs, application_contracts, domain, domain_shared

application_contracts
  -> domain, domain_shared

domain
  -> domain_shared

db_migrator
  -> infrastructure, persistence

identity_domain_shared
  -> no local packages

identity_domain
  -> identity_domain_shared

identity_application_contracts
  -> identity_domain, identity_domain_shared, domain_shared

identity_application
  -> identity_application_contracts, identity_domain, identity_domain_shared,
     domain_shared

identity_sqlx
  -> identity_application, identity_application_contracts, identity_domain,
     identity_domain_shared, persistence

identity_http
  -> application, application_contracts, domain_shared, http_support,
     leptos_support, presentation

identity_leptos
  -> application, application_contracts, domain_shared, leptos_support,
     presentation, web
```

Domain and application code do not depend on Axum, Leptos, SQLx, Redis, or
vendor SDKs. Infrastructure implements business-facing ports. The deployable
application owns adapter, route, telemetry-settings, and worker composition.
Framework packages own reusable HTTP security policy, telemetry and operational
support, reusable Leptos integration and test support, capability identity,
configuration orchestration, provider-neutral persistence and background-work
primitives, runtime roles, process execution, and shutdown signaling.

### Enforced Workspace Dependencies

The following table is the allowlist for direct local Cargo dependencies.
Entries are permitted edges, not required dependencies. Removing an edge does
not violate the policy; adding an edge requires an architecture decision and a
matching policy update.

| Package | Permitted direct local dependencies |
|---|---|
| `hegira` | `application`, `application_contracts`, `background_jobs`, `configuration`, `domain`, `domain_shared`, `http_support`, `identity_http`, `identity_leptos`, `infrastructure`, `observability`, `persistence`, `platform_core`, `presentation`, `runtime`, `test_support`, `web` |
| `platform_core` | None |
| `configuration` | None |
| `persistence` | None |
| `background_jobs` | None |
| `http_support` | None |
| `leptos_support` | None |
| `observability` | `background_jobs` |
| `test_support` | `application` |
| `domain_shared` | None |
| `domain` | `domain_shared` |
| `application_contracts` | `domain`, `domain_shared` |
| `application` | `application_contracts`, `background_jobs`, `domain`, `domain_shared` |
| `infrastructure` | `application`, `application_contracts`, `background_jobs`, `configuration`, `domain`, `domain_shared`, `persistence`, `platform_core`, `runtime` |
| `presentation` | `application`, `application_contracts`, `domain_shared`, `infrastructure`, `leptos_support`, `observability` |
| `web` | `application`, `application_contracts`, `domain_shared`, `leptos_support`, `presentation` |
| `runtime` | None |
| `db_migrator` | `infrastructure`, `persistence` |
| `identity_domain_shared` | None |
| `identity_domain` | `identity_domain_shared` |
| `identity_application_contracts` | `domain_shared`, `identity_domain`, `identity_domain_shared` |
| `identity_application` | `domain_shared`, `identity_application_contracts`, `identity_domain`, `identity_domain_shared` |
| `identity_sqlx` | `identity_application`, `identity_application_contracts`, `identity_domain`, `identity_domain_shared`, `persistence` |
| `identity_http` | `application`, `application_contracts`, `domain_shared`, `http_support`, `leptos_support`, `presentation` |
| `identity_leptos` | `application`, `application_contracts`, `domain_shared`, `leptos_support`, `presentation`, `web` |

The boundary check reads declared local dependencies from locked Cargo metadata,
classifies every workspace package by its repository location, and then applies
both the location ownership matrix and the package-specific table above.

| Package location | Ownership | May depend on ownership |
|---|---|---|
| `apps/` | Application composition | Application, framework, module |
| `crates/` | Framework | Framework |
| `modules/` | Official module | Framework, module |
| `templates/` | Application template | Framework, module, template |
| `tools/` | Repository tooling | Framework, module, template, tooling |

The checker does not require an unimplemented repository directory to exist.
An ownership rule becomes active when locked Cargo metadata contains a workspace
package under that location. Consequently, framework packages cannot depend on
modules, templates, tools, or deployable applications; module packages cannot
depend on templates, tools, or deployable applications. The same rules cover
normal, optional, development, and build dependencies, so dependency kind
cannot bypass the boundary. General third-party dependency policy remains the
responsibility of the supply-chain checks.

## Repository Layout

The repository root is a virtual Cargo workspace and remains the command entry
point for development, validation, packaging, and container commands. It does
not define a Rust package of its own.

```text
.
├── apps/
│   └── hegira/              deployable Axum/Leptos package
│       ├── Cargo.toml       feature composition and Cargo-Leptos metadata
│       ├── src/             server and hydration entry points
│       └── tests/           full-stack integration tests
├── crates/                  reusable layered workspace packages
├── modules/
│   └── identity/            official layered Identity module and adapters
├── config/                  environment configuration profiles
├── docs/                    current architecture and operations guides
├── ops/                     local observability configuration
├── scripts/                 validation, operations, and release helpers
├── Cargo.toml               virtual workspace manifest
└── Dockerfile               production image contract
```

The `apps/` directory contains deployable packages. Packages outside `apps/`
must remain reusable and cannot depend on an application package.

The `modules/identity/` directory owns the canonical Identity Domain Shared,
Domain, Application Contracts, Application, SQLx, Axum HTTP, and Leptos
adapter sources. The existing `domain_shared`, `domain`,
`application_contracts`, `application`, `infrastructure`, `presentation`, and
`web` packages compile the applicable canonical files as compatibility views
for current consumers without adding a forbidden framework dependency on a
module package. The host explicitly selects the Identity HTTP adapter. The
current `web` package compiles the module-owned Leptos source and consumes its
explicit route and navigation contributions while it continues to own the
application shell and shared UI primitives. General background-job, settings,
and storage application ports remain framework-owned.

## Workspace Packages

| Package | Location | Responsibility |
|---|---|---|
| `hegira` | `apps/hegira` | Deployable Axum/Leptos package and full-stack composition |
| `platform_core` | `crates/platform_core` | Application-independent compiled capability primitives |
| `configuration` | `crates/configuration` | Configuration profile sources and ordered validation orchestration |
| `persistence` | `crates/persistence` | Database provider selection, pools, health checks, transaction primitives, and host-owned migration planning and execution |
| `background_jobs` | `crates/background_jobs` | Job contracts, handler registration, observation, and recurring execution |
| `http_support` | `crates/http_support` | Application-independent Axum middleware, transport-policy markers, CSRF, trusted-proxy resolution, and rate limiting |
| `leptos_support` | `crates/leptos_support` | Product-neutral Leptos form state, mutation state, context access, and safe server-function errors |
| `observability` | `crates/observability` | Tracing initialization, health/readiness primitives, HTTP and background-job metrics, and Prometheus exposure |
| `test_support` | `crates/test_support` | Shared application-port test doubles and application-independent Axum request/response helpers |
| `domain_shared` | `crates/domain_shared` | Current compatibility view of Identity shared contracts plus application localization resources |
| `domain` | `crates/domain` | Current compatibility view of Identity entities and repository ports |
| `application_contracts` | `crates/application_contracts` | Current compatibility view of Identity DTOs, inputs, permissions, and feature metadata |
| `application` | `crates/application` | Current compatibility view of Identity use cases and provider-facing ports, plus framework-owned background-job, settings, and storage application ports |
| `infrastructure` | `crates/infrastructure` | Host infrastructure composition and adapters for jobs, security, cache, mail, search, storage, and the current Identity SQLx compatibility view |
| `presentation` | `crates/presentation` | Current service construction, host state, Leptos server-service context, and operational probe composition |
| `web` | `crates/web` | Leptos application shell, dashboard, shared UI primitives, and the current compatibility view of the Identity Leptos adapter |
| `runtime` | `crates/runtime` | Runtime roles, Tokio process lifecycle, and shutdown signaling |
| `db_migrator` | `crates/db_migrator` | Migration, reset, seed, and search reindex commands |
| `identity_domain_shared` | `modules/identity/domain_shared` | Identity errors, protected principal names, and shared security values |
| `identity_domain` | `modules/identity/domain` | Identity entities, value objects, and provider-neutral repository ports |
| `identity_application_contracts` | `modules/identity/application_contracts` | Identity DTOs, inputs, permission registry, and serialized application contracts |
| `identity_application` | `modules/identity/application` | Transport-independent Identity use cases, authorization, validation, transaction intent, and provider-facing ports |
| `identity_sqlx` | `modules/identity/sqlx` | PostgreSQL and SQLite Identity repositories, migrations, seeds, cleanup, reset, and search projection reads |
| `identity_http` | `modules/identity/http` | Identity Axum controllers, Bearer extraction, secure session-cookie handling, OpenAPI document, route contribution, and explicit cookie/Bearer transport-policy contribution |
| `identity_leptos` | `modules/identity/leptos` | Identity authentication, account, user, and role pages; server functions; and explicit Leptos route and navigation contributions |

Code is grouped by bounded context and capability rather than by database
table. Identity is the first official layered module and its domain and
application layers cover users, roles, sessions, OAuth, and TOTP without
exposing Axum, Leptos, SQLx, or provider types. `identity_sqlx` is the explicit
outward adapter that implements those ports for PostgreSQL and SQLite.
`identity_http` is the separately selected Axum adapter; its controller state
contains application services rather than host configuration or persistence.
`identity_leptos` is the separately buildable Leptos adapter. It uses Identity
contracts and host-provided application services without declaring SQLx or
persistence dependencies; UI permission gates remain presentation behavior,
while application services continue to authorize protected operations.
The authenticated web surface starts from a neutral dashboard instead of
composing a sample business capability.

## Application Composition And Build Ownership

`apps/hegira/Cargo.toml` is the package-level composition root. It forwards
database and optional capability features to the packages that implement them,
selects the application migration sources used for startup migration, and owns
the Cargo-Leptos metadata for the server binary, hydrated library, stylesheet,
public assets, and workspace-defined WASM release profile.

The server entry point at `apps/hegira/src/main.rs` delegates application
startup to `apps/hegira/src/server`. That composition root loads the concrete
application configuration, runs the framework-owned structural, capability,
and production validation pipeline, and only then constructs infrastructure
adapters, presentation services, HTTP routes, telemetry, and worker loops. The
framework `runtime` package supplies the Tokio process runner, runtime-role
primitive, and operating-system shutdown signal without depending on
application packages. The hydrated entry point at `apps/hegira/src/lib.rs`
mounts the `web` application. Full-stack integration tests remain beside the
deployable package under `apps/hegira/tests`.

Cargo commands are run from the repository root and select the application with
`-p hegira`. Cargo-Leptos reads the package metadata from
`apps/hegira/Cargo.toml`; frontend sources and public assets remain owned by
`crates/web`, while generated server and site outputs are written to the
workspace-level `target/release` and `target/site` directories.

## Request Boundaries

The browser-facing Leptos application uses a BFF session model:

```text
Browser -> HttpOnly cookie -> Leptos server function -> application service
```

Hydrated client code never receives the actor token. External clients use the
Bearer-authenticated Axum API:

```text
Client -> Axum controller -> application service
```

Both entry points use the same service instance from `AppServices`.
`identity_http` receives an `IdentityHttpState` containing only those services;
controllers cannot reach the host database or provider adapters directly.
Controllers and server functions deserialize and delegate; business
validation, authorization, and concurrency policy stay in the application
layer.

### Transport Middleware Contract

Runtime composition uses the explicit `identity_http` cookie-BFF and Bearer-API
policy contribution, backed by `http_support` primitives, to keep transport
authentication policies separate instead of inferring them from URL prefixes:

| Route group | Authentication model | CSRF policy |
|---|---|---|
| Operational routes | Health/readiness; restrict operational exposure at the network layer | No browser CSRF layer |
| Axum API | Bearer token | No browser CSRF layer |
| Leptos BFF and browser routes | HttpOnly session cookie where authenticated | CSRF validation on unsafe methods |

Cookie-authenticated `POST`, `PUT`, `PATCH`, and `DELETE` requests require a
same-origin `Origin` header, or a same-origin `Referer` only when `Origin` is
absent. `GET`, `HEAD`, and `OPTIONS` are exempt. When `Origin` is present it is
authoritative; a malformed or cross-origin value cannot be rescued by
`Referer`.

The trusted origin comes from validated `application.public_url`, never from a
request `Host` or forwarded host header. Bearer API mutations remain usable by
non-browser clients without browser-origin headers. Both transports still pass
through the common request ID, security header, CORS, timeout, body-limit,
compression, tracing, and configured rate-limit layers. Application-layer
authorization remains mandatory for both.

`http_support` owns request IDs, security headers, CSRF, trusted-proxy-aware
client address resolution, rate limiting, and request tracing without depending
on application packages. `observability` owns provider-neutral liveness and
readiness response construction, tracing subscriber initialization, and the
existing optional OTLP and Prometheus adapters. Application operational routes
remain outside these framework packages because they select and probe concrete
application dependencies.

## Persistence And Transactions

Repository traits describe business persistence without SQLx types. PostgreSQL
and SQLite use separate adapters and migration directories so provider-specific
behavior remains visible.

The `persistence` framework package owns explicit database-provider selection,
connection pools, health checks, SQLx transaction types, and the reusable
migration contribution and execution contract. It does not own application
schemas, migration SQL, or repositories. A migration source has a stable
lowercase module identity and contributes immutable SQLx migration identities
and checksums. The host sorts selected migrations by their global numeric
identity and rejects invalid or duplicate module identities, duplicate
migration identities, and checksum conflicts before database execution.

`identity_sqlx` owns the PostgreSQL and SQLite Identity repositories,
provider-specific migration sources, seed behavior, cleanup queries, reset
behavior, and search projection reads. The remaining host migrations own
application settings, durable messaging, search projection state, audit
storage, and the frozen Catalog creation history. The immutable PostgreSQL 22
and SQLite 9 retirement migrations remain with the Identity source because
they remove retired permissions; their cleanup of host outbox, projection, and
Catalog state is a historical compatibility exception whose bytes and
checksums cannot be split. The current `infrastructure` compatibility view
compiles the module-owned adapter sources for existing consumers.

The deployable application and the dedicated `db_migrator` are hosts: each
explicitly selects both the host and Identity sources, builds one validated
plan, and delegates execution to `persistence`. Existing migration numbers,
SQL, and checksums remain immutable so databases created by v0.2.0 retain valid
SQLx history. New module sources must use globally unique migration numbers
across the complete host plan.

Standard mutations follow this order:

1. Resolve the current actor.
2. Require the application permission.
3. Validate input and domain invariants.
4. Execute the mutation and audit write in one database transaction.
5. Return a transport-independent application result.

Transactions are use-case boundaries, not HTTP-request middleware. Durable
outbox records are committed with state changes when asynchronous work must be
published reliably.

The `background_jobs` framework package owns job dispatch, durable queue and
handler contracts, handler registration, observation, and recurring execution.
PostgreSQL and SQLite outbox workers remain host infrastructure adapters because
their SQL contract is backed by host-owned durable-message migrations. Identity
mutations publish the stable mail and search payload contracts into that outbox
inside the same SQLx transaction as the Identity state change.

## Feature Registration

Permission descriptors are compile-time Rust values. The host explicitly
selects the `identity_http` route and OpenAPI contributions, while the current
Leptos composition explicitly consumes `identity_leptos` route and navigation
contributions. There is no runtime plugin discovery or automatic endpoint
publication. Concrete service composition and adapter contributions remain
explicit because Rust and the relevant proc macros require concrete types.

## Frontend

The `web` crate contains the app shell, dashboard, branding, and shared visual
components. `identity_leptos` owns the Identity authentication, account, user,
and role pages and their feature-local server functions. During the current
compatibility phase, `web` compiles those canonical sources through an explicit
source view, so framework packages still do not depend on module packages.
Product-neutral form state, mutation state, server context access, and safe
server-function error construction live in `leptos_support`; they do not grant
authorization. Standard CRUD pages reuse `CrudListState`, `CrudDialog`, and
`MutationStatus`; custom workflows may use ordinary Leptos signals and typed
services.

The `test_support` package owns reusable recording and in-memory implementations
of application capability ports plus generic Axum request and JSON response
helpers. It depends on the application interfaces it implements; production
feature graphs do not enable it. Host database helpers remain in
`infrastructure`; canonical Identity migrations, focused provider tests, and
seed behavior live with `identity_sqlx`.

SSR renders the initial route and hydration adds browser interaction. Release
WASM uses the `wasm-release` profile with size optimization, LTO, one codegen
unit, symbol stripping, and aborting panics. Optional route splitting is behind
`wasm-split` and should only be evaluated with release builds.

## Runtime Roles

One compiled server can run in three modes:

| Role | Web server | Worker loops |
|---|---:|---:|
| `all` | Yes | Yes |
| `web` | Yes | No |
| `worker` | No | Yes |

This permits a single process for small deployments and independently scaled
web and worker replicas for larger deployments. SQLite should use a single
application/worker process; distributed roles are intended for PostgreSQL.

## Optional Capabilities

External integrations are ports in the application layer and adapters in
infrastructure. Cargo features remove unused vendor dependencies from the
build; runtime configuration selects an enabled adapter. See
[Configuration](configuration.md) for the complete matrix.

## Security Model

- Argon2 password hashing and signed application tokens
- HttpOnly session cookies for the Leptos BFF
- Bearer authentication for the external API
- Application-layer RBAC checks
- Origin-based CSRF validation for every unsafe cookie-authenticated mutation
- Request limits, rate limiting, security headers, and request IDs
- OAuth state expiry/single use and TOTP backup-code replay protection
- Production configuration validation for secrets, OpenAPI, seed, and metrics

UI permission checks only control presentation. Every protected operation is
authorized again in the application service.

## Design Rules

- Keep domain rules independent from transports and providers.
- Keep SQL explicit and provider-specific.
- Do not expose database rows as API contracts.
- Do not open transactions around entire HTTP requests.
- Do not automatically publish entities as endpoints.
- Prefer typed composition over service locators.
- Keep optional services disabled until a use case requires them.
- Test provider adapters against the same behavioral contract.
