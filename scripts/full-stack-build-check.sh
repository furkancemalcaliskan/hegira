#!/usr/bin/env sh
set -eu

script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
repo_root="$(CDPATH= cd -- "$script_dir/.." && pwd)"
cd "$repo_root"

frontend_dir="crates/web/src"
frontend_bin="$repo_root/$frontend_dir/node_modules/.bin"

require_file() {
  path="$1"
  description="$2"

  if [ ! -s "$path" ]; then
    echo "missing $description: $path" >&2
    exit 1
  fi
}

require_executable() {
  path="$1"
  description="$2"

  require_file "$path" "$description"
  if [ ! -x "$path" ]; then
    echo "$description is not executable: $path" >&2
    exit 1
  fi
}

npm ci --prefix "$frontend_dir"
PATH="$frontend_bin:$PATH"
export PATH

tailwind_bin="$(command -v tailwindcss || true)"
if [ "$tailwind_bin" != "$frontend_bin/tailwindcss" ]; then
  echo "repository-local tailwindcss not found after npm ci: $frontend_bin/tailwindcss" >&2
  exit 1
fi

cargo build --locked --release -p db_migrator \
  --no-default-features --features ssr,db-postgres
cargo leptos build --release \
  --bin-features ssr,db-postgres \
  --bin-cargo-args="--locked" \
  --lib-features hydrate \
  --lib-cargo-args="--locked"

require_executable "target/release/hegira" "release server binary"
require_executable "target/release/db_migrator" "release database migrator"
require_file "target/site/pkg/hegira.wasm" "hydrated WebAssembly bundle"
require_file "target/site/pkg/hegira.js" "hydration JavaScript bundle"
require_file "target/site/pkg/hegira.css" "compiled stylesheet"
require_file \
  "target/site/assets/branding/hegira-logo.png" \
  "branded Hegira asset"

echo "full-stack build contract verified"
