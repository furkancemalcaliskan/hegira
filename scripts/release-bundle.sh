#!/usr/bin/env sh
set -eu

script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
repo_root="$(CDPATH= cd -- "$script_dir/.." && pwd)"
cd "$repo_root"

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
archive_contents="$dist_dir/hegira-$release_ref-archive-contents.txt"

rm -rf "$bundle_dir"
rm -f "$archive" "$checksum" "$packaged_notes" "$archive_contents"

sh "$script_dir/full-stack-build-check.sh"

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
tar -tzf "$archive" >"$archive_contents"

for required_path in \
  ./hegira \
  ./db_migrator \
  ./config/production.yaml \
  ./target/site/pkg/hegira.css \
  ./target/site/pkg/hegira.js \
  ./target/site/assets/branding/hegira-logo.png \
  ./README.md \
  ./CHANGELOG.md \
  "./docs/releases/$release_ref.md"
do
  grep -Fqx "$required_path" "$archive_contents"
done

rm -f "$archive_contents"

echo "release bundle verified: $archive"
