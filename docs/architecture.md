# Architecture

Hegira is a production-oriented, opinionated full-stack application framework.
It uses an ABP-inspired layered design adapted to Rust, Axum, Leptos, SQLx,
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

The framework is the reusable system, an official module is a layered capability
such as Identity, and an application template is source consumed during generation.
A generated application is the independent, application-owned result, not the
framework repository itself. These terms describe ownership, not a promise of
automatic module discovery, application upgrades, or registry distribution.

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
│   ├── application_mutator/ existing-application change-plan core
│   ├── hegira_cli/          source-runnable CLI command shell
│   ├── resource_generator/  layered resource generation and composition core
│   └── template_renderer/   render core and repository-validation adapter
├── docs/                    current technical and maintainer documentation
├── scripts/                 validation and release helpers
├── Cargo.toml               virtual framework workspace manifest
└── Cargo.lock               locked framework dependency graph
```

`templates/applications/layered/` is deliberately excluded from the root Cargo
workspace. Its canonical `Cargo.lock` records the registry checksums and exact
framework git revision verified for the package release. Every normal render
receives those bytes unchanged. The generated application then owns its server,
web client, DDD layers, configuration, migrations, deployment files, dependency
lock, and future product changes. The framework repository does not own an
application runtime configuration or production image. Application owners may
update dependencies intentionally; generation never performs an implicit
dependency upgrade.

Template changes affect subsequent generation, not existing applications.
Generated files are application-owned source, not a synchronized view of the
template. Official Identity implementations stay under `modules/identity/` in
the framework source and are consumed through pinned Cargo dependencies; they
are not an application-local module fork. Product rules belong in the generated
DDD layers, while server and web composition explicitly select adapters.
Changing an existing application's framework version, module composition, or
database requires coordinated source, dependency, configuration, and migration
review; the current CLI does not perform those changes.

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
| `application_mutator` | None |
| `hegira_cli` | `application_manifest`, `application_mutator`, `resource_generator`, `template_renderer` |
| `resource_generator` | `application_manifest`, `application_mutator` |
| `template_renderer` | `application_manifest` |

Normal, optional, development, and build dependencies use the same ownership
checks. The retired package names `hegira`, `domain_shared`, `domain`,
`application_contracts`, `application`, `infrastructure`, `presentation`,
`web`, and `db_migrator` are reserved by policy and cannot be reintroduced as
framework compatibility surfaces or consumed by generated applications.

## Framework Packages

| Package | Responsibility |
|---|---|
| `application_manifest` | Versioned, validated, deterministic `hegira.toml` generation-state and mutation-compatibility contract |
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

## Layered resource naming and ownership

`tools/resource_generator` owns the typed naming boundary shared by layered
resource emitters. A resource is supplied as an ASCII Rust type identity. Its
plural type defaults to the explicit and deterministic suffix `s`; irregular
forms require an explicit plural identity rather than language inference. The
validated result provides the Rust module, plural route segment, SQL table,
permission prefix, and application-relative source path for each supported
layer. Acronym boundaries have deterministic snake-case and kebab-case forms.

Resource artifacts are assigned once to Domain (`app_domain`), Application
Contracts (`app_application_contracts`), Application (`app_application`),
Infrastructure (`app_infrastructure`), Presentation (`app_presentation`), or
Web (`app_web`). Those application-owned package names and roots match the
enforced canonical generated-application graph and contain no Hegira branding.
Reserved canonical application identities and collisions with the application,
official modules, or observed application source fail before a change plan is
constructed. Inputs cannot supply source fragments, routes, SQL, or filesystem
paths; every derived artifact path is validated by the application-mutation
contract.

The same package owns the immutable typed resource specification. Raw field
input is restricted to lowercase ASCII snake_case,
an explicit nullable flag, and the closed scalar set `string`, `bool`, `i64`,
`uuid`, and `datetime`; arbitrary Rust and SQL types are not accepted. Every
resource receives a required, non-null UUID `id`, so user fields cannot redefine
the identifier. At least one non-identifier field is required, field names are
sorted canonically, and duplicates plus Rust, SQL, and generator-reserved names
fail before planning. The selected SQLite or PostgreSQL adapter and the Leptos
client are resolved from the validated application manifest rather than caller
defaults. A versioned, deterministic summary exposes only the validated model.
This contract performs no schema introspection or database access.

The inward-layer emitter turns that specification into one deterministic
change plan for Domain, Application Contracts, and Application source. Domain
owns the UUID identifier value and entity skeleton. Application Contracts owns
serializable commands, queries, responses, permission identifiers, and the
service boundary. Application owns the repository, authorization, and
identifier-generation ports plus the service implementation. Every generated
use case requires authorization before repository access, including create,
update, and delete. The generated source deliberately contains no Axum,
Leptos, SQLx, or vendor types and makes no business-invariant or aggregate
assumptions. Developers extend the generated application-owned domain source
when product rules require them.

The same plan registers each module only through the controlled integration
block in its layer root. New source uses absent-file preconditions and root
edits use observed-content digests, so an existing registration, file,
concurrent change, or unsupported root fails instead of being overwritten.
Infrastructure, HTTP presentation, and web source remain outside this inward
plan.

A separate persistence emitter consumes the same validated specification and
produces only the selected database adapter. It creates an application-owned
SQLx repository that implements the Application port, a UUID identifier
adapter, and an explicit typed factory that composes those adapters with an
application-selected authorization implementation. The provider-specific
module is registered through the controlled Infrastructure root; no service
locator, reflection, database inspection, or inward SQLx dependency is used.
Runtime query values are bound parameters, while table and column identifiers
come only from validated generator identities. List consistency is scoped to
the repository operation, and no transaction is extended to an HTTP request.

The persistence plan also creates the selected provider's forward-only table
migration in the application-owned migration history. SQLite and PostgreSQL
types and placeholder syntax are emitted independently; generating one does
not claim or create support for the other. Historical migrations remain
immutable.

The HTTP emitter adds a transport-focused Axum adapter for the same resource.
Its handlers perform only Bearer extraction, request and path mapping,
application-service delegation, response serialization, and stable HTTP error
mapping. The concrete resource service is composed in application-owned
Infrastructure and registered through explicit keyed service and server
integration blocks; there is no route discovery, reflection, or service
locator. Generated Bearer routes are merged separately from
cookie-authenticated Leptos BFF routes, so they do not inherit browser CSRF
policy. Authorization still runs inside every generated application use case
before repository access.

When OpenAPI is compiled, resource paths and schemas contribute a typed
document which the application server explicitly merges with the Identity API
document. The same managed-entry contract rejects missing, duplicate,
reordered, or malformed service, route, and document registrations before a
change plan is produced. Provider migrations seed generated permission
identifiers for the canonical administrator role so the composed authorization
boundary is usable after migration.

The Leptos emitter creates an application-owned list and create/edit surface
for the resource, with typed field conversion, mutation feedback, and an
explicit delete confirmation. Its server functions are transport adapters:
they recover the secure browser session and delegate to the generated
application service through a typed context composed by the application host.
Application-owned localization keys, native and split routes, navigation, and
icons are registered through keyed managed blocks. Route, localization, and
registration conflicts fail before publication. Permission gates improve the
presentation experience but do not replace authorization in the generated
application service.

The package also plans application-owned migration scaffolds independently of
the general resource specification. It resolves the selected SQLite or
PostgreSQL adapter from the validated application manifest, observes only that
provider's canonical migration filenames, and derives the next append-only
numeric identity. Existing migration contents are neither read into plan output
nor edited. Duplicate identities, malformed or symlinked histories, and stale
publication preconditions are explicit conflicts.

Each plan creates one provider-labelled SQL scaffold and creates or advances
`crates/infrastructure/migrations/.hegira-generator.toml`. This private,
application-owned coordination record contains the selected adapter and next
version only. Publishing both files through `application_mutator` serializes
Hegira generator operations and prevents concurrent plans from silently
claiming the same version. Planning does not connect to a database, execute or
revert migrations, accept arbitrary SQL input, or infer runtime configuration.

## Existing-application change planning

`tools/application_mutator` owns the internal typed contract for coordinated
changes to an existing validated application. New-file operations require the
target path to be absent. Structured edits carry the SHA-256 digest of the
observed content as an explicit publication precondition. Every result also
carries its digest. Application-relative canonical paths and a sorted complete
plan make validation and summaries deterministic; duplicate paths and mixed
operations against one path are typed conflicts.

Plan summaries expose only relative paths, operation identities, preconditions,
and digests. They never expose source or resulting file content. The crate does
not execute generated code or provide the repository-validation dependency
rewriting available to maintainer tooling.

Structured editors operate only on declared integration points. Canonical Rust
layer roots contain an explicit generated-module block; registrations inside
that block must be unique and deterministically ordered, while matching
registrations outside it are treated as conflicts rather than adopted. TOML
editors target declared tables, arrays, and string keys through a lossless
document model so unrelated keys, ordering, and comments remain owned by the
application. Repeated edits return an explicit already-present result. Missing,
duplicated, reordered, or type-incompatible integration points fail with typed
diagnostics before a plan is produced.

Failure-safe publication is a separate stage over the validated plan. The
publisher opens the real application root and every change parent without
following symlinks, creates an exclusive `.hegira-mutation.lock` recovery
marker, and stages private files on each target filesystem. Required exclusive
rename and atomic-exchange behavior is probed before application files change.
All target identities and absent or digest preconditions are then rechecked
immediately before publication. Edits exchange the staged result with the
original so the original remains available for rollback; creates use an
exclusive no-replace rename. A recoverable failure rolls published changes back
in reverse order after verifying that neither the generated result nor its
rollback source changed concurrently.

Successful publication durably removes transaction files and then the marker.
A crash, changed published file, failed rollback, or uncertain cleanup leaves
the marker in place and blocks subsequent mutations for explicit manual
recovery. Cleanup and rollback remain anchored to the opened application-owned
directories, including when a path ancestor is replaced. Platforms or
filesystems without the required safety semantics fail before application files
are modified; the contract does not claim universal filesystem transactions.

## Source-runnable CLI

`tools/hegira_cli` owns the `hegira` binary command shell. It defines top-level
help, version reporting, guided and deterministic non-interactive application
creation, read-only application inspection, concise diagnostics, and stable
process outcomes without reading a user home directory or global configuration.
It delegates new-application component planning and atomic publication to
`template_renderer`, and existing-application publication to
`application_mutator`. Application migration planning is delegated to
`resource_generator`; repository-local dependency rewrites remain unavailable
to the public command. Help, version information, successful creation
instructions, inspection results, and mutation plans are written to standard
output; usage and failure diagnostics are written to standard error.

The process outcomes are `0` for success, `1` for an internal error, `2` for
invalid usage, `3` for validation failure, and `4` for a destination or state
conflict. `hegira new <name> --destination <path>` renders the canonical
layered application with SQLite, Leptos, and Identity defaults. The database,
client, and component selections can also be stated explicitly. Generation
writes the destination atomically and never executes generated or external
commands.

The CLI library provides common `--dry-run` and `--json` options for mutation
commands. A command constructs and validates one typed `ChangePlan`, then hands
that same value to the shared execution path for either preview or publication;
dry-run does not open or write the application root. Human output lists every
ordered relative path and operation. Machine output is deterministic,
explicitly versioned, and includes the content-redacted plan summary rather
than file bodies, runtime configuration, credentials, environment values, or
machine-local framework paths. Empty plans are successful no-ops, planning and
state conflicts retain the conflict process outcome, and invalid plans retain a
validation outcome. `hegira generate resource <name> --field <name:type>` uses
this contract to compose the Domain, Application Contracts, Application,
selected SQLx, Axum/OpenAPI, and selected Leptos emitter plans into one atomic
change. Chained edits preserve the first observed precondition and final
content without publishing an intermediate state. `hegira generate migration
<identity>` uses the same contract to preview or publish the provider-specific
migration plan selected by the application manifest. Both commands create
source only and do not execute migrations, generated code, formatters, or
builds.

The CLI library also owns a read-only existing-application context resolver.
`hegira inspect` uses it to provide concise human-readable application identity,
framework, selection, and mutation-compatibility information. `--json` exposes
the same state through an explicitly versioned deterministic output contract,
and `--application-root <path>` selects a root for automation. Otherwise the
resolver discovers `hegira.toml` from a real working directory and its real
ancestors. Discovery rejects multiple candidate manifests as ambiguous rather
than selecting one implicitly. Directory-relative, no-follow reads anchor the
manifest and the required application-owned `apps/`, `crates/`, and `config/`
roots to the opened application root. The resolver returns the typed manifest
when the current parser supports it and always returns the mutation
compatibility assessment when one can be determined. Inspection reads no
runtime configuration, environment value, user-home state, or secret, and it
performs no writes.

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
| `app_web` | application contracts, `identity_leptos`, and `leptos_support` |
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
actually rendered component sets. Deterministic serialization records the
validated generation contract; it is not a runtime configuration or secret
store. Editing it does not trigger regeneration or upgrades. The field-level
contract is documented in
[Getting started](getting-started.md#generated-ownership-and-hegiratoml).

The application-manifest package also exposes a pure, fail-closed mutation
compatibility assessment. A manifest is compatible only when its schema,
framework repository and exact release, component set, and single selected
database/client adapters match the caller's supported policy. A valid manifest
from another release or with an unknown supported-shape capability is reported
as unsupported; a current-shape manifest that conflicts with canonical
selection or framework identity is reported as incompatible with the exact
field identified. Normal parsing remains separate, so older valid manifests
can still be read without becoming writable. This assessment performs no file
write, network access, source mutation, dependency change, or upgrade.

Use these focused gates:

```sh
sh scripts/architecture-boundaries.sh
sh scripts/framework-check.sh
sh scripts/official-modules-check.sh
sh scripts/layered-template-check.sh
sh scripts/cli-check.sh # CLI and existing-application mutation tooling
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
