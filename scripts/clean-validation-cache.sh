#!/usr/bin/env sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
. "$repo_root/scripts/validation-cache.sh"

case "${1:-}" in
  "") validation_cache_clean "$repo_root" false ;;
  --dry-run) validation_cache_clean "$repo_root" true ;;
  *)
    echo "usage: sh scripts/clean-validation-cache.sh [--dry-run]" >&2
    exit 2
    ;;
esac
