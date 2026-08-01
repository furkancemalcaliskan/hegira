#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
template_root="$repo_root/templates/applications/layered"
staging_parent=$(mktemp -d "/tmp/hegira-layered-template.XXXXXX")
staging_root="$staging_parent/application"

cleanup() {
  rm -rf "$staging_parent"
}
trap cleanup EXIT INT TERM

if cargo metadata --locked --no-deps --format-version 1 |
  grep -F "\"$template_root/" >/dev/null; then
  echo "canonical template must not be a member of the framework workspace" >&2
  exit 1
fi

cargo test --locked -p template_renderer
cargo run --locked --quiet -p template_renderer -- render \
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
  CARGO_TARGET_DIR="$repo_root/target/layered-template-check" \
    cargo check --workspace --all-targets --all-features
  CARGO_TARGET_DIR="$repo_root/target/layered-template-check" \
    cargo check -p app_server --no-default-features --features hydrate \
      --target wasm32-unknown-unknown
  CARGO_TARGET_DIR="$repo_root/target/layered-template-check" \
    cargo clippy --workspace --all-targets --all-features -- -D warnings
  CARGO_TARGET_DIR="$repo_root/target/layered-template-check" \
    cargo test --workspace --all-features
  CARGO_TARGET_DIR="$repo_root/target/layered-template-check" \
    cargo leptos build -p app_server --release \
      --bin-features ssr,db-postgres --lib-features hydrate
)

echo "canonical layered application template: ok"
