#!/usr/bin/env sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
. "$repo_root/scripts/validation-cache.sh"
validation_cache_prepare "$repo_root" "official-modules-check"
export CARGO_TARGET_DIR="$HEGIRA_VALIDATION_TARGET"

cleanup() {
  status=$?
  trap - EXIT INT TERM
  validation_cache_release || status=1
  exit "$status"
}
trap cleanup EXIT INT TERM

with_ignored_db_tests="${WITH_IGNORED_DB_TESTS:-false}"

run_step() {
  name="$1"
  shift
  echo "==> $name"
  "$@"
}

run_step "Identity module Clippy" cargo clippy --locked --all-features -p identity_domain_shared -p identity_domain -p identity_application_contracts -p identity_application -p identity_sqlx -p identity_http -p identity_leptos -- -D warnings
run_step "Identity module contract tests" cargo test --locked --all-features -p identity_domain_shared -p identity_domain -p identity_application_contracts -p identity_application -p identity_sqlx -p identity_http -p identity_leptos
if [ "$with_ignored_db_tests" = "true" ]; then
  if [ -z "${DATABASE_URL:-}" ]; then
    echo "DATABASE_URL must be set when WITH_IGNORED_DB_TESTS=true" >&2
    exit 1
  fi

  run_step "DB-backed Identity SQLx contracts" cargo test --locked \
    --all-features -p identity_sqlx -- --ignored
fi
