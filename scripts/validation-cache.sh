#!/usr/bin/env sh

# Repository-owned validation workspace and Cargo artifact lifecycle.

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

validation_cache_prepare() {
  repository_root="$1"
  check_name="$2"
  validation_cache_paths "$repository_root" "$check_name" || return 1

  validation_cache_require_directory "$repository_root/target" "Cargo target root" || return 1
  mkdir -p "$repository_root/target"
  validation_cache_require_directory "$HEGIRA_VALIDATION_ROOT" "validation root" || return 1
  mkdir -p "$HEGIRA_VALIDATION_ROOT"

  for directory in workspaces build locks; do
    path="$HEGIRA_VALIDATION_ROOT/$directory"
    validation_cache_require_directory "$path" "validation $directory directory" || return 1
    mkdir -p "$path"
  done

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
}

validation_cache_release() {
  validation_cache_require_directory "$HEGIRA_VALIDATION_ROOT" "validation root" || return 1
  validation_cache_require_directory "$HEGIRA_VALIDATION_WORKSPACE" "validation workspace" || return 1
  validation_cache_require_directory "$HEGIRA_VALIDATION_LOCK" "validation lock" || return 1
  rm -rf "$HEGIRA_VALIDATION_WORKSPACE"
  rmdir "$HEGIRA_VALIDATION_LOCK"
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
