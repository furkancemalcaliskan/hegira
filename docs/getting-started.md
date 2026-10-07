# Getting Started

Hegira currently ships framework source, official modules, a canonical layered
application base, and a source-runnable CLI with guided and non-interactive
application creation. The CLI writes the selected source tree and next-step
instructions; it does not install dependencies, run migrations, initialize a
Git repository, or execute generated code.

## Build And Invoke The CLI

Use a complete Hegira source checkout or extracted source archive and the Rust
toolchain pinned by its `rust-toolchain.toml`. Releases remain source-only:
there is no published CLI executable or supported crates.io installation.
From the framework repository root:

```sh
cargo build --locked -p hegira_cli
cargo run --locked -p hegira_cli -- --help
cargo run --locked -p hegira_cli -- new --help
cargo run --locked -p hegira_cli -- inspect --help
cargo run --locked -p hegira_cli -- doctor --help
cargo run --locked -p hegira_cli -- check --help
cargo run --locked -p hegira_cli -- test --help
cargo run --locked -p hegira_cli -- upgrade status --help
cargo run --locked -p hegira_cli -- upgrade --help
cargo run --locked -p hegira_cli -- component add --help
cargo run --locked -p hegira_cli -- generate resource --help
cargo run --locked -p hegira_cli -- generate migration --help
```

`cargo run` builds and invokes the `hegira` binary. Its canonical package is
loaded from the source tree recorded at compilation, not downloaded from a
registry. Keep that tree available at its original location; copying the binary
alone is not a standalone installation. Rebuild after relocating the source.
Node, Docker, and `cargo-leptos` are not needed just to generate files.

## Check And Test An Application

From an existing application or one of its subdirectories, review the current
composition's validation plan using the compatible framework source checkout:

```sh
cargo run --locked --manifest-path /path/to/hegira/Cargo.toml \
  -p hegira_cli -- check --dry-run
cargo run --locked --manifest-path /path/to/hegira/Cargo.toml \
  -p hegira_cli -- test --dry-run --json
```

Alternatively pass `--application-root <path>`. A preview reads the manifest
and authenticated bundled composition only. It does not probe tools, load runtime
configuration, compile code, create a build cache, or connect to a database.
Prerequisites are requirements, not a readiness certificate. Previewing does
not authorize a later execution, which constructs and authenticates a new plan.

On Linux, explicitly trusted execution uses the same typed plan:

```sh
cargo run --locked --manifest-path /path/to/hegira/Cargo.toml \
  -p hegira_cli -- check --execute --trust-application \
  --cargo /absolute/trusted/bin/cargo \
  --tool-directory /absolute/trusted/bin \
  --tool-directory /usr/bin
```

Replace the example paths with your reviewed tool locations; `--cargo` does not
search ambient `PATH`. Repeat `--tool-directory` for trusted auxiliary tool
directories needed by Rust and the linker. Tools and search directories must
be absolute and outside the application. Ordinary rustup proxies are supported;
scripts and set-id Cargo executables are not. Missing/unsafe prerequisite files
fail before spawning; missing WASM targets or Rust dependencies can still fail
in Cargo. Tools are not installed automatically. `RUSTUP_AUTO_INSTALL=0`
disables automatic toolchain installation, and an inherited `RUSTUP_TOOLCHAIN`
override is removed so the application's pinned toolchain remains effective.
Other platforms fail execution closed rather than silently relaxing these checks.

Use `test` in place of `check` for native tests. Both operations disable default
features, select the provider from `hegira.toml`, and preserve `--locked`.
`check` checks native workspace targets; `test` runs native workspace tests.
Both then check `app_server` hydration for `wasm32-unknown-unknown`; browser tests
are not executed. There is no arbitrary Cargo argument passthrough and no
`--ignored` option. Ignored/destructive database tests remain a separate manual
workflow requiring explicit opt-in and a verified disposable target; these
commands never grant database reset, seed, migration, or Docker authority.

Trust includes application code, build scripts, tests, Cargo configuration,
toolchain, and inherited environment. This is **not a sandbox**. Approved code
can access configured services, credentials, and files with your privileges;
application tests may themselves have side effects. Hegira performs no source or
lockfile rewrite, but cannot make arbitrary trusted code read-only. Cargo may
fetch already locked dependencies and write ordinary developer build output.
No Hegira-owned application cache or cleanup policy is implied.

Choose exactly one of `--dry-run` and `--execute`; execution also requires
`--trust-application`, `--cargo`, and at least one `--tool-directory`. There are
no interactive execution prompts. Human execution inherits raw child stdout and
stderr, which are **not redacted**. With `--json`, child output is discarded and
stdout contains one deterministic schema-1 envelope: `output_schema`, `mode`
(`preview` or `execute`), `plan`, `execution`, and `diagnostics`. Planning or
preflight failures use a null execution; planning failures also use a null plan.
Usage errors still use stderr, even with `--json`.

Successful previews and completed operations return `0`; failed/signalled
children, cancellation, termination, spawn/cleanup, and output failures return
`1`; usage errors return `2`; validation failures return `3`; recovery or
concurrent-operation conflicts return `4`. Child exit codes are reported, not
copied as Hegira's exit code. Ctrl-C and SIGTERM stop and reap owned child groups
through the existing executor. Pending mutation recovery blocks execution
without deleting recovery state. Do not mutate application source while an
operation runs. See the [execution boundary](architecture.md#trusted-application-process-execution-library)
for process-group, environment, tool trust, and Linux limitations.

## Guided Creation

In an interactive terminal, the guided form can collect the application name,
destination, and implemented adapter selections:

```sh
cargo run --locked -p hegira_cli -- new
```

It shows defaults and a final summary before writing files. Cancellation leaves
no generated application. Scripts, CI, redirected input, and other non-TTY
execution must provide the name and destination explicitly as shown below.

Supplying both the name and destination skips prompts and confirmation, even
in a terminal. Omitted adapter flags then use defaults. If either required
input is missing, the guided flow collects missing values and confirms the
selection; explicit flags are retained.

## Application Prerequisites And Non-Interactive Creation

- the Rust toolchain pinned by `rust-toolchain.toml`;
- the `wasm32-unknown-unknown` target;
- `cargo-leptos` version `0.3.7`;
- Node.js 22 or newer and npm 10 or newer for the Leptos stylesheet toolchain
  (CI selects Node.js 22 through the committed `.node-version`);
- Docker Engine 24 or newer and Docker Compose 2 or newer when using the local
  PostgreSQL service or container checks.

From the framework repository root:

```sh
rustup target add wasm32-unknown-unknown
cargo install --locked cargo-leptos --version 0.3.7
cargo run --locked -p hegira_cli -- new my-application \
  --destination ../my-application
cd ../my-application
tool_bin=$(sh scripts/prepare-wasm-bindgen.sh install Cargo.lock target/hegira-tools/wasm-bindgen/bin)
export PATH="$tool_bin:$PATH"
npm ci --prefix apps/web/src
```

The preparation script reads the exact `wasm-bindgen` version from
`Cargo.lock`, downloads only the declared official release asset with bounded
retries, verifies its committed SHA-256 digest, and rejects a version mismatch
before Cargo Leptos starts an application build. The production Dockerfile runs
the same preparation contract before copying application source, so a missing
tool cannot surface only after the expensive release compilation.

The output is an independent Cargo workspace. Its normal dependencies use the
framework repository and release tag declared by the template rather than
paths into the maintainer checkout.

Use source from a compatible release for an independently buildable application.
Generation itself does not fetch or build those pinned dependencies. An
unreleased checkout can contain changes absent from its declared release tag;
successful generation alone does not prove that tag contains the required
packages. Maintainer checks validate current source in disposable copies, not
by silently changing the application's release pin.

## Identity And Destination Safety

Application identity uses 1–64 lowercase ASCII letters, digits, and single
internal hyphens, starting with a letter. Rust keywords, Cargo's `target`, and
portable device names such as `con`, `aux`, and `com1` are reserved. This same
identity is shown in the CLI summary and stored in `hegira.toml`; application
crate names remain the template's brand-neutral `app_*` names.

The destination's final directory name accepts 1–64 ASCII letters, digits,
hyphens, and underscores, starting with a letter or digit. Reserved names are
rejected case-insensitively. Its parent must already exist without symlinks.
Absolute destinations and leading `../` for sibling locations are supported;
traversal through a named directory (`child/../application`) is rejected.
Existing files, empty directories, non-empty directories, and dangling symlinks
are all conflicts. There is no overwrite or force mode.

Safe publication uses directory handles and an exclusive atomic rename on
Linux/Android and Apple platforms. Linux is covered by repository validation;
other platforms and filesystems without this primitive fail closed. On systems
where an ancestor is an alias (for example `/tmp` on macOS), use its real path.
Generated files and directories start with owner-only permissions (0600/0700).

## Supported Selections

| Flag | Accepted values | Default |
|---|---|---|
| `--database` | `sqlite`, `postgres` | `sqlite` |
| `--client` | `leptos` | `leptos` |
| `--composition` (`--component` alias) | `identity`, `minimal` | `identity` |

Each invocation selects one database. Identity resolves to `layered-base` and
`layered-leptos-identity` through the bundled package's authenticated
composition graph; that single resolved result controls both rendered files and
the composition recorded in `hegira.toml`. The explicit `minimal` selection
resolves to `layered-base` and `layered-leptos-minimal`. It preserves the
Leptos client, server host, application-owned DDD layers, selected SQLx
provider, configuration, and deployment source without installing an official
module or recording authentication and authorization capabilities. The secure
Identity composition remains the default.

Minimal does not synthesize anonymous or allow-all authorization. Protected
resource generation is unavailable until a compatible authorization-providing
module has been installed. Database selection sets the generated default Cargo
feature and recommended startup profile, not database credentials or
provisioning.

For an explicit PostgreSQL
application, use:

```sh
cargo run --locked -p hegira_cli -- new my-application \
  --destination ../my-application \
  --database postgres \
  --client leptos \
  --composition identity
```

This is an alternative to the SQLite example, not a second command to run
against the same destination.

Select the module-free starting point only when the application is intended to
add its capabilities explicitly:

```sh
cargo run --locked -p hegira_cli -- new my-minimal-application \
  --destination ../my-minimal-application \
  --database sqlite \
  --client leptos \
  --composition minimal
```

## Generated Ownership And `hegira.toml`

The generated directory belongs to your application. Its `apps/` composition
roots, layered `crates/`, configuration, migration composition, and deployment
files are editable application source. Official Identity and framework packages
remain dependencies, not copied module implementations. See the
[architecture ownership contract](architecture.md#canonical-generated-application)
for the layer boundaries.

The generated root `hegira.toml` records generation state:

| Field | Meaning |
|---|---|
| `schema` | Manifest format version, currently `3` |
| `application` | Validated project identity; does not rename the `app_*` crates |
| `framework.repository` | Package-controlled HTTPS framework source |
| `framework.version` | Package-controlled stable SemVer release tag |
| `composition.package` | Package-controlled component-package identity and version |
| `composition.components` | Installed component identities and versions |
| `composition.modules` | Installed official module identities and versions |
| `composition.capabilities` | Capabilities provided by the installed composition |
| `selection.databases` | Selected database adapter |
| `selection.clients` | Selected client adapter |
| `upgrade.framework` | Exact framework repository and release recorded for upgrade planning; must match `framework` |
| `upgrade.package` | Exact component-package identity and release recorded for upgrade planning; must match `composition.package` |
| `upgrade.ownership.default` | Ownership for every unclaimed path; always `application-owned` |
| `upgrade.ownership.claims` | Explicit, non-overlapping source ownership and managed-integration declarations |

Schema 3 distinguishes four source classes:

| Class | Application control and upgrade boundary |
| --- | --- |
| `application-owned` | Product code and every unclaimed path remain under developer control; an upgrade cannot adopt or overwrite them |
| `managed-integration` | An explicit integration identity permits only a declared, digest-preconditioned transition; multiple distinct points may share a file |
| `generated-once` | Scaffolding such as deployment files is written at creation and may then be customized; upgrades cannot overwrite or retire it |
| `immutable-history` | Provider migration history is append-only; upgrades cannot edit or retire historical files |

Claims use canonical relative paths within the application root. Undeclared
paths are never inferred to be framework-managed. A claim is not blanket
permission to rewrite a file: the authenticated edge must also declare the
operation and its source/target digests. The current direct edge manages the
whole contents of `Cargo.toml`, `Cargo.lock`, and `hegira.toml`, so custom edits
to those files block that edge even if they appear outside a framework dependency.

The renderer validates and writes this manifest during creation. Editing it
does not regenerate files, change Cargo dependencies, switch the running
database, or upgrade an application. Keep it consistent with application source.
Runtime settings belong in `config/{APP_ENV}.yaml` and environment overrides;
credentials never belong in `hegira.toml`. See
[Configuration](configuration.md) for the separate runtime contract.
Valid schema-1 and schema-2 manifests remain readable for inspection, but the
current CLI cannot treat them as writable schema-3 manifests by inference.
The bundled v0.6.0-to-v0.7.0 edge supplies an explicit authenticated transition
for its supported released states. Manually changing `schema` or ownership
claims does not establish upgrade compatibility. Reading or validating the
manifest does not access the network or modify application files.

The CLI currently exposes application creation, read-only inspection, upgrade
readiness, dry-run plans and explicit atomic apply, a reviewable bundled-component
addition boundary, complete layered resource generation, and application-owned
migration scaffold generation. Component
addition accepts one bundled component identity, resolves the authenticated
package graph, and uses the shared `--dry-run` and `--json` mutation contract.
An already installed component, an unknown component, or a component without a
bundled additive contribution unit fails without changing the application. The
command never downloads a package, executes component code, or interprets a
remote coordinate. The CLI does not provide component removal, module
management, migration execution or rollback, unreviewed upgrades, remote
component installation, or additional client templates. Optional runtime
providers are configured explicitly in the application; they are not extra
`new` selections.

## Install Identity In A Minimal Application

The default application already contains the Identity composition and must not
run this installation flow. For an explicitly created minimal application,
`inspect` is the read-only composition-status command. From the application
root, inspect and diagnose the starting state with the CLI source matching the
application's recorded framework release:

```sh
cargo run --locked --manifest-path /path/to/hegira/Cargo.toml \
  -p hegira_cli -- inspect
cargo run --locked --manifest-path /path/to/hegira/Cargo.toml \
  -p hegira_cli -- doctor
```

Review the exact content-redacted installation plan before applying it:

```sh
cargo run --locked --manifest-path /path/to/hegira/Cargo.toml \
  -p hegira_cli -- component add identity --dry-run
cargo run --locked --manifest-path /path/to/hegira/Cargo.toml \
  -p hegira_cli -- component add identity
```

Add `--json` to inspection, doctor, dry-run, or apply when automation requires
the corresponding versioned machine-readable contract. Dry-run performs no
application write. Apply publishes the same validated plan atomically; a stale
file, incompatible composition, occupied path, concurrent mutation, or unsafe
recovery state fails instead of being overwritten.

For a compatible minimal Leptos application, `component add identity` previews
or applies the bundled Identity integration for its selected SQLite or
PostgreSQL provider. Review first with `hegira component add identity --dry-run`
from the application root. The apply plan updates `hegira.toml`, Cargo
dependencies, application-owned Identity integration, provider migrations,
Bearer API and cookie-BFF composition, and Leptos routes together. It does not
run migrations or regenerate `Cargo.lock`; after applying, review runtime
configuration, run `cargo generate-lockfile`, apply migrations against the
intended database, and validate the application before deployment. A repeated
add is a conflict and does not modify the application. Identity owns `/` after
installation; the original dashboard remains at `/dashboard`.

The required post-install work is deliberately explicit:

1. Review the Identity and seed settings in the selected `config/{APP_ENV}.yaml`
   profile; keep credentials in environment-backed secret configuration.
2. Run `cargo generate-lockfile` and review the dependency change before
   committing the regenerated application lockfile.
3. Apply the selected provider's application-owned migration plan through the
   application's deployment process. Hegira does not execute migrations for
   `component add`.
4. Run `cargo fmt --all` and the application checks appropriate to the selected
   SQLite or PostgreSQL profile.
5. Run `inspect` again to confirm the resolved Identity component, module, and
   capabilities, then run `doctor` to check integration and local prerequisites.

After Identity is installed, `generate resource` can add a protected resource
to this minimal composition. Its generated page uses the minimal shell's
dashboard navigation and local English labels, rather than the default
application's sidebar and localization files. Preview with `--dry-run --json`
before applying; generation creates source and a provider-specific migration
but does not execute it.

Successful creation and help use stdout; diagnostics use stderr. Exit codes
are `0` (success, including guided cancellation), `1` (internal error),
`2` (usage error), `3` (validation failure), and `4` (destination conflict).

## Inspect An Existing Application

Run inspection from the application root or any directory below it:

```sh
cargo run --locked --manifest-path /path/to/hegira/Cargo.toml \
  -p hegira_cli -- inspect
```

Without `--application-root`, the CLI searches the real working directory and
its ancestors for `hegira.toml`. More than one candidate is ambiguous and
fails with a conflict; the CLI never chooses between nested applications.
Automation can select one root explicitly:

```sh
cargo run --locked --manifest-path /path/to/hegira/Cargo.toml \
  -p hegira_cli -- inspect --application-root /path/to/my-application --json
```

Human output reports the application identity, resolved root, manifest schema,
framework repository and version, installed package, versioned components and
modules, capabilities, database, client, composition status, and mutation
compatibility. Composition is `compatible` when the recorded state resolves
against the bundled canonical graph, `unresolved` with sorted typed diagnostics
when it does not, and `unavailable` for a legacy or unparsed manifest. JSON
output has `output_schema: 2`; its `composition` object exposes the same status,
recorded or resolved composition, adapters, and diagnostics in stable order.
The typed manifest remains available when its schema is understood, and
`mutation_compatibility` remains `compatible`, `incompatible`, or `unsupported`
with the responsible manifest field.

Inspection is read-only. It opens the manifest and required application-owned
`apps/`, `crates/`, and `config/` roots without following symlinks. It does not
read application source, runtime configuration, environment values, user-home
state, or secrets; it exposes no machine-local framework path and does not
write application files.

## Assess Application Upgrade Readiness

From an application root or descendant directory, invoke the source-built CLI:

```sh
cargo run --locked --manifest-path /path/to/hegira/Cargo.toml \
  -p hegira_cli -- upgrade status --json
```

Omit `--json` for human output, or select `--application-root /path/to/application`
explicitly. Discovery uses the same no-follow, ambiguity-rejecting contract as
`inspect`. Assessment authenticates the bundled package, exact source and direct
target release identities, composition, ownership, managed source digests, and
the declared manifest transition. The supported source states are the immutable
v0.6.0 default, minimal, and Identity-added SQLite/PostgreSQL profiles, each with
one direct v0.7.0 target. Compatible current v0.7.0 profiles report `no-upgrade`;
their managed-boundary check is `not-applicable` because there is no outgoing
edge to assess.

Use the target v0.7.0 source tree or source release archive to run these upgrade
commands; the source application's v0.6.0 CLI does not acquire a new edge by
reading its manifest. A prerelease checkout can exercise its bundled source
contract, but does not prove that the target tag is published or resolvable.

| Source composition | Databases | Direct target and preserved composition |
| --- | --- | --- |
| v0.6.0 default Identity | SQLite, PostgreSQL | v0.7.0 with Identity and authentication/authorization retained |
| v0.6.0 minimal | SQLite, PostgreSQL | v0.7.0 without Identity, login, or protected resource generation |
| v0.6.0 minimal plus installed Identity | SQLite, PostgreSQL | v0.7.0 with the installed Identity integrations retained |
| Compatible v0.7.0 composition | SQLite, PostgreSQL | No outgoing upgrade; status/preview succeed, apply is unsupported |

Each supported transition preserves the application identity, selected database
and Leptos client, product customizations outside its managed files, and existing
migration history. Other source versions, altered managed files, arbitrary
component graphs, downgrades, skipped releases, client switches, and database
switches are not supported transitions. There is no arbitrary three-way merge,
dependency solver, or database migration runner in the upgrade command.

The schema-1 JSON report contains `output_schema`, `status`, `source`, `target`,
sorted `composition` components/modules/capabilities/adapters, `recovery`,
`managed_boundaries`, and sorted content-redacted `diagnostics`. Release output
contains framework versions and package identities, not machine-local paths or
arbitrary recorded repository URLs. Human and JSON assessments are written to
stdout, including unsuccessful assessments. Parser usage errors use stderr.

| Status | Exit code | Meaning |
| --- | --- | --- |
| `ready` | 0 | The supported direct transition passed preflight |
| `no-upgrade` | 0 | The current composition has no outgoing bundled upgrade |
| `unsupported` | 3 | The source release or schema has no supported direct edge |
| `incompatible` | 3 | Release source, composition, or manifest transition does not match |
| `invalid-input` | 3 | Application discovery or manifest validation failed |
| `conflict` | 4 | Application source cannot be safely authenticated |
| `recovery-blocked` | 4 | A mutation recovery marker exists |
| `internal-error` | 1 | The bundled package or CLI contract cannot be established |

Malformed command syntax exits 2. Recovery checks only inspect marker presence;
they do not read, remove, or follow the marker. Assessment creates no mutation
plan or publication state and performs no source writes, network requests,
database access, subprocess execution, or runtime configuration/secret reads.
`ready` is an observation, not authorization to mutate: source must be
reauthenticated for a subsequent operation. Review a complete plan with
`upgrade --dry-run`; after review, `upgrade` applies the supported direct edge.

## Preview An Application Upgrade

From an application root or descendant, review the exact supported direct edge:

```sh
cargo run --locked --manifest-path /path/to/hegira/Cargo.toml \
  -p hegira_cli -- upgrade --dry-run --json
```

Omit `--json` for human output. `--application-root` uses the same discovery
contract as readiness. Optional `--target v0.7.0` must exactly match the
authenticated direct edge; skipped releases, downgrades, arbitrary coordinates,
and unprefixed versions are rejected without echoing the supplied value.
Invoking `upgrade` without `status` or `--dry-run` applies the supported edge.
Execution options cannot be combined with `status`.

Schema-1 JSON contains `output_schema`, `mode: "dry-run"`, `outcome`,
`assessment` (the readiness report), nullable `plan`, and `preserved_boundaries`.
The plan is the renderer's schema-1 `UpgradePlanSummary`, not a second CLI plan:
it identifies the edge, exact source/target release identities, authenticated
source package and baseline digests, manifest transitions, framework dependency
names, target components/modules, and ordered file changes. Each change records
its application-relative path, operation, component owner, integration,
managed ownership, absent/exact-digest precondition, and resulting digest or
absence. Create, edit, and retirement use the same typed summary contract.
Human output describes the same operations and conditions without file contents.

`outcome` is `planned` when an authenticated plan exists, `no-upgrade` for a
compatible current release without a requested target, and `unavailable` on a
blocking assessment. Failures never contain an apparently applicable plan.
Assessment exit codes and stdout/stderr behavior match `upgrade status`.
Requesting a target when no direct edge exists is unsupported, not a no-op.

Application-owned, generated-once, and immutable-history boundaries remain
preserved. The current edge edits only `Cargo.lock`, `Cargo.toml`, and
`hegira.toml`. Preview writes no file, cached plan, recovery marker, database,
or application modification timestamp. It does not execute subprocesses,
access runtime secrets, or contact a network. A preview is not a reusable
authorization token; later publication must enforce the same in-memory plan's
digest preconditions.

## Apply An Application Upgrade

Back up the application and database, stop competing source mutations, and
review `upgrade --dry-run` before invoking:

```sh
cargo run --locked --manifest-path /path/to/hegira/Cargo.toml \
  -p hegira_cli -- upgrade --target v0.7.0 --json
```

`--target` is optional but, when supplied, must exactly match the authenticated
direct edge. `--application-root` and root/descendant discovery behave as in
dry-run. Apply recomputes the same typed in-memory plan from authenticated
source. The existing publisher serializes mutation through a private recovery
marker and validates every digest precondition before the first publication
and again at each mutation boundary. Publication is directory-anchored,
no-follow, privately staged, and rolled back on recoverable failures under
the documented filesystem contract; it is not a distributed transaction.

Successful schema-1 output uses `mode: "apply"`, `outcome: "applied"`, and the
exact plan summary used by dry-run. `assessment` records the pre-publication
source state. A content-redacted `receipt` contains the changed-file count,
edge, and resulting release identity. `next_steps` contains application-owner
operations; human output reports the same receipt and steps. Failures emit
`outcome: "unavailable"` without a plan or success receipt. Apply to an already
current application exits 3 with no applicable direct edge and performs no
write, rather than reporting a second upgrade. Preview still reports
`no-upgrade` successfully for that state.

Concurrent/recovery conflicts exit 4; publication errors and uncertain rollback
exit 1. An interrupted or uncertain mutation retains recovery information and
blocks later mutation. Preserve the marker and staging files; inspect the
application and restore a verified consistent state before retrying. Never
delete a marker merely to make the next command proceed. If output delivery
fails after publication, inspect source and recovery state before assuming the
upgrade failed or retrying.

Hegira owns only the authenticated managed transitions. Apply does not regenerate
the lockfile, execute database migrations, start services, access runtime secrets,
or run network/subprocess operations. Product source and immutable history remain
untouched. A receipt is not evidence that the application is ready to deploy.

## After An Application Upgrade

The application's owner, whether working directly or with an agent, completes
these steps before starting or deploying the upgraded application:

1. Review the plan, receipt, and source diff. For the bundled edge, only
   `Cargo.toml`, `Cargo.lock`, and `hegira.toml` change. Confirm the exact target,
   application identity, installed composition, and unchanged provider/client.
2. Review the lockfile alongside the manifest. Upgrade publishes authenticated
   lockfile bytes; it does not resolve registry or Git dependencies. Keep the
   reviewed release-pinned lock and use locked builds. Do not blindly run
   `cargo update` or regenerate the lockfile to hide a conflict. If the
   application needs a different dependency graph, make that an explicit,
   separately reviewed application change and validate its resulting lock.
3. With backups and the correct environment, review pending migrations and use
   the application's documented operation workflow. Source upgrade does not
   execute SQL. Migration history remains append-only; repeating an unchanged
   applied migration must not require editing its original bytes or checksum.
   See [Operations](operations.md) for migration and recovery responsibilities.
4. Run the application's tests, selected-provider native and WASM hydration
   checks, and deployment validation. Recheck startup capability/production
   validation and relevant authentication, authorization, health, and HTTP
   policies. A compatible manifest or doctor result does not replace these checks.
5. Deploy only after those checks succeed. Source publication rollback does not
   restore a database or roll back a running deployment; use the application's
   backup and deployment recovery procedure for those operations.

From a version-controlled application root, these read-only review commands help
inspect the source result without executing SQL or changing dependencies:

```sh
git status --short
git diff -- Cargo.toml Cargo.lock hegira.toml
```

Use the selected application's documented build and deployment commands, not
framework-repository validation scripts against a live application database.
Production migrations, credentials, backups, and rollout authorization remain
application-owned.

### Frontend Dependency Remediation

Source upgrade preserves application-owned `apps/web/src/package.json` and
`package-lock.json`; it does not install the canonical template's updated
frontend dependencies. Existing applications must review their own npm graph
separately, including applications upgraded from v0.6.0.

The canonical frontend keeps Tailwind CLI 4.3.3 and uses this narrowly scoped
override while that CLI pins an older watcher:

```json
{
  "overrides": {
    "@tailwindcss/cli": {
      "@parcel/watcher": "2.6.0"
    }
  }
}
```

Watcher 2.6.0 removes the `micromatch`/`braces` dependency chain affected by
[GHSA-vfj7-8cjw-p6xm](https://github.com/advisories/GHSA-vfj7-8cjw-p6xm).
Merge this override with existing application overrides rather than replacing
the whole manifest. If the application uses different frontend tooling, review
its supported dependency graph instead of applying this configuration blindly.

From the application root, after reviewing that manifest change:

```sh
npm install --package-lock-only --ignore-scripts --prefix apps/web/src
git diff -- apps/web/src/package.json apps/web/src/package-lock.json
npm ci --prefix apps/web/src
npm audit --audit-level=high --include=dev --include=optional --include=peer \
  --prefix apps/web/src
```

Review the changed lockfile and installation scripts, then validate CSS output,
the development watcher, hydration, and the application build before deployment.
Do not use `npm audit fix --force`, omit build dependencies from audit, or remove
an audit finding merely to complete source upgrade. These are explicit
application-owner dependency changes, not extra managed upgrade operations.

## Resolve Upgrade Conflicts And Recovery

| Outcome | Safe next action |
| --- | --- |
| Unsupported source/target or incompatible composition (exit 3) | Check the release tree, exact edge and recorded composition; do not fabricate a manifest version or ownership claim |
| Managed-source conflict (exit 4) | Preserve custom work, compare the three managed files with the matching released state, and decide whether to retain the customization outside this supported transition |
| Recovery marker present (exit 4) | Stop competing mutations, preserve the marker and staged files, and establish a consistent source state before authorizing another mutation |
| Publication failure or uncertain rollback (exit 1) | Inspect source and recovery information; no success receipt was issued, but do not assume nothing changed |
| Output delivery failure (exit 1) | Publication may have succeeded; inspect actual files and recovery state before deciding whether to retry |

There is no force-upgrade, automatic conflict merge, recovery-cleanup, or
downgrade command. Do not change edge digests, copy target files over a customized
source, delete recovery state, or reclassify application-owned files just to
obtain a ready result. A conflict means the bundled transition cannot preserve
the observed custom state under its authenticated contract.

For an interrupted or uncertain mutation:

1. Stop processes that might mutate the application's source. Preserve a private
   copy of the application, `.hegira-mutation.lock`, and any remaining private
   transaction files before making a recovery decision. Do not publish their
   contents or credentials in an issue or log.
2. Compare affected paths with the reviewed pre-upgrade backup/version-control
   state and expected plan digests. Determine whether they form the complete
   source state, complete target state, or an incomplete publication. A marker
   alone is not proof that a process has stopped or that rollback finished.
3. Have the application owner approve manual recovery to one verified consistent
   state, preserving product changes and migration history. Resolve retained
   transaction/marker state only after that recovery is verified and no publisher
   is active. The CLI does not provide a universal filesystem repair procedure.
4. Re-run read-only `upgrade status --json` and `doctor`, review a fresh dry-run
   where a direct edge still applies, and repeat application validation before
   any apply or deployment. An already upgraded application has no remaining
   direct edge; repeated apply exits 3 without writes.

## Upgrade Automation Contract

The committed Draft 2020-12 schemas are
[readiness v1](../tools/hegira_cli/schemas/upgrade-status-v1.schema.json) and
[execution v1](../tools/hegira_cli/schemas/upgrade-execution-v1.schema.json).
Execution includes the typed plan, applied receipt, and failure shapes; its
readiness reference resolves locally. Validation requires no remote schema
lookup. Unknown fields, versions, diagnostic codes, and malformed digests are
rejected by the current closed schemas. Applied outcomes require a receipt and
owner steps; failed outcomes cannot contain an applicable plan or receipt.

Use `output_schema`, `status`, `outcome`, and `diagnostics[].code`, not prose,
for automation. Assessments and execution results, including failures, go to
stdout as one JSON object with `--json`; stderr is empty for those outcomes.
Command usage errors exit 2 with empty stdout and diagnostics on stderr.
Output-delivery failures exit 1 and cannot guarantee a complete JSON result;
inspect application state before retrying an apply.

| Exit | Stable diagnostic codes |
| --- | --- |
| 0 | No blocker: ready, planned, applied, or no-upgrade preview/status |
| 1 | `compatibility-policy`, `package-authentication`, `package-contract`, `receipt-mismatch`, `publication-plan`, `publication-failed`, `recovery-uncertain` |
| 3 | `manifest-schema`, `manifest-composition`, `framework-source`, `composition-adapters`, `current-manifest`, `composition`, `direct-edge`, `manifest-transition`, `manifest`, `target`, `publication-platform` |
| 4 | `manifest-changed`, `recovery-pending`, `recovery-inspection`, `ownership`, `managed-source-digest`, `managed-source-missing`, `managed-source-occupied`, `unsafe-source`, `source-limit`, `application-changed`, `publication-precondition` |
| 3 or 4 | `application-context` (invalid input versus unsafe/conflicting discovery); `upgrade-plan` (unsupported/incompatible versus blocked/conflicting planning) |

Diagnostics are sorted by code. Components and modules are sorted by identity;
capabilities and adapters use stable typed order. Plans use application-relative
path order. Filesystem creation order and declaration order do not determine
output order. Digests identify content; source bytes, runtime values, staging
paths, and machine-local roots are never output.

Upgrade commands do not prompt or consume interactive answers. Closed stdin or
supplied stdin produces the same command contract. Explicit flags, not stdin,
select the root, target, and dry-run/apply mode. Human output is a reviewed
presentation of the same state and plan, not the automation API.

## Diagnose An Existing Application

From the application root, run the source-built CLI's read-only doctor:

```sh
cargo run --locked --manifest-path /path/to/hegira/Cargo.toml \
  -p hegira_cli -- doctor
```

Use `--application-root /path/to/my-application` for an explicit root and
`--json` for a versioned `output_schema: 1` report. Doctor checks manifest and
component compatibility, the mutation recovery marker, selected Identity
route/transport/migration integration points, the selected database's runtime
requirements, and local Rust, WASM, cargo-leptos, Node.js, and npm prerequisites.
It reports `PASS`, `WARN`, and `FAIL` in fixed order. Missing development tools
and an unprobed PostgreSQL service are warnings; invalid composition, unsafe
integration state, or a recovery marker is a validation failure (exit code `3`).
Warnings alone return exit code `0`.

Doctor neither changes application files nor connects to a database or external
service. It reads only bounded, symlink-safe application integration sources and
reports no source bodies, runtime configuration, environment values, credentials,
or machine-local paths. Integration-reference checks are diagnostics, not proof
that the compiled HTTP policy is secure. It does not repair a failed check;
review the reported action before attempting another mutation.
The local `rustup` target probe receives only tool-discovery and Rustup-specific
environment settings, not application runtime secrets.

## Compatibility And Mutation Safety

`inspect` can report an incompatible or unsupported application successfully,
but `component add`, `generate resource`, and `generate migration` fail with
conflict exit code `4` unless the application is compatible with the running
CLI. The current mutation policy requires:

- manifest schema `3`, valid source-ownership state, and the canonical Hegira
  framework repository;
- the exact framework release version compiled into the CLI;
- the canonical component package and supported installed component, module,
  and capability composition generated by
  the current application template;
- exactly one supported database (`sqlite` or `postgres`); and
- exactly one supported client (`leptos`).

This exact-version rule prevents a source-built CLI from silently editing an
application generated for another framework release. Use the Hegira source or
release archive matching `framework.version` in the application's
`hegira.toml`.

Every mutation command constructs and validates one ordered change plan.
`--dry-run` reports that same plan without opening the application for writes;
apply rechecks absent-file and SHA-256 content preconditions immediately before
publication. Human and JSON summaries contain paths, operations, and digests,
not generated source, existing file contents, credentials, or runtime values.
An existing generated file, stale source edit, occupied namespace, malformed
managed integration block, concurrent mutation, or symlinked path fails rather
than being overwritten or adopted.

Publication uses the application-owned `.hegira-mutation.lock` recovery marker
and private staging files. Recoverable failures are rolled back. If a crash,
concurrent file change, incomplete rollback, or uncertain cleanup leaves the
marker in place, later mutations stop with a conflict. Inspect the marker,
transaction files, and affected application paths against version-control
state before manual recovery; do not delete the marker blindly. Platforms or
filesystems without the required no-follow and atomic rename behavior fail
before application files are changed.

## Generate A Layered Resource

Run the source-built CLI from a generated application and provide each field
as `name:type`; append `?` to the type for a nullable field:

```sh
cargo run --locked --manifest-path /path/to/hegira/Cargo.toml \
  -p hegira_cli -- generate resource Order \
  --field name:string --field fulfilled_at:datetime?
```

The supported scalar set is `string`, `bool`, `i64`, `uuid`, and `datetime`.
Use `--plural <NAME>` for an irregular UpperCamelCase plural. The command reads
the database, client, components, and capabilities from `hegira.toml`. It
requires both authentication and authorization before reading integration
sources or constructing a change plan. A minimal application without those
capabilities fails with exit code `3`, leaves its files unchanged, and points
to `hegira component add identity`; with `--json`, the deterministic
`missing-capabilities` diagnostic is written to stderr. The command validates all names
and integration points, and publishes one atomic plan covering Domain,
Application Contracts, Application, SQLx persistence and migration, Axum and
OpenAPI, and the selected Leptos client. `--dry-run` previews the identical
content-redacted plan; `--json` emits its deterministic machine form.

For a resource such as `Order`, the command creates an application-owned
`order.rs` in each selected layer and edits only declared managed integration
points:

| Owner | Generated responsibility |
|---|---|
| `crates/domain` | UUID entity skeleton |
| `crates/application_contracts` | Commands, queries, responses, permissions, and service contract |
| `crates/application` | Repository, authorization, and identifier ports plus use-case service |
| `crates/infrastructure` | Selected SQLx repository, typed composition, and provider migration |
| `crates/presentation` | Bearer HTTP handlers and OpenAPI contribution |
| `apps/server` and `apps/web` | Explicit server composition, Leptos pages, server functions, routes, navigation, and localization |

All of this generated source belongs to the application and may be extended.
Framework primitives and official Identity packages remain pinned dependencies;
their implementations are not copied into the generated application.

The generated application service authorizes every use case before repository
access. Bearer API routes remain separate from cookie-authenticated browser/BFF
routes. Leptos permission gates improve navigation and page behavior only;
client-side or UI visibility checks are never an authorization boundary.

Generation does not infer business invariants or relationships and does not
run formatters, builds, application processes, or migrations. Review the
generated aggregate shape, validation, authorization, transaction needs,
relationships, API behavior, and user experience. Complete and apply the
forward migration, then run the formatting and application checks reported by
the command. The generator automates repeatable integration work; these product
and business decisions remain developer-owned.

## Generate An Application Migration

Run the source-built CLI from a generated application to create a migration
for the database adapter selected in its `hegira.toml`:

```sh
cargo run --locked --manifest-path /path/to/hegira/Cargo.toml \
  -p hegira_cli -- generate migration add_orders
```

Alternatively, pass `--application-root <path>` when the working directory is
outside the application. Migration identities use 1–64 bytes of lowercase
ASCII snake_case. The command rejects reserved identities, an identity already
present in the selected provider history, malformed histories, and unsafe or
incompatible application roots before publication.

The generated SQL file is an intentionally empty, provider-labelled scaffold
under `crates/infrastructure/migrations/sqlite/` or
`crates/infrastructure/migrations/postgres/`. Add the forward-only SQL required
by the application after generation. The command never connects to a database,
executes or reverts migrations, translates SQL, or changes an existing
migration.

`--dry-run` reports the exact typed plan used by apply without writing the
application. `--json` emits its deterministic, versioned, content-redacted
machine representation. Alongside the SQL file, the command maintains
`crates/infrastructure/migrations/.hegira-generator.toml`. This application-
owned coordination record reserves the next migration version through the
same failure-safe mutation transaction; it contains no runtime configuration,
credentials, or SQL content. Concurrent or repeated stale plans fail as
conflicts instead of silently replacing migration history.

## Run With SQLite

The generated `Cargo.lock` is part of the release-verified application source.
Keep it in version control and use explicit `cargo update` operations when the
application intentionally adopts a different dependency graph.

```sh
cd ../my-application
npm ci --prefix apps/web/src
APP_ENV=sqlite cargo leptos watch -p app_server \
  --bin-features ssr,db-sqlite --lib-features hydrate \
  --bin-cargo-args=--locked --lib-cargo-args=--locked
```

Open `http://127.0.0.1:3000`. The SQLite development profile creates its local
database, runs the application-owned migration plan, and applies configured
Identity seed behavior at startup. Review `config/sqlite.yaml` before using the
application in a shared environment.

## Run With PostgreSQL

From the generated application's root, install its frontend dependencies with
`npm ci --prefix apps/web/src` if not already done. Start a disposable local
database:

```sh
POSTGRES_PASSWORD=local-development-only docker compose up -d database
APP_ENV=development \
APP__DATABASE__URL=postgres://postgres:local-development-only@localhost:5432/application \
cargo leptos watch -p app_server \
  --bin-features ssr,db-postgres --lib-features hydrate \
  --bin-cargo-args=--locked --lib-cargo-args=--locked
```

The development profile may run migrations and seed data automatically.
Production intentionally disables both behaviors; deployment automation must
execute the application-owned migration plan before rollout.

## Validate Framework Source

Run framework-repository checks from the Hegira repository root:

```sh
sh scripts/repository-policy.sh
sh scripts/backend-check.sh
```

The complete generated application contract requires Docker and uses only
disposable state:

```sh
sh scripts/generated-application-check.sh
# Separate released-application upgrade lifecycle (all six profiles):
sh scripts/upgraded-application-check.sh
```

`scripts/cli-check.sh` owns focused inspection, compatibility, upgrade
readiness/dry-run/apply, schemas, process outcomes, conflict, recovery, and
generator command contracts. The generated-application
gate creates both provider profiles through the public CLI, applies a generated
resource only to disposable validation copies, and exercises its migration,
authorization, HTTP, UI, and production-container behavior.
The upgraded-application gate additionally checks all three released compositions
with both providers, customized product source, immutable history/data,
post-upgrade migrations, and production behavior. CI and release validation
require both creation cells and all three upgrade composition cells.

Validation owns bounded caches below `target/validation/`, not normal developer
Cargo output. Use `sh scripts/clean-validation-cache.sh --status` to inspect
usage and `--prune-dry-run` to preview budget reclamation. See
[Maintainer cache ownership](maintainers.md#validation-build-cache-lifecycle) for
the 64 GiB default budget, active-lock protection, pruning, and cleanup boundaries.

See [Architecture](architecture.md), [Configuration](configuration.md), and
[Deployment](deployment.md) before changing providers or production defaults.
