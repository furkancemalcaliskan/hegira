#!/usr/bin/env sh
set -eu

: "${DATABASE_URL:?DATABASE_URL is required}"
output="${1:-hegira-$(date -u +%Y%m%dT%H%M%SZ).dump}"
pg_dump --format=custom --no-owner --no-acl --file="$output" "$DATABASE_URL"
pg_restore --list "$output" >/dev/null
echo "verified backup written to $output"
