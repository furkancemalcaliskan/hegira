#!/bin/sh
set -eu

[ "$#" -eq 1 ] || { echo 'usage: sh scripts/ci-tools.sh <cli|frontend>' >&2; exit 2; }
case "$1" in cli|frontend) ;; *) echo 'unsupported CI tool selection' >&2; exit 2 ;; esac
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
: "${RUNNER_TEMP:?select a disposable absolute runner tool directory}"
case "$RUNNER_TEMP" in /*) ;; *) exit 2 ;; esac
RUNNER_TEMP=$(CDPATH= cd -- "$RUNNER_TEMP" && pwd -P)
export RUNNER_TEMP
case "$RUNNER_TEMP/" in "$root/"*) echo 'runner tools must be outside the application' >&2; exit 2 ;; esac

if [ "$1" = cli ]; then
  : "${HEGIRA_SOURCE:?select the reviewed complete CLI source checkout}"
  release_commit=$(git -C "$HEGIRA_SOURCE" rev-parse --verify "refs/tags/{{framework_version}}^{commit}")
  [ "$(git -C "$HEGIRA_SOURCE" rev-parse HEAD)" = "$release_commit" ] || {
    echo 'CI CLI source does not match the selected release' >&2; exit 1;
  }
  CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 \
    cargo build --locked --manifest-path "$HEGIRA_SOURCE/Cargo.toml" \
    -p hegira_cli --bin hegira --target-dir "$RUNNER_TEMP/hegira-ci-bootstrap"
  exit 0
fi

# These are job-owned tool installations, outside the application cache budget.
sh scripts/prepare-wasm-bindgen.sh install Cargo.lock "$RUNNER_TEMP/hegira-ci-wasm/bin"
destination="$RUNNER_TEMP/hegira-ci-binaryen"
[ ! -e "$destination" ] && [ ! -L "$destination" ] || {
  echo 'Binaryen tool destination must be absent' >&2; exit 1;
}
mkdir -m 700 "$destination"
archive="$destination/binaryen.tar.gz"
curl --fail --location --silent --show-error --proto '=https' --tlsv1.2 \
  --connect-timeout 15 --max-time 300 --retry 3 --output "$archive" \
  https://github.com/WebAssembly/binaryen/releases/download/version_123/binaryen-version_123-x86_64-linux.tar.gz
printf '%s  %s\n' e959f2170af4c20c552e9de3a0253704d6a9d2766e8fdb88e4d6ac4bae9388fe "$archive" | sha256sum --check --status
tar -xzf "$archive" -C "$destination" --strip-components=1
rm "$archive"
version=$("$destination/bin/wasm-opt" --version)
case "$version" in
  'wasm-opt version 123'|'wasm-opt version 123 (version_123)') ;;
  *) echo 'CI requires wasm-opt version 123' >&2; exit 1 ;;
esac
npm ci --prefix apps/web/src
