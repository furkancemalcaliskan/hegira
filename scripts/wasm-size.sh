#!/usr/bin/env sh
set -eu

lib_features="hydrate"
split_arg=""
if [ "${1:-}" = "--split" ]; then
  split_arg="--split"
  lib_features="hydrate,wasm-split"
fi

cargo leptos build --release --lib-features "$lib_features" $split_arg

pkg="target/site/pkg"
if [ ! -d "$pkg" ]; then
  echo "WASM package directory was not found: $pkg" >&2
  exit 1
fi

find "$pkg" -name '*.wasm' -type f -exec ls -lh {} \; | sort -k5 -hr
