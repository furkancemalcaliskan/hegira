# {{application_name}}

A layered Axum server and Leptos SSR/hydration application using the selected
`{{database_adapter}}` SQLx adapter. Generated initially with the minimal
composition: no official module, login pages, authentication, or authorization
capability is installed at creation.

The current composition is recorded in [hegira.toml](hegira.toml) and reported by
`hegira inspect`. If Identity has since been added, its module adapters provide
authentication and authorization; see the
[Identity installation guidance](docs/architecture.md#adding-identity).
Installation preserves this generated-once documentation, so the owner maintains
the description of subsequent product changes.

Run commands from this application workspace root. Begin with the
[development guide](docs/development.md) for prerequisites, reviewed development
startup, checks, tests, release builds, and explicit database operations.

- [Architecture and security boundaries](docs/architecture.md)
- [Source ownership, upgrades, and recovery](docs/ownership.md)

These documents belong to the application owner. Source generation and upgrades
do not authorize database access or deployment.
