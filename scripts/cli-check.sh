#!/bin/sh
set -eu

run_step() {
  name="$1"
  shift
  echo "==> $name"
  "$@"
}

run_step "CLI format" cargo fmt --all -- --check
run_step "Application mutation Clippy" cargo clippy --locked -p application_mutator --all-targets -- -D warnings
run_step "Application mutation contracts" cargo test --locked -p application_mutator
run_step "CLI Clippy" cargo clippy --locked -p hegira_cli --all-targets -- -D warnings
run_step "CLI command contracts" cargo test --locked -p hegira_cli

echo "Hegira CLI and application mutation tooling: ok"
