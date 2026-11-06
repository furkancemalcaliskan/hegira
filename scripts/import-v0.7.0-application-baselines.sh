#!/usr/bin/env sh
set -eu

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <repository-with-v0.7.0-tag> <absent-temporary-destination>" >&2
  exit 2
fi
repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
release_repository=$(CDPATH= cd -- "$1" && pwd -P)
destination_parent=$(CDPATH= cd -- "$(dirname -- "$2")" && pwd -P)
destination="$destination_parent/$(basename -- "$2")"
case "$destination" in "$repo_root"|"$repo_root/"*) echo 'import into an absent temporary destination outside the repository' >&2; exit 1 ;; esac
[ ! -e "$destination" ] && [ ! -L "$destination" ] || { echo 'destination must be absent' >&2; exit 1; }

tag_object=f83979ef149eb1ed0017edee143cd502c070ce07
release_commit=635c163d50375f7d250311426c67413da382ed23
release_tree=2547cb85aae856024771cbe4432a2a88e4703574
[ "$(git -C "$release_repository" cat-file -t refs/tags/v0.7.0)" = tag ]
[ "$(git -C "$release_repository" rev-parse refs/tags/v0.7.0)" = "$tag_object" ]
[ "$(git -C "$release_repository" rev-parse 'refs/tags/v0.7.0^{}')" = "$release_commit" ]
[ "$(git -C "$release_repository" rev-parse 'refs/tags/v0.7.0^{tree}')" = "$release_tree" ]

. "$repo_root/scripts/validation-cache.sh"
validation_cache_prepare "$repo_root" baseline-import-v0-7-0
cleanup() {
  result=$?
  trap - EXIT INT TERM
  validation_cache_release || result=1
  exit "$result"
}
trap cleanup EXIT INT TERM

# Public key reviewed against GitHub's verified, immutable v0.7.0 release tag.
# Signature verification is local and does not depend on the owner's current keys.
printf '%s\n' 'furkancemalcaliskan ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIIcF1QFoefqa45Iw5uwaEjBxfBJPKodnBmROpcf6ViX4' > "$HEGIRA_VALIDATION_WORKSPACE/allowed-signers"
git -C "$release_repository" -c gpg.ssh.allowedSignersFile="$HEGIRA_VALIDATION_WORKSPACE/allowed-signers" verify-tag "$tag_object"

source="$HEGIRA_VALIDATION_WORKSPACE/released-source"
archive="$HEGIRA_VALIDATION_WORKSPACE/source.tar"
git -C "$release_repository" archive --format=tar --output="$archive" "$release_commit"
mkdir "$source"
tar -xf "$archive" -C "$source"
grep -Fq 'content_digest = "sha256:34ddf6356d441791c93f306f5ea6aeb5927cab1062d2e6d7f8b8928ee198b053"' "$source/templates/package.toml"
locked=$(sha256sum "$source/Cargo.lock")
CARGO_TARGET_DIR="$HEGIRA_VALIDATION_TARGET" cargo build --locked --manifest-path "$source/Cargo.toml" -p hegira_cli --bin hegira
[ "$(sha256sum "$source/Cargo.lock")" = "$locked" ]

outputs="$HEGIRA_VALIDATION_WORKSPACE/outputs"
mkdir "$outputs"
for composition in default minimal identity-added; do
  for database in postgres sqlite; do
    application="$outputs/$composition-$database"
    if [ "$composition" = default ]; then
      "$HEGIRA_VALIDATION_TARGET/debug/hegira" new baseline-application --destination "$application" --database "$database"
    else
      "$HEGIRA_VALIDATION_TARGET/debug/hegira" new baseline-application --destination "$application" --database "$database" --composition minimal
    fi
    if [ "$composition" = identity-added ]; then
      "$HEGIRA_VALIDATION_TARGET/debug/hegira" component add identity --application-root "$application"
      # Match the established v0.6.0 baseline convention without resolving new
      # dependencies: the released default lock contains this Identity graph.
      cp "$outputs/default-$database/Cargo.lock" "$application/Cargo.lock"
    fi
  done
done

# This current test-support example only packs data. Rendering above exclusively
# uses the authenticated released source and its released mutation publisher.
CARGO_TARGET_DIR="$HEGIRA_VALIDATION_TARGET" cargo run --locked --manifest-path "$repo_root/Cargo.toml" -p upgrade_test_support --example import_v0_7_0 -- "$outputs" "$destination"
echo "Authenticated v0.7.0 baselines imported at $destination"
