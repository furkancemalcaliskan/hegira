#!/usr/bin/env sh
set -eu

error() {
  echo "generated tooling: $*" >&2
  exit 1
}

locked_version() {
  lockfile="$1"
  [ -f "$lockfile" ] || error "Cargo lockfile is missing: $lockfile"
  count=$(grep -c '^name = "wasm-bindgen"$' "$lockfile" || true)
  [ "$count" -eq 1 ] || error "Cargo lockfile must contain exactly one wasm-bindgen package: $lockfile"
  version=$(awk '
    $0 == "name = \"wasm-bindgen\"" { package = 1; next }
    package && /^version = "/ {
      value = $0
      sub(/^version = "/, "", value)
      sub(/"$/, "", value)
      print value
      exit
    }
  ' "$lockfile")
  [ -n "$version" ] || error "wasm-bindgen version is missing from Cargo lockfile: $lockfile"
  printf '%s\n' "$version"
}

platform_contract() {
  version="$1"
  machine=$(uname -m)
  system=$(uname -s)
  case "$version:$system:$machine" in
    0.2.128:Linux:x86_64)
      asset="wasm-bindgen-0.2.128-x86_64-unknown-linux-musl.tar.gz"
      checksum="b51f0208fdff83515a787bd8ab9ac5865ed84dabb66d0c709957bb59793c645f"
      ;;
    0.2.128:Linux:aarch64 | 0.2.128:Linux:arm64)
      asset="wasm-bindgen-0.2.128-aarch64-unknown-linux-musl.tar.gz"
      checksum="079731dd1bc7798c1efa4f08fcc45130827cbcc9ff60a0b4c6047d64fc6fd25c"
      ;;
    0.2.128:Darwin:x86_64)
      asset="wasm-bindgen-0.2.128-x86_64-apple-darwin.tar.gz"
      checksum="59d9af11d0a61b8019898d555de31153c3a50e7f1797e9849fb38589d16add43"
      ;;
    0.2.128:Darwin:arm64 | 0.2.128:Darwin:aarch64)
      asset="wasm-bindgen-0.2.128-aarch64-apple-darwin.tar.gz"
      checksum="67ba17f260977725c0b541b516dbb5153538140f079a900329fb6077661b47ab"
      ;;
    *)
      error "no authenticated wasm-bindgen CLI asset is declared for version $version on $system/$machine"
      ;;
  esac
  printf '%s\t%s\n' "$asset" "$checksum"
}

verify_binary() {
  binary="$1"
  expected="$2"
  [ -x "$binary" ] || return 1
  actual=$("$binary" --version 2>/dev/null || true)
  [ "$actual" = "wasm-bindgen $expected" ]
}

checksum_file() {
  expected="$1"
  archive="$2"
  if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$archive" | awk '{ print $1 }')
  elif command -v shasum >/dev/null 2>&1; then
    actual=$(shasum -a 256 "$archive" | awk '{ print $1 }')
  else
    error "sha256sum or shasum is required to authenticate wasm-bindgen CLI"
  fi
  [ "$actual" = "$expected" ] || error "wasm-bindgen CLI checksum mismatch"
}

install_cli() {
  lockfile="$1"
  bin_dir="$2"
  version=$(locked_version "$lockfile")
  binary="$bin_dir/wasm-bindgen"
  if verify_binary "$binary" "$version"; then
    printf '%s\n' "$bin_dir"
    return 0
  fi

  command -v curl >/dev/null 2>&1 || error "curl is required to prepare wasm-bindgen CLI"
  contract=$(platform_contract "$version")
  asset=${contract%%	*}
  checksum=${contract#*	}
  [ "$asset" != "$contract" ] || error "invalid wasm-bindgen platform contract"
  url="https://github.com/wasm-bindgen/wasm-bindgen/releases/download/$version/$asset"
  parent=$(dirname -- "$bin_dir")
  mkdir -p "$parent"
  temporary=$(mktemp -d "$parent/.wasm-bindgen.XXXXXX") || error "could not create private wasm-bindgen staging directory"
  trap 'rm -rf "$temporary"' EXIT INT TERM
  archive="$temporary/$asset"
  curl --fail --location --silent --show-error \
    --proto '=https' --tlsv1.2 \
    --connect-timeout 15 --max-time 300 \
    --retry 3 --retry-delay 2 --retry-connrefused --retry-all-errors \
    --output "$archive" "$url" || error "could not download authenticated wasm-bindgen CLI $version after bounded retries"
  checksum_file "$checksum" "$archive"
  tar -xzf "$archive" -C "$temporary"
  extracted="$temporary/wasm-bindgen-$version-${asset#wasm-bindgen-$version-}"
  extracted=${extracted%.tar.gz}
  [ -d "$extracted" ] || error "wasm-bindgen CLI archive has an unexpected layout"
  staged="$temporary/bin"
  mkdir "$staged"
  for name in wasm-bindgen wasm-bindgen-test-runner wasm2es6js; do
    [ -f "$extracted/$name" ] || error "wasm-bindgen CLI archive is missing $name"
    install -m 0755 "$extracted/$name" "$staged/$name"
  done
  verify_binary "$staged/wasm-bindgen" "$version" || error "prepared wasm-bindgen CLI has an unexpected version"
  mkdir -p "$bin_dir"
  for name in wasm-bindgen wasm-bindgen-test-runner wasm2es6js; do
    destination="$bin_dir/$name"
    temporary_destination="$bin_dir/.$name.$$"
    install -m 0755 "$staged/$name" "$temporary_destination"
    mv "$temporary_destination" "$destination"
  done
  verify_binary "$binary" "$version" || error "published wasm-bindgen CLI has an unexpected version"
  trap - EXIT INT TERM
  rm -rf "$temporary"
  printf '%s\n' "$bin_dir"
}

if [ "$#" -lt 2 ]; then
  error "usage: prepare-wasm-bindgen.sh <version|verify|install> <Cargo.lock> [binary-or-bin-directory]"
fi

command_name="$1"
lockfile="$2"
case "$command_name" in
  version)
    [ "$#" -eq 2 ] || error "usage: prepare-wasm-bindgen.sh version <Cargo.lock>"
    locked_version "$lockfile"
    ;;
  verify)
    [ "$#" -eq 3 ] || error "usage: prepare-wasm-bindgen.sh verify <Cargo.lock> <wasm-bindgen-binary>"
    expected=$(locked_version "$lockfile")
    verify_binary "$3" "$expected" || error "wasm-bindgen CLI must exactly match lockfile version $expected"
    ;;
  install)
    [ "$#" -eq 3 ] || error "usage: prepare-wasm-bindgen.sh install <Cargo.lock> <bin-directory>"
    install_cli "$lockfile" "$3"
    ;;
  *)
    error "unknown command: $command_name"
    ;;
esac
