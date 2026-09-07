#!/usr/bin/env bash
# Dream-cycle evaluator: socket_flow wire messages + V3 frame codec.
# Covers deep socket-flow-handlers (scans position-updates, handler-types).
# Leaf crate — no GPU, no headset, no workspace build. Measured ~21s cold.
#
# pipefail is required: the tee would otherwise mask cargo's exit status and
# this evaluator could never fail.
set -u
set -o pipefail
if cargo test -p visionclaw-protocol 2>&1 | tee /tmp/vc-protocol-tests.log; then
  echo PROTOCOL-TESTS-OK
else
  echo PROTOCOL-TESTS-FAIL
  exit 1
fi
