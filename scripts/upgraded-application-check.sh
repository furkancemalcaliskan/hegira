#!/usr/bin/env sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
mode="${1:-all}"
case "$mode" in
  all) compositions="default minimal identity-added" ;;
  default|minimal|identity-added) compositions="$mode" ;;
  *) echo "usage: sh scripts/upgraded-application-check.sh [all|default|minimal|identity-added]" >&2; exit 2 ;;
esac
case "$mode" in
  minimal) default_http_port=38084; default_postgres_port=35435 ;;
  identity-added) default_http_port=38085; default_postgres_port=35436 ;;
  *) default_http_port=38083; default_postgres_port=35434 ;;
esac
[ "$#" -le 1 ] || { echo "only one lifecycle selection is supported" >&2; exit 2; }
. "$repo_root/scripts/validation-cache.sh"
generated_tool_bin=$(sh "$repo_root/scripts/generated-toolchain.sh" prepare \
  templates/applications/layered/Cargo.lock --container)
PATH="$generated_tool_bin:$PATH"
export PATH
docker info >/dev/null
export UPGRADE_DB_PASSWORD="$(node -e 'process.stdout.write(require("crypto").randomBytes(32).toString("hex"))')"
export UPGRADE_JWT_SECRET="$(node -e 'process.stdout.write(require("crypto").randomBytes(32).toString("hex"))')"
check_name="upgraded-application-$mode"
validation_cache_prepare "$repo_root" "$check_name"
export CARGO_TARGET_DIR="$HEGIRA_VALIDATION_TARGET"
staging_parent="$HEGIRA_VALIDATION_WORKSPACE"
application="$staging_parent/application"
compose_file="$repo_root/scripts/upgraded-application-smoke.yml"
phase=preparation
case_id=matrix
image_built=false
export COMPOSE_PROJECT_NAME="hegira-upgrade-$mode-$$"
export UPGRADE_APP_IMAGE="hegira-upgrade-$mode:$$"
export UPGRADE_HTTP_PORT="${UPGRADE_HTTP_PORT:-$default_http_port}"
export UPGRADE_POSTGRES_PORT="${UPGRADE_POSTGRES_PORT:-$default_postgres_port}"
export UPGRADE_DATABASE=sqlite
export UPGRADE_CONTAINER_DATABASE_URL=sqlite:///data/upgrade.db
export UPGRADE_SQLITE_DIRECTORY="$staging_parent/database"

compose() { docker compose --file "$compose_file" "$@"; }
phase_begin() {
  phase="$1"
  phase_started=$(date +%s)
  echo "==> [upgrade/$case_id] $phase"
}
phase_finish() {
  elapsed=$(( $(date +%s) - phase_started ))
  echo "==> [upgrade/$case_id] $phase: passed (${elapsed}s)"
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    printf '| `%s` | %s | passed | %ss |\n' "$case_id" "$phase" "$elapsed" >>"$GITHUB_STEP_SUMMARY"
  fi
  phase=between-phases
}
cleanup() {
  status=$?
  trap - EXIT INT TERM
  set +e
  if [ "$status" -ne 0 ]; then
    echo "upgrade lifecycle failed: composition/provider=$case_id phase=$phase" >&2
    if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
      printf '| `%s` | %s | failed/interrupted | - |\n' "$case_id" "$phase" >>"$GITHUB_STEP_SUMMARY"
    fi
    compose ps --all
    compose logs --no-color postgres web
  fi
  compose down --volumes --remove-orphans || status=1
  if [ "$image_built" = true ] && docker image inspect "$UPGRADE_APP_IMAGE" >/dev/null 2>&1; then
    docker image rm "$UPGRADE_APP_IMAGE" || status=1
  fi
  cache_size_kib=$(du -sk "$HEGIRA_VALIDATION_TARGET" | awk '{ print $1 }')
  echo "upgrade lifecycle cache footprint: ${cache_size_kib} KiB ($check_name)"
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    printf '\nRepository Cargo cache: `%s KiB` (`%s`).\n' "$cache_size_kib" "$check_name" >>"$GITHUB_STEP_SUMMARY"
  fi
  validation_cache_release || status=1
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  printf '\n### Released application upgrade lifecycle\n\n| Composition/provider | Phase | Result | Duration |\n|---|---|---|---:|\n' >>"$GITHUB_STEP_SUMMARY"
fi
phase_begin "authenticated baselines, customization, and public upgrade workflow"
node --test "$repo_root/scripts/upgraded-application-http.test.mjs"
cargo build --locked -p hegira_cli --bin hegira
cargo run --locked --quiet -p template_renderer --example upgrade_preservation -- \
  "$repo_root" "$staging_parent/fixtures" "$CARGO_TARGET_DIR/debug/hegira" --production
phase_finish

for composition in $compositions; do
  for database in sqlite postgres; do
    case_id="$composition-$database"
    source="$staging_parent/fixtures/$case_id-source"
    artifacts="$staging_parent/artifacts/$case_id"
    mkdir -p "$artifacts"
    (
      cd "$source"
      find . -type f -print | LC_ALL=C sort | xargs sha256sum
    ) >"$artifacts/public-source.sha256"
    phase_begin "isolated local-source staging and lockfile review"
    # One source location for equal package names; copy only after the preceding build.
    rm -rf "$application" "$UPGRADE_SQLITE_DIRECTORY"
    cp -R "$staging_parent/fixtures/$case_id" "$application"
    mkdir -p "$UPGRADE_SQLITE_DIRECTORY" "$application/.hegira-validation/framework"
    if find "$repo_root/.cargo" "$repo_root/crates" "$repo_root/modules" "$repo_root/tools" \
      -name node_modules -prune -o -type l -print -quit | grep . >/dev/null; then
      echo "framework validation source contains a symbolic link" >&2; exit 1
    fi
    tar -C "$repo_root" --exclude='.git' --exclude='.env' --exclude='target' \
      --exclude='node_modules' --exclude='*.sqlite3*' \
      -cf - Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates modules tools \
      | tar -xf - -C "$application/.hegira-validation/framework"
    generated_tool_bin=$(sh "$repo_root/scripts/generated-toolchain.sh" application "$application" --container)
    PATH="$generated_tool_bin:$PATH"
    export PATH
    # The application-owner lock operation is confined to the separate staging copy.
    cp "$application/Cargo.lock" "$artifacts/local-source.Cargo.lock"
    node "$repo_root/scripts/architecture-boundaries.mjs" check-generated --root "$application"
    phase_finish

    phase_begin "native, hydration, and existing application tests"
    (
      cd "$application"
      cargo check --locked --workspace --all-targets --no-default-features \
        --features "app_server/ssr,app_server/db-$database"
      cargo test --locked --workspace --no-default-features \
        --features "app_server/ssr,app_server/db-$database" -- --skip fresh_and_released_upgrade
      cargo check --locked -p app_server --no-default-features --features hydrate --target wasm32-unknown-unknown
    )
    phase_finish

    export UPGRADE_DATABASE="$database"
    export HEGIRA_UPGRADE_IDENTITY=true
    [ "$composition" != minimal ] || export HEGIRA_UPGRADE_IDENTITY=false
    export HEGIRA_UPGRADE_ADMIN_TOKEN="$(node "$repo_root/scripts/upgraded-application-http.mjs" token)"
    phase_begin "disposable fresh and released database migration/data contracts"
    if [ "$database" = postgres ]; then
      export UPGRADE_CONTAINER_DATABASE_URL="postgres://upgrade_app:$UPGRADE_DB_PASSWORD@postgres:5432/upgrade_app"
      compose up --detach --wait postgres
      compose exec --no-TTY postgres createdb --username upgrade_app fresh_app
      export HEGIRA_UPGRADE_DATABASE_URL="postgres://upgrade_app:$UPGRADE_DB_PASSWORD@127.0.0.1:$UPGRADE_POSTGRES_PORT/upgrade_app"
      export HEGIRA_UPGRADE_FRESH_DATABASE_URL="postgres://upgrade_app:$UPGRADE_DB_PASSWORD@127.0.0.1:$UPGRADE_POSTGRES_PORT/fresh_app"
    else
      export UPGRADE_CONTAINER_DATABASE_URL="sqlite:///data/upgrade.db"
      export HEGIRA_UPGRADE_DATABASE_URL="sqlite://$UPGRADE_SQLITE_DIRECTORY/upgrade.db?mode=rwc"
      export HEGIRA_UPGRADE_FRESH_DATABASE_URL="sqlite://$UPGRADE_SQLITE_DIRECTORY/fresh.db?mode=rwc"
    fi
    (
      cd "$application"
      ALLOW_UPGRADE_DISPOSABLE_DATABASES=true \
        cargo test --locked -p app_server --no-default-features --features "ssr,db-$database" \
          --test upgrade_lifecycle -- --test-threads=1
    )
    phase_finish

    phase_begin "locked native and hydration release assets"
    (
      cd "$application"
      npm ci --prefix apps/web/src
      PATH="$application/apps/web/src/node_modules/.bin:$PATH"
      export PATH
      cargo leptos build -p app_server --release \
        --bin-features "ssr,db-$database" --lib-features hydrate \
        --bin-cargo-args=--locked --lib-cargo-args=--locked
    )
    phase_finish

    phase_begin "upgraded-source production image build"
    # Compile in the canonical Debian builder; do not package host-linked binaries.
    image_built=true
    docker build --tag "$UPGRADE_APP_IMAGE" "$application"
    phase_finish
    phase_begin "production readiness, HTTP, security, and CRUD contracts"
    compose up --detach --wait web
    if [ "$composition" != minimal ]; then
      # Server-function paths include the build root; query the image's own build artifact.
      compose exec --no-TTY web cat /app/upgrade-bff-path >"$application/.hegira-validation/bff-path"
    fi
    node "$repo_root/scripts/upgraded-application-http.mjs" check \
      "http://127.0.0.1:$UPGRADE_HTTP_PORT" "$composition" "$application"
    (
      cd "$source"
      sha256sum -c "$artifacts/public-source.sha256" >/dev/null
    )
    phase_finish
    compose down --volumes --remove-orphans
    docker image rm "$UPGRADE_APP_IMAGE"
    image_built=false
  done
done
echo "released v0.6.0 -> v0.7.0 $mode production lifecycle: ok"
