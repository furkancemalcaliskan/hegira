#!/usr/bin/env sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
installer="$repo_root/templates/applications/layered/scripts/prepare-wasm-bindgen.sh"
cargo_leptos_version=0.3.7
node_minimum_major=22
npm_minimum_major=10
docker_minimum_major=24
compose_minimum_major=2

error() {
  echo "generated tooling: $*" >&2
  exit 1
}

major_version() {
  printf '%s\n' "$1" | sed -E 's/^[^0-9]*([0-9]+).*/\1/'
}

require_version() {
  label="$1"
  actual="$2"
  expected="$3"
  [ "$actual" = "$expected" ] || error "$label must be $expected; found ${actual:-unavailable}"
}

require_minimum_major() {
  label="$1"
  value="$2"
  minimum="$3"
  major=$(major_version "$value")
  case "$major" in '' | *[!0-9]*) error "$label version is not parseable: ${value:-unavailable}" ;; esac
  [ "$major" -ge "$minimum" ] || error "$label major version must be at least $minimum; found $value"
}

verify_prerequisites() {
  lockfile="$1"
  require_container="$2"
  expected_rust=$(awk -F '"' '/^channel = / { print $2; exit }' "$repo_root/rust-toolchain.toml")
  actual_rust=$(rustc --version 2>/dev/null | awk '{ print $2 }' || true)
  require_version "Rust" "$actual_rust" "$expected_rust"
  rustup target list --installed 2>/dev/null | grep -Fxq wasm32-unknown-unknown || error "Rust target wasm32-unknown-unknown is not installed for $expected_rust"

  actual_leptos=$(cargo leptos --version 2>/dev/null | awk '{ print $2 }' || true)
  require_version "cargo-leptos" "$actual_leptos" "$cargo_leptos_version"
  require_minimum_major "Node.js" "$(node --version 2>/dev/null || true)" "$node_minimum_major"
  require_minimum_major "npm" "$(npm --version 2>/dev/null || true)" "$npm_minimum_major"

  lockfile_version=$(awk '
    /"lockfileVersion":/ {
      value = $0
      sub(/.*"lockfileVersion":[[:space:]]*/, "", value)
      sub(/,.*/, "", value)
      print value
      exit
    }
  ' "$(dirname -- "$lockfile")/apps/web/src/package-lock.json")
  [ "$lockfile_version" = 3 ] || error "npm lockfileVersion must be 3; found ${lockfile_version:-unavailable}"

  if [ "$require_container" = true ]; then
    require_minimum_major "Docker Engine" "$(docker --version 2>/dev/null || true)" "$docker_minimum_major"
    require_minimum_major "Docker Compose" "$(docker compose version 2>/dev/null || true)" "$compose_minimum_major"
  fi
}

prepare_application() {
  application_root="$1"
  require_container="$2"
  [ -f "$application_root/Cargo.toml" ] ||
    error "generated application manifest is missing: $application_root/Cargo.toml"
  (
    cd "$application_root"
    cargo generate-lockfile
  )
  lockfile="$application_root/Cargo.lock"
  verify_prerequisites "$lockfile" "$require_container"
  "$installer" install "$lockfile" "$bin_dir"
}

if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
  error "usage: generated-toolchain.sh <prepare|verify> <Cargo.lock> [--container] | application <application-root> [--container]"
fi
command_name="$1"
lockfile="$2"
bin_dir="$repo_root/target/validation/tools/wasm-bindgen/bin"
if [ "$command_name" = application ]; then
  application_root="$lockfile"
  case "$application_root" in
    /*) ;;
    *) application_root="$repo_root/$application_root" ;;
  esac
  require_container=false
  case "${3:-}" in
    "") ;;
    --container) require_container=true ;;
    *) error "unknown option: $3" ;;
  esac
  prepare_application "$application_root" "$require_container"
  exit 0
fi
require_container=false
if [ "${3:-}" = --container ]; then
  require_container=true
elif [ "$#" -eq 3 ]; then
  error "unknown option: $3"
fi

case "$lockfile" in
  /*) ;;
  *) lockfile="$repo_root/$lockfile" ;;
esac
[ -f "$lockfile" ] || error "Cargo lockfile is missing: $lockfile"
verify_prerequisites "$lockfile" "$require_container"

case "$command_name" in
  prepare)
    "$installer" install "$lockfile" "$bin_dir"
    ;;
  verify)
    "$installer" verify "$lockfile" "$bin_dir/wasm-bindgen"
    printf '%s\n' "$bin_dir"
    ;;
  *) error "unknown command: $command_name" ;;
esac
