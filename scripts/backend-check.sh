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
run_step "Clippy" cargo clippy -p hegira --features ssr,test-support -- -D warnings
run_step "Identity module Clippy" cargo clippy --all-features -p identity_domain_shared -p identity_domain -p identity_application_contracts -p identity_application -p identity_sqlx -p identity_http -p identity_leptos -- -D warnings
run_step "SSR check" cargo check -p hegira --features ssr
run_step "Hydrate check" cargo check -p hegira --no-default-features --features hydrate --target wasm32-unknown-unknown
run_step "OpenAPI check" cargo check -p hegira --features openapi
run_step "All server features" cargo check -p hegira --all-features
run_step "DbMigrator check" cargo check -p db_migrator --features ssr
run_step "SQLite migrator smoke" env APP_ENV=sqlite APP__DATABASE__URL=sqlite::memory: ALLOW_DB_RESET=true cargo run -q -p db_migrator --features ssr -- recreate
run_step "Library tests" cargo test -p hegira --features ssr --lib
run_step "Identity module contract tests" cargo test --all-features -p identity_domain_shared -p identity_domain -p identity_application_contracts -p identity_application -p identity_sqlx -p identity_http -p identity_leptos
run_step "SQLite provider contracts" cargo test -p infrastructure --features ssr sqlite_
run_step "Capability test support" cargo test -p hegira --features ssr,test-support --test capability_test_support
run_step "Identity composition parity" cargo test -p hegira --features ssr --test identity_composition
run_step "API identity tests" cargo test -p hegira --features ssr --test api_identity

if [ "$with_ignored_db_tests" = "true" ]; then
  if [ -z "${DATABASE_URL:-}" ]; then
    echo "DATABASE_URL must be set when WITH_IGNORED_DB_TESTS=true" >&2
    exit 1
  fi

  run_step "DB-backed migration compatibility tests" cargo test -p identity_sqlx --all-features postgres_v020_upgrade_retires_catalog_state_and_preserves_history -- --ignored
  run_step "DB-backed API identity tests" cargo test -p hegira --features ssr --test api_identity -- --ignored
  run_step "DB-backed identity persistence tests" cargo test -p hegira --features ssr --test identity_persistence -- --ignored
  run_step "DB-backed durable jobs tests" cargo test -p hegira --features ssr --test durable_jobs -- --ignored
  run_step "DB-backed search/outbox tests" cargo test -p hegira --features ssr --test search_outbox -- --ignored
fi
