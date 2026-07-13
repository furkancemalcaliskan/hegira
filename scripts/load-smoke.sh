#!/usr/bin/env sh
set -eu

base_url="${BASE_URL:-http://127.0.0.1:3000}"
requests="${REQUESTS:-200}"
concurrency="${CONCURRENCY:-10}"

seq "$requests" | xargs -P "$concurrency" -I '{}' sh -c \
  'curl --fail --silent --show-error "$1/healthz" >/dev/null' _ "$base_url"

echo "$requests requests completed with concurrency $concurrency"
