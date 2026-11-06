#!/usr/bin/env sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
mode="${1:-all}"
case "$mode" in
  all)
    providers="sqlite postgres"
    if [ "${ALLOW_DATABASE_OPERATION_DISPOSABLE_TARGETS:-false}" != true ] ||
       [ -z "${DATABASE_OPERATION_POSTGRES_URL:-}" ]; then
      echo "all requires explicit disposable PostgreSQL URL and authorization" >&2
      exit 2
    fi
    ;;
  sqlite) providers=sqlite ;;
  *) echo "usage: sh scripts/database-operation-check.sh [all|sqlite]" >&2; exit 2 ;;
esac
[ "$#" -le 1 ] || { echo "only one database matrix selection is supported" >&2; exit 2; }
. "$repo_root/scripts/validation-cache.sh"
validation_cache_prepare "$repo_root" database-operation-check
export CARGO_TARGET_DIR="$HEGIRA_VALIDATION_TARGET"
cleanup() {
  status=$?
  trap - EXIT INT TERM
  validation_cache_release || status=1
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

cargo build --locked -p hegira_cli --bin hegira
export DATABASE_OPERATION_CLI="$CARGO_TARGET_DIR/debug/hegira"
export DATABASE_OPERATION_CARGO="$(rustup which cargo)"
export DATABASE_OPERATION_TOOL_DIRECTORY="$(dirname "$DATABASE_OPERATION_CARGO")"
export DATABASE_OPERATION_DIRECTORY="$HEGIRA_VALIDATION_WORKSPACE"
mkdir -p "$HEGIRA_VALIDATION_WORKSPACE/sources"
application="$HEGIRA_VALIDATION_WORKSPACE/application"

for composition in default minimal identity-added; do
  for database in $providers; do
    echo "==> Public database authority/history: $composition/$database"
    source="$HEGIRA_VALIDATION_WORKSPACE/sources/$composition-$database"
    if [ "$composition" = default ]; then
      set --
    else
      set -- --composition minimal
    fi
    "$DATABASE_OPERATION_CLI" new database-application --destination "$source" \
      --database "$database" "$@"
    source_flag=--generated-source
    if [ "$composition" = identity-added ]; then
      "$DATABASE_OPERATION_CLI" component add identity --application-root "$source"
      source_flag=--identity-added-source
    fi
    if [ "$composition" = default ]; then
      set --
    else
      set -- --component layered-base --component layered-leptos-minimal
    fi
    # Only the preceding iteration's repository-owned disposable staging copy.
    rm -rf "$application"
    cargo run --locked --quiet -p template_renderer \
      --example repository_validation_renderer -- render \
      --repository-root "$repo_root" --template layered --output "$application" \
      --framework-root "$repo_root" "$source_flag" "$source" "$@" \
      --set application_name=database-application \
      --set "database_adapter=$database" --set "database_feature=db-$database"
    cp "$repo_root/scripts/database-operation-contract.rs" \
      "$application/apps/server/tests/database_operation_contract.rs"
    migration_root="$application/crates/infrastructure/migrations/$database"
    cp "$repo_root/test-fixtures/database-operations/1000001_completed.sql" "$migration_root"
    cp "$repo_root/test-fixtures/database-operations/1000002_transaction.sql" "$migration_root"
    export DATABASE_OPERATION_APPLICATION="$application"
    export DATABASE_OPERATION_IDENTITY=false
    [ "$composition" = minimal ] || export DATABASE_OPERATION_IDENTITY=true
    (
      cd "$application"
      cargo generate-lockfile
      rustfmt --edition 2024 --check apps/server/tests/database_operation_contract.rs
      cargo clippy --locked -p app_server --no-default-features \
        --features "database-operations,db-$database" \
        --test database_operation_contract -- -D warnings
      # Run outside Cargo: the public CLI itself invokes Cargo and must not wait
      # for an outer test command's compilation lock.
      cargo test --locked -p app_server --no-default-features \
        --features "database-operations,db-$database" \
        --test database_operation_contract --no-run --message-format=json |
        node "$repo_root/scripts/database-operation-test-binary.mjs" "$CARGO_TARGET_DIR"
    )
    test_binary=$(node "$repo_root/scripts/database-operation-test-binary.mjs" \
      --read "$CARGO_TARGET_DIR")
    "$test_binary" --test-threads=1
    # A duplicate ordering identity must fail composition before DB access.
    # No released migration is rewritten; this extra file exists only in staging.
    cp "$repo_root/test-fixtures/database-operations/1000001_completed.sql" \
      "$migration_root/1000001_duplicate.sql"
    # SQLx's stable macro does not reliably discover a newly added directory
    # entry. Recompile only this fixture package in the owned validation cache.
    (cd "$application" && cargo clean -p app_infrastructure)
    DATABASE_OPERATION_COMPOSITION_CONFLICT=true "$test_binary" --test-threads=1
  done
done
echo "Public database operation security matrix: ok"
