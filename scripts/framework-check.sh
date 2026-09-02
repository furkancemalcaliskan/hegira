#!/usr/bin/env sh
set -eu

with_ignored_db_tests="${WITH_IGNORED_DB_TESTS:-false}"

run_step() {
  name="$1"
  shift
  echo "==> $name"
  "$@"
}

run_step "Format" cargo fmt --all -- --check
run_step "DX baseline" sh scripts/dx-audit.sh
run_step "Framework and host Clippy" cargo clippy -p hegira --features ssr,test-support -- -D warnings
run_step "SSR check" cargo check -p hegira --features ssr
run_step "Hydrate check" cargo check -p hegira --no-default-features --features hydrate --target wasm32-unknown-unknown
run_step "OpenAPI check" cargo check -p hegira --features openapi
run_step "All server features" cargo check -p hegira --all-features
run_step "Minimal-capability platform contracts" cargo test --locked --no-default-features \
  -p platform_core -p audit -p background_jobs -p cache -p configuration \
  -p http_support -p leptos_support -p mail -p observability -p persistence \
  -p runtime -p search -p security -p settings -p storage -p test_support
run_step "All-capability platform contracts" cargo test --locked --all-features \
  -p platform_core -p audit -p background_jobs -p cache -p configuration \
  -p http_support -p leptos_support -p mail -p observability -p persistence \
  -p runtime -p search -p security -p settings -p storage -p test_support
run_step "DbMigrator check" cargo check -p db_migrator --features ssr
run_step "SQLite migrator smoke" env APP_ENV=sqlite APP__DATABASE__URL=sqlite::memory: ALLOW_DB_RESET=true cargo run -q -p db_migrator --features ssr -- recreate
run_step "Library tests" cargo test -p hegira --features ssr --lib
run_step "SQLite provider contracts" cargo test -p infrastructure --features ssr sqlite_
run_step "Capability test support" cargo test -p hegira --features ssr,test-support --test capability_test_support

if [ "$with_ignored_db_tests" = "true" ]; then
  if [ -z "${DATABASE_URL:-}" ]; then
    echo "DATABASE_URL must be set when WITH_IGNORED_DB_TESTS=true" >&2
    exit 1
  fi

  run_step "DB-backed durable jobs tests" cargo test -p hegira --features ssr --test durable_jobs -- --ignored
  run_step "DB-backed search/outbox tests" cargo test -p hegira --features ssr --test search_outbox -- --ignored
fi
