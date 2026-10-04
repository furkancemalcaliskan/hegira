#!/usr/bin/env sh

# Repository-owned validation workspace and Cargo artifact lifecycle.

HEGIRA_VALIDATION_CACHE_DEFAULT_MAX_MIB=65536

validation_cache_error() {
  echo "validation cache: $*" >&2
  return 1
}

validation_cache_require_directory() {
  path="$1"
  label="$2"
  if [ -L "$path" ]; then
    validation_cache_error "$label may not be a symbolic link: $path"
    return 1
  fi
  if [ -e "$path" ] && [ ! -d "$path" ]; then
    validation_cache_error "$label must be a directory: $path"
    return 1
  fi
}

validation_cache_validate_name() {
  case "$1" in
    "" | *[!a-z0-9-]* | -* | *-)
      validation_cache_error "check names must use lowercase ASCII letters, digits, and internal hyphens"
      return 1
      ;;
  esac
}

validation_cache_budget_mib() {
  budget="${HEGIRA_VALIDATION_CACHE_MAX_MIB:-$HEGIRA_VALIDATION_CACHE_DEFAULT_MAX_MIB}"
  case "$budget" in
    "" | *[!0-9]*)
      validation_cache_error "HEGIRA_VALIDATION_CACHE_MAX_MIB must be a positive integer"
      return 1
      ;;
  esac
  if [ "$budget" -eq 0 ]; then
    validation_cache_error "HEGIRA_VALIDATION_CACHE_MAX_MIB must be greater than zero"
    return 1
  fi
  printf '%s\n' "$budget"
}

validation_cache_size_kib() {
  if [ -d "$1" ]; then
    du -sk "$1" | awk '{ print $1 }'
  else
    echo 0
  fi
}

validation_cache_prepare_roots() {
  repository_root="$1"
  validation_root="$repository_root/target/validation"
  validation_cache_require_directory "$repository_root/target" "Cargo target root" || return 1
  mkdir -p "$repository_root/target"
  validation_cache_require_directory "$validation_root" "validation root" || return 1
  mkdir -p "$validation_root"
  for directory in workspaces build locks state tools; do
    path="$validation_root/$directory"
    validation_cache_require_directory "$path" "validation $directory directory" || return 1
    mkdir -p "$path"
  done
}

validation_cache_paths() {
  repository_root="$1"
  check_name="$2"
  validation_cache_validate_name "$check_name" || return 1
  HEGIRA_VALIDATION_ROOT="$repository_root/target/validation"
  HEGIRA_VALIDATION_WORKSPACE="$HEGIRA_VALIDATION_ROOT/workspaces/$check_name"
  HEGIRA_VALIDATION_TARGET="$HEGIRA_VALIDATION_ROOT/build/$check_name"
  HEGIRA_VALIDATION_LOCK="$HEGIRA_VALIDATION_ROOT/locks/$check_name"
  export HEGIRA_VALIDATION_ROOT HEGIRA_VALIDATION_WORKSPACE HEGIRA_VALIDATION_TARGET HEGIRA_VALIDATION_LOCK
}

validation_cache_assert_owned_children() {
  parent="$1"
  label="$2"
  [ -d "$parent" ] || return 0
  for path in "$parent"/*; do
    [ -e "$path" ] || [ -L "$path" ] || continue
    name=${path##*/}
    validation_cache_validate_name "$name" || return 1
    if [ -L "$path" ] || [ ! -d "$path" ]; then
      validation_cache_error "$label entries must be real directories: $path"
      return 1
    fi
  done
}

validation_cache_assert_state() {
  state_root="$1"
  [ -d "$state_root" ] || return 0
  for marker in "$state_root"/*; do
    [ -e "$marker" ] || [ -L "$marker" ] || continue
    name=${marker##*/}
    validation_cache_validate_name "$name" || return 1
    if [ -L "$marker" ] || [ ! -f "$marker" ]; then
      validation_cache_error "validation access markers must be regular files: $marker"
      return 1
    fi
  done
}

validation_cache_record_access() {
  state_root="$1/state"
  marker="$state_root/$2"
  temporary="$state_root/.$2.$$"
  if [ -L "$marker" ] || { [ -e "$marker" ] && [ ! -f "$marker" ]; }; then
    validation_cache_error "validation access marker must be a regular file: $marker"
    return 1
  fi
  if [ -e "$temporary" ] || [ -L "$temporary" ]; then
    validation_cache_error "temporary validation access marker already exists: $temporary"
    return 1
  fi
  (umask 077 && date +%s >"$temporary") || return 1
  if ! mv -f "$temporary" "$marker"; then
    rm -f "$temporary"
    return 1
  fi
}

validation_cache_prune_locked() {
  repository_root="$1"
  dry_run="${2:-false}"
  validation_root="$repository_root/target/validation"
  build_root="$validation_root/build"
  locks_root="$validation_root/locks"
  state_root="$validation_root/state"
  budget_mib=$(validation_cache_budget_mib) || return 1
  budget_kib=$((budget_mib * 1024))
  case "$dry_run" in
    true | false) ;;
    *) validation_cache_error "prune mode must be true or false"; return 1 ;;
  esac

  validation_cache_assert_owned_children "$build_root" "validation build" || return 1
  validation_cache_assert_owned_children "$locks_root" "validation lock" || return 1
  validation_cache_assert_owned_children "$validation_root/workspaces" "validation workspace" || return 1
  validation_cache_assert_owned_children "$validation_root/tools" "validation tool" || return 1
  validation_cache_assert_state "$state_root" || return 1
  total_kib=$(validation_cache_size_kib "$validation_root")
  [ "$total_kib" -gt "$budget_kib" ] || return 0

  projected_kib=$total_kib
  for workspace in "$validation_root/workspaces"/*; do
    [ -d "$workspace" ] || continue
    check_name=${workspace##*/}
    [ ! -d "$locks_root/$check_name" ] || continue
    workspace_kib=$(validation_cache_size_kib "$workspace")
    if [ "$dry_run" = true ]; then
      echo "would remove inactive validation workspace: $workspace (${workspace_kib} KiB)"
    else
      validation_cache_require_directory "$workspace" "validation workspace" || return 1
      rm -rf "$workspace"
      echo "removed inactive validation workspace: $workspace (${workspace_kib} KiB)"
    fi
    projected_kib=$((projected_kib - workspace_kib))
  done
  if [ "$dry_run" = false ]; then
    projected_kib=$(validation_cache_size_kib "$validation_root")
  fi
  [ "$projected_kib" -gt "$budget_kib" ] || return 0

  candidates=$(mktemp "${TMPDIR:-/tmp}/hegira-validation-prune.XXXXXX") || return 1
  for path in "$build_root"/*; do
    [ -d "$path" ] || continue
    check_name=${path##*/}
    [ ! -d "$locks_root/$check_name" ] || continue
    marker="$state_root/$check_name"
    if [ -L "$marker" ] || { [ -e "$marker" ] && [ ! -f "$marker" ]; }; then
      rm -f "$candidates"
      validation_cache_error "validation access marker must be a regular file: $marker"
      return 1
    fi
    last_used=0
    if [ -f "$marker" ]; then
      IFS= read -r last_used <"$marker" || last_used=0
      case "$last_used" in "" | *[!0-9]*) last_used=0 ;; esac
    fi
    size_kib=$(validation_cache_size_kib "$path")
    printf '%s|%s|%s\n' "$last_used" "$check_name" "$size_kib" >>"$candidates"
  done
  LC_ALL=C sort -n -t '|' -k1,1 -k2,2 "$candidates" -o "$candidates"

  while IFS='|' read -r last_used check_name size_kib; do
    [ "$projected_kib" -gt "$budget_kib" ] || break
    path="$build_root/$check_name"
    [ ! -d "$locks_root/$check_name" ] || continue
    if [ "$dry_run" = true ]; then
      echo "would prune inactive validation cache: $path (${size_kib} KiB, last used ${last_used})"
    else
      validation_cache_require_directory "$path" "validation build directory" || {
        rm -f "$candidates"
        return 1
      }
      rm -rf "$path"
      rm -f "$state_root/$check_name"
      echo "pruned inactive validation cache: $path (${size_kib} KiB)"
    fi
    projected_kib=$((projected_kib - size_kib))
  done <"$candidates"
  rm -f "$candidates"

  if [ "$dry_run" = false ]; then
    remaining_kib=$(validation_cache_size_kib "$validation_root")
    if [ "$remaining_kib" -gt "$budget_kib" ]; then
      validation_cache_error "repository-owned validation state is ${remaining_kib} KiB, above the ${budget_kib} KiB budget, and no inactive cache can be reclaimed; finish active validation or raise HEGIRA_VALIDATION_CACHE_MAX_MIB explicitly"
      return 1
    fi
  fi
}

validation_cache_prune() (
  repository_root="$1"
  dry_run="${2:-false}"
  validation_cache_prepare_roots "$repository_root" || return 1
  maintenance_lock="$repository_root/target/validation/.maintenance-lock"
  validation_cache_require_directory "$maintenance_lock" "validation maintenance lock" || return 1
  attempts=0
  until mkdir "$maintenance_lock" 2>/dev/null; do
    attempts=$((attempts + 1))
    if [ "$attempts" -ge 30 ]; then
      validation_cache_error "validation cache maintenance did not become available: $maintenance_lock"
      return 1
    fi
    sleep 1
  done
  cleanup_maintenance_lock() {
    rmdir "$maintenance_lock" 2>/dev/null || true
  }
  trap cleanup_maintenance_lock EXIT INT TERM
  validation_cache_prune_locked "$repository_root" "$dry_run"
)

validation_cache_prepare() {
  repository_root="$1"
  check_name="$2"
  validation_cache_paths "$repository_root" "$check_name" || return 1
  validation_cache_prepare_roots "$repository_root" || return 1
  validation_cache_prune "$repository_root" false || return 1
  validation_cache_require_directory "$HEGIRA_VALIDATION_TARGET" "validation build directory" || return 1
  mkdir -p "$HEGIRA_VALIDATION_TARGET"

  if ! mkdir "$HEGIRA_VALIDATION_LOCK" 2>/dev/null; then
    validation_cache_error "check '$check_name' is already running or has a stale lock; remove it only after confirming no validation process is active: $HEGIRA_VALIDATION_LOCK"
    return 1
  fi
  if ! validation_cache_require_directory "$HEGIRA_VALIDATION_WORKSPACE" "validation workspace"; then
    rmdir "$HEGIRA_VALIDATION_LOCK"
    return 1
  fi
  rm -rf "$HEGIRA_VALIDATION_WORKSPACE"
  mkdir "$HEGIRA_VALIDATION_WORKSPACE"
  validation_cache_record_access "$HEGIRA_VALIDATION_ROOT" "$check_name"

  # Repository-owned validation does not retain incremental compiler state or
  # debug information. Normal developer Cargo output is not affected.
  CARGO_INCREMENTAL=0
  CARGO_PROFILE_DEV_DEBUG=0
  CARGO_PROFILE_TEST_DEBUG=0
  export CARGO_INCREMENTAL CARGO_PROFILE_DEV_DEBUG CARGO_PROFILE_TEST_DEBUG
}

validation_cache_release() {
  validation_cache_require_directory "$HEGIRA_VALIDATION_ROOT" "validation root" || return 1
  validation_cache_require_directory "$HEGIRA_VALIDATION_WORKSPACE" "validation workspace" || return 1
  validation_cache_require_directory "$HEGIRA_VALIDATION_LOCK" "validation lock" || return 1
  rm -rf "$HEGIRA_VALIDATION_WORKSPACE"
  validation_cache_record_access "$HEGIRA_VALIDATION_ROOT" "${HEGIRA_VALIDATION_LOCK##*/}" || return 1
  rmdir "$HEGIRA_VALIDATION_LOCK"
  validation_cache_assert_owned_children "$HEGIRA_VALIDATION_ROOT/locks" "validation lock" || return 1
  for active_lock in "$HEGIRA_VALIDATION_ROOT/locks"/*; do
    [ -d "$active_lock" ] && return 0
  done
  validation_cache_prune "${HEGIRA_VALIDATION_ROOT%/target/validation}" false
}

validation_cache_status() {
  repository_root="$1"
  target_root="$repository_root/target"
  validation_root="$target_root/validation"
  budget_mib=$(validation_cache_budget_mib) || return 1
  validation_cache_require_directory "$target_root" "Cargo target root" || return 1
  validation_cache_require_directory "$validation_root" "validation root" || return 1
  if [ -d "$validation_root" ]; then
    validation_cache_assert_owned_children "$validation_root/build" "validation build" || return 1
    validation_cache_assert_owned_children "$validation_root/locks" "validation lock" || return 1
    validation_cache_assert_owned_children "$validation_root/workspaces" "validation workspace" || return 1
    validation_cache_assert_owned_children "$validation_root/tools" "validation tool" || return 1
    validation_cache_assert_state "$validation_root/state" || return 1
  fi
  target_kib=$(validation_cache_size_kib "$target_root")
  validation_kib=$(validation_cache_size_kib "$validation_root")
  developer_kib=$((target_kib - validation_kib))
  [ "$developer_kib" -ge 0 ] || developer_kib=0
  echo "validation cache budget: ${budget_mib} MiB"
  echo "repository-owned validation state: ${validation_kib} KiB ($validation_root)"
  echo "developer-owned Cargo state: ${developer_kib} KiB ($target_root excluding validation/)"
  if [ -d "$validation_root/build" ]; then
    for path in "$validation_root/build"/*; do
      [ -d "$path" ] || continue
      size_kib=$(validation_cache_size_kib "$path")
      printf '%s|validation build cache: %s KiB (%s)\n' "$size_kib" "$size_kib" "$path"
    done | sort -nr | cut -d '|' -f 2-
  fi
}

validation_cache_clean() {
  repository_root="$1"
  dry_run="${2:-false}"
  validation_root="$repository_root/target/validation"
  legacy_layered="$repository_root/target/layered-template-check"
  legacy_feature="$repository_root/target/generated-feature-check"
  legacy_application="$repository_root/target/generated-application-check"

  validation_cache_require_directory "$repository_root/target" "Cargo target root" || return 1
  validation_cache_require_directory "$validation_root" "validation root" || return 1
  validation_cache_require_directory "$legacy_layered" "legacy layered-template validation cache" || return 1
  validation_cache_require_directory "$legacy_feature" "legacy generated-feature validation cache" || return 1
  validation_cache_require_directory "$legacy_application" "legacy generated-application validation cache" || return 1

  maintenance_lock="$validation_root/.maintenance-lock"
  validation_cache_require_directory "$maintenance_lock" "validation maintenance lock" || return 1
  if [ -d "$maintenance_lock" ]; then
    validation_cache_error "refusing cleanup while validation cache maintenance is active: $maintenance_lock"
    return 1
  fi

  locks="$validation_root/locks"
  validation_cache_require_directory "$locks" "validation locks directory" || return 1
  if [ -d "$locks" ] && find "$locks" -mindepth 1 -maxdepth 1 -print -quit | grep . >/dev/null; then
    validation_cache_error "refusing cleanup while a validation lock exists under $locks"
    return 1
  fi

  found=false
  for path in "$validation_root" "$legacy_layered" "$legacy_feature" "$legacy_application"; do
    if [ -d "$path" ]; then
      found=true
      if [ "$dry_run" = true ]; then
        echo "would remove repository-owned validation cache: $path"
      fi
    fi
  done
  if [ "$found" = false ]; then
    echo "repository-owned validation cache is already empty"
    return 0
  fi
  if [ "$dry_run" = true ]; then
    return 0
  fi
  if [ "$dry_run" != false ]; then
    validation_cache_error "cleanup mode must be true or false"
    return 1
  fi
  for path in "$validation_root" "$legacy_layered" "$legacy_feature" "$legacy_application"; do
    if [ -d "$path" ]; then
      rm -rf "$path"
      echo "removed repository-owned validation cache: $path"
    fi
  done
}
