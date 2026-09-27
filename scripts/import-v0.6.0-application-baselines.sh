#!/usr/bin/env sh
set -eu

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <source-root> <absent-destination>" >&2
  exit 2
fi

source_root=$1
destination=$2

if [ ! -d "$source_root" ]; then
  echo "baseline source root does not exist: $source_root" >&2
  exit 1
fi
if [ -e "$destination" ]; then
  echo "baseline destination must not exist: $destination" >&2
  exit 1
fi

ids="default-postgres default-sqlite identity-added-postgres identity-added-sqlite minimal-postgres minimal-sqlite"
for id in $ids; do
  if [ ! -d "$source_root/$id" ]; then
    echo "missing baseline source: $id" >&2
    exit 1
  fi
  database=${id##*-}
  grep -Fq 'application = "baseline-application"' "$source_root/$id/hegira.toml"
  grep -Fq 'version = "v0.6.0"' "$source_root/$id/hegira.toml"
  grep -Fq 'id = "hegira-canonical"' "$source_root/$id/hegira.toml"
  grep -Fq "databases = [\"$database\"]" "$source_root/$id/hegira.toml"
  grep -Fq '?tag=v0.6.0#67d0708e135a233335969752890c23d5bb1fb3c6' "$source_root/$id/Cargo.lock"
done

if find "$source_root" -type l -print -quit | grep -q .; then
  echo "baseline sources must not contain symbolic links" >&2
  exit 1
fi
if find "$source_root" -type d \( -name .git -o -name target -o -name node_modules \) -print -quit | grep -q .; then
  echo "baseline sources contain a forbidden generated directory" >&2
  exit 1
fi
if find "$source_root" -type f \( -name .env -o -name '*.db' -o -name '*.sqlite' -o -name '*.pem' -o -name '*.key' \) -print -quit | grep -q .; then
  echo "baseline sources contain runtime state or credential-shaped files" >&2
  exit 1
fi

mkdir -p "$destination/objects" "$destination/trees"

release_entries=
for id in $ids; do
  source=$source_root/$id
  tree=$destination/trees/$id.toml
  composition=${id%-postgres}
  composition=${composition%-sqlite}
  database=${id##*-}

  {
    printf 'schema = 1\n'
    printf 'id = "%s"\n' "$id"
    printf 'composition = "%s"\n' "$composition"
    printf 'database = "%s"\n' "$database"
    printf 'client = "leptos"\n'
  } >"$tree"

  LC_ALL=C find "$source" -type f -print | LC_ALL=C sort | while IFS= read -r file; do
    relative=${file#"$source/"}
    case "$relative" in
      *[!A-Za-z0-9._/-]*|/*|*..*)
        echo "unsupported baseline path: $relative" >&2
        exit 1
        ;;
    esac
    digest=$(sha256sum "$file" | cut -d' ' -f1)
    if [ ! -e "$destination/objects/$digest" ]; then
      cp "$file" "$destination/objects/$digest"
      chmod 0644 "$destination/objects/$digest"
    fi
    executable=false
    if [ -x "$file" ]; then
      executable=true
    fi
    {
      printf '\n[[files]]\n'
      printf 'path = "%s"\n' "$relative"
      printf 'digest = "sha256:%s"\n' "$digest"
      printf 'executable = %s\n' "$executable"
    } >>"$tree"
  done

  tree_digest=$(sha256sum "$tree" | cut -d' ' -f1)
  release_entries="$release_entries

[[baselines]]
id = \"$id\"
composition = \"$composition\"
database = \"$database\"
client = \"leptos\"
tree = \"trees/$id.toml\"
tree_digest = \"sha256:$tree_digest\""
done

{
  printf 'schema = 1\n'
  printf 'tag = "v0.6.0"\n'
  printf 'tag_object = "9cb1f4aa01a16cc762653a8b170d0c4fcdb412b8"\n'
  printf 'commit = "91c522b0dcfa71502ef5b4f0beed2ac6fb676dd3"\n'
  printf 'source_tree = "65cdcaf0bd0dbebfd81c657ea8424b044d2e81e2"\n'
  printf 'framework_repository = "https://github.com/furkancemalcaliskan/hegira.git"\n'
  printf 'package_id = "hegira-canonical"\n'
  printf 'package_version = "v0.6.0"\n'
  printf 'package_digest = "sha256:cff6969dadc6e0155af7b944877dba5c61b6e0c4c3ea041d9a8a2665949c285e"\n'
  printf 'lock_revision = "67d0708e135a233335969752890c23d5bb1fb3c6"\n'
  printf 'application = "baseline-application"\n'
  printf '%s\n' "$release_entries"
} >"$destination/release.toml"

echo "v0.6.0 application baselines imported at $destination"
