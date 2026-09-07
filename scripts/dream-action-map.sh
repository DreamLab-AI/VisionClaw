#!/usr/bin/env bash
# Dream-cycle evaluator: the OpenXR action map must exist and be non-empty.
# Checked-in script because the annexe ssh dispatch strips nested double
# quotes from inline entrypoints.
set -u
echo "openxr actions: $(grep -c OpenXRAction xr-client/openxr_action_map.tres)"
# The failure branch MUST exit non-zero: the harness grades on exit code, and
# the old `&& echo OK || echo MISSING` tail always returned 0 (the `||` echo
# succeeds), so a deleted action map was recorded PASSED.
if [ -s xr-client/openxr_action_map.tres ]; then
  echo MAP-OK
else
  echo MAP-MISSING
  exit 1
fi
