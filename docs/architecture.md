# Architecture

Hegira uses an ABP-inspired layered design adapted to Rust, Axum, Leptos, and
explicit compile-time composition. It avoids runtime reflection, assembly
scanning, ambient request transactions, and a universal repository.

## Dependency Direction

```text
web
  -> presentation
    -> application
      -> domain
        -> domain_shared

infrastructure implements domain and application ports.
runtime composes adapters, services, HTTP, and worker loops.
db_migrator owns migration and seed commands.
```

Dependencies point toward business rules. Domain and application code do not
depend on Axum, Leptos, SQLx, Redis, or vendor SDKs.

### Enforced Workspace Dependencies

The following table is the allowlist for direct local Cargo dependencies.
Entries are permitted edges, not required dependencies. Removing an edge does
not violate the policy; adding an edge requires an architecture decision and a
matching policy update.

| Package | Permitted direct local dependencies |
|---|---|
| `hegira` | `application`, `application_contracts`, `domain`, `domain_shared`, `infrastructure`, `presentation`, `runtime`, `web` |
| `domain_shared` | None |
| `domain` | `domain_shared` |
| `application_contracts` | `domain`, `domain_shared` |
| `application` | `application_contracts`, `domain`, `domain_shared` |
| `infrastructure` | `application`, `application_contracts`, `domain`, `domain_shared` |
| `presentation` | `application`, `application_contracts`, `domain_shared`, `infrastructure` |
| `web` | `application`, `application_contracts`, `domain_shared`, `presentation` |
| `runtime` | `infrastructure`, `presentation`, `web` |
| `db_migrator` | `infrastructure` |

The boundary check reads declared local dependencies from locked Cargo metadata,
rejects edges outside this table, and prevents packages outside `apps/` from
depending on deployable packages under `apps/`. It covers normal, development,
optional, and build dependencies declared between workspace packages. General
third-party dependency policy remains the responsibility of the supply-chain
checks.

## Workspace Crates

The repository root is a virtual Cargo workspace and remains the command entry
point for development and validation. The deployable full-stack package lives
at `apps/hegira`; it owns the server and hydration entry points, integration
tests, application feature composition, and Cargo-Leptos metadata. Shared
layers remain under `crates/`.

| Crate | Responsibility |
|---|---|
| `apps/hegira` | Deployable Axum/Leptos package and full-stack composition |
| `domain_shared` | Shared errors, identifiers, and localization resources |
| `domain` | Entities, invariants, repository ports, and business concepts |
| `application_contracts` | DTOs, inputs, permissions, and feature metadata |
| `application` | Use cases, authorization, validation, and transaction intent |
| `infrastructure` | SQLx adapters, security, jobs, cache, mail, search, and storage |
| `presentation` | Axum controllers, middleware, OpenAPI, sessions, and composition |
| `web` | Leptos routes, pages, components, and server functions |
| `runtime` | Process startup, web/worker roles, telemetry, and shutdown |
| `db_migrator` | Migration, reset, seed, and search reindex commands |

Code is grouped by bounded context and capability rather than by database
table. `Catalog::Products` is the reference feature; Identity is the larger
example covering users, roles, sessions, OAuth, and TOTP.

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

Runtime composition keeps transport authentication policies separate instead
of inferring them from URL prefixes:

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

## Persistence And Transactions

Repository traits describe business persistence without SQLx types. PostgreSQL
and SQLite use separate adapters and migration directories so provider-specific
behavior remains visible.

Standard mutations follow this order:

1. Resolve the current actor.
2. Require the application permission.
3. Validate input and domain invariants.
4. Execute the mutation and audit write in one database transaction.
5. Return a transport-independent application result.

Transactions are use-case boundaries, not HTTP-request middleware. Durable
outbox records are committed with state changes when asynchronous work must be
published reliably.

## Feature Registration

Feature descriptors are compile-time Rust values. They contribute permission
discovery and Axum/OpenAPI registration without introducing runtime plugins.
Concrete service composition, Leptos routes, and navigation remain explicit
because Rust and the relevant proc macros require concrete types.

The reference workflow is documented in [CRUD Tutorial](crud-tutorial.md).

## Frontend

The `web` crate contains app shell, shared UI primitives, and bounded-context
features. Feature-local server functions form the data boundary. Standard CRUD
pages reuse `CrudListState`, `CrudDialog`, and `MutationStatus`; custom
workflows may use ordinary Leptos signals and typed services.

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
