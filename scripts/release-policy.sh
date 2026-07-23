#!/usr/bin/env sh
set -eu

repository_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"

if [ "$#" -gt 1 ]; then
  echo "usage: sh scripts/release-policy.sh [vX.Y.Z]" >&2
  exit 2
fi

if [ "$#" -eq 1 ]; then
  node "$repository_root/scripts/release-policy.mjs" \
    check --root "$repository_root" --release-ref "$1"
else
  node "$repository_root/scripts/release-policy.mjs" \
    check --root "$repository_root"
fi

node "$repository_root/scripts/release-policy.test.mjs"
