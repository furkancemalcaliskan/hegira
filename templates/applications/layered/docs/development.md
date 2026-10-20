# Developing {{application_name}}

Run all commands from this application workspace root. The selected provider is
`{{database_adapter}}`; development uses `config/{{development_profile}}.yaml`
under [config](../config) and native features `ssr,{{database_feature}}`.
Hydration uses `hydrate` with `wasm32-unknown-unknown`. The same provider commands
apply after adding Identity.

## CLI and prerequisites

Use the compatible complete Hegira source tree used for generation, with its
pinned toolchain. Hegira is distributed as source; the application workspace does
not contain `hegira_cli`, and copying its binary alone is not an installation.
Keep the compiled CLI's source tree available at its original location. For an
unreleased generation checkout, the recorded release identity alone does not
prove these commands are already available in that public release.

Replace the placeholder with the reviewed source location, then define this
shell helper for the examples below:

```sh
export HEGIRA_SOURCE=/absolute/path/to/compatible/hegira
hegira() {
  cargo run --locked --manifest-path "$HEGIRA_SOURCE/Cargo.toml" -p hegira_cli -- "$@"
}
hegira --help
hegira inspect
```

Install the Rust version pinned by [rust-toolchain.toml](../rust-toolchain.toml),
its WASM target, Cargo Leptos 0.3.7, Node.js 22+ (see [.node-version](../.node-version)),
and npm 10+. Keep [Cargo.lock](../Cargo.lock) and the
[frontend lock](../apps/web/src/package-lock.json) under review. Explicit setup:

```sh
rustup target add wasm32-unknown-unknown
cargo install --locked cargo-leptos --version 0.3.7
tool_bin=$(sh scripts/prepare-wasm-bindgen.sh install Cargo.lock target/hegira-tools/wasm-bindgen/bin)
export PATH="$tool_bin:$PATH"
npm ci --prefix apps/web/src
```

The [wasm-bindgen helper](../scripts/prepare-wasm-bindgen.sh) prepares the exact
Cargo-lock version. Release builds also require separately installed Binaryen
`wasm-opt` 123 from a reviewed distribution. The CLI never installs prerequisites.
SQLite needs no service; PostgreSQL requires an explicitly chosen development
server and database. Configure its connection privately, without committing or
putting credentials in command arguments. Generation provisions neither target.
Review [.env.example](../.env.example) and the selected configuration profile.
Keep required connection, signing, and enabled Identity seed secrets in private
environment settings. Adapt the example to the selected provider; its PostgreSQL
URL override must not be inherited when running SQLite.

## Review and diagnose

```sh
hegira doctor --operation check
hegira check --dry-run
hegira test --dry-run
hegira dev --dry-run
hegira build --release --dry-run
hegira db status --profile {{development_profile}} --dry-run
hegira db migrate --profile {{development_profile}} --dry-run
```

Previews inspect composition without loading runtime configuration, probing tools,
building, or connecting to a database. Doctor reads operation prerequisites;
native version probes additionally require `--probe-tools` and trusted tool
selections. Doctor never executes application code or database operations, and
warnings or a zero exit do not authorize execution. Use `--json` for versioned
reports. Use `--application-root /absolute/path/to/application` when invoking the
CLI outside this workspace.

## Trusted Linux execution

Review application code, dependencies, build scripts, tests, Cargo configuration,
tools, and inherited environment first. Execution is not a sandbox. Replace the
following paths with reviewed absolute locations outside this application:

```sh
export APP_CARGO=/absolute/trusted/bin/cargo
export APP_TOOL_DIRECTORY=/absolute/trusted/bin
export APP_WASM_OPT=/absolute/trusted/bin/wasm-opt
export APP_WASM_BINDGEN="$PWD/target/hegira-tools/wasm-bindgen/bin/wasm-bindgen"
hegira check --execute --trust-application --cargo "$APP_CARGO" \
  --tool-directory "$APP_TOOL_DIRECTORY" --tool-directory /usr/bin
hegira test --execute --trust-application --cargo "$APP_CARGO" \
  --tool-directory "$APP_TOOL_DIRECTORY" --tool-directory /usr/bin
```

Repeat `--tool-directory` for required native tools and the linker. Dev/build
require pinned Rust, the WASM target, Cargo Leptos, and Node in trusted directories.
The explicitly selected lock-matched wasm-bindgen may reside in the prepared
application tools directory. A general application directory or node_modules bin
directory must not be selected as a trusted tool directory.

Check compiles native workspace targets; test runs native workspace tests. Both
then check WASM hydration, preserve locked dependencies, and select the manifest's
provider without default features. Browser tests and ignored database tests are
not run. Tests execute trusted code and can have their own side effects; a test
command does not grant reset or production database authority. Keep any manually
selected integration targets isolated and explicitly disposable.

After reviewing the intended development target, runtime configuration, and
inherited `DATABASE_URL`/`APP__DATABASE__URL` overrides, start foreground watch:

```sh
hegira dev --execute --trust-application --cargo "$APP_CARGO" \
  --tool-directory "$APP_TOOL_DIRECTORY" --tool-directory /usr/bin \
  --wasm-bindgen "$APP_WASM_BINDGEN"
```

Dev selects `APP_ENV={{development_profile}}`, `{{database_feature}}`, localhost
`127.0.0.1:3000`, and reload port 3001. Visit `http://127.0.0.1:3000` after startup.
Trusted startup can open/create a configured database, migrate, and initialize
selected providers; installed Identity can seed when enabled. A development
profile alone does not prove the URL is disposable. Ctrl-C/SIGTERM stop and reap
owned child groups. Pending recovery blocks execution; preserve recovery files.

## Release bundle

Build with a reviewed environment containing no production credentials:

```sh
hegira build --release --execute --trust-application --cargo "$APP_CARGO" \
  --tool-directory "$APP_TOOL_DIRECTORY" --tool-directory /usr/bin \
  --wasm-bindgen "$APP_WASM_BINDGEN" --wasm-opt "$APP_WASM_OPT"
```

This builds the selected provider without starting HTTP or selecting a production
runtime profile. A successful report verifies the server at
`target/hegira/release-build/release/app_server`, the site at
`target/hegira/release-build/site`, and WASM at
`target/hegira/release-build/site/pkg/app_bg.wasm`. Deployment approval remains
separate. Failed/interrupted builds may leave partial output.

The CLI claims only `target/hegira/release-build`; an existing unclaimed output,
symlink, or external hard link fails closed. Never fabricate its
`.hegira-release-build.json` ownership marker. Keep unrelated data outside that
root. Ordinary developer output (`target/debug`, `target/release`, `target/site`),
prepared tools, global Cargo/Git caches, and Docker storage are separate. Current
ordinary application output has no automatic cleanup or disk-budget guarantee.
Explicit `--validation-cache` on check/test/build instead selects disposable
`target/hegira/validation/{check,test,build}` output with a configurable
65536 MiB default allocated-byte budget, pre/post enforcement and inactive LRU
reclamation. It is not a live quota. `hegira artifacts cache-status` inspects that
state; `hegira artifacts prune --dry-run` previews reclamation before any explicit
`--execute`. Never forge claims. Validation build receipts refer to disposable
outputs; use ordinary release builds when retaining the original bundle location.

## Application quality CI

The generated [quality workflow](../.github/workflows/quality.yml) runs the
application's selected provider/composition, without the framework's matrix.
Its `source` job checks format, inward dependency boundaries and CI contract
fixtures. `native` runs locked native checks/tests and WASM hydration through
the public CLI; ignored database tests remain excluded. `release` builds the
locked bundle and verifies migration startup, readiness, health and SSR against
new disposable `{{database_adapter}}` state. `supply-chain` checks Cargo audit,
licenses/sources/bans, and every frontend dependency class at high severity.
The unconditional `quality` job fails if any constituent fails, is skipped or
is cancelled. There are no path filters, publication steps or production secrets.

[Tool setup](../.github/actions/application-tools/action.yml) retains a complete
source CLI checkout beside the application, pinned to the reviewed #391 merge
`b869b3ae3b60da7052ca91c500547081f9fc9f54`. This operational tool pin is separate
from the application's release-pinned framework dependencies; it does not
pretend that unreleased CLI commands exist in the public v0.7.0 CLI. Review and
align the pin explicitly when moving to a new framework/package release.
Action sources are immutable commits; Rust is 1.96.1, Cargo Leptos 0.3.7,
Cargo Audit 0.22.2, Cargo Deny 0.20.2 and Binaryen 123. Binaryen's archive and
the lock-matched wasm-bindgen installer authenticate SHA-256 before use.
Node uses the selected major from [.node-version](../.node-version).

CI selects an 8192 MiB application validation budget. Check output is pruned
before the test graph grows; the release graph stays until disposable boot
finishes. Cleanup invokes only the CLI's reauthenticated owned-cache prune,
including after failures. No remote application Cargo cache is restored or
published, so there is no cache marker adoption. CLI bootstrap/tool installs,
registry/Git/npm caches, and Docker are separate job-owned ephemeral state,
outside the application budget. Adjust budgets/timeouts for reviewed runner
capacity; pre/post enforcement cannot prevent a transient disk-full failure.
Ordinary developer/watch output remains outside cleanup authority.

For reviewed local reproduction on Linux x64, provide an absolute
`RUNNER_TEMP` directory outside the application, the exact reviewed
`HEGIRA_SOURCE` checkout, and the pinned prerequisites from the workflow.
`sh scripts/ci-tools.sh cli` builds the source CLI outside the application;
`sh scripts/ci-tools.sh frontend` explicitly installs browser/release tools
and runs `npm ci`. Then select one phase with `sh scripts/ci.sh source`,
`check`, `test`, `build`, `security`, or `cleanup`. These commands execute trusted
code, not a sandbox, and verify both committed dependency locks even on failure.
Run no CI phase with production secrets or inherited service credentials.

`APPLICATION_CI_DISPOSABLE=true sh scripts/ci.sh integration` separately
authorizes a fresh local boot. It accepts no database URL. SQLite creates only
a private temporary file; PostgreSQL creates only one digest-pinned container
with tmpfs data, a random password and random localhost port. It overrides
inherited database targets, disables provisioning/Identity seeding/workers,
and permits only migrations of the new target. Server output and database
credentials are not printed. The owned server/container and private files are
removed on normal completion/failure/signals; SIGKILL or runner loss relies on
the disposable runner lifecycle. This is neither a production boot/deployment
approval nor authority to reset another target.

The generated [audit exception](../.cargo/audit.toml) covers only SQLx's dormant
MySQL/RSA lock entry. Before auditing, the security phase must prove
`rsa@0.9.10` is unreachable with all application features and targets; failure
or a nonempty graph blocks CI. Remove the exception and review policy before
enabling that dependency. The [deny policy](../deny.toml) retains informational
unsoundness bans and rejects unknown sources. These generated-once policies,
workflows and scripts belong to the application owner; installation and source
upgrades preserve customizations. See [repository setup](repository.md).

## Database operations

Review the actual target, inherited URL overrides, permissions, and backups
before execution. After previewing the selected operation:

```sh
hegira db status --profile {{development_profile}} --execute --trust-application \
  --cargo "$APP_CARGO" --tool-directory "$APP_TOOL_DIRECTORY" --tool-directory /usr/bin
hegira db migrate --profile {{development_profile}} --execute --trust-application \
  --cargo "$APP_CARGO" --tool-directory "$APP_TOOL_DIRECTORY" --tool-directory /usr/bin
```

Both delegate locked Cargo execution to the separately selected
[app_database](../apps/server/src/bin/app_database.rs) entry point using
`database-operations,{{database_feature}}`. Status preserves data, schema, and
history and creates neither a missing database nor metadata; SQLite WAL
coordination can affect sidecars. Migrate applies the composed forward plan only
to an existing target. Neither operation provisions, resets, rolls back, seeds,
starts HTTP/workers, or runs arbitrary SQL. Compilation can still write output.
Failures are non-success; earlier committed migrations can remain after a later
failure. Inspect actual history and data before retrying.

Profiles must match the selected provider: `sqlite` is SQLite; `development`
and `production` are PostgreSQL. The `test` profile uses PostgreSQL in the initial
default Identity composition, and SQLite in minimal/Identity-added applications.
Review [config/test.yaml](../config/test.yaml) before selecting it. PostgreSQL
production migration requires independent `--approve-production-migration`
in addition to execution trust; that flag is invalid for status, previews, or
non-production migration. Never infer production approval from source upgrades.

Human dev/check/test/build logs are raw; their JSON mode discards child logs.
Database execution suppresses raw logs in both modes and accepts only a bounded
validated result. CLI exits are 0 success, 1 internal/child failure, 2 usage,
3 validation, and 4 conflict. A path or partial output is not a success receipt.

See [architecture](architecture.md) for use-case/security boundaries and
[ownership and recovery](ownership.md) before source changes or upgrades.
