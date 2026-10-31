#!/usr/bin/env bash
# VisionFlow ADR-2007 (ported): adr-index-gen.js --check validates evidence_class as one or more of
# EVIDENCE_CLASSES joined by '+'. Four cases: valid single, valid joined, unknown value, empty segment.
set -uo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
gen="$root/scripts/adr-index-gen.js"
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
mkdir "$tmp/adr"
rec() { # evidence_class value
cat > "$tmp/adr/ADR-9001-x.md" <<EOR
---
id: ADR-9001
title: Fixture
date: 2026-01-01
decision_status: accepted
implementation_status: complete
activation_status: live
supersedes: []
superseded_by: []
verified_commit: abc1234
owner: test
review_trigger: never
repo: visionclaw
evidence_class: $1
acceptance_boundary: fixture only
---
# ADR-9001
EOR
}
fails=0
expect() { # name value want-exit
  rec "$2"
  node "$gen" "$tmp/adr" --check >"$tmp/out" 2>&1; got=$?
  if [ "$3" = 1 ] && ! grep -q "evidence_class" "$tmp/out"; then got=bad-reason; fi
  if [ "$got" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1 (exit $got, want $3)"; sed 's/^/     /' "$tmp/out"; fails=$((fails+1)); fi
}
expect "valid single value passes" "source" 0
expect "valid +-joined value passes" "source+test" 0
expect "unknown value (vibes) fails" "vibes" 1
expect "empty segment (source+) fails" "source+" 1
exit "$fails"
