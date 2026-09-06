# Architecture

Hegira uses an ABP-inspired layered design adapted to Rust, Axum, Leptos, SQLx,
and explicit compile-time composition. It avoids runtime reflection, assembly
scanning, ambient request transactions, and a universal repository.

The repository contains three authoritative source categories:

- application-independent framework packages under `crates/`;
- official layered modules under `modules/`;
- an independently owned application base under
  `templates/applications/layered/`.

The repository does not contain a deployable compatibility application. Release
and integration validation render the canonical application and exercise it as
an external framework consumer. `tools/template_renderer` is internal
maintainer tooling, not the public Hegira CLI.

## Repository Layout

```text
.
├── crates/                  application-independent framework packages
├── modules/
│   └── identity/            official layered Identity module and adapters
├── templates/
│   ├── package.toml         versioned canonical package contract
│   ├── applications/
│   │   └── layered/         independent full-stack application source
│   └── components/          typed application-component manifests
├── tools/
│   ├── hegira_cli/          source-runnable CLI command shell
│   └── template_renderer/   render core and repository-validation adapter
├── docs/                    current technical and maintainer documentation
├── scripts/                 validation and release helpers
├── Cargo.toml               virtual framework workspace manifest
└── Cargo.lock               locked framework dependency graph
```

`templates/applications/layered/` is deliberately excluded from the root Cargo
workspace. A rendered application owns its server, web client, DDD layers,
configuration, migrations, deployment files, dependency lock, and future
product changes. The framework repository does not own an application runtime
configuration or production image.

## Dependency Direction

Framework packages cannot depend on official modules, generated applications,
templates, or tools. Official modules may depend on framework packages and
their own inward layers. Repository tooling may consume framework and module
metadata, but runtime packages never depend on tooling.

```text
generated application
  -> selected official modules
  -> framework packages

official module adapters
  -> official module application/domain layers
  -> framework packages

framework packages
  -> framework packages only
```

The direct local dependency allowlist is enforced from locked Cargo metadata by
`scripts/architecture-boundaries.mjs`.

| Workspace package | Permitted direct local dependencies |
|---|---|
| `application_manifest` | None |
| `platform_core` | None |
| `audit` | None |
| `cache` | None |
| `mail` | `background_jobs` |
| `search` | `background_jobs` |
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
| `runtime` | None |
| `identity_domain_shared` | None |
| `identity_domain` | `identity_domain_shared` |
| `identity_application_contracts` | `identity_domain`, `identity_domain_shared` |
| `identity_application` | `audit`, `cache`, `identity_application_contracts`, `identity_domain`, `identity_domain_shared`, `mail`, `search`, `security` |
| `identity_sqlx` | `background_jobs`, `identity_application`, `identity_application_contracts`, `identity_domain`, `identity_domain_shared`, `persistence`, `search` |
| `identity_http` | `http_support`, `identity_application`, `identity_application_contracts`, `leptos_support` |
| `identity_leptos` | `identity_application`, `identity_application_contracts`, `identity_domain_shared`, `leptos_support` |
| `hegira_cli` | `template_renderer` |
| `template_renderer` | `application_manifest` |

Normal, optional, development, and build dependencies use the same ownership
checks. The retired package names `hegira`, `domain_shared`, `domain`,
`application_contracts`, `application`, `infrastructure`, `presentation`,
`web`, and `db_migrator` are reserved by policy and cannot be reintroduced as
framework compatibility surfaces or consumed by generated applications.

## Framework Packages

| Package | Responsibility |
|---|---|
| `application_manifest` | Versioned, validated, deterministic `hegira.toml` generation-state contract |
| `platform_core` | Compiled capability identities and application-independent primitives |
| `audit` | Provider-neutral audit records and logging port |
| `cache` | Cache port plus null, memory, and optional Redis adapters |
| `mail` | Mail values and delivery port plus null, log, optional SMTP, and durable-handler adapters |
| `search` | Search contracts plus null, optional Meilisearch, and SQL projection-job adapters |
| `security` | Provider-neutral password hashing and token ports |
| `settings` | Validated setting keys, serialization helpers, and settings port |
| `storage` | Validated storage paths and storage port plus null, local, and optional S3 adapters |
| `configuration` | Profile sources and ordered configuration-validation orchestration |
| `persistence` | Provider selection, connections, pools, health, transactions, and migration primitives |
| `background_jobs` | Job contracts, handlers, observation, recurring execution, and durable workers |
| `http_support` | Axum request policy, CSRF, trusted-proxy resolution, rate limiting, and security headers |
| `leptos_support` | Product-neutral Leptos UI, form, routing, toast, cookie-session, and server-function primitives |
| `observability` | Tracing, probes, worker heartbeat state, metrics, Prometheus, and OTLP integration |
| `test_support` | Framework test doubles and application-independent Axum helpers |
| `runtime` | Runtime roles, Tokio lifecycle, process execution, and shutdown signaling |

These packages expose reusable primitives and provider adapters. They do not
contain application domain, application service, presentation, host
composition, or product UI code.

## Source-runnable CLI

`tools/hegira_cli` owns the `hegira` binary command shell. It defines top-level
help, version reporting, deterministic non-interactive application creation,
concise diagnostics, and stable process outcomes without reading a user home
directory or global configuration. It delegates canonical component planning
and atomic publication to `template_renderer`; repository-local dependency
rewrites remain unavailable to the public command. Help, version information,
and successful creation instructions are written to standard output; usage and
failure diagnostics are written to standard error.

The process outcomes are `0` for success, `1` for an internal error, `2` for
invalid usage, `3` for validation failure, and `4` for a destination or state
conflict. `hegira new <name> --destination <path>` renders the canonical
layered application with SQLite, Leptos, and Identity defaults. The database,
client, and component selections can also be stated explicitly. Generation
writes the destination atomically and never executes generated or external
commands.

When an application name or destination is omitted in an interactive terminal,
the same command gathers missing values through a guided workflow, displays the
implemented selections and defaults, and requires confirmation after a final
summary. The prompt path resolves into the same render request used by explicit
arguments. Cancellation or end-of-input occurs before rendering and leaves no
destination. Non-TTY execution never reads prompt input and requires both the
name and destination.

Project identity validation is shared with `application_manifest`. The CLI
validates identity and destination before rendering. The renderer requires an
existing real parent, opens every ancestor without following symlinks, and
creates private staging content through directory-relative operations. Before
publication it rechecks the parent's identity, staging identity, and destination
absence; an exclusive atomic rename also rejects destinations created after
the final check. Cleanup is anchored to the open directories and removes only
tracked staging entries. Unsupported publication platforms fail before writes.
The [getting-started contract](getting-started.md) defines the accepted names,
paths, permissions, and platform limitations.

## Official Identity Module

`modules/identity/` contains seven separately compiled packages:

```text
identity_domain_shared
  <- identity_domain
      <- identity_application_contracts
          <- identity_application
              <- identity_sqlx
              <- identity_http
              <- identity_leptos
```

The Domain Shared, Domain, Application Contracts, and Application packages are
transport- and provider-independent. `identity_sqlx` owns PostgreSQL and SQLite
repositories, module migrations, seed behavior, cleanup jobs, reset behavior,
and search projection reads. `identity_http` contributes explicit Axum routes,
OpenAPI, Bearer extraction, secure session-cookie handling, and separate
cookie/BFF and Bearer policies. `identity_leptos` contributes explicit pages,
server functions, routes, navigation, state, and layout integration.

Adapters are selected by the application composition root. Nothing discovers
or publishes modules implicitly. HTTP controllers and Leptos server functions
deserialize and delegate through object-safe application contracts; they do not
reach concrete databases or provider clients. UI permission checks remain
presentation behavior, while application services authorize protected use
cases.

## Canonical Generated Application

The layered application base renders this independent workspace:

```text
apps/server                 Axum runtime, Leptos SSR host, hydration entry
apps/web                    Leptos shell, navigation, localization, assets
crates/domain_shared        application shared domain values
crates/domain               application entities and ports
crates/application_contracts
crates/application          application use cases
crates/infrastructure       configuration and outward adapters
crates/presentation         HTTP-facing application composition
config                      application-owned environment profiles
Dockerfile                  application production image
docker-compose.yml          local PostgreSQL dependency
hegira.toml                 generation identity and selected adapters
```

The generated dependency contract is:

| Generated package | Permitted direct application and Hegira dependencies |
|---|---|
| `app_domain_shared` | None |
| `app_domain` | `app_domain_shared` |
| `app_application_contracts` | `app_domain`, `app_domain_shared` |
| `app_application` | `app_application_contracts`, `app_domain`, `app_domain_shared` |
| `app_infrastructure` | application layers; selected framework providers; Identity Domain, Application, and SQLx packages |
| `app_presentation` | application contracts, application service, shared domain values, and `http_support` |
| `app_web` | `identity_leptos`, `leptos_support` |
| `app_server` | application adapters, selected framework runtime packages, and selected Identity adapters |

Application domain and application packages remain independent from Axum,
Leptos, SQLx, Redis, and vendor SDKs. Transactions live at use-case boundaries.
The server explicitly composes configuration, persistence, providers, routes,
telemetry, runtime roles, and worker loops after validation succeeds.

The application infrastructure layer owns the immutable host migration history
needed for supported upgrades together with application-specific migrations.
It composes those sources with selected module migration sources. Historical
migration identifiers and checksums remain unchanged even after their runtime
feature is retired. Destructive reset requires an explicit disposable-database
authorization token and is never part of normal startup.

## Rendering And Validation

Component manifests select the layered base and the Leptos Identity adapter.
`templates/package.toml` gives this data-only graph a release-aligned package
identity, declares its compatible HTTPS framework source and stable SemVer
tag, enumerates the contained template and components, and locks every
manifest and included source path with a deterministic SHA-256 digest. Package
loading rejects unknown or unsorted identities, source-tree changes, local or
credentialed framework locations, mismatched versions, and undeclared
component manifests before planning output. Component manifests cannot define
execution hooks.

The reusable renderer exposes typed request, plan, publication-result, and
error-category contracts. It resolves requirements and conflicts, substitutes
declared variables, detects output collisions, rejects symbolic links and path
traversal, constructs the entire output plan before writing, and atomically
publishes into a previously absent destination. It has no network, process, or
repository-event dependency and does not execute component scripts.

Normal renders retain pinned release-source dependencies. Repository
validation selects a separate adapter that rewrites only a disposable render
to consume a staged, credential-free view of the current framework source.
The generated-application gate first invokes the public CLI for SQLite and
PostgreSQL. Its staging adapter verifies the CLI output paths and bytes against
the canonical request, then patches declared dependencies in a separate copy.
An in-tree framework copy is excluded from automatic Cargo workspace membership
so application checks cannot enable framework/module defaults accidentally.
Native tests, hydration, release builds, upgrades, and production-container
checks consume these verified copies; the original CLI outputs remain unchanged.
The normal renderer command does not expose this adapter's local-source
options. Machine-local maintainer paths must never appear in canonical
template files, normal output, or user-facing validation diagnostics.
The package owns the reserved framework repository and version variables, so a
normal render cannot replace its compatible release source through a variable
override. Both normal rendering and the repository-validation adapter load and
verify this same canonical package contract.

Every render includes schema-versioned `hegira.toml`. It records the
application identifier, HTTPS framework repository and stable SemVer tag,
resolved component set, and selected database and client adapters. The parser
rejects unknown fields, unsupported values, invalid component combinations,
credentials, local framework paths, and mismatches between the recorded and
actually rendered component sets. Deterministic serialization makes the file a
future CLI and upgrade-tool input; it is not a runtime configuration or secret
store.

Use these focused gates:

```sh
sh scripts/architecture-boundaries.sh
sh scripts/framework-check.sh
sh scripts/official-modules-check.sh
sh scripts/layered-template-check.sh
sh scripts/cli-check.sh
sh scripts/generated-application-check.sh
```

The generated-application gate is the release integration unit. It verifies
fresh SQLite and PostgreSQL schemas, the supported v0.2.0 database upgrade,
provider feature sets, the release build, the production image, readiness,
hydration assets, security headers, and unauthenticated Bearer behavior using
only disposable output, containers, and databases.

## Request Boundaries

Browser traffic uses a BFF session model:

```text
Browser -> HttpOnly cookie -> Leptos server function -> application service
```

External clients use Bearer-authenticated Axum routes:

```text
Client -> Bearer token -> Axum controller -> application service
```

Cookie-authenticated unsafe requests require same-origin CSRF validation.
Bearer API mutations do not require browser Origin or Referer headers. Both
route groups share request IDs, security headers, CORS, timeouts, body limits,
compression, tracing, and configured rate limiting, but their authentication
and CSRF policies remain explicit and separate.

The trusted origin comes from validated `application.public_url`. Forwarded
client addresses are used only when the direct TCP peer belongs to an explicit
trusted-proxy CIDR. Application-layer authorization is mandatory regardless of
transport or UI state.

## Configuration And Runtime

The generated application owns `config/{APP_ENV}.yaml` and environment
overrides. Validation runs before telemetry, pools, or external clients:

1. structural invariants;
2. runtime selections against compiled capabilities;
3. production-only security and topology policy.

The application supports SQLite and PostgreSQL, optional Redis, SMTP, S3,
Meilisearch, Prometheus, and OTLP adapters, and `all`, `web`, or `worker`
runtime roles. Provider selection at runtime cannot enable an adapter omitted
at compile time. Split SQLite web and worker replicas are rejected.

## Design Rules

- Keep business rules independent from transports, persistence, and vendors.
- Keep controllers and server functions transport-focused.
- Keep transactions at explicit use-case boundaries.
- Keep module and route composition typed and explicit.
- Keep cookie/BFF and Bearer policies separate.
- Validate configuration before external dependency initialization.
- Keep historical migration identities immutable.
- Treat generated applications as owners of application code and operations.
- Do not add compatibility packages or a repository-owned deployable host.
