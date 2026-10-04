#!/usr/bin/env sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
. "$repo_root/scripts/validation-cache.sh"
validation_cache_prepare "$repo_root" "composition-matrix-check"
workspace="$HEGIRA_VALIDATION_WORKSPACE"
export CARGO_TARGET_DIR="$HEGIRA_VALIDATION_TARGET"

cleanup() {
  status=$?
  trap - EXIT INT TERM
  validation_cache_release || status=1
  exit "$status"
}
trap cleanup EXIT INT TERM

fingerprint() (
  cd "$1"
  find . -type f -print | LC_ALL=C sort | xargs sha256sum
)

expect_exit() {
  expected="$1"
  shift
  set +e
  "$@"
  actual=$?
  set -e
  if [ "$actual" -ne "$expected" ]; then
    echo "expected exit $expected but command returned $actual" >&2
    exit 1
  fi
}

for database in sqlite postgres; do
  echo "==> Minimal and Identity-added $database composition"
  application="$workspace/$database-application"
  artifacts="$workspace/$database-artifacts"
  mkdir -p "$artifacts"

  cargo run --locked --quiet -p hegira_cli -- new "matrix-$database" \
    --destination "$application" --composition minimal --database "$database"

  grep -Fq 'id = "layered-leptos-minimal"' "$application/hegira.toml"
  if grep -Fq 'id = "identity"' "$application/hegira.toml"; then
    echo "minimal application unexpectedly records Identity" >&2
    exit 1
  fi
  if grep -Eq 'identity_(application|http|leptos|sqlx)' "$application/Cargo.toml"; then
    echo "minimal application unexpectedly depends on Identity" >&2
    exit 1
  fi
  if grep -Fq 'identity_runtime' "$application/apps/server/src/server.rs"; then
    echo "minimal application unexpectedly exposes Identity routes" >&2
    exit 1
  fi
  if grep -Fq 'identity_leptos' "$application/apps/web/src/routes.rs"; then
    echo "minimal application unexpectedly exposes Identity pages" >&2
    exit 1
  fi

  fingerprint "$application" >"$artifacts/minimal-before.sha256"
  expect_exit 3 cargo run --locked --quiet -p hegira_cli -- generate resource \
    MatrixRecord --field name:string --application-root "$application" \
    --dry-run --json >"$artifacts/minimal-resource.stdout" \
    2>"$artifacts/minimal-resource.json"
  test ! -s "$artifacts/minimal-resource.stdout"
  node -e '
    const fs = require("fs");
    const diagnostic = JSON.parse(fs.readFileSync(process.argv[1]));
    if (diagnostic.code !== "missing-capabilities" ||
        JSON.stringify(diagnostic.missing) !== JSON.stringify(["authentication", "authorization"]))
      process.exit(1);
  ' "$artifacts/minimal-resource.json"

  expect_exit 3 cargo run --locked --quiet -p hegira_cli -- component add \
    missing-component --application-root "$application" \
    >"$artifacts/invalid.stdout" 2>"$artifacts/invalid.stderr"
  fingerprint "$application" >"$artifacts/minimal-after-failures.sha256"
  cmp "$artifacts/minimal-before.sha256" "$artifacts/minimal-after-failures.sha256"

  (
    cd "$application"
    cargo check --locked --workspace --all-targets --features app_server/ssr
    cargo check --locked -p app_server --no-default-features --features hydrate \
      --target wasm32-unknown-unknown
  )

  cargo run --locked --quiet -p hegira_cli -- component add identity \
    --application-root "$application" --dry-run --json \
    >"$artifacts/identity-dry-run.json"
  fingerprint "$application" >"$artifacts/minimal-after-preview.sha256"
  cmp "$artifacts/minimal-before.sha256" "$artifacts/minimal-after-preview.sha256"

  cargo run --locked --quiet -p hegira_cli -- component add identity \
    --application-root "$application" --json >"$artifacts/identity-applied.json"
  node -e '
    const fs = require("fs");
    const dry = JSON.parse(fs.readFileSync(process.argv[1]));
    const applied = JSON.parse(fs.readFileSync(process.argv[2]));
    if (JSON.stringify(dry.plan) !== JSON.stringify(applied.plan)) process.exit(1);
  ' "$artifacts/identity-dry-run.json" "$artifacts/identity-applied.json"

  grep -Fq 'capabilities = ["authentication", "authorization"]' "$application/hegira.toml"
  grep -Fq 'id = "identity"' "$application/hegira.toml"
  grep -Fq "identity_sqlx::identity::migrations::${database}_migration_source()" \
    "$application/crates/infrastructure/src/operations.rs"
  case "$database" in
    sqlite) other_database=postgres ;;
    postgres) other_database=sqlite ;;
  esac
  if grep -Fq "identity_sqlx::identity::migrations::${other_database}_migration_source()" \
    "$application/crates/infrastructure/src/operations.rs"; then
    echo "Identity installed a migration source for the unselected provider" >&2
    exit 1
  fi
  grep -Fq '.merge(identity_runtime.bearer_routes())' \
    "$application/apps/server/src/server.rs"
  grep -Fq 'identity_runtime.cookie_policy()' \
    "$application/apps/server/src/server.rs"
  grep -Fq '<identity_leptos::identity::routes::IdentityRoutes/>' \
    "$application/apps/web/src/routes.rs"
  test -f "$application/apps/server/src/identity_runtime.rs"

  fingerprint "$application" >"$artifacts/identity-before-repeat.sha256"
  expect_exit 4 cargo run --locked --quiet -p hegira_cli -- component add identity \
    --application-root "$application" \
    >"$artifacts/repeat.stdout" 2>"$artifacts/repeat.stderr"
  fingerprint "$application" >"$artifacts/identity-after-repeat.sha256"
  cmp "$artifacts/identity-before-repeat.sha256" "$artifacts/identity-after-repeat.sha256"

  (
    cd "$application"
    cargo generate-lockfile
    cargo check --locked --workspace --all-targets --features app_server/ssr
    cargo check --locked -p app_server --no-default-features --features hydrate \
      --target wasm32-unknown-unknown
  )
done

echo "Minimal and Identity-added SQLite/PostgreSQL composition matrix: ok"
