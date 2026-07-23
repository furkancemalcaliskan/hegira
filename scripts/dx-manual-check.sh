#!/usr/bin/env sh
set -eu

./scripts/dx-audit.sh scripts/fixtures/dx/catalog-products.manifest

for path in \
  crates/application_contracts/src/features.rs \
  crates/presentation/src/http/feature.rs \
  crates/web/src/shared/crud/dialog.rs \
  crates/web/src/shared/crud/state.rs
do
  test -f "$path" || { echo "missing manual DX API: $path" >&2; exit 1; }
done

cargo check -p hegira --no-default-features --features ssr,db-sqlite
cargo check -p hegira --no-default-features --features ssr,db-postgres
cargo check -p hegira --no-default-features --features hydrate,db-sqlite

echo "manual feature workflow contracts passed"
