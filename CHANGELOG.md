# Changelog

All notable changes are documented here. Releases follow Semantic Versioning.

## [Unreleased]

### Added

- Added release validation through public CLI-generated SQLite and PostgreSQL
  applications, verified disposable dependency staging, native and hydration
  builds, historical database upgrades, and production HTTP/asset contracts.
  Scoped the SQLite worker-readiness test to its provider so PostgreSQL-only
  generated test targets compile without enabling SQLite.
- Added CLI regression coverage for both provider output snapshots, guided/flag
  parity, prompt cancellation, isolated process state, invalid selections, and
  renderer failure recovery with a real child-process write limit on Linux.
- Added the source-runnable `hegira` CLI foundation with top-level and `new`
  help, version reporting, separated output and diagnostics, and stable process
  outcomes for usage, validation, conflict, and internal failures.
- Added deterministic non-interactive `hegira new` application creation with
  SQLite, Leptos, and Identity defaults, explicit supported selections,
  release-source dependencies, and side-effect-free next-step guidance.
- Added guided `hegira new` prompts for missing application inputs and
  implemented selections, including a final summary, confirmation, safe
  cancellation, deterministic prompt tests, and non-TTY safeguards.
- Added a release-aligned canonical component-package contract with explicit
  framework compatibility, a declared data-only component graph, and a
  deterministic SHA-256 source digest.

### Changed

- Clarified source-runnable CLI setup, guided and non-interactive creation,
  supported selections, manifest semantics, and generated-application ownership
  in the current documentation, including source-only distribution limitations.
- Enforced shared project-identity validation, portable destination names,
  existing real parents, and explicit no-overwrite behavior. Renderer publication
  now uses private directory-relative staging and exclusive atomic rename with
  race checks; unsupported platforms fail closed before writing.

- Separated the reusable template rendering request, plan, publication, result,
  and typed diagnostic contracts from the repository-only local dependency
  rewrite adapter.
- Added the versioned and validated `hegira.toml` generated-application
  identity contract with deterministic serialization and explicit framework,
  component, database-adapter, and client-adapter selections.
- Retired the repository compatibility host and legacy layered compatibility
  packages after generated-application validation became the authoritative
  framework integration consumer.
- Reduced the root Cargo workspace to application-independent framework
  packages, official modules, and internal repository tooling.
- Moved repository setup, architecture, deployment, and operations guidance to
  the canonical rendered-application ownership model.

### Removed

- Removed compatibility-host configuration, container, smoke, packaging,
  backup, restore, and frontend-size helpers that no longer had a runnable root
  application target.

## [0.3.0] - 2026-08-01

### Added

- Added application-independent platform packages for configuration,
  persistence, background work, HTTP policy, Leptos integration,
  observability, runtime lifecycle, capability identity, and test support.
- Added the official layered Identity module with canonical Domain Shared,
  Domain, Application Contracts, Application, SQLx, Axum HTTP, and Leptos
  adapter packages.
- Added a workspace-external, brand-neutral layered application template with
  application-owned server, web, configuration, migration composition, and
  production container surfaces.
- Added typed component manifests and an internal deterministic, atomic
  template renderer for repository validation.
- Added generated-application validation for fresh SQLite and PostgreSQL
  databases, supported v0.2.0 upgrades, production images, hydration assets,
  HTTP security, and unauthenticated Bearer behavior.

### Changed

- Replaced the sample Catalog capability and UI with a neutral authenticated
  dashboard while preserving historical migration compatibility.
- Established repository ownership boundaries for framework packages,
  official modules, application templates, internal tooling, and deployable
  applications.
- Made module HTTP and Leptos integration explicit at application composition
  roots instead of relying on automatic discovery or publication.
- Split repository validation into framework, official-module, template, and
  generated-application gates aggregated by the stable `quality` check.
- Updated architecture, setup, deployment, operations, contribution, agent,
  and maintainer documentation to describe the implemented repository model.
- Aligned release automation with source-only framework distribution and
  removed compatibility-host application builds from the release contract.

### Security

- Preserved cookie/BFF and Bearer API policy separation across the extracted
  Identity HTTP adapter and explicit host composition.
- Kept release workflows read-only by default, restricted publication to the
  tag-triggered job, and prohibited crate, CLI, container-registry, and other
  package publication.
- Retained immutable releases, the source-scoped SPDX SBOM, and GitHub release
  attestations without publishing application or deployment artifacts.

### Upgrade And Maintainer Notes

- Framework source now spans `crates/`, `modules/identity/`,
  `templates/applications/layered/`, and `tools/template_renderer/`.
  `apps/hegira/` remains the compatibility host for framework integration.
- PostgreSQL and SQLite upgrades from v0.2.0 are validated. Historical Catalog
  migration identities and checksums remain immutable while Catalog runtime
  state is retired.
- The canonical layered template consumes framework packages from the pinned
  `v0.3.0` source tag. The internal renderer is not a public CLI.
- This release does not publish Cargo crates, a `hegira` CLI, prebuilt
  executables, container images, or preview deployments.

## [0.2.0] - 2026-07-23

### Added

- Added a canonical repository operating and contribution contract covering
  issue, branch, commit, pull-request, integration, and release conventions.
- Added shared repository instructions for AGENTS.md-compatible tools, Codex,
  Claude Code, and Cursor without duplicating the canonical rules.
- Added deterministic repository-policy validation for pull-request metadata,
  documentation links, agent adapters, and release workflow invariants.
- Added a reproducible full-stack packaging contract that verifies the server,
  database migrator, hydrated WebAssembly, JavaScript, CSS, and branding
  outputs without creating a platform archive.
- Added a locked Cargo-metadata check that enforces the documented direct
  dependency boundaries between workspace packages in local validation and
  required CI.

### Changed

- Separated issue-prefixed commit titles from issue-free pull-request titles
  and enforced the distinction in repository policy.
- Moved the deployable `hegira` package, its entry points, Cargo-Leptos
  metadata, and integration tests under `apps/hegira`.
- Converted the repository root into a virtual Cargo workspace while
  preserving root-level development, validation, packaging, and container
  commands.
- Aligned architecture, setup, deployment, operations, and maintainer
  documentation with the app-owned workspace and its root-level command
  contract.
- Replaced platform-specific Linux application bundles and checksums with
  source-first GitHub Releases backed by immutable SemVer tags, versioned
  release notes, GitHub source archives, and a source-scoped SPDX SBOM.

### Operator Notes

- This release does not change runtime behavior, database schemas,
  authentication, authorization, production configuration, or deployment
  topology.
- Commands documented from the repository root remain valid. Tooling that
  directly referenced the former root `src/`, `tests/`, or package manifest
  must use `apps/hegira`.
- GitHub source archives replace the v0.1.x Linux application bundle as the
  release distribution.

## [0.1.2] - 2026-07-22

### Added

- Established the Hegira identity and project narrative in the repository
  landing section.
- Documented the Hegira philosophy around production-oriented defaults,
  explicit conventions, and a proven path from idea to production.
- Added canonical repository and web-public branding asset locations.

### Changed

- Replaced temporary letter-based branding placeholders in the application
  shell, boot loader, and authentication surfaces with the Hegira logo.
- Optimized the public logo variant for web delivery while retaining the
  original canonical branding asset.
- Removed the stale version-pinned default from the manual release workflow so
  maintainers must explicitly select the release ref being verified.

### Operator Notes

- This release does not change runtime configuration, database schemas,
  authentication behavior, or deployment contracts.
- No database migration or application configuration update is required when
  upgrading from `v0.1.1`.

## [0.1.1] - 2026-07-18

### Fixed

- Aligned the committed production profile with the default
  `ssr,db-postgres` server build so the minimal image boots without optional
  provider features.
- Validated runtime provider selections against compiled Cargo capabilities
  before telemetry, database, or external dependency initialization.
- Applied structural configuration validation consistently in development,
  test, and production environments.
- Made the standalone release bundle include the production configuration,
  database migrator, and complete Leptos static assets.

### Security

- Separated cookie-authenticated BFF middleware from Bearer API middleware so
  browser CSRF policy does not change the external API contract.
- Extended CSRF validation to POST, PUT, PATCH, and DELETE cookie mutations and
  derived the trusted origin from `application.public_url`.
- Replaced unconditional forwarded-address trust with TCP-peer-based client IP
  resolution and explicit trusted-proxy CIDRs.
- Rejected universal proxy trust ranges and malformed trusted forwarding
  chains.

### CI And Release

- Restricted full validation to pull requests, integration branches, and
  explicit manual runs while preserving required quality and supply-chain
  checks.
- Added a disposable production container smoke test covering PostgreSQL,
  migrations, health, readiness, HTML, CSS, and JavaScript assets.
- Added a pre-publish release-candidate path that verifies the release bundle,
  checksum, SPDX SBOM, and prepared release notes without creating a preview
  deployment.
- Kept dependency auditing read-only on GitHub-hosted runners and isolated
  database-backed release tests from ambient connection settings.
- Aligned PostgreSQL search outbox assertions with the flattened document
  payload contract and scoped lifecycle checks to their target identity.
- Made release bundles resolve Tailwind from the lockfile-pinned frontend
  dependencies instead of an ambient or automatically downloaded executable.

### Operator Notes

- The default production build contains PostgreSQL support only. Redis, SMTP,
  S3, Meilisearch, Prometheus, and OTLP require their matching Cargo features
  before they can be selected at runtime.
- `security.trusted_proxies` is empty by default. Deployments behind a reverse
  proxy must configure only the CIDRs that connect directly to Hegira.
- Keep production auto-migration and identity seed disabled; run
  `db_migrator migrate` as a one-shot release step before application rollout.
- Existing Bearer API clients do not need browser Origin or Referer headers.
  Cookie-authenticated unsafe methods must be same-origin.

## [0.1.0] - 2026-07-13

### Added

- ABP-inspired DDD workspace for Axum, Leptos, and SQLx.
- PostgreSQL and SQLite provider-specific migrations and adapters.
- Identity, RBAC, audit, OAuth, TOTP, sessions, and a reference CRUD feature.
- Optional Redis, SMTP, S3, Meilisearch, Prometheus, and OpenTelemetry adapters.
- Single-process and separated web/worker deployment roles.
- Durable jobs, scheduler, health checks, OpenAPI, and production configuration validation.

### Security

- Dependency advisory and license policy checks.
- Private vulnerability reporting policy.
- Provider-isolated CI and checksummed release artefacts with an SBOM.
