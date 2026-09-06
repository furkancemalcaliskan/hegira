#!/usr/bin/env sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
compose_file="$repo_root/scripts/generated-application-smoke.yml"
staging_parent=$(mktemp -d "/tmp/hegira-generated-application.XXXXXX")
generated_root="$staging_parent/postgres-validation"
artifacts_dir="$staging_parent/artifacts"

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
  rm -rf "$staging_parent"
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
  cargo run --locked --quiet -p template_renderer \
    --example repository_validation_renderer -- render \
    --repository-root "$repo_root" --template layered \
    --generated-source "$staging_parent/$database-source" \
    --output "$validation_root" --framework-root "$repo_root" \
    --framework-path .hegira-validation/framework \
    --set "application_name=$database-application" \
    --set "database_adapter=$database" --set "database_feature=db-$database" \
    --set client_adapter=leptos --set component_id=layered-leptos-identity

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
    export CARGO_TARGET_DIR="$repo_root/target/generated-application-check"
    cargo check --workspace --all-targets --features app_server/ssr
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
  CARGO_TARGET_DIR="$repo_root/target/generated-application-check" \
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

echo "CLI-generated application validation passed for SQLite, PostgreSQL, v0.2.0 upgrades, and the production container"
