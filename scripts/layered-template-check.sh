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
  /^application = \{ git = / {
    print "application = { path = \"" root "/crates/application\", default-features = false }"
    next
  }
  /^application_contracts = \{ git = / {
    print "application_contracts = { path = \"" root "/crates/application_contracts\" }"
    next
  }
  /^background_jobs = \{ git = / {
    print "background_jobs = { path = \"" root "/crates/background_jobs\" }"
    next
  }
  /^configuration = \{ git = / {
    print "configuration = { path = \"" root "/crates/configuration\" }"
    next
  }
  /^domain = \{ git = / {
    print "domain = { path = \"" root "/crates/domain\" }"
    next
  }
  /^domain_shared = \{ git = / {
    print "domain_shared = { path = \"" root "/crates/domain_shared\" }"
    next
  }
  /^http_support = \{ git = / {
    print "http_support = { path = \"" root "/crates/http_support\" }"
    next
  }
  /^identity_http = \{ git = / {
    print "identity_http = { path = \"" root "/modules/identity/http\", default-features = false }"
    next
  }
  /^identity_leptos = \{ git = / {
    print "identity_leptos = { path = \"" root "/modules/identity/leptos\", default-features = false }"
    next
  }
  /^infrastructure = \{ git = / {
    print "infrastructure = { path = \"" root "/crates/infrastructure\", default-features = false }"
    next
  }
  /^leptos_support = \{ git = / {
    print "leptos_support = { path = \"" root "/crates/leptos_support\", default-features = false }"
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
  /^presentation = \{ git = / {
    print "presentation = { path = \"" root "/crates/presentation\", default-features = false }"
    next
  }
  /^runtime = \{ git = / {
    print "runtime = { path = \"" root "/crates/runtime\" }"
    next
  }
  /^web = \{ git = / {
    print "web = { path = \"" root "/crates/web\", default-features = false }"
    next
  }
  { print }
' "$staging_root/Cargo.toml" >"$staging_root/Cargo.toml.local"
mv "$staging_root/Cargo.toml.local" "$staging_root/Cargo.toml"

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
