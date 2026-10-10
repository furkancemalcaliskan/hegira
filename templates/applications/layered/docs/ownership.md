# {{application_name}} source ownership and recovery

[hegira.toml](../hegira.toml) records application identity, exact framework/package
releases, installed components/modules/capabilities, selected database/client,
and explicit ownership claims. Runtime configuration and secrets stay outside
this manifest. Unclaimed paths belong to the application owner.

README and these three guides are `generated-once`: the owner can edit them, and
component installation and source upgrades preserve them. Keep them current after
composition or product changes; `hegira inspect` reports current composition.
Generated-once scaffolding is not blanket managed rewrite permission. Managed
integrations require an exact authenticated transition and observed digests.
Historical migration files are immutable; append a new application migration
instead of changing a checksum already recorded by a database.

Using the compatible CLI helper in the [development guide](development.md),
inspect supported source-upgrade readiness and preview a direct transition:

```sh
hegira inspect
hegira upgrade status --json
hegira upgrade --dry-run --json
```

The generation package identifies framework `{{framework_version}}` and package
`{{package_version}}`. Its bundled direct edge is v0.6.0 → v0.7.0 for default,
minimal, and Identity-added SQLite/PostgreSQL applications; it changes only
`Cargo.toml`, `Cargo.lock`, and `hegira.toml`. A newly generated current application
has no remaining direct edge. No later upgrade is promised by these documents.

For a supported older source, review the exact plan and make a private source
backup before explicitly authorizing `hegira upgrade --apply --json`. Preview
does not cache publication authority. Custom managed files fail closed; there is
no force-upgrade, automatic conflict merge, or downgrade. Do not edit release
versions, ownership, edge/source digests, or source merely to bypass a check.

An apply receipt confirms source publication only. Review lockfiles, test product
behavior, check native/hydration compilation, and validate runtime settings.
Database migration, seeding, service startup, and deployment need separate owner
intent; source upgrades execute none of them. Source rollback is not database
rollback.

## Interrupted or conflicting mutation

1. Stop competing source mutations. Preserve a private copy of the application,
   `.hegira-mutation.lock`, and any retained private staged transaction files.
   A marker does not prove that the publisher stopped or rollback completed.
2. Compare affected paths with the reviewed backup and plan digests. Determine
   whether source is a complete pre-change state, complete target state, or a
   partial publication. Output-delivery failure may occur after publication.
3. The application owner approves manual recovery to a verified consistent
   state, preserving product work and immutable history. Resolve marker/staged
   state only after recovery is verified and no publisher is active. Never
   delete recovery files merely to enable another mutation. No universal CLI
   recovery-cleanup or repair command exists.
4. Re-run read-only `hegira upgrade status --json` and `hegira doctor`, review a
   fresh preview if an edge still applies, then validate the application before
   another apply or deployment. Repeated apply on the target exits 3 without writes.

Keep recovery contents, credentials, private configuration, database dumps, and
raw production logs out of commits, public issues, and command reports. Build
output, installed frontend dependencies, prepared tools, and local databases
are not application source to commit.
