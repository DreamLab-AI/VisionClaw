#!/usr/bin/env bash
# Dream-cycle evaluator: every res:// resource referenced by xr-client
# .tscn/.tres files must exist on disk. Invoked quote-free from
# dream.config.json — the annexe ssh dispatch strips nested double quotes,
# so evaluator logic must live in a checked-in script, never inline.
set -u
miss=0; total=0
for f in $(grep -rhoE 'path="res://[^"]+"' xr-client --include='*.tscn' --include='*.tres' | sed 's/path="res:\/\///;s/"$//' | sort -u); do
  total=$((total+1))
  [ -e "xr-client/$f" ] || { miss=$((miss+1)); echo "MISSING: $f"; }
done
echo "refs: $total missing: $miss"
# The failure branch MUST exit non-zero: the harness grades on exit code, and
# the old `&& echo OK || echo FAIL` tail always returned 0 (the `||` echo
# succeeds), so a dangling res:// reference was recorded PASSED.
if [ "$miss" -eq 0 ]; then
  echo SCENE-INTEGRITY-OK
else
  echo SCENE-INTEGRITY-FAIL
  exit 1
fi
