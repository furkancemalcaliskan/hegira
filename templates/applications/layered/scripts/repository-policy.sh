#!/usr/bin/env sh
set -eu

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
policy="$repository_root/scripts/repository-policy.mjs"

case "$#" in
  0)
    node "$policy" repository --root "$repository_root"
    node --test "$repository_root/scripts/repository-policy.test.mjs"
    ;;
  2)
    [ "$1" = --event ] || { echo 'expected --event <path>' >&2; exit 2; }
    node "$policy" pull-request --root "$repository_root" --event "$2"
    ;;
  *) echo 'usage: sh scripts/repository-policy.sh [--event <path>]' >&2; exit 2 ;;
esac
