# {{application_name}} architecture

This workspace selects `{{database_adapter}}` persistence and the Leptos client,
with official Identity authentication and authorization adapters. Check the
current composition in [hegira.toml](../hegira.toml) after owner changes.

| Application path | Responsibility |
| --- | --- |
| [Domain Shared](../crates/domain_shared/src/lib.rs) | Shared domain types |
| [Domain](../crates/domain/src/lib.rs) | Product entities, invariants, and domain rules |
| [Application Contracts](../crates/application_contracts/src/lib.rs) | Use-case contracts, DTOs, and ports |
| [Application](../crates/application/src/lib.rs) | Use cases, business validation, authorization, and transaction boundaries |
| [Infrastructure](../crates/infrastructure/src/lib.rs) | Selected SQLx repositories, configuration, migrations, and provider adapters |
| [Presentation](../crates/presentation/src/lib.rs) | HTTP transport and explicit route/OpenAPI contributions |
| [Server](../apps/server/src/server.rs) | Typed composition of application services, module adapters, HTTP, and runtime providers |
| [Web](../apps/web/src/lib.rs) | Application shell, pages, server functions, routes, localization, and navigation |

Dependencies point inward. Domain and Application rules remain independent of
Axum, Leptos, SQLx, and vendor SDKs. HTTP handlers and Leptos server functions
translate requests and delegate business validation and authorization to
application services. UI permission checks are presentation only. Transactions
belong to use cases, not the whole HTTP request.

Identity Application, SQLx, HTTP, and Leptos adapters are selected explicitly.
Cookie-authenticated browser/BFF requests use the secure session-cookie and CSRF
policy; Bearer API routes use their separate token policy. Do not infer trust
from forwarded headers or request metadata. Configure trusted proxies explicitly.

Provider SQL and migration sources remain explicit under
[Infrastructure migrations](../crates/infrastructure/migrations). Configuration
and compiled capabilities are validated before external providers initialize.
[Configuration profiles](../config) and local runtime secrets are separate from
the composition manifest. Optional providers require matching compiled features
and validated settings; their presence in configuration does not enable them.

The separate [app_database binary](../apps/server/src/bin/app_database.rs)
delegates to [Infrastructure database operations](../crates/infrastructure/src/database_operations.rs).
It performs status or forward migration on an existing target without starting
HTTP, workers, or Identity seeding. Normal server startup has different effects;
review the [operation guide](development.md) before starting it.
