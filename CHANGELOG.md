# Changelog

All notable changes are documented here. Releases follow Semantic Versioning.

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
