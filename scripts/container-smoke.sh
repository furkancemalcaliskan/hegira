#!/usr/bin/env sh
set -eu

compose_file="docker-compose.smoke.yml"
export COMPOSE_PROJECT_NAME="${COMPOSE_PROJECT_NAME:-hegira-smoke-${GITHUB_RUN_ID:-$$}}"
export HEGIRA_SMOKE_IMAGE="${HEGIRA_SMOKE_IMAGE:-hegira-smoke:${GITHUB_SHA:-local}}"
export SMOKE_PORT="${SMOKE_PORT:-38080}"
artifacts_dir=""

compose() {
  docker compose --file "$compose_file" "$@"
}

cleanup() {
  status=$?
  trap - EXIT INT TERM
  set +e
  if [ "$status" -ne 0 ]; then
    compose ps --all
    compose logs --no-color postgres migrate web
  fi
  if [ -n "$artifacts_dir" ]; then
    rm -rf "$artifacts_dir"
  fi
  compose down --volumes --remove-orphans
  exit "$status"
}
trap cleanup EXIT INT TERM

docker build \
  --target final \
  --build-arg SERVER_FEATURES=ssr,db-postgres \
  --tag "$HEGIRA_SMOKE_IMAGE" \
  .

compose up --detach postgres
compose run --rm migrate
compose up --detach web

base_url="http://127.0.0.1:$SMOKE_PORT"
curl --fail --silent \
  --retry 60 --retry-all-errors --retry-delay 1 \
  "$base_url/healthz" | grep -q '"status":"ok"'
curl --fail --silent \
  --retry 60 --retry-all-errors --retry-delay 1 \
  "$base_url/readyz" | grep -q '"status":"ok"'

artifacts_dir="$(mktemp -d)"
curl --fail --silent --show-error "$base_url/" --output "$artifacts_dir/index.html"
curl --fail --silent --show-error "$base_url/pkg/hegira.css" --output "$artifacts_dir/hegira.css"
curl --fail --silent --show-error "$base_url/pkg/hegira.js" --output "$artifacts_dir/hegira.js"

grep -Eqi '<!doctype html|<html' "$artifacts_dir/index.html"
test -s "$artifacts_dir/hegira.css"
test -s "$artifacts_dir/hegira.js"
rm -rf "$artifacts_dir"

echo "production container smoke passed at $base_url"
