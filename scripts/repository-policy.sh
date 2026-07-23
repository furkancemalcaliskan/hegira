#!/usr/bin/env sh
set -eu

repository_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"

node "$repository_root/scripts/repository-policy.mjs" \
  repository --root "$repository_root"
node "$repository_root/scripts/repository-policy.test.mjs"
sh "$repository_root/scripts/architecture-boundaries.sh"

if [ "$#" -eq 0 ]; then
  exit 0
fi

if [ "$#" -ne 2 ] || [ "$1" != "--event" ]; then
  echo "usage: sh scripts/repository-policy.sh [--event <path>]" >&2
  exit 2
fi

node "$repository_root/scripts/repository-policy.mjs" \
  pull-request --event "$2"
