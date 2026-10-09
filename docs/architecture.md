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
automatic module discovery, unattended upgrades, or registry distribution.

```text
.
├── crates/                  application-independent framework packages
├── modules/
│   └── identity/            official layered Identity module and adapters
├── templates/
│   ├── package.toml         versioned canonical package contract
│   ├── applications/
│   │   ├── layered/         recommended Identity-enabled application source
│   │   └── layered-minimal/ explicit module-free outward-layer variant
│   ├── components/          typed application-component manifests
│   └── upgrades/            authenticated data-only direct release edges
├── tools/
│   ├── application_mutator/ existing-application change-plan core
│   ├── hegira_cli/          source-runnable CLI command shell
│   ├── resource_generator/  layered resource generation and composition core
│   ├── template_renderer/   render core and repository-validation adapter
│   └── upgrade_test_support/ authenticated released-application test data
├── test-fixtures/
│   └── application-baselines/ immutable content-addressed upgrade baselines
├── docs/                    current technical and maintainer documentation
├── scripts/                 validation and release helpers
├── Cargo.toml               virtual framework workspace manifest
└── Cargo.lock               locked framework dependency graph
```

The application sources under `templates/applications/` are deliberately
excluded from the root Cargo workspace. Their canonical `Cargo.lock` files
record the registry checksums and exact framework git revision verified for the
package release. Every normal render receives the selected composition's bytes
unchanged. Both supported compositions resolve the common `layered-base` DDD
source. The recommended default adds `layered-leptos-identity`; the explicit
minimal selection adds `layered-leptos-minimal` and records no official module,
authentication capability, or authorization capability. Both retain a Leptos
client, server host, selected SQLx provider, configuration, and deployment
source.

The generated application then owns its server, web client, DDD layers,
configuration, migrations, deployment files, dependency lock, and future
product changes. The framework repository does not own an application runtime
configuration or production image. Application owners may update dependencies
intentionally; generation never performs an implicit dependency upgrade.

Template changes affect subsequent generation, not existing applications.
Generated files are application-owned source, not a synchronized view of the
template. Official Identity implementations stay under `modules/identity/` in
the framework source and are consumed through pinned Cargo dependencies; they
are not an application-local module fork. Product rules belong in the generated
DDD layers, while server and web composition explicitly select adapters.
Changing an existing application's framework version, module composition, or
database requires coordinated source, dependency, configuration, and migration
review. The CLI supports bundled additive component installation and the exact
authenticated direct framework transition documented below. It does not switch
database/client adapters or merge arbitrary application customizations.

## Components, Modules, And Composition

Hegira keeps generation units separate from runtime ownership:

| Term | Current contract |
|---|---|
| Component package | The release-aligned, digest-verified catalog described by `templates/package.toml` |
| Component | A typed graph node that either contributes source during initial rendering or describes one additive installation unit |
| Official module | Versioned layered framework packages under `modules/` that own a reusable capability and its adapters |
| Capability | A machine-checked contract, such as `authentication` or `authorization`, derived from the resolved module graph |
| Composition | The exact package, component, module, capability, database, and client state recorded in `hegira.toml` |

A component is not a second name for an official module. The bundled
`identity` component is inert installation metadata that owns the official
Identity module contribution. Installing it adds release-pinned dependencies
on the Identity packages and edits declared application-owned integration
points; it does not copy `modules/identity/` into the application. For this
reason the public mutation command is `hegira component add identity`; there is
no generic module loader, runtime plugin mechanism, or module-management
command.

The recommended `identity` composition remains the creation default. It
renders `layered-base` with `layered-leptos-identity` and records the official
Identity module plus its authentication and authorization capabilities. The
explicit `minimal` composition renders `layered-base` with
`layered-leptos-minimal`; it retains the layered application, selected SQLx
provider, Axum host, Leptos client, configuration, and deployment boundaries,
but records no official module or authentication and authorization capability.
It does not substitute anonymous or allow-all authorization.

`hegira inspect` is the composition-status command: it resolves the recorded
state against the bundled authenticated graph without modifying the
application. `hegira doctor` additionally checks local prerequisites, recovery
state, and bounded application integration points. Mutating commands require
an exact compatible framework and package release, a supported composition,
and the recorded database and client. Protected resource generation also
requires authentication and authorization and therefore fails closed for a
minimal application until Identity has been installed.

Installation is additive only. Dry-run and apply consume the same typed,
content-redacted plan; apply publishes manifest, dependency, configuration,
migration-source, server, HTTP, OpenAPI, and Leptos contributions through the
application mutation transaction. The CLI currently provides no removal,
unattended upgrades, remote package source, or database migration execution or
rollback. Explicit direct application upgrades are a separate command contract;
publication rollback on recoverable source failures is not a public downgrade
or database rollback command. The application owner must review configuration,
regenerate `Cargo.lock`, apply the selected provider migrations, and validate
the application after installation.

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
| `application_mutator` | `application_manifest` |
| `hegira_cli` | `application_manifest`, `application_mutator`, `resource_generator`, `template_renderer`; test-only `upgrade_test_support` |
| `resource_generator` | `application_manifest`, `application_mutator` |
| `template_renderer` | `application_manifest`, `application_mutator`; test-only `resource_generator`, `upgrade_test_support` |
| `upgrade_test_support` | None |

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

The Identity-added minimal composition uses its smaller Leptos shell instead:
the generator registers a native route and a permission-gated dashboard link,
and emits local English labels and visible mutation feedback in the resource
page. It does not assume the default shell's sidebar or localization files.
The minimal host composes the same resource service through its Identity
runtime and merges the resource OpenAPI document into the Identity document
when OpenAPI is enabled. Both compositions retain application-layer
authorization and keep Bearer routes separate from browser cookie policy.

The package also plans application-owned migration scaffolds independently of
the general resource specification. It resolves the selected SQLite or
PostgreSQL adapter from the validated application manifest, observes only that
provider's canonical migration filenames, and derives the next append-only
numeric identity. Generated application-owned migrations begin at version
`1000000`, keeping their identities separate from the lower range used by
official module migration history. Existing migration contents are neither read
into plan output nor edited. Duplicate identities, malformed or symlinked
histories, and stale publication preconditions are explicit conflicts.

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
observed content as an explicit publication precondition. Managed-file
retirement additionally requires a matching `managed-integration` ownership
claim from the application manifest and an exact observed digest; application-
owned, generated-once, and immutable-history paths cannot be retired. Present
results carry their digest, while retirement has an explicit absent result.
Application-relative canonical paths and a sorted complete plan make validation
and summaries deterministic; duplicate paths and incompatible operations
against one path are typed conflicts.

Plan summaries expose only relative paths, operation and managed-integration
identities, preconditions, and digests. They never expose source, resulting, or
retired file content. Publication moves a retired file to a private transaction
name atomically, preserving its bytes and metadata until the transaction is
durable; a later failure restores it through the same anchored no-follow
boundary. The crate does not execute generated code or provide the repository-
validation dependency rewriting available to maintainer tooling.

Additive component installation has a separate typed plan over the same change
contract. A request names one not-yet-installed component and supplies only
application-owned artifacts or digest-preconditioned integrations. The closed
owner set covers the workspace and application manifests, configuration, both
application hosts, and each canonical layered package; a contribution outside
its declared owner is rejected. Component artifacts always become absent-file
creations. Integrations always remain digest-preconditioned edits, and existing
application migration files cannot be edited. Empty, duplicate, already
installed, invalidly named, cross-owner, and path-conflicting requests fail
before publication. The versioned installation summary adds component and
owner identities to the underlying content-redacted operation metadata.

Structured editors operate only on declared integration points. Canonical Rust
layer roots contain an explicit generated-module block; registrations inside
that block must be unique and deterministically ordered, while matching
registrations outside it are treated as conflicts rather than adopted. TOML
editors target declared tables, arrays, and string keys through a lossless
document model so unrelated keys, ordering, and comments remain owned by the
application. Repeated edits return an explicit already-present result. Missing,
duplicated, reordered, or type-incompatible integration points fail with typed
diagnostics before a plan is produced.

Component edit operations further close the available destinations to typed
layer module roots, package manifests, application capability configuration,
server and Leptos contribution blocks, Infrastructure configuration fields,
and provider-specific module migration lists. Cargo dependency declarations
can only consume an existing workspace dependency or declare a credential-free
HTTPS framework repository at a stable SemVer tag in the root workspace;
package paths, branches, revisions, registries, and command-shaped sources are
not represented. Feature entries and managed Rust entries use the same
lossless, conflict-aware editors. Sequential operations against one file must
form an unbroken result-digest chain and collapse into one owner-preserving
edit before entering the installation plan. The canonical application exposes
managed PostgreSQL and SQLite module-migration blocks and an Infrastructure
module-configuration field block; these markers do not execute migrations or
change runtime configuration by themselves.

Failure-safe publication is a separate stage over the validated plan. The
publisher opens the real application root and every change parent without
following symlinks, creates an exclusive `.hegira-mutation.lock` recovery
marker, and stages private files on each target filesystem. Staged result
files remain private until their atomic rename; their final permissions are
applied and verified on the published file. Required exclusive
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
creation, read-only application inspection and direct-upgrade readiness,
content-redacted upgrade preview and explicit atomic apply, concise diagnostics,
and stable process outcomes without reading a user home directory or global
configuration.
It delegates new-application component planning and atomic publication to
`template_renderer`, and existing-application publication to
`application_mutator`. Application migration planning is delegated to
`resource_generator`; repository-local dependency rewrites remain unavailable
to the public command. Help, version information, successful creation
instructions, inspection results, and mutation plans are written to standard
output; usage and failure diagnostics are written to standard error, except
upgrade assessments and JSON check/test reports, whose typed unsuccessful
outcomes also remain on stdout.

The process outcomes are `0` for success, `1` for an internal error, `2` for
invalid usage, `3` for validation failure, and `4` for a destination or state
conflict. `hegira new <name> --destination <path>` renders the canonical
layered application with SQLite, Leptos, and Identity defaults. The database,
client, and component selections can also be stated explicitly. Generation
maps the component selection to package roots and hands those roots to the
renderer. The renderer resolves them through the authenticated package graph;
the CLI does not maintain a second file, module, or capability composition.
Generation writes the destination atomically and never executes generated or
external commands.

The CLI library provides common `--dry-run` and `--json` options for mutation
commands. A command constructs and validates one typed `ChangePlan`, then hands
that same value to the shared execution path for either preview or publication;
dry-run does not open or write the application root. Human output lists every
ordered relative path and operation. Machine output is deterministic,
explicitly versioned, and includes the content-redacted plan summary rather
than file bodies, runtime configuration, credentials, environment values, or
machine-local framework paths. Empty plans are successful no-ops, planning and
state conflicts retain the conflict process outcome, and invalid plans retain a
validation outcome. `hegira component add <component>` first validates the
application mutation contract, authenticates the bundled package, and resolves
the requested target graph. It rejects installed, unknown, conflicting, or
non-additive components before publication and never executes component code or
accepts a remote package coordinate. A resolved additive unit is converted to
one `ComponentInstallationPlan` and handed to the same mutation executor.
The bundled Identity unit composes into a compatible minimal Leptos application
for the selected SQLite or PostgreSQL provider. It updates the application
manifest and source in one preconditioned publication, with configuration
preflight before database initialization. The command does not connect to a
database, run migrations, regenerate the lockfile, or initialize providers that
the minimal host has not composed; the maintainer performs those post-install
steps explicitly. `hegira generate resource <name> --field <name:type>` uses
the recorded composition state to require authentication and authorization
before it reads integration sources or plans any files. Missing capabilities
fail closed with a stable human or JSON diagnostic; the generator specification
enforces the same requirement for callers outside the CLI. The command uses
the mutation contract to compose the Domain, Application Contracts, Application,
selected SQLx, Axum/OpenAPI, and selected Leptos emitter plans into one atomic
change. Chained edits preserve the first observed precondition and final
content without publishing an intermediate state. `hegira generate migration
<identity>` uses the same contract to preview or publish the provider-specific
migration plan selected by the application manifest. Both commands create
source only and do not execute migrations, generated code, formatters, or
builds.

The CLI library also owns a read-only existing-application context resolver.
`hegira inspect` uses it to provide concise human-readable application identity,
framework, adapters, mutation compatibility, and component-composition status.
For a current manifest, inspection resolves its recorded package, components,
modules, and capabilities against the bundled canonical graph. Compatible
state includes exact versions; unresolved state includes every sorted typed
graph diagnostic without blocking inspection. Legacy or unparsed manifests
report composition as unavailable rather than inventing state. `--json`
exposes the same information through output schema 2, and
`--application-root <path>` selects a root for automation. Otherwise the
resolver discovers `hegira.toml` from a real working directory and its real
ancestors. Discovery rejects multiple candidate manifests as ambiguous rather
than selecting one implicitly. Directory-relative, no-follow reads anchor the
manifest and the required application-owned `apps/`, `crates/`, and `config/`
roots to the opened application root. The resolver returns the typed manifest
when the current parser supports it and always returns the mutation
compatibility assessment when one can be determined. Inspection reads no
runtime configuration, environment value, user-home state, application source,
or secret, exposes no machine-local framework path, and performs no writes.

The `doctor` command reuses that resolver and the bundled composition graph. It
checks the recovery marker and bounded, no-follow application integration
sources without reading runtime configuration or connecting to a provider.
Its local tool checks and selected-provider requirements are diagnostics, not
startup preflight or application mutation.

`doctor --operation` additionally obtains the same privately typed operation
plan as execution. The closed selections cover development, check, test,
release build, and the still non-executable database status/migrate contracts.
It diagnoses required toolchain/lock metadata, selected provider/profile,
frontend assets and installed receipts, pending recovery, concurrent Hegira
operations, and reusable release-output ownership. The executor's read-only
frontend source check and release ownership check are shared rather than
duplicated. Diagnosis never creates a tool shim, output directory, or marker.
Operation-specific readiness currently supports Linux; unsupported hosts fail
closed without a probe. The default doctor contract is unchanged.

Without `--probe-tools`, operation doctor starts no process. Explicit native
tool selection enables only closed version arguments and the Rust WASM target
library query. Probes run from `/`, not the application, with a cleared
environment except trusted tool discovery and Rustup home/toolchain settings;
auto-installation is disabled. They reuse the native executable anchors,
bounded output/deadlines, signals, and owned child-group cleanup. No Cargo
metadata, compilation, tests, npm lifecycle, Tailwind JavaScript, database,
or application hook is invoked. A failing bounded probe prevents later probes.
The selected tools remain explicitly trusted code, not a sandbox guarantee.

The existing schema-1 doctor report retains its default shape; operation mode
adds a typed `operation` identity and deterministic prerequisite checks.
Missing/mismatched or unprobed tools are warnings, while invalid composition,
recovery, unsafe tool selections/output ownership, or interrupted/over-limit
probes are failures. Frontend execution and provider connectivity remain
unprobed. A diagnostic report cannot grant consent or certify runtime settings,
compiled authorization, or executable readiness. See
[Operation diagnostics](getting-started.md#diagnose-operation-prerequisites).

### Application operation planning library

`hegira_cli::operations::plan_application_operation` accepts the source
repository root and an `OperationRequest` containing the existing
`ApplicationContextRequest` and a closed `OperationIntent`. It reuses the
no-follow context resolver, current release compatibility policy, and
authenticated bundled composition graph. Missing, ambiguous, unsafe, legacy,
unsupported, or graph-inconsistent application state fails before a plan is
returned. It reads no application source, runtime profile, environment value,
or credential, and does not execute a tool or connect to a provider.

The privately constructed `OperationPlan` retains an open root directory and
the observed manifest privately for later precondition checks, but excludes
machine-local paths and source content from Debug, human review text, and
the deterministic schema-1 `OperationPlanSummary`. Summaries record exact
component/module versions and capabilities, selected adapters, potential
execution effects, ordered steps, unprobed prerequisites, and execution
requirements. Tool steps use a closed program identity and separate arguments,
not shell scripts. Native check/test steps select only the recorded database
feature, disable defaults, and preserve Cargo locks; a separate hydration check
selects only `hydrate`. Development and release-bundle plans delegate to the
existing locked Cargo Leptos contract. Required frontend dependencies,
lockfile-selected Tailwind/wasm-bindgen tools, and toolchain versions are
declarations, not installed or probed by the planner.

Database intents carry a typed status/forward-migrate request and an explicit
baseline profile. They require an application-owned operation entry point;
the planner declares that requirement without invoking the application-owned
`app_database` binary or inventing a CLI SQL engine. Baseline profile/provider mismatches
are rejected: `sqlite` selects SQLite; `development` and `production` select
PostgreSQL; `test` selects PostgreSQL for the default Identity template and
SQLite for minimal or Identity-added applications. User runtime overrides are
not read or validated by planning. Production migration plans additionally
declare explicit approval as an execution requirement.

Planning is read-only, but planned check/test/build execution would still run
trusted application and toolchain code; development execution can initialize
the application's configured dependencies. A summary is not execution
authority, a readiness certificate, a cached publication token, or a sandbox.
Public `dev`, `check`, `test`, `build --release`, `db status`, and `db migrate`
commands reuse this planner. Database commands require an explicit profile.
Development plans
explicitly select the recorded backend and development profile, localhost site
and server address, reload port, canonical CSS input, and standard Cargo
server-build command without reading runtime
configuration or certifying the intended database target.
Static schema-1 errors preserve validation/conflict/internal
outcomes without echoing input, parser excerpts, or source paths.

### Trusted application process execution library

`hegira_cli::operations::execution::execute_application_operation` is separate
from inspection and planning. It requires a privately constructed plan,
`ExecutionConsent::ExecuteTrustedApplicationAndToolchain`, an explicitly
resolved `TrustedToolchain`, an `ExecutionControl`, and a child-output policy.
Public `dev`, `check`, `test`, and `build --release` commands delegate to this library.
Database steps fail before spawning through this general executor. The separate
`execute_database_operation` API additionally requires `DatabaseExecutionApproval`;
production forward migration requires its independent `ProductionMigration`
variant, while other database requests require `Ordinary`. A preview, generic
execution consent, or source upgrade cannot substitute for that approval.
It validates the registered `app_database` binary, database-operations feature,
Infrastructure source, and selected real profile file before spawning locked
Cargo with only the selected database feature. It explicitly selects `APP_ENV`
and the manifest's backend; inherited URL overrides remain the owner's responsibility.
Database configuration and SQL behavior remain in the trusted application.
Check/test plans execute native provider validation and a hydration check.
Development plans require explicit lock-matched wasm-bindgen selection plus a
verified frontend/toolchain preflight before foreground Cargo Leptos watch/serve.
Release builds additionally require explicit native Binaryen `wasm-opt` 123
selection, canonical host/profile metadata, and a separately claimed output
root before compilation. Directory presence alone never
establishes tool readiness; no missing-tool installation is delegated to Leptos.

Execution currently supports Linux with accessible `/proc/self/fd`; other
platforms fail closed. The application root and manifest are checked again,
and the current release and authenticated bundled composition must still match
the reviewed plan. A separate open description acquires a nonblocking advisory
lock on the application directory, so concurrent executors fail without creating
or deleting lock files. Any existing `.hegira-mutation.lock`, including a
directory or dangling symlink, blocks execution without being read or removed.
This coordination applies to Hegira executors, not arbitrary editors or source
mutation publishers. Owners must not publish mutations while operations run.

The primary Cargo executable is selected by an explicit absolute path, not
ambient `PATH`. Normal rustup proxy symlinks are resolved once to a native ELF
file; scripts and set-id executables are rejected. Its open file descriptor and
the root's open directory descriptor provide the executable and working
directory through verified `/proc/self/fd` paths. Root, manifest, executable,
and auxiliary-directory identity changes fail preconditions before each step.
Auxiliary tools use only explicitly trusted, absolute directories outside the
application. Empty and application-relative search entries are rejected; the
primary executable must also be outside the application. The executor never
interpolates a shell command or accepts arbitrary operation arguments.

Basic native prerequisite paths (`Cargo.toml`, `Cargo.lock`, and
`rust-toolchain.toml`) must be real regular files reached without following
symlinks. This is not a tool-version or
provider-readiness certificate: missing WASM targets, tool versions, dependencies,
or runtime services can still cause child failure. The executor installs nothing
and sets `RUSTUP_AUTO_INSTALL=0`, disabling rustup's automatic toolchain
installation. It removes an inherited `RUSTUP_TOOLCHAIN` override so the
application's toolchain file remains effective; ordered Cargo arguments remain
locked. Cargo may still download locked dependencies when executing an explicitly
approved build. Application-controlled Cargo configuration, build scripts,
tests, and tools execute with the owner's privileges.
Trusted source must remain trusted: descriptor anchoring prevents pathname
substitution, not malicious in-place writes to approved tool files or arbitrary
replacement of tools that Cargo itself invokes.

Leptos development and release builds verify the pinned Rust version and WASM core target,
Cargo Leptos 0.3.7, Node.js 22+, the exact Cargo-lock wasm-bindgen version, npm
installed receipts, and the lock-selected Tailwind entry point/version. Probes
have bounded output and deadlines and use owned process-group cleanup.
Consistency checks do not authenticate every installed dependency's contents;
the installed application frontend remains trusted code. Required metadata and
frontend sources are rechecked before startup through no-follow reads. The
canonical Tailwind input and disabled native/hydration defaults are required;
customized tool composition fails closed rather than being reinterpreted.

A private 0700 temporary directory places only the reviewed Cargo proxy,
Cargo Leptos, Rust, Node, wasm-bindgen, Tailwind, and (for release) wasm-opt names ahead of trusted
external auxiliary directories. The general application `node_modules/.bin`
is never trusted. The separately selected wasm-bindgen executable may reside
under the application's explicitly prepared tool directory. The native proxy
uses the source-built Hegira executable and the approved Cargo inode, accepts
only the required Cargo operations, and adds `--locked` when absent; this
includes Cargo Leptos 0.3.7's otherwise unlocked initial metadata call. It
exec-replaces itself, preserving the owned process group. Hegira removes its
private tool directory on ordinary completion/error/unwinding, not after
SIGKILL or a crash; it never recursively removes application directories.

Apart from that toolchain policy and the plan's explicit environment overrides,
the child inherits the caller's environment, including runtime overrides and
possibly credentials. Trust approval must cover this environment and Cargo
configuration as well as application and toolchain source. Credentials are not
copied into arguments, plan summaries, execution reports, or framework errors.
For non-database operations, child output is either inherited unchanged or
discarded, not captured in schema-1 framework reports; arbitrary tool/application
output is **not redacted**. Database execution discards stderr and captures at
most 1 MiB of stdout. Only a closed schema-1 success result with matching
operation/provider, known module identities, and ordered migration states is
accepted. Forward-migration success additionally requires present history and
no pending entries. Malformed, oversized, or failed child output produces a
non-success outcome without raw output. Successful execution adds the optional
`database` field to its report; it is a trusted application observation, not an
independent CLI audit of the database. Child stdin is closed. Execution is a trust decision, not an OS
sandbox, network isolation, or a promise of secret-output sanitization.

Each child owns a new process group. Cancellation or termination requested
through `ExecutionControl` sends SIGTERM, allows a 250 ms grace period, then
SIGKILLs remaining group members and waits for the direct child. The leader is
observed without reaping until group cleanup finishes, preventing PID reuse
during cleanup. Normal leader exit also stops leftover group members. The
child guard kills and reaps on errors or unwinding. Callers are responsible for
forwarding their OS signal handling to `ExecutionControl`; the library installs
no global handlers. SIGKILL of the parent, detached descendants that deliberately
leave the group, and uninterruptible kernel waits cannot be handled as a sandbox
guarantee. Orphaned grandchildren are ultimately reaped by the host's init or
subreaper, not by the executor.

Versioned, content-redacted reports distinguish success, child failure, child
signal, cancellation, and termination and record only completed successful
steps. Spawn and lifecycle errors remain non-success diagnostics. A successful
report is produced only after all steps and owned direct-child cleanup finish.
`ExecutionReport::exit()` maps only success to zero; every other outcome maps
to the CLI's existing nonzero internal outcome while retaining its typed status.

### Public application validation commands

`hegira check` and `hegira test` require either `--dry-run` or `--execute`.
Preview constructs the existing privately typed plan without probing tools or
running code. Execution builds that same plan type and requires
`--trust-application`, an absolute `--cargo`, and explicit absolute
`--tool-directory` selections; it does not infer consent from an earlier preview.
The commands accept no arbitrary Cargo arguments or ignored-database-test flag.
The existing steps cover locked native/provider validation followed by hydration
checking for all six supported compositions. These are trusted application
tests/builds, not guaranteed read-only or service-free code execution.

The CLI installs scoped SIGINT/SIGTERM actions only for explicit execution,
forwarding cancellation/termination directly into the executor's atomic control;
it unregisters its actions before returning. The library still installs no
handlers on its own. Human execution inherits raw child output. JSON execution
discards it and emits one schema-1 envelope with mode, the reviewed plan,
optional execution outcome, and static diagnostics. Preview and execution
consume the same plan within one invocation; a serialized preview cannot be
submitted as execution authority. See [Application checks and tests](getting-started.md#check-and-test-an-application)
for arguments, output channels, exit codes, and trust limitations.

### Public application development command

`hegira dev` uses the same explicit preview/execution, consent, JSON envelope,
exit outcomes, scoped signal handling, and recovery/concurrency boundaries as
validation commands. Execution additionally requires `--wasm-bindgen`; preview
does not probe installed tools or start a server. All six compositions select
their recorded provider, `APP_ENV=sqlite` or `development`, `ssr,db-<provider>`
for native code and `hydrate` for the browser. Server/site bind is explicitly
`127.0.0.1:3000`, with reload port `3001`; the canonical CSS path is relative to
the server package's Cargo Leptos metadata. No production-profile/public-bind
flag or arbitrary command passthrough is provided.

After preflight, the existing executor delegates foreground watch/serve to
Cargo Leptos, not a new runtime or daemon. Configured application startup may
connect to providers, create/open a database, migrate, and seed. Review the
development configuration and inherited database URL overrides before explicit
execution: the selected profile does not certify disposable data. Other
inherited settings remain effective; this is trusted execution, not network
isolation. No Docker, deployment, tool installation, source mutation, or
lockfile regeneration is performed by Hegira. See [Application development](getting-started.md#develop-an-application).

### Public application release build command

`hegira build --release` exposes preview or explicit Linux execution through
the same typed plan, trust consent, frontend preflight, locked Cargo proxy,
signal lifecycle, and schema-1 envelope. It delegates native release and
`wasm-release` hydration compilation to Cargo Leptos, selecting only the
recorded provider. Binaryen `wasm-opt` 123 is explicitly selected and probed
before compilation so missing release optimization cannot trigger an automatic
download. Hegira loads no runtime profile and starts neither application nor
provider. Build code and inherited environment remain trusted, not sandboxed.

The release plan records closed application-relative artifact identities and
explicit output overrides. The executor creates only a new
`target/hegira/release-build` root or reuses its exact application-bound ownership
claim. It never adopts an existing unclaimed root. Bounded no-follow metadata
walks reject symlinks, special files, external hard links, and unsupported deep
trees. Internal Cargo hard links are allowed only when every link is observed
inside the claimed root. This is a root-specific claim, not adoption of normal
developer Cargo output or a cache size/cleanup policy. The builder can clear
its owned site subtree; users must not put unrelated data there or forge the
ownership marker to bypass rejection.

Native output, the site, browser WASM, JavaScript, and CSS stay inside the
claimed root. Canonical host/profile metadata and explicit Cargo target,
site, package-name, standard Cargo command, and asset-path overrides constrain
the supported layout; customized cross-target/intermediate paths fail closed.
Frontend sources and locks are rechecked after the builder. Only successful
execution with verified ELF, WASM, JS, and CSS outputs adds verified artifact
locations to the execution report. Preview artifact paths are expectations,
not existing outputs. Failure/cancellation never grants an artifact receipt,
even if older or partial output remains. Trusted builder changes are not
silently reverted. A receipt certifies source-build output only, not secret-free
assets, runtime correctness, tests, container validation, signing, or deployment.
See [Application release builds](getting-started.md#build-an-application-release-bundle).

### Application creation and source publication

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

The same `persistence::migrations::MigrationPlan` exposes typed read-only
`status(&DatabaseConfig)` inspection. It retains each migration's module owner
without changing execution identities, checksums, or order. Provider-specific
inspection uses a dedicated connection and validates observed history against
the composed plan; it never executes migrations or provisions missing state.
SQLite WAL coordination is distinct from database/schema writes. Infrastructure
still owns composition and explicit database-access intent; neither Presentation
nor the Hegira CLI acquires a new database command through this primitive.
See [migration status](operations.md#read-only-migration-status) for provider
limits, typed errors, and the point-in-time observation boundary.

Fresh canonical applications also contain the transport-only `app_database`
binary in `apps/server`, explicitly enabled by `database-operations`. It parses
only status/forward-migrate requests and delegates to Infrastructure's
`database_operations` module, which shares normal profile sources/defaults but
deserializes only database-relevant settings. Structural, selected-provider,
and database production validation precedes connection. It reuses the same
`migration_plan`, never normal initialization, ensure, seed, or worker paths.
Its exact binary, Cargo registration, Infrastructure module/config integration,
and operation-source claims are declared for governed upgrades. Claims alone
do not authorize source publication: the existing v0.6.0-to-v0.7.0 edge still
changes only three files and does not install this entry point.

## Rendering And Validation

Component manifests select the layered base and the Leptos Identity adapter.
`templates/package.toml` gives this data-only graph a release-aligned package
identity, declares its compatible HTTPS framework source and stable SemVer
tag, enumerates the contained template, components and official modules, and
locks every manifest and included source path with a deterministic SHA-256
digest. Package loading rejects unknown or unsorted identities, source-tree
changes, local or credentialed framework locations, mismatched versions, and
undeclared component manifests before planning output. Component manifests
cannot define execution hooks.

The package source is opened below a directory descriptor without following
symlinks. Every descendant must be a regular file or real directory and is
bounded by per-file, total-byte, and file-count limits. The loader rechecks the
opened package-root identity after traversal, rejects replacement races, and
retains one immutable byte snapshot for manifest parsing, digest verification,
and rendering. The observed file set must equal the manifests' declared
template, component, and included-source graph; missing and graph-undeclared
files fail before destination publication. Invalid manifests and path failures
produce content-redacted diagnostics, and package loading performs no network
or process execution. Safe package-source access currently fails closed outside
Linux and Apple platforms.

The schema-3 package manifest and schema-3 component manifests form a closed
composition graph. Schema-3 distinguishes rendered components from additive
installation units. An installation unit cannot include or vendor source; it
declares exactly one owned module, compatible database and client adapters,
typed contribution kinds, and sorted release-pinned framework dependencies.
The bundled `identity` unit requires the minimal Leptos composition and records
its configuration, seed, background work, provider migration sources, separate
cookie-BFF and Bearer API routes, OpenAPI, Leptos routes and navigation, and
capability-preflight contributions. This metadata is inert package data; it
does not execute code, migrations, or installation by itself.

Resolution accepts an explicit framework/package identity and component root
set, then produces a canonical topological component order, exact component
and module versions, and the accumulated capability set. Required dependencies
join the graph automatically; optional dependencies are validated but join it
only when explicitly selected. Cycles, missing dependencies, selected
conflicts, missing capabilities, duplicate module ownership, and incompatible
framework, package, module, or recorded capability state return sorted typed
diagnostics. Resolution reads data already loaded into the catalog and performs
no write, process execution, network access, source resolution, or runtime
configuration lookup.

The package manifest may also declare schema-1 upgrade-edge manifests directly
below `templates/upgrades/`. Their exact bytes and package-relative paths are
part of the package content digest and the closed package file set. Each edge
is inert data: it identifies one exact, direct, forward stable-SemVer release
transition into the authenticated package release; enumerates exact source and
target component, module, capability, database, and client states; and records
the manifest fields and managed integration points that the transition permits.
The edge also binds the exact source package and released baseline digests,
declares source and target ownership for each composition, and supplies
direction-correct source and target SHA-256 digests for every managed
integration. Create operations have only a target digest, edits have both
digests, and retirements have only a source digest. Managed file digests and
manifest transition sets may be scoped to a specific composition when provider
or module profiles differ; shared declarations apply to every selected
composition. A managed integration may explicitly take its target bytes from
another declared package component at the same path. Its ownership stays with
the installed component, while the target path and digest must match the
authenticated package snapshot. This lets an Identity-added application use
the canonical Identity lockfile without installing the default UI component.
Edges cannot contain executable commands or change the framework source,
package identity, database adapter, or client adapter.

Upgrade-graph loading normalizes declaration order and rejects duplicate edge
or composition identities, a source composition mapped more than once,
downgrades, skipped releases, mismatched target releases, unsupported adapters,
and component, module, capability, or managed-path references outside the
closed component graph. Every applicable managed transition must have an exact
managed-integration claim in the ownership contract for the state where it
exists: creates in the target, retirements in the source, and edits in both.
Target digests for creates and edits must match bytes in the package snapshot.
Rejections expose a stable typed diagnostic containing only a bounded
kind and structural subject; graph, composition, and managed integration counts
are bounded before semantic traversal. A target composition must resolve
through the same component graph used by rendering.

Existing-application authentication opens the application root and declared
parents through anchored no-follow directory handles. It validates
`hegira.toml`, resolves exactly one release and composition edge, and observes
only applicable managed paths. Creates require absence; edits and retirements
require regular bounded files with the edge-declared digest. Schema-3 local
ownership must agree with the authenticated edge ownership, while an explicitly
supported older schema obtains its ownership proof from that edge rather than
from the local manifest. Root replacement, symlinks, special files, oversized
inputs, path aliases, ownership disagreement, and digest mismatch fail before a
plan exists. The returned in-memory boundary contains authenticated source and
target bytes for later planning; diagnostics are versioned and content-redacted.
Authentication performs no write, network access, process execution, dependency
initialization, or application mutation.

An authenticated direct edge is resolved against the target component graph and
converted into one complete `application_mutator::ChangePlan`. The planner
requires exact target component, module, and capability state; preserves the
selected database and client; moves authenticated framework dependencies to the
target release without changing their feature policy; and constructs the target
`hegira.toml` from typed composition data. Declared manifest transitions must
match the fields that actually change, and neither the manifest nor dependency
state can advance without its required managed source transition. Edge-declared
creates use absent preconditions, edits retain the observed source digest, and
retirements retain the observed digest plus the exact managed integration
owner. The planner sorts the complete plan by canonical application path,
rejects duplicate or conflicting operations, and never edits or retires
application migration history. Its versioned summary binds the exact source and
target releases, package and baseline digests, target components, modules,
framework dependencies and manifest transitions, and component and integration
owners, preconditions, and result digests without exposing source or resulting
content.
Blocked, unsupported, incompatible, and conflicting inputs are distinct typed,
content-redacted outcomes. Planning performs no publication, database, network,
or process operation.

The bundled package declares one direct v0.6.0-to-v0.7.0 edge for default,
minimal, and Identity-added layered applications with SQLite or PostgreSQL.
It authenticates the released baseline and changes only `hegira.toml`, the
workspace `Cargo.toml`, and `Cargo.lock`. Default and later-installed Identity
compositions retain their explicit server, HTTP, migration-source, configuration,
and Leptos integrations because these application files do not change across
this release edge; the version-pinned official packages advance together.
Product layers, runtime configuration values, generated resources, and migration
history remain application-owned or immutable. The public CLI exposes this edge
through read-only `upgrade status`, `upgrade --dry-run`, and explicit `upgrade`
application through the existing mutation publisher.
Authentication checks the edge-declared managed files, not byte equality of the
entire application against the released fixture. Product source and generated
resources may therefore evolve independently. For this edge, all three managed
files must still match their authenticated source digests: even an unrelated
dependency or lockfile customization is a conflict, not an instruction to merge.
The general plan contract supports declared managed creates, edits, and
retirements, but this bundled edge uses only edits. It does not remove components,
skip releases, downgrade, run migration SQL, or supply another client adapter.
Readiness authenticates the package, application ownership and managed boundary,
then shares the planner's pure target-manifest validation without constructing
a change plan. Recovery-marker inspection is anchored and no-follow; any marker
blocks readiness without reading its contents. Sorted schema-1 JSON and human
reports redact source content and machine-local paths and distinguish unsupported,
incompatible, conflicting, and recovery-blocked states. Compatible current
compositions report no upgrade. Assessment does not authorize later mutation.

Dry-run reauthenticates the observed application and builds the renderer's
`UpgradePlan` directly. Its schema-1 CLI envelope includes the same readiness
assessment, nullable typed plan summary, outcome, and preserved ownership
classes. Explicit targets must match the authenticated direct edge exactly.
Human and JSON output retain ordered changes, owners, integrations, manifest
transitions, component/dependency identities, preconditions, and resulting
digests without source contents. Conflicts and recovery markers suppress the
plan. Preview neither creates publication state nor persists a cached plan;
the typed in-memory plan remains the contract for subsequent publication.

Apply uses that same preparation path and passes its plan directly to
`application_mutator::publish_change_plan`. The publisher owns marker
serialization, whole-plan and per-change digest preconditions, directory
anchoring, private staging, rollback, and retained recovery state. The CLI
does not implement a parallel filesystem mutation path. Successful schema-1
apply output binds the exact plan to a deterministic receipt and application-owner
next steps; the assessment records pre-publication state. Failures do not
emit a success receipt. Repeated apply rejects the absent direct edge without
writes. Migration execution, lockfile regeneration, subprocesses, network
access, and service startup remain outside upgrade publication.
An applied receipt proves source publication, not a successful build, database
migration, health check, or deployment. The application owner follows the
[post-upgrade workflow](getting-started.md#after-an-application-upgrade) and
[recovery guidance](getting-started.md#resolve-upgrade-conflicts-and-recovery).

Committed readiness/execution v1 JSON schemas describe the closed public
automation contract, including plan, receipt, and diagnostic definitions.
Structural validation, negative schema tests, typed exit/diagnostic mapping,
and reviewed human/JSON snapshots enforce separate aspects of the contract.
Filesystem creation order, reversed current component declarations, and supplied
versus closed stdin do not change deterministic output. Snapshot mismatches
require explicit review; they are not auto-accepted and are not the sole API
test. Upgrade commands have no interactive prompt protocol.

Upgrade tests obtain v0.6.0 application source from committed, content-addressed
release baselines rather than current templates or mutable remote content. One
release manifest pins the annotated tag object, commit, source tree, package
digest, and generated lock revision. Sorted tree manifests cover default,
minimal, and Identity-added SQLite and PostgreSQL states while a shared SHA-256
object store avoids duplicate bytes. The test-only `upgrade_test_support` tool
authenticates the complete closed fixture set before exposing an in-memory
snapshot or materializing it into a previously absent disposable directory; it
does not invoke Git, execute fixture content, or access the network.

Focused preservation tests customize these released baselines with product
code in every application layer, provider-specific generated resources where
Identity is installed, runtime configuration values, and append-only application
migrations. Sorted path/SHA-256 fingerprints prove that every file outside the
three declared release-managed files and every historical migration remains
unchanged. Minimal applications retain their capability boundary and reject
protected resource generation. Conflicting managed edits block planning, and
edits made after planning block publication without changing product files.
Fixed product-tree fingerprints also prevent later generator changes from
silently replacing the customized source used to exercise released applications.
The layered-template gate additionally compiles all six customized upgraded
profiles natively and with hydration. Only separate disposable compile copies
replace release dependencies with local framework paths; verified upgrade
output retains its release-pinned sources and lockfile.
Profile compilation is serialized through one stable application source path
to prevent Cargo from confusing same-named local packages across fixture trees
while reusing compatible framework artifacts.

The separate upgraded-application lifecycle validator extends those public-source
checks to disposable fresh and released-schema databases, migration checksum/data
preservation, release assets, and production images for both providers and all three
compositions. Released Identity SQL is verified against its v0.6.0 source inventory.
Only separate validation copies receive local dependencies, test fixtures, and a
post-upgrade application migration. Production HTTP probes retain authentication,
authorization, cookie/Bearer, and minimal-composition boundaries. The gate never
resets persistent data or modifies verified public upgrade output.
CI runs its three composition selections as additional cells of the existing
generated-application lifecycle matrix, each covering both providers. The stable
quality aggregate and source-release publisher depend on the complete matrix;
focused upgrade assertions remain in their framework, renderer, and CLI owners.

The reusable renderer exposes typed composition request/result/diagnostic,
render request, plan, publication-result, and error-category contracts. A
render request may choose component roots, but the loaded package exclusively
supplies the framework and package identities. The renderer resolves those
roots once and uses that same result to select component files and serialize
the component, module, and capability state in `hegira.toml`; template source
does not maintain a duplicate composition list. It consumes the same verified
package snapshot, substitutes declared variables, detects output collisions,
rejects symbolic links and path traversal, constructs the entire output plan
before writing, and atomically publishes into a previously absent destination.
It has no network, process, or repository-event dependency and does not execute
component scripts.

Normal renders retain pinned release-source dependencies. Repository
validation selects a separate adapter that rewrites only a disposable render
to consume a staged, credential-free view of the current framework source.
The generated-application gate first invokes the public CLI for SQLite and
PostgreSQL. Its staging adapter verifies the CLI output paths and bytes against
the canonical request, uses the render plan's resolved component graph when
selecting declared dependency rewrites, then patches those dependencies in a
separate copy.
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

Every render includes schema-versioned `hegira.toml`. Schema 3 records the
application identifier, HTTPS framework repository and stable SemVer tag,
installed component-package, component and module identities with their
versions, provided capabilities, selected database and client adapters, exact
framework/package upgrade state, and explicit source-ownership claims.
Unclaimed paths are application-owned. Managed integration points carry stable
integration identities, generated-once scaffolding is not adopted as managed
source, and immutable history can be represented without authorizing rewrites.
The parser rejects unknown fields, duplicate identities, unsupported values,
inconsistent composition or upgrade state, invalid or overlapping ownership
paths, credentials, local framework paths, and mismatches between the recorded
and actually rendered component sets.
Deterministic serialization records the validated generation contract; it is
not a runtime configuration or secret store. Editing it does not trigger
regeneration or upgrades. Schema-1 and schema-2 manifests remain readable for
inspection, but cannot be serialized or mutated as schema 3 without an
explicit supported transition. Manifest parsing, validation, and compatibility
assessment perform no network access or filesystem mutation.
The field-level contract is documented in
[Getting started](getting-started.md#generated-ownership-and-hegiratoml).

The application-manifest package also exposes a pure, fail-closed mutation
compatibility assessment. A manifest is compatible only when its schema,
framework repository and exact release, component-package identity, installed
component/module composition, capability set, and single selected
database/client adapters match the caller's supported policy. A valid manifest
from another release or with an unknown supported-shape composition member is
reported as unsupported; a current-shape manifest that conflicts with
canonical selection or framework identity is reported as incompatible with the
exact field identified. Normal parsing remains separate, so older valid
manifests can still be read without becoming writable. This assessment performs
no file write, network access, source mutation, dependency change, or upgrade.

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
