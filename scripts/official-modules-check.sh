#!/usr/bin/env sh
set -eu

with_ignored_db_tests="${WITH_IGNORED_DB_TESTS:-false}"

run_step() {
  name="$1"
  shift
  echo "==> $name"
  "$@"
}

run_step "Identity module Clippy" cargo clippy --all-features -p identity_domain_shared -p identity_domain -p identity_application_contracts -p identity_application -p identity_sqlx -p identity_http -p identity_leptos -- -D warnings
run_step "Identity module contract tests" cargo test --all-features -p identity_domain_shared -p identity_domain -p identity_application_contracts -p identity_application -p identity_sqlx -p identity_http -p identity_leptos
run_step "Identity composition parity" cargo test -p hegira --features ssr --test identity_composition
run_step "API identity tests" cargo test -p hegira --features ssr --test api_identity

if [ "$with_ignored_db_tests" = "true" ]; then
  if [ -z "${DATABASE_URL:-}" ]; then
    echo "DATABASE_URL must be set when WITH_IGNORED_DB_TESTS=true" >&2
    exit 1
  fi

  run_step "DB-backed migration compatibility tests" cargo test -p infrastructure --all-features postgres_v020_upgrade_retires_catalog_state_and_preserves_history -- --ignored
  run_step "DB-backed API identity tests" cargo test -p hegira --features ssr --test api_identity -- --ignored
  run_step "DB-backed identity persistence tests" cargo test -p hegira --features ssr --test identity_persistence -- --ignored
fi
