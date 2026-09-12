#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
template_root="$repo_root/templates/applications/layered"
. "$repo_root/scripts/validation-cache.sh"
validation_cache_prepare "$repo_root" layered-template-check
staging_parent="$HEGIRA_VALIDATION_WORKSPACE"
staging_root="$staging_parent/application"
export CARGO_TARGET_DIR="$HEGIRA_VALIDATION_TARGET"

cleanup() {
  status=$?
  trap - EXIT INT TERM
  validation_cache_release || status=1
  exit "$status"
}
trap cleanup EXIT INT TERM

if cargo metadata --locked --no-deps --format-version 1 |
  grep -F "\"$template_root/" >/dev/null; then
  echo "canonical template must not be a member of the framework workspace" >&2
  exit 1
fi

cargo fmt --all -- --check
sh "$repo_root/scripts/dx-audit.sh"
cargo test --locked -p template_renderer
cargo run --locked --quiet -p template_renderer \
  --example repository_validation_renderer -- render \
  --repository-root "$repo_root" \
  --template layered \
  --output "$staging_root" \
  --framework-root "$repo_root"

if find "$staging_root" -name Cargo.toml -exec grep -nE 'git[[:space:]]*=[[:space:]]*"https://github.com/furkancemalcaliskan/hegira.git"' {} + |
  grep . >/dev/null; then
  echo "repository validation render contains an unpatched framework dependency" >&2
  exit 1
fi

(
  cd "$staging_root"
  npm ci --prefix apps/web/src
  PATH="$staging_root/apps/web/src/node_modules/.bin:$PATH"
  export PATH
  cargo generate-lockfile
  test -f Cargo.lock
  cargo check --locked --workspace --all-targets --all-features
  node "$repo_root/scripts/architecture-boundaries.mjs" \
    check-generated --root "$staging_root"
  cargo check --locked -p app_server --no-default-features --features hydrate \
    --target wasm32-unknown-unknown
  cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
  cargo test --locked --workspace --all-features
  cargo leptos build -p app_server --release \
    --bin-features ssr,db-postgres --lib-features hydrate \
    --bin-cargo-args=--locked --lib-cargo-args=--locked
)

echo "canonical layered application template: ok"
