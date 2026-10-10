# {{application_name}} architecture

This workspace selects `{{database_adapter}}` persistence and the Leptos client.
It was generated with the minimal composition, without an official module or
authentication/authorization capability. [hegira.toml](../hegira.toml) and
`hegira inspect` describe the current composition, including later additions.

| Application path | Responsibility |
| --- | --- |
| [Domain Shared](../crates/domain_shared/src/lib.rs) | Shared domain types |
| [Domain](../crates/domain/src/lib.rs) | Product entities, invariants, and domain rules |
| [Application Contracts](../crates/application_contracts/src/lib.rs) | Use-case contracts, DTOs, and ports |
| [Application](../crates/application/src/lib.rs) | Use cases, business validation, and transaction boundaries |
| [Infrastructure](../crates/infrastructure/src/lib.rs) | Selected SQLx repositories, configuration, and migrations |
| [Presentation](../crates/presentation/src/lib.rs) | HTTP transport and explicit route contributions |
| [Server](../apps/server/src/server.rs) | Typed application composition, HTTP host, and configuration startup |
| [Web](../apps/web/src/lib.rs) | Application shell, pages, server functions, routes, localization, and navigation |

Dependencies point inward. Domain and Application rules remain independent of
Axum, Leptos, SQLx, and vendor SDKs. HTTP handlers and Leptos server functions
translate requests and delegate business validation to application services.
Transactions belong to use cases, not the whole HTTP request. Compose services
and routes explicitly. Do not treat UI checks, request metadata, or forwarded
headers as an authorization boundary.

Provider SQL and migration sources remain explicit under
[Infrastructure migrations](../crates/infrastructure/migrations). Configuration
and compiled capabilities are validated before external providers initialize.
[Configuration profiles](../config) and local runtime secrets are separate from
the composition manifest.

The separate [app_database binary](../apps/server/src/bin/app_database.rs)
delegates to [Infrastructure database operations](../crates/infrastructure/src/database_operations.rs).
It performs status or forward migration on an existing target without starting
HTTP or workers. Review the [operation guide](development.md) before execution.

## Adding Identity

Protected resource generation requires authentication and authorization
capabilities. The initial minimal composition does not supply them. Using the
compatible CLI configured in the [development guide](development.md), inspect
and preview an explicit installation:

```sh
hegira inspect
hegira component add identity --dry-run
```

After reviewing the source plan, `hegira component add identity` publishes the
additive source changes. It does not connect to a database, migrate, seed, or
start a server. Review source and lockfile changes and run check/test before
authorizing runtime operations. Installation preserves existing documentation.

Once `hegira.toml` records Identity and its authentication/authorization
capabilities, application-owned Infrastructure, server, and web integrations
select official Application, SQLx, HTTP, and Leptos adapters. Login pages and
protected resource generation then become available. Protected operations must
authorize in Application before repository access; UI checks remain presentation.
Cookie browser/BFF routes retain session-cookie/CSRF policy separately from Bearer
API routes. Startup may initialize the added providers and seed Identity when
configured, so review runtime settings again. The provider and development
profile remain `{{database_adapter}}` and `{{development_profile}}`.
