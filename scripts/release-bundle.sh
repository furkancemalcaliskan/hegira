#!/usr/bin/env sh
set -eu

package_version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)"
expected_ref="v$package_version"
release_ref="${1:-$expected_ref}"

if [ "$release_ref" != "$expected_ref" ]; then
  echo "release ref $release_ref does not match package version $expected_ref" >&2
  exit 1
fi

release_notes="docs/releases/$release_ref.md"
if [ ! -s "$release_notes" ]; then
  echo "release notes not found: $release_notes" >&2
  exit 1
fi

dist_dir="dist"
bundle_dir="$dist_dir/bundle"
archive="$dist_dir/hegira-$release_ref-linux-x86_64.tar.gz"
checksum="$archive.sha256"
packaged_notes="$dist_dir/hegira-$release_ref-release-notes.md"
frontend_dir="crates/web/src"
frontend_bin="$PWD/$frontend_dir/node_modules/.bin"

rm -rf "$bundle_dir"
rm -f "$archive" "$checksum" "$packaged_notes"

npm ci --prefix "$frontend_dir"
PATH="$frontend_bin:$PATH"
export PATH

tailwind_bin="$(command -v tailwindcss || true)"
if [ "$tailwind_bin" != "$frontend_bin/tailwindcss" ]; then
  echo "repository-local tailwindcss not found after npm ci: $frontend_bin/tailwindcss" >&2
  exit 1
fi

cargo build --locked --release -p db_migrator \
  --no-default-features --features ssr,db-postgres
cargo leptos build --release \
  --bin-features ssr,db-postgres \
  --lib-features hydrate

mkdir -p "$bundle_dir/config" "$bundle_dir/target"
cp target/release/hegira target/release/db_migrator "$bundle_dir/"
cp config/production.yaml "$bundle_dir/config/"
cp -R target/site "$bundle_dir/target/site"
cp LICENSE.txt README.md CHANGELOG.md "$bundle_dir/"
cp -R docs "$bundle_dir/docs"
cp "$release_notes" "$packaged_notes"

tar -C "$bundle_dir" -czf "$archive" .
sha256sum "$archive" >"$checksum"
sha256sum --check "$checksum"

for required_path in \
  ./hegira \
  ./db_migrator \
  ./config/production.yaml \
  ./target/site/pkg/hegira.css \
  ./target/site/pkg/hegira.js \
  ./README.md \
  ./CHANGELOG.md \
  "./docs/releases/$release_ref.md"
do
  tar -tzf "$archive" | grep -Fqx "$required_path"
done

echo "release bundle verified: $archive"
