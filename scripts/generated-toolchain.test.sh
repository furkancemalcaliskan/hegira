#!/usr/bin/env sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
installer="$repo_root/templates/applications/layered/scripts/prepare-wasm-bindgen.sh"
fixture=$(mktemp -d)
cleanup() {
  rm -rf "$fixture"
}
trap cleanup EXIT INT TERM

expected_wasm_bindgen=0.2.128
actual=$("$installer" version "$repo_root/templates/applications/layered/Cargo.lock")
test "$actual" = "$expected_wasm_bindgen"
test "$("$installer" version "$repo_root/templates/applications/layered-minimal/Cargo.lock")" = "$expected_wasm_bindgen"

cat >"$fixture/Cargo.lock" <<'EOF'
version = 4

[[package]]
name = "wasm-bindgen"
version = "0.2.128"
EOF
cat >"$fixture/wasm-bindgen" <<'EOF'
#!/usr/bin/env sh
echo "wasm-bindgen 0.2.128"
EOF
chmod +x "$fixture/wasm-bindgen"
"$installer" verify "$fixture/Cargo.lock" "$fixture/wasm-bindgen"

cat >"$fixture/wrong-wasm-bindgen" <<'EOF'
#!/usr/bin/env sh
echo "wasm-bindgen 0.2.127"
EOF
chmod +x "$fixture/wrong-wasm-bindgen"
if "$installer" verify "$fixture/Cargo.lock" "$fixture/wrong-wasm-bindgen" >"$fixture/mismatch.stdout" 2>"$fixture/mismatch.stderr"; then
  echo "generated tooling test: mismatched wasm-bindgen unexpectedly passed" >&2
  exit 1
fi
test ! -s "$fixture/mismatch.stdout"
grep -Fxq "generated tooling: wasm-bindgen CLI must exactly match lockfile version 0.2.128" "$fixture/mismatch.stderr"

cat >"$fixture/duplicate.lock" <<'EOF'
version = 4

[[package]]
name = "wasm-bindgen"
version = "0.2.128"

[[package]]
name = "wasm-bindgen"
version = "0.2.127"
EOF
if "$installer" version "$fixture/duplicate.lock" >"$fixture/duplicate.stdout" 2>"$fixture/duplicate.stderr"; then
  echo "generated tooling test: duplicate wasm-bindgen packages unexpectedly passed" >&2
  exit 1
fi
grep -Fq "must contain exactly one wasm-bindgen package" "$fixture/duplicate.stderr"

cmp "$repo_root/rust-toolchain.toml" "$repo_root/templates/applications/layered/rust-toolchain.toml"
cmp "$repo_root/.node-version" "$repo_root/templates/applications/layered/.node-version"

for dockerfile in "$repo_root/templates/applications/layered/Dockerfile" "$repo_root/templates/applications/layered-minimal/Dockerfile"; do
  grep -Fxq "FROM node:22-bookworm-slim AS node" "$dockerfile"
  grep -Fxq "FROM rust:1.96.1-bookworm AS tooling" "$dockerfile"
  grep -Fxq "FROM tooling AS builder" "$dockerfile"
  grep -Fq "prepare-wasm-bindgen.sh install Cargo.lock /opt/wasm-bindgen/bin" "$dockerfile"
  prepare_line=$(grep -n "prepare-wasm-bindgen.sh install" "$dockerfile" | cut -d: -f1)
  builder_line=$(grep -n "^FROM tooling AS builder$" "$dockerfile" | cut -d: -f1)
  source_line=$(grep -n "^COPY \. \.$" "$dockerfile" | cut -d: -f1)
  test "$prepare_line" -lt "$builder_line"
  test "$prepare_line" -lt "$source_line"
done

grep -Fq -- "--retry 3" "$installer"
grep -Fq "wasm-bindgen CLI checksum mismatch" "$installer"
grep -Fq 'scripts/generated-toolchain.sh" application' \
  "$repo_root/scripts/layered-template-check.sh"
grep -Fq 'scripts/generated-toolchain.sh" application' \
  "$repo_root/scripts/generated-application-check.sh"
grep -Fq "82d12bb940e2d4e72e0d5605387fc1b8ca179044e012b620f0ce4e7440e8320e" \
  "$installer"
grep -Fq "node-version-file: .node-version" "$repo_root/.github/workflows/backend.yml"
grep -Fq "node-version-file: .node-version" "$repo_root/.github/workflows/release.yml"

# Concurrent gates can require different locks: verifying one must not replace
# the other's versioned executable or resolve through the previous global bin.
tool_repository="$fixture/tool-repository"
mkdir -p "$tool_repository/scripts" "$tool_repository/templates/applications/layered/scripts" "$fixture/tool-stubs"
cp "$repo_root/scripts/generated-toolchain.sh" "$tool_repository/scripts/"
cp "$installer" "$tool_repository/templates/applications/layered/scripts/"
cp "$repo_root/rust-toolchain.toml" "$tool_repository/"
for tool in rustc rustup cargo node npm; do
  cat >"$fixture/tool-stubs/$tool" <<'EOF'
#!/bin/sh
case "${0##*/}" in
  rustc) echo 'rustc 1.96.1' ;;
  rustup) echo wasm32-unknown-unknown ;;
  cargo) echo 'cargo-leptos 0.3.7' ;;
  node) echo v22.0.0 ;;
  npm) echo 10.0.0 ;;
esac
EOF
  chmod +x "$fixture/tool-stubs/$tool"
done
for version in 0.2.128 0.2.130 0.2.128; do
  lock_root="$fixture/lock-$version"
  mkdir -p "$lock_root/apps/web/src"
  printf '[[package]]\nname = "wasm-bindgen"\nversion = "%s"\n' "$version" >"$lock_root/Cargo.lock"
  # Match the production lockfileVersion reader's line-oriented JSON format.
  printf '{\n  "lockfileVersion": 3,\n  "packages": {}\n}\n' >"$lock_root/apps/web/src/package-lock.json"
  selected="$tool_repository/target/validation/tools/wasm-bindgen/$version/bin"
  mkdir -p "$selected"
  printf '#!/bin/sh\necho "wasm-bindgen %s"\n' "$version" >"$selected/wasm-bindgen"
  chmod +x "$selected/wasm-bindgen"
  actual=$(PATH="$fixture/tool-stubs:$PATH" sh "$tool_repository/scripts/generated-toolchain.sh" verify "$lock_root/Cargo.lock")
  test "$actual" = "$selected"
done

echo "generated tooling contract: ok"
