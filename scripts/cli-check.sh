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
run_step "Resource generator Clippy" cargo clippy --locked -p resource_generator --all-targets -- -D warnings
run_step "Resource generator contracts" cargo test --locked -p resource_generator
run_step "CLI Clippy" cargo clippy --locked -p hegira_cli --all-targets -- -D warnings
run_step "CLI command contracts" cargo test --locked -p hegira_cli

echo "Hegira CLI, application mutation, and resource generator tooling: ok"
