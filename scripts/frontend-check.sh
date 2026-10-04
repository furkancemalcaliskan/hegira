#!/usr/bin/env sh
set -eu

if [ "$#" -ne 0 ]; then
  echo "usage: sh scripts/frontend-check.sh" >&2
  exit 2
fi
repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

for template in layered layered-minimal; do
  frontend="$repo_root/templates/applications/$template/apps/web/src"
  echo "==> $template frontend dependency contract and audit"
  node --input-type=module - "$frontend" <<'JS'
import assert from 'node:assert/strict';
import fs from 'node:fs';
const root = process.argv[2];
const manifest = JSON.parse(fs.readFileSync(`${root}/package.json`, 'utf8'));
const lock = JSON.parse(fs.readFileSync(`${root}/package-lock.json`, 'utf8'));
assert.equal(manifest.overrides?.['@tailwindcss/cli']?.['@parcel/watcher'], '2.6.0', 'Review the scoped watcher security override');
assert.equal(lock.packages['node_modules/@parcel/watcher']?.version, '2.6.0', 'Lock must resolve the reviewed watcher');
for (const name of Object.keys(lock.packages)) {
  assert(!/\/node_modules\/(?:braces|micromatch)$/.test(`/${name}`), 'Vulnerable brace parsing must not return to the frontend graph');
}
JS
  npm audit --audit-level=high --include=dev --include=optional --include=peer \
    --prefix "$frontend"
done
