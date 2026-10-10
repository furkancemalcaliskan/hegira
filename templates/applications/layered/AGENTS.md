# {{application_name}} working contract

This is the canonical working contract for humans and coding agents in this
application repository. [Claude](CLAUDE.md) and
[Cursor](.cursor/rules/application.mdc) adapters delegate here and add no rules.
The application owner maintains these generated-once instructions and decides
contribution availability, accepted issue scope, and repository hosting policy.

## Read before editing

- [README](README.md): initial composition and application purpose.
- [Architecture](docs/architecture.md): layers and transport/security boundaries.
- [Development](docs/development.md): prerequisites, CLI setup, validation,
  startup, release builds, and database operations.
- [Ownership and recovery](docs/ownership.md): source changes, upgrades, and
  interrupted publication.
- [hegira.toml](hegira.toml): exact releases, current composition, selected
  adapters, and source ownership.

Source and committed configuration are authoritative. Update affected
documentation with behavior changes; never describe planned work as implemented.
Keep product rules in this application. Framework and official module packages
are pinned dependencies, not application-local source to vendor or rewrite.

## Composition and layers

The selected provider is `{{database_adapter}}`, the client is Leptos, and the
development profile is `{{development_profile}}`. Review `hegira inspect` and
the manifest before changing capabilities. The default composition includes
Identity; minimal starts without an official module, login pages, authentication,
or authorization. Only an explicit Identity installation adds those capabilities
to minimal. Installation preserves these instructions; update product-specific
guidance while using the manifest as the current composition record.

| Path | Responsibility |
| --- | --- |
| `crates/domain_shared` | Shared domain types |
| `crates/domain` | Product entities, invariants, and domain rules |
| `crates/application_contracts` | Use-case contracts, DTOs, and ports |
| `crates/application` | Use cases, business validation, authorization where supported, and transaction boundaries |
| `crates/infrastructure` | Configuration, SQLx repositories, provider adapters, and explicit migration composition |
| `crates/presentation` | HTTP transport and route/OpenAPI contributions where selected |
| `apps/server` | Typed server composition and the separate `app_database` entry point |
| `apps/web` | Application shell, Leptos pages/server functions, routes, localization, and navigation |

Dependencies point inward. Domain and Application rules must not depend on
Axum, Leptos, SQLx, or vendor SDKs. Keep HTTP controllers and Leptos server
functions transport-focused and delegate business validation to Application.
Protected operations must authorize in Application before repository/count
access; UI permission checks are presentation only. Minimal is not an allow-all
authorization implementation. Protected resource generation requires installed
authentication and authorization capabilities.

Keep cookie-authenticated browser/BFF and Bearer API policies separate when
Identity is installed. Preserve session-cookie and CSRF controls. Treat request
metadata and forwarded headers as untrusted outside explicitly configured proxy
policy. Keep transactions at use-case boundaries, provider SQL explicit, and
services/routes composed through typed integration points. Validate configuration
and compiled capabilities before initializing external dependencies.

## Development and validation

Run commands from this workspace root using the compatible source CLI helper
defined in [Development](docs/development.md#cli-and-prerequisites). There is no
`hegira_cli` package in this application workspace. Review the selected operation:

```sh
hegira inspect
hegira doctor --operation check
hegira check --dry-run
hegira test --dry-run
hegira dev --dry-run
hegira build --release --dry-run
hegira db status --profile {{development_profile}} --dry-run
hegira db migrate --profile {{development_profile}} --dry-run
cargo fmt --all -- --check
```

After explicit owner trust, follow the development guide's Linux execution
commands with absolute trusted Cargo and tool-directory selections. Check/test
select `{{database_feature}}` without default features, preserve Cargo locks,
and check WASM hydration. Tests execute trusted source; ignored database tests
never run by default. Dev startup may create/open a configured database, migrate,
initialize selected providers, and seed installed Identity when enabled. Review
the actual target and inherited URL overrides before approving startup.

Doctor and previews grant no execution authority. Database status preserves data,
schema, and history without provisioning, with the documented SQLite WAL sidecar
exception. Forward migration needs explicit execution; PostgreSQL production
migration also needs independent `--approve-production-migration`. Neither
database command seeds or starts HTTP/workers. Use only explicitly disposable
targets for destructive integration tests; never infer reset or production access
from a check, test, source-generation, or source-upgrade request.

Run the smallest relevant checks while iterating, then the checks required by the
affected application contract. Report missing prerequisites and checks not run;
do not weaken quality gates or security defaults. Keep runtime secrets out of
build environments and browser assets. A successful release build is not
deployment approval and its ownership marker grants no cleanup of developer output.

## Issue, branch, commit, and pull request conventions

Before implementation, verify the owner-accepted issue, scope, dependencies,
milestone, and acceptance criteria. Inspect Git status, branch, recent history,
and unrelated tracked, untracked, ignored, and stashed work. Preserve that work.
Create the issue branch from the latest `develop`; never implement directly on
`develop` or `main`. Generation itself does not initialize Git or configure a
remote. The owner configures hosting, protections, and maintenance exceptions.

Use this integration path:

```text
issue -> issue branch -> pull request -> develop
      -> release promotion pull request -> main -> signed tag -> release
```

Supported change types are `feat`, `fix`, `refactor`, `test`, `docs`, `ci`,
`release`, and `chore`. Ordinary issue work uses:

| Item | Convention |
| --- | --- |
| Branch | `<type>/<issue>-<short-description>` |
| Commit and squash commit | `#<issue> <type>(<scope>): <description>` |
| Ordinary PR title | `<type>(<scope>): <description>` without an issue number |
| PR body | Exactly one `Closes #<issue>` matching the branch issue |
| PR target and merge | `develop`, squash merge |

For example, branch `feat/123-order-rules` uses commit
`#123 feat(domain): add order rules`, PR title
`feat(domain): add order rules`, and body `Closes #123`.
Keep changes limited to the accepted issue. Contribution availability is the
application owner's policy; do not infer Hegira's contribution restrictions.

Promote only a completed, verified milestone from `develop` to `main` using a
merge commit. The promotion PR title is
`release: promote {{application_name}} vX.Y.Z to main`.
The owner reviews version, changelog/release notes, checks, and signed tag/release
identity before authorizing publication. These conventions do not create CI,
remote protections, release automation, or a contribution policy.

## Ownership, recovery, and security

Unclaimed paths are application-owned. README, guides, this file, and both
adapters are generated-once and editable by the owner; installation and upgrades
preserve them. Managed integration claims require exact authenticated transition
authority and observed digest preconditions. Append migrations instead of editing
immutable history. Keep dependencies and lockfiles under deliberate review.

Review read-only `hegira upgrade status --json` and `hegira upgrade --dry-run --json`
before any explicitly authorized apply. Never change versions, claims, edge
digests, or source to bypass authentication. There is no automatic conflict merge
or force-upgrade. Preserve `.hegira-mutation.lock` and private staged transaction
files after interrupted or uncertain publication; follow
[manual recovery](docs/ownership.md#interrupted-or-conflicting-mutation).
Do not delete recovery state merely to unblock another operation. Source rollback
is not database rollback.

Local preparation does not grant publication authority. Do not commit, push,
create/modify a PR, merge, tag, release, deploy, change remote settings, or access
databases without explicit owner authority for that action. Destructive changes,
scope expansion, and unresolved product decisions require owner direction.
Do not commit credentials, tokens, private keys, personal data, production logs,
database dumps, private runtime configuration, build output, installed frontend
dependencies, or local databases. Report suspected vulnerabilities privately to
the application owner through their chosen security channel.

Before handing work back, review the complete diff and Git status; confirm issue
acceptance criteria; summarize behavior, changed files, risks, and remaining
decisions; and report passed, failed, and unrun checks. Provide separate commit
and push commands, the issue-free PR title, and a reviewable PR description when
requested. Leave publication to the owner unless explicitly authorized.
