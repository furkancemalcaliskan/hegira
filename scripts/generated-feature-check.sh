#!/usr/bin/env sh
set -eu

if [ "$#" -ne 2 ]; then
  echo "usage: sh scripts/generated-feature-check.sh <features> <target-or-empty>" >&2
  exit 2
fi

features="$1"
target="$2"

case "$features" in
  "" | *[!a-zA-Z0-9,_-]*)
    echo "features must be a non-empty comma-separated Cargo feature list" >&2
    exit 2
    ;;
esac

case "$target" in
  "" | wasm32-unknown-unknown) ;;
  *)
    echo "unsupported generated-application target: $target" >&2
    exit 2
    ;;
esac

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
staging_parent=$(mktemp -d "/tmp/hegira-generated-feature.XXXXXX")
generated_root="$staging_parent/application"

cleanup() {
  rm -rf "$staging_parent"
}
trap cleanup EXIT INT TERM

cargo run --locked --quiet -p template_renderer \
  --example repository_validation_renderer -- render \
  --repository-root "$repo_root" \
  --template layered \
  --output "$generated_root" \
  --framework-root "$repo_root"

(
  cd "$generated_root"
  cargo generate-lockfile
  if [ -n "$target" ]; then
    CARGO_TARGET_DIR="$repo_root/target/generated-feature-check" \
      cargo check --locked --no-default-features \
        --package app_server --features "$features" --target "$target"
  else
    CARGO_TARGET_DIR="$repo_root/target/generated-feature-check" \
      cargo check --locked --no-default-features \
        --package app_server --features "$features"
  fi
  node "$repo_root/scripts/architecture-boundaries.mjs" \
    check-generated --root "$generated_root"
)

echo "generated application feature contract: ok ($features${target:+, $target})"
