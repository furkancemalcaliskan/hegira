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

# Repository-owned validation builds disable the two largest sources of
# redundant local Cargo artifacts without changing normal developer builds.
settings_repository="$fixture/settings-repository"
validation_cache_prepare "$settings_repository" bounded-settings
test "$CARGO_INCREMENTAL" = 0
test "$CARGO_PROFILE_DEV_DEBUG" = 0
test "$CARGO_PROFILE_TEST_DEBUG" = 0
validation_cache_release

# A small fixture budget proves deterministic LRU pruning without allocating a
# machine-sized cache. The most recently used inactive cache remains warm.
budget_repository="$fixture/budget-repository"
mkdir -p "$budget_repository/target/debug"
HEGIRA_VALIDATION_CACHE_MAX_MIB=1
export HEGIRA_VALIDATION_CACHE_MAX_MIB
validation_cache_prepare_roots "$budget_repository"
mkdir "$budget_repository/target/validation/build/older"
mkdir "$budget_repository/target/validation/build/newer"
mkdir "$budget_repository/target/validation/workspaces/stale"
dd if=/dev/zero of="$budget_repository/target/validation/workspaces/stale/artifact" bs=1024 count=100 2>/dev/null
dd if=/dev/zero of="$budget_repository/target/validation/build/older/artifact" bs=1024 count=700 2>/dev/null
dd if=/dev/zero of="$budget_repository/target/validation/build/newer/artifact" bs=1024 count=700 2>/dev/null
printf '1\n' >"$budget_repository/target/validation/state/older"
printf '2\n' >"$budget_repository/target/validation/state/newer"
validation_cache_prune "$budget_repository" true >"$fixture/prune-dry-run.stdout"
grep -Fq "would remove inactive validation workspace: $budget_repository/target/validation/workspaces/stale" "$fixture/prune-dry-run.stdout"
test -d "$budget_repository/target/validation/workspaces/stale"
grep -Fq "would prune inactive validation cache: $budget_repository/target/validation/build/older" "$fixture/prune-dry-run.stdout"
test -d "$budget_repository/target/validation/build/older"
validation_cache_prune "$budget_repository" false >"$fixture/prune.stdout"
test ! -e "$budget_repository/target/validation/workspaces/stale"
test ! -e "$budget_repository/target/validation/build/older"
test -d "$budget_repository/target/validation/build/newer"

# Active caches are never selected. If they alone exceed the configured budget,
# pruning fails with an actionable diagnostic instead of deleting live state.
mkdir "$budget_repository/target/validation/locks/newer"
dd if=/dev/zero of="$budget_repository/target/validation/build/newer/active" bs=1024 count=400 2>/dev/null
if validation_cache_prune "$budget_repository" false 2>"$fixture/active-budget.stderr"; then
  echo "pruning unexpectedly accepted an over-budget active cache" >&2
  exit 1
fi
grep -Fq "no inactive cache can be reclaimed" "$fixture/active-budget.stderr"
test -f "$budget_repository/target/validation/build/newer/active"
rmdir "$budget_repository/target/validation/locks/newer"

validation_cache_status "$budget_repository" >"$fixture/status.stdout"
grep -Fq "validation cache budget: 1 MiB" "$fixture/status.stdout"
grep -Fq "repository-owned validation state:" "$fixture/status.stdout"
grep -Fq "developer-owned Cargo state:" "$fixture/status.stdout"
deferred_repository="$fixture/deferred-repository"
validation_cache_prepare "$deferred_repository" completed-check
dd if=/dev/zero of="$HEGIRA_VALIDATION_TARGET/artifact" bs=1024 count=1200 2>/dev/null
mkdir "$HEGIRA_VALIDATION_ROOT/locks/still-active"
validation_cache_release
test -e "$deferred_repository/target/validation/build/completed-check"
rmdir "$deferred_repository/target/validation/locks/still-active"
validation_cache_prune "$deferred_repository" false >"$fixture/deferred-prune.stdout"
test ! -e "$deferred_repository/target/validation/build/completed-check"

maintenance_repository="$fixture/maintenance-repository"
validation_cache_prepare_roots "$maintenance_repository"
mkdir "$maintenance_repository/target/validation/.maintenance-lock"
(
  sleep 1
  rmdir "$maintenance_repository/target/validation/.maintenance-lock"
) &
maintenance_holder=$!
validation_cache_prune "$maintenance_repository" false
wait "$maintenance_holder"

automatic_repository="$fixture/automatic-repository"
validation_cache_prepare "$automatic_repository" automatic-prune
dd if=/dev/zero of="$HEGIRA_VALIDATION_TARGET/artifact" bs=1024 count=1200 2>/dev/null
validation_cache_release >"$fixture/automatic-prune.stdout"
test ! -e "$automatic_repository/target/validation/build/automatic-prune"

grep -Fq "validation build cache:" "$fixture/status.stdout"

HEGIRA_VALIDATION_CACHE_MAX_MIB=invalid
if validation_cache_status "$budget_repository" 2>"$fixture/budget.stderr"; then
  echo "validation cache unexpectedly accepted an invalid budget" >&2
  exit 1
fi
grep -Fq "must be a positive integer" "$fixture/budget.stderr"
unset HEGIRA_VALIDATION_CACHE_MAX_MIB

unsafe_repository="$fixture/unsafe-repository"
validation_cache_prepare_roots "$unsafe_repository"
ln -s "$outside" "$unsafe_repository/target/validation/build/unsafe"
if validation_cache_prune "$unsafe_repository" false 2>"$fixture/child-symlink.stderr"; then
  echo "validation pruning unexpectedly accepted a symlinked cache" >&2
  exit 1
fi
grep -Fq "entries must be real directories" "$fixture/child-symlink.stderr"
test -f "$outside/sentinel"

for owned_check in \
  "framework-check.sh:framework-check" \
  "official-modules-check.sh:official-modules-check" \
  "cli-check.sh:cli-check" \
  "layered-template-check.sh:layered-template-check" \
  "generated-feature-check.sh:generated-feature-check" \
  "composition-matrix-check.sh:composition-matrix-check"
do
  owned_script=${owned_check%%:*}
  owned_name=${owned_check##*:}
  grep -Fq '. "$repo_root/scripts/validation-cache.sh"' "$repo_root/scripts/$owned_script"
  grep -Fq "validation_cache_prepare \"\$repo_root\" \"$owned_name\"" "$repo_root/scripts/$owned_script"
  grep -Fq 'export CARGO_TARGET_DIR="$HEGIRA_VALIDATION_TARGET"' "$repo_root/scripts/$owned_script"
done
grep -Fq 'validation_cache_prepare "$repo_root" "$check_name"' "$repo_root/scripts/generated-application-check.sh"
echo "validation cache lifecycle: ok"
