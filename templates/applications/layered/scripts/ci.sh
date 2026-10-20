#!/bin/sh
set -eu

[ "$#" -eq 1 ] || { echo 'usage: sh scripts/ci.sh <source|check|test|build|integration|security|cleanup>' >&2; exit 2; }
case "$1" in source|check|test|build|integration|security|cleanup) ;; *) echo 'unsupported application CI selection' >&2; exit 2 ;; esac
action="$1"
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
: "${RUNNER_TEMP:?select a disposable absolute runner directory}"
case "$RUNNER_TEMP" in /*) ;; *) exit 2 ;; esac
RUNNER_TEMP=$(CDPATH= cd -- "$RUNNER_TEMP" && pwd -P)
export RUNNER_TEMP
case "$RUNNER_TEMP/" in "$root/"*) echo 'runner state must be outside the application' >&2; exit 2 ;; esac
locks=$(mktemp "$RUNNER_TEMP/application-locks.XXXXXX")
sha256sum Cargo.lock apps/web/src/package-lock.json >"$locks"
finish() {
  result=$?
  trap - EXIT
  sha256sum --check --status "$locks" || result=1
  rm "$locks"
  exit "$result"
}
trap finish EXIT

case "$action" in
  source)
    : "${HEGIRA_SOURCE:?select the reviewed complete tool source}"
    cargo fmt --all -- --check
    node "$HEGIRA_SOURCE/scripts/architecture-boundaries.mjs" check-generated --root "$root"
    node --test scripts/ci.test.mjs
    exit 0
    ;;
  security)
    # The single inherited SQLx/MySQL lock-only RSA exception remains conditional
    # on proving that no enabled application feature reaches that exact crate.
    graph=$(cargo tree --locked --target all --all-features --invert rsa@0.9.10)
    [ -z "$graph" ] || { echo 'RSA audit exception is active in the application graph' >&2; exit 1; }
    cargo deny --locked --all-features check
    cargo audit --file Cargo.lock
    npm audit --audit-level=high --include=dev --include=optional --include=peer --prefix apps/web/src
    exit 0
    ;;
  integration)
    node scripts/ci-integration.mjs
    exit 0
    ;;
esac

cli="$RUNNER_TEMP/hegira-ci-bootstrap/debug/hegira"
budget="${APPLICATION_CI_BUDGET_MIB:-8192}"
case "$budget" in ''|*[!0-9]*|0) echo 'CI budget must be a positive MiB integer' >&2; exit 2 ;; esac
if [ "$action" = cleanup ]; then
  # A missing CLI means setup did not complete; never substitute unowned cleanup.
  [ -x "$cli" ] || exit 0
  "$cli" artifacts cache-status --cache-budget-mib "$budget" --json
  "$cli" artifacts prune --cache-budget-mib 1 --dry-run --json
  "$cli" artifacts prune --cache-budget-mib 1 --execute --json
  exit 0
fi

cargo_path=$(rustup which cargo)
rust_bin=$(dirname -- "$cargo_path")
node_bin=$(dirname -- "$(command -v node)")
tool_bin=$(dirname -- "$(command -v cargo)")
case "$action" in
  check|test)
    "$cli" "$action" --validation-cache --cache-budget-mib "$budget" --dry-run --json
    "$cli" "$action" --validation-cache --cache-budget-mib "$budget" --execute --trust-application \
      --cargo "$cargo_path" --tool-directory "$rust_bin" --tool-directory "$tool_bin" \
      --tool-directory "$node_bin" --tool-directory /usr/bin --json
    # Do not retain a duplicate check graph while compiling the test graph.
    if [ "$action" = check ]; then
      "$cli" artifacts prune --cache-budget-mib 1 --execute --json
    fi
    ;;
  build)
    "$cli" build --release --validation-cache --cache-budget-mib "$budget" --dry-run --json
    "$cli" build --release --validation-cache --cache-budget-mib "$budget" --execute --trust-application \
      --cargo "$cargo_path" --tool-directory "$rust_bin" --tool-directory "$tool_bin" \
      --tool-directory "$node_bin" --tool-directory /usr/bin \
      --wasm-bindgen "$RUNNER_TEMP/hegira-ci-wasm/bin/wasm-bindgen" \
      --wasm-opt "$RUNNER_TEMP/hegira-ci-binaryen/bin/wasm-opt" --json
    ;;
esac
"$cli" artifacts cache-status --cache-budget-mib "$budget" --json
