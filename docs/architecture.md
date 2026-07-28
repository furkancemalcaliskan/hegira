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
  -> platform_core, configuration, background_jobs, http_support,
     observability, runtime, test_support, web,
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
  -> infrastructure
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
| `hegira` | `application`, `application_contracts`, `background_jobs`, `configuration`, `domain`, `domain_shared`, `http_support`, `infrastructure`, `observability`, `platform_core`, `presentation`, `runtime`, `test_support`, `web` |
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
| `db_migrator` | `infrastructure` |

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
├── config/                  environment configuration profiles
├── docs/                    current architecture and operations guides
├── ops/                     local observability configuration
├── scripts/                 validation, operations, and release helpers
├── Cargo.toml               virtual workspace manifest
└── Dockerfile               production image contract
```

The `apps/` directory contains deployable packages. Packages outside `apps/`
must remain reusable and cannot depend on an application package.

## Workspace Packages

| Package | Location | Responsibility |
|---|---|---|
| `hegira` | `apps/hegira` | Deployable Axum/Leptos package and full-stack composition |
| `platform_core` | `crates/platform_core` | Application-independent compiled capability primitives |
| `configuration` | `crates/configuration` | Configuration profile sources and ordered validation orchestration |
| `persistence` | `crates/persistence` | Database provider selection, pools, health checks, and transaction primitives |
| `background_jobs` | `crates/background_jobs` | Job contracts, handler registration, observation, and recurring execution |
| `http_support` | `crates/http_support` | Application-independent Axum middleware, transport-policy markers, CSRF, trusted-proxy resolution, and rate limiting |
| `leptos_support` | `crates/leptos_support` | Product-neutral Leptos form state, mutation state, context access, and safe server-function errors |
| `observability` | `crates/observability` | Tracing initialization, health/readiness primitives, HTTP and background-job metrics, and Prometheus exposure |
| `test_support` | `crates/test_support` | Shared application-port test doubles and application-independent Axum request/response helpers |
| `domain_shared` | `crates/domain_shared` | Shared errors, identifiers, and localization resources |
| `domain` | `crates/domain` | Entities, invariants, repository ports, and business concepts |
| `application_contracts` | `crates/application_contracts` | DTOs, inputs, permissions, and feature metadata |
| `application` | `crates/application` | Use cases, authorization, validation, and transaction intent |
| `infrastructure` | `crates/infrastructure` | Application migrations and SQLx adapters for Identity, jobs, security, cache, mail, search, and storage |
| `presentation` | `crates/presentation` | Application Axum controllers, OpenAPI, sessions, operational probe composition, and route composition |
| `web` | `crates/web` | Leptos routes, pages, components, and server functions |
| `runtime` | `crates/runtime` | Runtime roles, Tokio process lifecycle, and shutdown signaling |
| `db_migrator` | `crates/db_migrator` | Migration, reset, seed, and search reindex commands |

Code is grouped by bounded context and capability rather than by database
table. Identity is the active application context and covers users, roles,
sessions, OAuth, and TOTP. The authenticated web surface starts from a neutral
dashboard instead of composing a sample business capability.

## Application Composition And Build Ownership

`apps/hegira/Cargo.toml` is the package-level composition root. It forwards
database and optional capability features to the packages that implement them
and owns the Cargo-Leptos metadata for the server binary, hydrated library,
stylesheet, public assets, and workspace-defined WASM release profile.

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

Both entry points use the same service instance from `AppServices`. Controllers
and server functions deserialize and delegate; business validation,
authorization, and concurrency policy stay in the application layer.

### Transport Middleware Contract

Runtime composition uses the `http_support` cookie-BFF and Bearer-API policy
primitives to keep transport authentication policies separate instead of
inferring them from URL prefixes:

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
connection pools, health checks, and SQLx transaction types. It does not own
application schemas, migrations, or repositories. The `infrastructure`
package embeds the current application migrations and implements Identity
repositories against the selected provider.

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
PostgreSQL and SQLite outbox workers remain infrastructure adapters because
their SQL contract is backed by the application-owned migrations. This keeps
framework primitives reusable without prematurely assigning module migration
ownership.

## Feature Registration

Feature descriptors are compile-time Rust values. They contribute permission
discovery and Axum/OpenAPI registration without introducing runtime plugins.
Concrete service composition, Leptos routes, and navigation remain explicit
because Rust and the relevant proc macros require concrete types.

## Frontend

The `web` crate contains the app shell, branding, shared visual components, and
bounded-context features. Feature-local server functions form the data
boundary. Product-neutral form state, mutation state, server context access,
and safe server-function error construction live in `leptos_support`; they do
not grant authorization. Standard CRUD pages reuse `CrudListState`,
`CrudDialog`, and `MutationStatus`; custom workflows may use ordinary Leptos
signals and typed services.

The `test_support` package owns reusable recording and in-memory implementations
of application capability ports plus generic Axum request and JSON response
helpers. It depends on the application interfaces it implements; production
feature graphs do not enable it. Database reset, application migration, and
Identity seed helpers remain in `infrastructure` because they operate on the
current application schema rather than a framework-neutral test contract.

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
