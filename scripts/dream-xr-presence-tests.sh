#!/usr/bin/env bash
# Dream-cycle evaluator: the XR presence wire codec + pose validators.
# Covers deep xr-presence-wire (scans wire-protocol, presence-messages).
# Leaf crate, 5 light deps (serde, bytes, thiserror, tracing, nalgebra) —
# no GPU, no headset, no workspace build. Measured ~17s cold.
#
# pipefail is required: the tee would otherwise mask cargo's exit status and
# this evaluator could never fail.
set -u
set -o pipefail
if cargo test -p visionclaw-xr-presence 2>&1 | tee /tmp/vc-xr-presence-tests.log; then
  echo XR-PRESENCE-TESTS-OK
else
  echo XR-PRESENCE-TESTS-FAIL
  exit 1
fi
