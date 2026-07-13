#!/usr/bin/env sh
set -eu

base_url="${BASE_URL:-http://127.0.0.1:3000}"

check() {
  path="$1"
  expected="$2"
  actual="$(curl --silent --show-error --output /tmp/hegira-smoke-body --write-out '%{http_code}' "$base_url$path")"
  if [ "$actual" != "$expected" ]; then
    cat /tmp/hegira-smoke-body >&2
    echo "$path returned $actual, expected $expected" >&2
    exit 1
  fi
}

check /healthz 200
check /readyz 200
echo "smoke checks passed for $base_url"
