#!/usr/bin/env sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
. "$repo_root/scripts/validation-cache.sh"

fixture=$(mktemp -d "/tmp/hegira-validation-cache-test.XXXXXX")
cleanup() {
  rm -rf "$fixture"
}
trap cleanup EXIT INT TERM

repository="$fixture/repository"
mkdir -p "$repository/target/debug"
echo preserved >"$repository/target/debug/user-artifact"

validation_cache_prepare "$repository" generated-application-check
first_workspace="$HEGIRA_VALIDATION_WORKSPACE"
first_target="$HEGIRA_VALIDATION_TARGET"
echo artifact >"$first_target/artifact"

if (
  validation_cache_prepare "$repository" generated-application-check
) 2>"$fixture/concurrent.stderr"; then
  echo "concurrent validation unexpectedly acquired the same lock" >&2
  exit 1
fi
grep -Fq "already running or has a stale lock" "$fixture/concurrent.stderr"

if validation_cache_clean "$repository" false 2>"$fixture/locked-clean.stderr"; then
  echo "cleanup unexpectedly removed an active validation cache" >&2
  exit 1
fi
grep -Fq "refusing cleanup while a validation lock exists" "$fixture/locked-clean.stderr"

validation_cache_release
validation_cache_prepare "$repository" generated-application-check
test "$HEGIRA_VALIDATION_WORKSPACE" = "$first_workspace"
test "$HEGIRA_VALIDATION_TARGET" = "$first_target"
test -f "$HEGIRA_VALIDATION_TARGET/artifact"
validation_cache_release

mkdir -p "$repository/target/layered-template-check"
echo legacy >"$repository/target/layered-template-check/artifact"
validation_cache_clean "$repository" true >"$fixture/dry-run.stdout"
test -d "$repository/target/validation"
test -d "$repository/target/layered-template-check"
validation_cache_clean "$repository" false >"$fixture/clean.stdout"
test ! -e "$repository/target/validation"
test ! -e "$repository/target/layered-template-check"
test -f "$repository/target/debug/user-artifact"

outside="$fixture/outside"
mkdir "$outside"
echo preserved >"$outside/sentinel"
ln -s "$outside" "$repository/target/validation"
if validation_cache_clean "$repository" false 2>"$fixture/symlink.stderr"; then
  echo "cleanup unexpectedly accepted a symlinked validation root" >&2
  exit 1
fi
grep -Fq "may not be a symbolic link" "$fixture/symlink.stderr"
test -f "$outside/sentinel"

if validation_cache_paths "$repository" ../escape 2>"$fixture/name.stderr"; then
  echo "validation cache unexpectedly accepted an unsafe check name" >&2
  exit 1
fi
grep -Fq "check names must use" "$fixture/name.stderr"

echo "validation cache lifecycle: ok"
