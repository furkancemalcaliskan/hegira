#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
template_root="$repo_root/templates/applications/layered"
staging_root=$(mktemp -d "/tmp/hegira-layered-template.XXXXXX")

cleanup() {
  rm -rf "$staging_root"
}
trap cleanup EXIT INT TERM

if find "$template_root" -name Cargo.toml -exec grep -nE 'path[[:space:]]*=[[:space:]]*".*(crates|modules)/' {} + |
  grep -vE 'path[[:space:]]*=[[:space:]]*"crates/' >/dev/null; then
  echo "template contains a repository-local framework or module path" >&2
  exit 1
fi

if grep -R -F "$repo_root" "$template_root" >/dev/null; then
  echo "template contains an absolute repository path" >&2
  exit 1
fi

if cargo metadata --locked --no-deps --format-version 1 |
  grep -F "\"$template_root/" >/dev/null; then
  echo "canonical template must not be a member of the framework workspace" >&2
  exit 1
fi

cp -R "$template_root/." "$staging_root/"

# The release tag does not exist while its source is being prepared. Rewrite
# only the disposable manifest so Cargo can validate against this checkout
# without adding repository paths to the canonical template.
awk -v root="$repo_root" '
  /^configuration = \{ git = / {
    print "configuration = { path = \"" root "/crates/configuration\" }"
    next
  }
  /^http_support = \{ git = / {
    print "http_support = { path = \"" root "/crates/http_support\" }"
    next
  }
  /^observability = \{ git = / {
    print "observability = { path = \"" root "/crates/observability\" }"
    next
  }
  /^persistence = \{ git = / {
    print "persistence = { path = \"" root "/crates/persistence\", default-features = false }"
    next
  }
  /^platform_core = \{ git = / {
    print "platform_core = { path = \"" root "/crates/platform_core\" }"
    next
  }
  /^runtime = \{ git = / {
    print "runtime = { path = \"" root "/crates/runtime\" }"
    next
  }
  { print }
' "$staging_root/Cargo.toml" >"$staging_root/Cargo.toml.local"
mv "$staging_root/Cargo.toml.local" "$staging_root/Cargo.toml"

CARGO_TARGET_DIR="$repo_root/target/layered-template-check" \
  cargo check --manifest-path "$staging_root/Cargo.toml" --workspace --all-targets --all-features
CARGO_TARGET_DIR="$repo_root/target/layered-template-check" \
  cargo clippy --manifest-path "$staging_root/Cargo.toml" --workspace --all-targets --all-features -- -D warnings
CARGO_TARGET_DIR="$repo_root/target/layered-template-check" \
  cargo test --manifest-path "$staging_root/Cargo.toml" --workspace --all-features

echo "canonical layered application template: ok"
