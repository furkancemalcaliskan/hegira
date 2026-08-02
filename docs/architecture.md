# Architecture

Hegira uses an ABP-inspired layered design adapted to Rust, Axum, Leptos, and
explicit compile-time composition. It avoids runtime reflection, assembly
scanning, ambient request transactions, and a universal repository.

This repository has three distinct application-facing roles. Retained framework
packages and official modules provide reusable source,
`templates/applications/layered/` is the canonical source for an independently
owned generated application, and `apps/hegira/` plus the legacy layered crates
form the current compatibility application used to exercise framework
integration. The compatibility host is not the generated application's
ownership model, and the internal renderer is not a public CLI.

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

platform_core, audit, cache, mail, search, security, settings, storage,
configuration, persistence, background_jobs, http_support, leptos_support,
runtime
  -> no local application packages

observability
  -> background_jobs

test_support
  -> audit, background_jobs, cache, mail, settings, storage

domain_shared
  -> identity_domain_shared

domain
  -> identity_domain

application_contracts
  -> identity_application_contracts

application
  -> background_jobs, identity_application, settings, storage

web
  -> leptos_support, presentation, application, application_contracts,
     domain_shared

presentation
  -> leptos_support, observability, infrastructure, application,
     application_contracts, domain_shared

infrastructure
  -> platform_core, configuration, persistence, background_jobs, runtime,
     application, application_contracts, domain, domain_shared

db_migrator
  -> infrastructure, persistence

identity_domain_shared
  -> no local packages

identity_domain
  -> identity_domain_shared

identity_application_contracts
  -> identity_domain, identity_domain_shared

identity_application
  -> audit, cache, mail, search, security, identity_application_contracts,
     identity_domain, identity_domain_shared

identity_sqlx
  -> identity_application, identity_application_contracts, identity_domain,
     identity_domain_shared, persistence

identity_http
  -> identity_application, identity_application_contracts, http_support,
     leptos_support

identity_leptos
  -> application, application_contracts, domain_shared, leptos_support,
     presentation, web

template_renderer
  -> no local packages
```

Domain and application code do not depend on Axum, Leptos, SQLx, Redis, or
vendor SDKs. Infrastructure implements business-facing ports. The deployable
application owns adapter, route, telemetry-settings, and worker composition.
Framework packages own reusable HTTP security policy, telemetry and operational
support, reusable Leptos integration and test support, provider-neutral audit,
cache, mail, search, security, settings, storage, persistence, and
background-work ports, capability identity, configuration orchestration,
runtime roles, process execution, and shutdown signaling.

### Enforced Workspace Dependencies

The following table is the allowlist for direct local Cargo dependencies.
Entries are permitted edges, not required dependencies. Removing an edge does
not violate the policy; adding an edge requires an architecture decision and a
matching policy update.

| Package | Permitted direct local dependencies |
|---|---|
| `hegira` | `application`, `application_contracts`, `background_jobs`, `configuration`, `domain`, `domain_shared`, `http_support`, `identity_http`, `identity_leptos`, `infrastructure`, `observability`, `persistence`, `platform_core`, `presentation`, `runtime`, `test_support`, `web` |
| `platform_core` | None |
| `audit` | None |
| `cache` | None |
| `mail` | None |
| `search` | None |
| `security` | None |
| `settings` | None |
| `storage` | None |
| `configuration` | None |
| `persistence` | None |
| `background_jobs` | None |
| `http_support` | None |
| `leptos_support` | None |
| `observability` | `background_jobs` |
| `test_support` | `audit`, `background_jobs`, `cache`, `mail`, `settings`, `storage` |
| `domain_shared` | `identity_domain_shared` |
| `domain` | `identity_domain` |
| `application_contracts` | `identity_application_contracts` |
| `application` | `background_jobs`, `identity_application`, `settings`, `storage` |
| `infrastructure` | `application`, `application_contracts`, `background_jobs`, `configuration`, `domain`, `domain_shared`, `identity_sqlx`, `persistence`, `platform_core`, `runtime` |
| `presentation` | `application`, `application_contracts`, `domain_shared`, `infrastructure`, `leptos_support`, `observability` |
| `web` | `application`, `application_contracts`, `domain_shared`, `leptos_support`, `presentation` |
| `runtime` | None |
| `db_migrator` | `infrastructure`, `persistence` |
| `identity_domain_shared` | None |
| `identity_domain` | `identity_domain_shared` |
| `identity_application_contracts` | `identity_domain`, `identity_domain_shared` |
| `identity_application` | `audit`, `cache`, `identity_application_contracts`, `identity_domain`, `identity_domain_shared`, `mail`, `search`, `security` |
| `identity_sqlx` | `identity_application`, `identity_application_contracts`, `identity_domain`, `identity_domain_shared`, `persistence` |
| `identity_http` | `http_support`, `identity_application`, `identity_application_contracts`, `leptos_support` |
| `identity_leptos` | `application`, `application_contracts`, `domain_shared`, `leptos_support`, `presentation`, `web` |
| `template_renderer` | None |

The boundary check reads declared local dependencies from locked Cargo metadata
and applies both the package-specific allowlist above and the explicit package
role contract below. A physical directory alone does not make a package part of
the final framework: compatibility packages under `crates/` remain classified
as compatibility code until their accepted extraction and retirement work is
complete.

| Package role | May depend on role |
|---|---|
| Framework | Framework |
| Official module | Framework, official module |
| Application | Framework, official module, application |
| Compatibility | Framework, official module, compatibility |
| Repository tooling | Framework, official module, repository tooling |

Generated-application and application-template packages are deliberately not
members of the framework workspace. They are validated as independent Cargo
workspaces and may consume framework and selected official-module packages.
Framework packages cannot depend on modules, compatibility code, tools, or
applications. Official modules cannot depend on compatibility code, tools, or
applications except for the exact issue-bound transition edges listed below.

### v0.4 Package Ownership And Disposition Contract

This table records the disposition accepted for every current framework
workspace package. It describes active transition work; it does not claim that
the referenced follow-up issue has already been implemented.

| Package | Role | Disposition | Follow-up |
|---|---|---|---|
| `hegira` | Compatibility host | Replace and retire | #145, #146 |
| `platform_core` | Framework | Retain | None |
| `audit` | Framework | Retain | None |
| `cache` | Framework | Retain | None |
| `mail` | Framework | Retain | None |
| `search` | Framework | Retain | None |
| `security` | Framework | Retain | None |
| `settings` | Framework | Retain | None |
| `storage` | Framework | Retain | None |
| `configuration` | Framework | Retain | None |
| `persistence` | Framework | Retain | None |
| `background_jobs` | Framework | Retain | None |
| `http_support` | Framework | Retain | None |
| `leptos_support` | Framework | Retain | None |
| `observability` | Framework | Retain | None |
| `test_support` | Framework | Retain | None |
| `runtime` | Framework | Retain | None |
| `domain_shared` | Compatibility | Extract and retire | #146 |
| `domain` | Compatibility | Extract and retire | #146 |
| `application_contracts` | Compatibility | Extract and retire | #146 |
| `application` | Compatibility | Extract and retire | #146 |
| `infrastructure` | Compatibility | Extract and retire | #137, #138, #146 |
| `presentation` | Compatibility | Extract and retire | #139, #146 |
| `web` | Compatibility | Extract and retire | #135, #141, #146 |
| `db_migrator` | Compatibility | Replace and retire | #142, #146 |
| `identity_domain_shared` | Official module | Retain | None |
| `identity_domain` | Official module | Retain | None |
| `identity_application_contracts` | Official module | Retain | None |
| `identity_application` | Official module | Retain | None |
| `identity_sqlx` | Official module | Retain | None |
| `identity_http` | Official module | Retain | None |
| `identity_leptos` | Official module | Decouple and retain | #135 |
| `template_renderer` | Repository tooling | Refactor and retain | #148 |

`Retain` means the package already has its final owner. `Decouple and retain`
and `canonicalize and retain` keep the package while removing compatibility
dependencies or duplicate implementations. `Extract and retire` requires its
reusable and application-owned behavior to move before deletion. `Replace and
retire` requires a verified successor before deletion. `Refactor and retain`
keeps an internal tool while changing its boundary.

The current graph needs these temporary outward edges while the accepted
follow-up issues remove them:

| Transitional edge | Removal issue |
|---|---|
| `identity_leptos -> application, application_contracts, domain_shared, presentation, web` | #135 |

The checker permits only these exact edges, requires every exception to name an
accepted issue, and fails when an exception becomes stale. New framework or
module dependencies on compatibility code are rejected. Normal, optional,
development, and build dependencies use the same checks, so dependency kind
cannot bypass the boundary.

The canonical layered application has a separate ownership contract:

| Template package | Owner | Disposition |
|---|---|---|
| `app_server` | Generated application | Retain as server composition root |
| `app_web` | Generated application | Retain as client composition root |
| `app_domain_shared` | Generated application | Retain as application DDD layer |
| `app_domain` | Generated application | Retain as application DDD layer |
| `app_application_contracts` | Generated application | Retain as application DDD layer |
| `app_application` | Generated application | Retain as application DDD layer |
| `app_infrastructure` | Generated application | Retain as application DDD layer |
| `app_presentation` | Generated application | Retain as application DDD layer |

These packages remain under `templates/applications/layered/` as source for an
independently owned generated repository. They must not be added to the Hegira
framework workspace. General third-party dependency policy remains the
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
├── templates/
│   ├── applications/
│   │   └── layered/         workspace-external layered full-stack source
│   └── components/          typed application-component manifests
├── tools/
│   └── template_renderer/   internal deterministic template renderer
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

The canonical source at `templates/applications/layered` renders a separate
Cargo workspace and is deliberately absent from the framework workspace member
list. It defines a brand-neutral `apps/server` composition root and explicit
Domain Shared, Domain, Application Contracts, Application, Infrastructure, and
Presentation packages. Its `apps/web` package owns the default Leptos shell,
branding assets, neutral dashboard, routes, and hydration entry point. The
server explicitly composes the Identity HTTP adapter, and the client explicitly
composes the Identity Leptos adapter; neither surface discovers module
contributions implicitly. The rendered base owns its application configuration
profiles, full-stack Dockerfile, and local PostgreSQL contract.

Framework dependencies in the canonical manifest use the pinned `v0.3.0` Git
source rather than paths into this repository. The typed template manifest
selects the `layered-base` and `layered-leptos-identity` component graph.
`template_renderer` resolves requirements and conflicts, substitutes declared
variables, detects output collisions, and constructs the complete output plan
before writing. It rejects symbolic links and path traversal, does not execute
component scripts, and publishes a successful render by renaming a private
staging directory into a previously absent destination.

Before the release tag exists, `scripts/layered-template-check.sh` asks the
renderer to patch declared framework dependencies only in its disposable
validation output. A normal render retains the release-style Git source and is
rejected if repository-local absolute paths leak into its files. Validation
covers renderer snapshots and failure paths, native server and test targets,
the browser hydration target, and the Cargo Leptos release output. This
maintainer tool is not exposed as the public Hegira CLI.

`scripts/generated-application-check.sh` exercises the rendered application as
the release unit. It applies fresh SQLite and PostgreSQL migration plans,
verifies the supported v0.2.0 upgrade with historical Catalog state, builds the
rendered production image, and boots it against disposable PostgreSQL. HTTP
smoke checks cover readiness, hydration assets, production security headers,
and unauthenticated Bearer API behavior. All databases and rendered outputs are
ephemeral; destructive PostgreSQL tests require the script's explicit reset
opt-in and dedicated generated-application database variable. Repository
validation stages a credential-free framework source view inside the disposable
render and uses validated relative dependency paths so the host and Docker
build consume the same manifests without leaking maintainer filesystem paths.

## Workspace Packages

| Package | Location | Responsibility |
|---|---|---|
| `hegira` | `apps/hegira` | Deployable Axum/Leptos package and full-stack composition |
| `platform_core` | `crates/platform_core` | Application-independent compiled capability primitives |
| `audit` | `crates/audit` | Provider-neutral audit records and logging port |
| `cache` | `crates/cache` | Provider-neutral string-cache port and expiration contract |
| `mail` | `crates/mail` | Provider-neutral mail message values and delivery port |
| `search` | `crates/search` | Provider-neutral search documents, queries, indexing commands, and search port |
| `security` | `crates/security` | Provider-neutral password hashing and token ports |
| `settings` | `crates/settings` | Validated setting keys, typed serialization helpers, and settings port |
| `storage` | `crates/storage` | Validated storage paths, stored objects, and object-storage port |
| `configuration` | `crates/configuration` | Configuration profile sources and ordered validation orchestration |
| `persistence` | `crates/persistence` | Database provider selection, pools, health checks, transaction primitives, and host-owned migration planning and execution |
| `background_jobs` | `crates/background_jobs` | Job contracts, handler registration, observation, and recurring execution |
| `http_support` | `crates/http_support` | Application-independent Axum middleware, transport-policy markers, CSRF, trusted-proxy resolution, and rate limiting |
| `leptos_support` | `crates/leptos_support` | Product-neutral Leptos form state, mutation state, context access, and safe server-function errors |
| `observability` | `crates/observability` | Tracing initialization, health/readiness primitives, HTTP and background-job metrics, and Prometheus exposure |
| `test_support` | `crates/test_support` | Shared framework capability-port test doubles and application-independent Axum request/response helpers |
| `domain_shared` | `crates/domain_shared` | Current compatibility view of Identity shared contracts plus application localization resources |
| `domain` | `crates/domain` | Current compatibility view of Identity entities and repository ports |
| `application_contracts` | `crates/application_contracts` | Current compatibility view of Identity DTOs, inputs, permissions, and feature metadata |
| `application` | `crates/application` | Current compatibility view of Identity use cases and framework capability ports |
| `infrastructure` | `crates/infrastructure` | Host infrastructure composition and adapters for jobs, security, cache, mail, search, storage, and compatibility re-exports of the canonical Identity SQLx adapter |
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
| `template_renderer` | `tools/template_renderer` | Internal typed component resolution, deterministic rendering, local validation patching, collision detection, and atomic output publication |

Code is grouped by bounded context and capability rather than by database
table. Identity is the first official layered module and its domain and
application layers cover users, roles, sessions, OAuth, and TOTP without
exposing Axum, Leptos, SQLx, or provider types. `identity_sqlx` is the explicit
outward adapter that implements those ports for PostgreSQL and SQLite.
`identity_http` is the separately selected Axum adapter; its controller state
contains object-safe Identity use-case contracts rather than host configuration,
persistence, or compatibility Presentation services. The application host
injects each concrete service explicitly when it selects the adapter.
`identity_leptos` is the separately buildable Leptos adapter. It uses Identity
contracts and host-provided application services without declaring SQLx or
persistence dependencies; UI permission gates remain presentation behavior,
while application services continue to authorize protected operations.
The authenticated web surface starts from a neutral dashboard instead of
composing a sample business capability.

## Compatibility Host And Generated Application Ownership

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

The canonical layered base is validated separately with:

```sh
sh scripts/layered-template-check.sh
```

That check compiles every template layer and feature against the current
framework checkout without adding the rendered application to the framework
workspace. The existing `apps/hegira` package remains the full-stack
compatibility host while the canonical template provides the default Leptos
and Identity composition for newly rendered applications.

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

Both entry points use the same host-selected service composition.
`identity_http` receives an `IdentityHttpState` containing only object-safe
use-case contracts; controllers cannot reach the host database, concrete
provider adapters, or compatibility Presentation state directly.
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

`identity_sqlx` is the single implementation owner for the PostgreSQL and
SQLite Identity repositories, provider-specific migration sources, seed
behavior, cleanup queries, reset behavior, and search projection reads. Its
typed adapter surface is consumed directly by hosts and re-exported by the
current `infrastructure` compatibility package; compatibility code does not
include or compile a second copy of the adapter sources.

The remaining host migrations own application settings, durable messaging,
search projection state, audit storage, and the frozen Catalog creation
history. The immutable PostgreSQL 22 and SQLite 9 retirement migrations are
application-owned historical migrations because they clean host outbox,
projection, and retired Catalog state in addition to retired Identity
permissions. They keep their original identities and exact bytes so existing
SQLx histories remain valid. Repository policy permits only these exact
checksummed exceptions outside `identity_sqlx`; new Identity schema SQL remains
module-owned.

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
of framework capability ports plus generic Axum request and JSON response
helpers. It depends directly on those provider-neutral contracts rather than
the compatibility Application package; production feature graphs do not enable
it. Host database helpers remain in
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
