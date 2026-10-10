# {{application_name}}

A layered Axum server and Leptos SSR/hydration application using the selected
`{{database_adapter}}` SQLx adapter. The initial composition includes the official
Identity module, authentication, and application-layer authorization. Application
code composes the released module adapters; module source stays in the framework.

Run commands from this application workspace root. Begin with the
[development guide](docs/development.md) for prerequisites, reviewed development
startup, checks, tests, release builds, and explicit database operations.

- [Architecture and security boundaries](docs/architecture.md)
- [Source ownership, upgrades, and recovery](docs/ownership.md)
- [Current release, composition, and ownership record](hegira.toml)

These documents are generated once and belong to the application owner. Keep
them current as product code, configuration, and composition change. Source
generation and upgrades do not authorize database access or deployment.
