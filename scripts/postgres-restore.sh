#!/usr/bin/env sh
set -eu

: "${DATABASE_URL:?DATABASE_URL is required}"
: "${1:?dump file is required}"
if [ "${ALLOW_DB_RESTORE:-false}" != "true" ]; then
  echo "set ALLOW_DB_RESTORE=true to acknowledge destructive restore" >&2
  exit 1
fi

pg_restore --clean --if-exists --no-owner --no-acl --dbname="$DATABASE_URL" "$1"
echo "restore completed from $1"
