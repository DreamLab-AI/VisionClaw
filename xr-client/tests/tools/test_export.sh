#!/usr/bin/env bash
# tools/export.sh: a commit becomes an immutable, stamped tree with everything
# the gdext build needs, and the refusals hold. Runs against this repository's
# own history in a throwaway directory.
set -uo pipefail
here="$(cd "$(dirname "$0")/../.." && pwd)"
repo="$(git -C "$here" rev-parse --show-toplevel)"
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
pass=0; fail=0
check() { if [ "$2" = 0 ]; then echo "ok   - $1"; pass=$((pass + 1)); else echo "FAIL - $1"; fail=$((fail + 1)); fi; }

sha="$(git -C "$repo" rev-parse HEAD)"
out="$(bash "$here/tools/export.sh" HEAD "$tmp/root" 2>&1)"; rc=$?
dest="$tmp/root/vc-${sha:0:9}"
check "HEAD exports to vc-<sha9> and prints it" "$(if [ $rc = 0 ] && [ "$out" = "$dest" ] && [ -d "$dest" ]; then echo 0; else echo 1; fi)"
missing=0
for f in xr-client/project.godot xr-client/rust/Cargo.toml xr-client/rust/Cargo.lock \
         crates/visionclaw-xr-presence/Cargo.toml crates/visionclaw-tri-layout/Cargo.toml \
         crates/visionclaw-splat/Cargo.toml crates/visionclaw-wire/Cargo.toml; do
  [ -f "$dest/$f" ] || { echo "     missing $f"; missing=1; }
done
check "the export carries xr-client, its lockfile and every path-dependency crate" "$missing"
check "the stamp names the full commit" "$(if grep -q "\"sha\": \"$sha\"" "$dest/xr-client/build_stamp.json"; then echo 0; else echo 1; fi)"
check "the stamp carries a UTC export time" "$(if grep -Eq '"exported_at": "[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:]{8}Z"' "$dest/xr-client/build_stamp.json"; then echo 0; else echo 1; fi)"
check "nothing outside the listed paths is exported" "$(if [ "$(ls "$dest" | tr '\n' ' ')" = "crates xr-client " ]; then echo 0; else echo 1; fi)"
check "no build products or ignored files ride along" "$(if [ ! -e "$dest/xr-client/rust/target" ] && [ ! -e "$dest/xr-client/.godot" ]; then echo 0; else echo 1; fi)"

bash "$here/tools/export.sh" HEAD "$tmp/root" >/dev/null 2>&1; rc=$?
check "an existing export is never overwritten" "$(if [ $rc = 1 ]; then echo 0; else echo 1; fi)"
bash "$here/tools/export.sh" no-such-ref-xyz "$tmp/root" >/dev/null 2>&1; rc=$?
check "an unknown ref is refused" "$(if [ $rc = 1 ]; then echo 0; else echo 1; fi)"
# The first commit that tracked the lockfile; its parent has none.
lock_commit="$(git -C "$repo" log --diff-filter=A --format=%H -- xr-client/rust/Cargo.lock | tail -1)"
if [ -n "$lock_commit" ]; then
  bash "$here/tools/export.sh" "$lock_commit^" "$tmp/root" >/dev/null 2>&1; rc=$?
  check "a commit without the tracked lockfile is refused" "$(if [ $rc = 1 ] && [ ! -e "$tmp/root/vc-$(git -C "$repo" rev-parse --short=9 "$lock_commit^")" ]; then echo 0; else echo 1; fi)"
fi
bash "$here/tools/export.sh" >/dev/null 2>&1; rc=$?
check "missing arguments are a usage error" "$(if [ $rc = 2 ]; then echo 0; else echo 1; fi)"

echo "export: $pass passed, $fail failed"
[ "$fail" = 0 ]
