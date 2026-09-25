#!/usr/bin/env sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
. "$repo_root/scripts/validation-cache.sh"
validation_cache_prepare "$repo_root" "framework-check"
export CARGO_TARGET_DIR="$HEGIRA_VALIDATION_TARGET"

cleanup() {
  status=$?
  trap - EXIT INT TERM
  validation_cache_release || status=1
  exit "$status"
}
trap cleanup EXIT INT TERM

run_step() {
  name="$1"
  shift
  echo "==> $name"
  "$@"
}

run_step "Framework Clippy" cargo clippy --locked --all-features \
  -p application_manifest -p platform_core -p audit -p background_jobs -p cache -p configuration \
  -p http_support -p leptos_support -p mail -p observability -p persistence \
  -p runtime -p search -p security -p settings -p storage -p test_support \
  -- -D warnings
run_step "Minimal-capability platform contracts" cargo test --locked --no-default-features \
  -p application_manifest -p platform_core -p audit -p background_jobs -p cache -p configuration \
  -p http_support -p leptos_support -p mail -p observability -p persistence \
  -p runtime -p search -p security -p settings -p storage -p test_support
run_step "All-capability platform contracts" cargo test --locked --all-features \
  -p application_manifest -p platform_core -p audit -p background_jobs -p cache -p configuration \
  -p http_support -p leptos_support -p mail -p observability -p persistence \
  -p runtime -p search -p security -p settings -p storage -p test_support
