#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
template_root="$repo_root/templates/applications/layered"
. "$repo_root/scripts/validation-cache.sh"
generated_tool_bin=$(sh "$repo_root/scripts/generated-toolchain.sh" prepare \
  templates/applications/layered/Cargo.lock)
PATH="$generated_tool_bin:$PATH"
export PATH
validation_cache_prepare "$repo_root" "layered-template-check"
staging_parent="$HEGIRA_VALIDATION_WORKSPACE"
staging_root="$staging_parent/application"
export CARGO_TARGET_DIR="$HEGIRA_VALIDATION_TARGET"

cleanup() {
  status=$?
  trap - EXIT INT TERM
  validation_cache_release || status=1
  exit "$status"
}
trap cleanup EXIT INT TERM

if cargo metadata --locked --no-deps --format-version 1 |
  grep -F "\"$template_root/" >/dev/null; then
  echo "canonical template must not be a member of the framework workspace" >&2
  exit 1
fi

cargo fmt --all -- --check
sh "$repo_root/scripts/dx-audit.sh"
echo "==> Renderer package authentication, upgrade graph, planner, and preservation contracts"
cargo test --locked -p template_renderer
cargo run --locked --quiet -p template_renderer \
  --example repository_validation_renderer -- render \
  --repository-root "$repo_root" \
  --template layered \
  --output "$staging_root" \
  --framework-root "$repo_root"

generated_tool_bin=$(sh "$repo_root/scripts/generated-toolchain.sh" application \
  "$staging_root")
PATH="$generated_tool_bin:$PATH"
export PATH

if find "$staging_root" -name Cargo.toml -exec grep -nE 'git[[:space:]]*=[[:space:]]*"https://github.com/furkancemalcaliskan/hegira.git"' {} + |
  grep . >/dev/null; then
  echo "repository validation render contains an unpatched framework dependency" >&2
  exit 1
fi

(
  cd "$staging_root"
  npm ci --prefix apps/web/src
  node "$repo_root/scripts/frontend-watcher-smoke.mjs" "$staging_root/apps/web/src"
  PATH="$staging_root/apps/web/src/node_modules/.bin:$PATH"
  export PATH
  test -f Cargo.lock
  cargo check --locked --workspace --all-targets --all-features
  node "$repo_root/scripts/architecture-boundaries.mjs" \
    check-generated --root "$staging_root"
  cargo check --locked -p app_server --no-default-features --features hydrate \
    --target wasm32-unknown-unknown
  cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
  cargo test --locked --workspace --all-features
  cargo leptos build -p app_server --release \
    --bin-features ssr,db-postgres --lib-features hydrate \
    --bin-cargo-args=--locked --lib-cargo-args=--locked
)

cargo build --locked -p hegira_cli --bin hegira

echo "==> Isolated application database entry-point matrix"
for composition in default minimal identity-added; do
  for database in sqlite postgres; do
    echo "==> Database entry point: $composition/$database"
    source_parent="$staging_parent/database-operation-sources"
    mkdir -p "$source_parent"
    source="$source_parent/$composition-$database"
    if [ "$composition" = default ]; then
      cargo run --locked --quiet -p hegira_cli -- new database-application \
        --destination "$source" --database "$database"
    else
      cargo run --locked --quiet -p hegira_cli -- new database-application \
        --destination "$source" --database "$database" --composition minimal
    fi
    source_flag=--generated-source
    if [ "$composition" = default ]; then
      set --
    else
      set -- --component layered-base --component layered-leptos-minimal
    fi
    if [ "$composition" = identity-added ]; then
      cargo run --locked --quiet -p hegira_cli -- component add identity \
        --application-root "$source"
      source_flag=--identity-added-source
    fi
    # Only this freshly generated, repository-owned disposable staging root.
    operation_root="$staging_parent/database-operation-application"
    rm -rf "$operation_root"
    cargo run --locked --quiet -p template_renderer \
      --example repository_validation_renderer -- render \
      --repository-root "$repo_root" --template layered \
      --output "$operation_root" --framework-root "$repo_root" \
      "$source_flag" "$source" "$@" --set application_name=database-application \
      --set database_adapter="$database" --set database_feature="db-$database"
    (
      cd "$operation_root"
      cargo generate-lockfile
      rustfmt --edition 2024 --check apps/server/src/bin/app_database.rs \
        apps/server/tests/database_operations.rs \
        crates/infrastructure/src/database_operations.rs
      cargo test --locked -p app_server --no-default-features \
        --features "database-operations,db-$database" \
        --bin app_database --test database_operations
      cargo test --locked -p app_infrastructure --no-default-features \
        --features "db-$database" --lib database_operations::
      cargo clippy --locked -p app_server --all-targets --no-default-features \
        --features "database-operations,db-$database" -- -D warnings
      node "$repo_root/scripts/architecture-boundaries.mjs" \
        check-generated --root "$operation_root"
    )
  done
done

echo "==> Public CLI released-application upgrade matrix and preservation"
cargo run --locked --quiet -p template_renderer --example upgrade_preservation -- \
  "$repo_root" "$staging_parent/upgrade-preservation" "$CARGO_TARGET_DIR/debug/hegira"
for composition in default minimal identity-added; do
  for database in sqlite postgres; do
    echo "==> Customized upgrade compilation: $composition/$database"
    # These workspaces intentionally share package names. Use one stable source
    # path, copied after the preceding build, so Cargo observes changed local
    # source instead of reusing artifacts from another fixture tree.
    compile_root="$staging_parent/upgrade-preservation/compile-application"
    rm -rf "$compile_root"
    cp -R "$staging_parent/upgrade-preservation/$composition-$database" "$compile_root"
    (
      cd "$compile_root"
      # Resolve only the separate local-source compile copy, then require its lock.
      cargo generate-lockfile
      cargo check --locked --workspace --all-targets --no-default-features \
        --features "app_server/ssr,app_server/db-$database"
      cargo check --locked -p app_server --no-default-features --features hydrate \
        --target wasm32-unknown-unknown
    )
  done
done

echo "canonical layered application template: ok"
