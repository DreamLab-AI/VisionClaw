#!/usr/bin/env bash
# GUT guard for CI and the HP rig: GUT skips any test script that fails to
# parse and still exits 0, so a broken script (or a vendored GUT file that does
# not parse on this engine) can hide tests silently. This check fails when:
#   * the import or the GUT run logged a GDScript Parse/Compile error, or
#   * GUT's "Scripts N" total differs from the test_*.gd files on disk.
#
# Usage (from xr-client/):  bash tests/gut_guard.sh <gut.log> [<import.log> ...]
set -euo pipefail
cd "$(dirname "$0")/.."

gut_log="${1:?usage: gut_guard.sh <gut.log> [<import.log> ...]}"
fail=0

for log in "$@"; do
  if grep -nE 'Parse Error|Compile Error' "$log"; then
    echo "gut_guard: GDScript parse/compile error logged in $log" >&2
    fail=1
  fi
done

expected=$(find tests/unit -name 'test_*.gd' | wc -l | tr -d ' ')
actual=$(sed -n -E 's/^Scripts[[:space:]]+([0-9]+).*/\1/p' "$gut_log" | tail -1)
if [ -z "$actual" ]; then
  echo "gut_guard: no 'Scripts N' total in $gut_log (did GUT run?)" >&2
  fail=1
elif [ "$actual" != "$expected" ]; then
  echo "gut_guard: GUT ran $actual test scripts but tests/unit has $expected test_*.gd files — a script was skipped" >&2
  fail=1
else
  echo "gut_guard: $actual/$expected test scripts ran; no parse errors"
fi
exit "$fail"
