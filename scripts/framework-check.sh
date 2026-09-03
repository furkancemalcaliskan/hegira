#!/usr/bin/env sh
set -eu

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
