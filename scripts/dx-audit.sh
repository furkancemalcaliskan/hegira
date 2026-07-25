#!/usr/bin/env sh
set -eu

manifest="${1:-scripts/fixtures/dx/platform-contracts.manifest}"

if [ ! -f "$manifest" ]; then
  echo "DX manifest not found: $manifest" >&2
  exit 1
fi

read_limit() {
  key="$1"
  value="$(sed -n "s/^${key}=//p" "$manifest")"
  if [ -z "$value" ]; then
    echo "missing ${key} in $manifest" >&2
    exit 1
  fi
  printf '%s' "$value"
}

max_files="$(read_limit max_feature_files)"
max_loc="$(read_limit max_feature_loc)"
max_central="$(read_limit max_central_touchpoints)"
feature_paths="$(sed -n 's/^feature://p' "$manifest")"
provider_paths="$(sed -n 's/^provider://p' "$manifest")"
central_paths="$(sed -n 's/^central://p' "$manifest")"

feature_files=0
feature_loc=0
for path in $feature_paths; do
  if [ ! -f "$path" ]; then
    echo "missing feature path: $path" >&2
    exit 1
  fi
  feature_files=$((feature_files + 1))
  lines="$(wc -l < "$path")"
  feature_loc=$((feature_loc + lines))
done

provider_loc=0
for path in $provider_paths; do
  if [ ! -f "$path" ]; then
    echo "missing provider path: $path" >&2
    exit 1
  fi
  feature_files=$((feature_files + 1))
  lines="$(wc -l < "$path")"
  provider_loc=$((provider_loc + lines))
done

central_touchpoints=0
for path in $central_paths; do
  if [ ! -f "$path" ]; then
    echo "missing central path: $path" >&2
    exit 1
  fi
  central_touchpoints=$((central_touchpoints + 1))
done

echo "DX baseline: files=$feature_files loc=$feature_loc provider_loc=$provider_loc central=$central_touchpoints"

if [ "$feature_files" -gt "$max_files" ]; then
  echo "feature file budget exceeded: $feature_files > $max_files" >&2
  exit 1
fi
if [ "$feature_loc" -gt "$max_loc" ]; then
  echo "feature LOC budget exceeded: $feature_loc > $max_loc" >&2
  exit 1
fi
max_provider_loc="$(sed -n 's/^max_provider_loc=//p' "$manifest")"
if [ -n "$max_provider_loc" ] && [ "$provider_loc" -gt "$max_provider_loc" ]; then
  echo "provider LOC budget exceeded: $provider_loc > $max_provider_loc" >&2
  exit 1
fi
if [ "$central_touchpoints" -gt "$max_central" ]; then
  echo "central touchpoint budget exceeded: $central_touchpoints > $max_central" >&2
  exit 1
fi
