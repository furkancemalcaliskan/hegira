#!/usr/bin/env sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
compose_file="$repo_root/scripts/generated-application-smoke.yml"
. "$repo_root/scripts/validation-cache.sh"
validation_cache_prepare "$repo_root" generated-application-check
staging_parent="$HEGIRA_VALIDATION_WORKSPACE"
generated_root="$staging_parent/postgres-validation"
artifacts_dir="$staging_parent/artifacts"
export CARGO_TARGET_DIR="$HEGIRA_VALIDATION_TARGET"

export COMPOSE_PROJECT_NAME="hegira-generated-${GITHUB_RUN_ID:-local}-$$"
export GENERATED_APP_IMAGE="hegira-generated:${GITHUB_RUN_ID:-local}-$$"
image_built=false
export GENERATED_APP_HTTP_PORT="${GENERATED_APP_HTTP_PORT:-38081}"
export GENERATED_APP_POSTGRES_PORT="${GENERATED_APP_POSTGRES_PORT:-35432}"
export GENERATED_APP_DB_PASSWORD="generated-${GITHUB_RUN_ID:-local}-${GITHUB_RUN_ATTEMPT:-0}-$$"
export GENERATED_APP_JWT_SECRET="generated-jwt-${GITHUB_RUN_ID:-local}-${GITHUB_RUN_ATTEMPT:-0}-$$-ephemeral"

compose() {
  docker compose --file "$compose_file" "$@"
}

application_fingerprint() {
  (
    cd "$1"
    find . -type f -print | LC_ALL=C sort | xargs sha256sum
  )
}

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

cleanup() {
  status=$?
  trap - EXIT INT TERM
  set +e
  if [ "$status" -ne 0 ]; then
    compose ps --all
    compose logs --no-color postgres web
  fi
  compose down --volumes --remove-orphans
  if [ "$image_built" = true ]; then
    docker image rm "$GENERATED_APP_IMAGE"
  fi
  validation_cache_release || status=1
  exit "$status"
}
trap cleanup EXIT INT TERM

# Exercise the real public command without any repository-local source options.
cargo run --locked --quiet -p hegira_cli -- new sqlite-application \
  --destination "$staging_parent/sqlite-source"
cargo run --locked --quiet -p hegira_cli -- new postgres-application \
  --destination "$staging_parent/postgres-source" \
  --database postgres --client leptos --component identity

if find "$repo_root/.cargo" "$repo_root/crates" \
  "$repo_root/modules" "$repo_root/tools" \
  -name node_modules -prune -o -type l -print -quit | grep . >/dev/null; then
  echo "framework validation source contains a symbolic link" >&2
  exit 1
fi

for database in sqlite postgres; do
  validation_root="$staging_parent/$database-validation"
  framework_root="$validation_root/.hegira-validation/framework"
  migration_artifacts="$staging_parent/$database-migration-artifacts"
  mkdir -p "$migration_artifacts"
  cargo run --locked --quiet -p template_renderer \
    --example repository_validation_renderer -- render \
    --repository-root "$repo_root" --template layered \
    --generated-source "$staging_parent/$database-source" \
    --output "$validation_root" --framework-root "$repo_root" \
    --framework-path .hegira-validation/framework \
    --set "application_name=$database-application" \
    --set "database_adapter=$database" --set "database_feature=db-$database" \
    --set client_adapter=leptos --set component_id=layered-leptos-identity

  case "$database" in
    sqlite)
      generated_migration_version=10
      generated_migration_path="crates/infrastructure/migrations/sqlite/010_generated_validation.sql"
      other_migration_path="crates/infrastructure/migrations/postgres/023_generated_validation.sql"
      ;;
    postgres)
      generated_migration_version=23
      generated_migration_path="crates/infrastructure/migrations/postgres/023_generated_validation.sql"
      other_migration_path="crates/infrastructure/migrations/sqlite/010_generated_validation.sql"
      ;;
  esac

  (
    cd "$validation_root"
    find crates/infrastructure/migrations -type f -name '*.sql' -print \
      | LC_ALL=C sort | xargs sha256sum
  ) >"$migration_artifacts/historical.sha256"
  application_fingerprint "$validation_root" >"$migration_artifacts/before-dry-run.sha256"

  expect_exit 3 cargo run --locked --quiet -p hegira_cli -- generate migration \
    ../invalid --application-root "$validation_root" \
    >"$migration_artifacts/invalid.stdout" 2>"$migration_artifacts/invalid.stderr"
  cargo run --locked --quiet -p hegira_cli -- generate migration \
    generated_validation --application-root "$validation_root" --dry-run --json \
    >"$migration_artifacts/dry-run-one.json"
  cargo run --locked --quiet -p hegira_cli -- generate migration \
    generated_validation --application-root "$validation_root" --dry-run --json \
    >"$migration_artifacts/dry-run-two.json"
  cmp "$migration_artifacts/dry-run-one.json" "$migration_artifacts/dry-run-two.json"
  application_fingerprint "$validation_root" >"$migration_artifacts/after-dry-run.sha256"
  cmp "$migration_artifacts/before-dry-run.sha256" "$migration_artifacts/after-dry-run.sha256"

  cargo run --locked --quiet -p hegira_cli -- generate migration \
    generated_validation --application-root "$validation_root" --json \
    >"$migration_artifacts/apply.json"
  node -e \
    'const fs = require("fs"); const dry = JSON.parse(fs.readFileSync(process.argv[1])); const applied = JSON.parse(fs.readFileSync(process.argv[2])); if (JSON.stringify(dry.plan) !== JSON.stringify(applied.plan)) process.exit(1);' \
    "$migration_artifacts/dry-run-one.json" "$migration_artifacts/apply.json"
  test -f "$validation_root/$generated_migration_path"
  test ! -e "$validation_root/$other_migration_path"
  grep -Fq "migration: generated_validation" "$validation_root/$generated_migration_path"
  grep -Fq "schema = 1" "$validation_root/crates/infrastructure/migrations/.hegira-generator.toml"
  grep -Fq "database = \"$database\"" "$validation_root/crates/infrastructure/migrations/.hegira-generator.toml"

  (
    cd "$validation_root"
    sha256sum -c "$migration_artifacts/historical.sha256"
  )
  application_fingerprint "$validation_root" >"$migration_artifacts/before-duplicate.sha256"
  expect_exit 4 cargo run --locked --quiet -p hegira_cli -- generate migration \
    generated_validation --application-root "$validation_root" \
    >"$migration_artifacts/duplicate.stdout" 2>"$migration_artifacts/duplicate.stderr"
  application_fingerprint "$validation_root" >"$migration_artifacts/after-duplicate.sha256"
  cmp "$migration_artifacts/before-duplicate.sha256" "$migration_artifacts/after-duplicate.sha256"

  mkdir -p "$framework_root"
  tar -C "$repo_root" \
    --exclude='.git' \
    --exclude='.env' \
    --exclude='node_modules' \
    --exclude='target' \
    --exclude='*.sqlite3' \
    --exclude='*.sqlite3-shm' \
    --exclude='*.sqlite3-wal' \
    -cf - \
    Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates modules tools |
    tar -xf - -C "$framework_root"

  (
    cd "$validation_root"
    cargo check --workspace --all-targets --features app_server/ssr
    HEGIRA_TEST_GENERATED_MIGRATION_VERSION="$generated_migration_version" \
    HEGIRA_TEST_GENERATED_MIGRATION_DESCRIPTION="generated validation" \
      cargo test --workspace --features app_server/ssr
    node "$repo_root/scripts/architecture-boundaries.mjs" \
      check-generated --root "$validation_root"
    cargo check -p app_server --no-default-features --features hydrate \
      --target wasm32-unknown-unknown
    npm ci --prefix apps/web/src
    PATH="$validation_root/apps/web/src/node_modules/.bin:$PATH"
    export PATH
    cargo leptos build -p app_server --release \
      --bin-features "ssr,db-$database" --lib-features hydrate
  )
done

compose up --detach postgres

(
  cd "$generated_root"
  ALLOW_GENERATED_APP_DB_RESET=true \
  GENERATED_APP_DATABASE_URL="postgres://generated_app:$GENERATED_APP_DB_PASSWORD@127.0.0.1:$GENERATED_APP_POSTGRES_PORT/generated_app" \
  HEGIRA_TEST_GENERATED_MIGRATION_VERSION=23 \
  HEGIRA_TEST_GENERATED_MIGRATION_DESCRIPTION="generated validation" \
    cargo test -p app_server --no-default-features --features ssr,db-postgres \
      --test database_contracts postgres_fresh_install_and_v020_upgrade_pass -- \
      --ignored --test-threads=1
)

docker build --tag "$GENERATED_APP_IMAGE" "$generated_root"
image_built=true
compose up --detach web

base_url="http://127.0.0.1:$GENERATED_APP_HTTP_PORT"
mkdir -p "$artifacts_dir"
curl --fail --silent \
  --retry 60 --retry-all-errors --retry-delay 1 \
  "$base_url/healthz" | grep -q '"status":"ok"'
curl --fail --silent \
  --retry 60 --retry-all-errors --retry-delay 1 \
  "$base_url/readyz" | grep -q '"status":"ok"'
curl --fail --silent --show-error \
  --dump-header "$artifacts_dir/headers" \
  "$base_url/" --output "$artifacts_dir/index.html"
curl --fail --silent --show-error \
  "$base_url/pkg/app.css" --output "$artifacts_dir/app.css"
curl --fail --silent --show-error \
  "$base_url/pkg/app.js" --output "$artifacts_dir/app.js"
curl --fail --silent --show-error \
  "$base_url/pkg/app.wasm" --output "$artifacts_dir/app.wasm"
curl --fail --silent --show-error \
  "$base_url/assets/branding/hegira-logo.png" \
  --output "$artifacts_dir/hegira-logo.png"

grep -Eqi '<!doctype html|<html' "$artifacts_dir/index.html"
test -s "$artifacts_dir/app.css"
test -s "$artifacts_dir/app.js"
test "$(od -An -tx1 -N4 "$artifacts_dir/app.wasm" | tr -d '[:space:]')" = "0061736d"
cmp \
  "$artifacts_dir/hegira-logo.png" \
  "$generated_root/apps/web/src/public/assets/branding/hegira-logo.png"

grep -Eqi '^x-content-type-options:[[:space:]]*nosniff' "$artifacts_dir/headers"
grep -Eqi '^x-frame-options:[[:space:]]*DENY' "$artifacts_dir/headers"
grep -Eqi '^content-security-policy:' "$artifacts_dir/headers"
grep -Eqi '^strict-transport-security:' "$artifacts_dir/headers"
grep -Eqi '^x-request-id:' "$artifacts_dir/headers"

unauthorized_status=$(curl --silent --show-error --output "$artifacts_dir/unauthorized.json" \
  --write-out '%{http_code}' "$base_url/api/identity/users")
test "$unauthorized_status" = "401"
grep -Fq 'auth:missing_bearer_token' "$artifacts_dir/unauthorized.json"

bearer_mutation_status=$(curl --silent --show-error --output /dev/null \
  --write-out '%{http_code}' \
  --header 'content-type: application/json' \
  --data '{}' \
  "$base_url/api/identity/auth/register")
test "$bearer_mutation_status" = "422"

echo "CLI-generated application validation passed for generated migrations, SQLite, PostgreSQL, v0.2.0 upgrades, and the production container"
