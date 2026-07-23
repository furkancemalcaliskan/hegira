#!/usr/bin/env sh
set -eu

repository_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"

node "$repository_root/scripts/architecture-boundaries.mjs" \
  check --root "$repository_root"
node "$repository_root/scripts/architecture-boundaries.test.mjs"
