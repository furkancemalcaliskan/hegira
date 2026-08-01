#!/usr/bin/env sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

sh "$repo_root/scripts/framework-check.sh"
sh "$repo_root/scripts/official-modules-check.sh"
sh "$repo_root/scripts/layered-template-check.sh"
