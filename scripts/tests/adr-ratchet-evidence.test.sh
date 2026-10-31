#!/usr/bin/env bash
# VisionFlow ADR-2007 (ported): adr-ratchet.sh gates the closing TRANSITION (implementation_status -> complete,
# activation_status -> live) on evidence_class + acceptance_boundary, however old the record.
set -uo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
ratchet="$root/scripts/adr-ratchet.sh"
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
cd "$tmp" && git init -q . && git config user.email t@example.invalid && git config user.name t
mkdir adr
rec() { # impl activation extra-frontmatter
cat > adr/ADR-9001-x.md <<EOT
---
id: ADR-9001
title: Fixture
date: 2026-01-01
decision_status: accepted
implementation_status: $1
activation_status: $2
supersedes: []
superseded_by: []
verified_commit: abc1234
owner: test
review_trigger: never
repo: fixture
$3
---
# ADR-9001
EOT
}
fails=0
expect() { # name want-exit
  ADR_RATCHET_UNTIL= bash "$ratchet" adr "$base" HEAD >"$tmp/out" 2>&1; got=$?
  if [ "$got" = "$2" ]; then echo "ok   $1"; else echo "FAIL $1 (exit $got, want $2)"; sed 's/^/     /' "$tmp/out"; fails=$((fails+1)); fi
}
step() { git add -A && git commit -q -m "$1"; }

rec partial inactive ""; step base; base="$(git rev-parse HEAD)"
rec complete inactive "";                                                                       step c1
expect "old record closed to complete without fields fails" 1
git reset -q --hard "$base"
rec complete inactive $'evidence_class: source+test\nacceptance_boundary: local only';        step c2
expect "closed to complete with both fields passes" 0
git reset -q --hard "$base"
rec partial live "";                                                                           step c3
expect "activated to live without fields fails" 1
git reset -q --hard "$base"
rec partial live $'evidence_class: runtime\nacceptance_boundary:';                             step c4
expect "empty acceptance_boundary fails" 1
git reset -q --hard "$base"
rec partial inactive $'owner2: unrelated edit';                                                step c5
expect "edit with no closing transition passes" 0
git reset -q --hard "$base"
rec complete live "";                                                                          step c6; base2="$(git rev-parse HEAD)"
rec complete live $'owner2: later edit';                                                       step c7
base="$base2" expect "record already complete and live is not a transition" 0
# A record longer than the pipe buffer: awk stops reading at the end of the frontmatter, so a
# bare `git show | awk` dies of SIGPIPE under pipefail (exit 141) on a big file.
rec partial inactive "";                                                                       step c8; base3="$(git rev-parse HEAD)"
rec complete inactive $'evidence_class: source\nacceptance_boundary: local only'; yes 'padding line to push the record past the 64 KiB pipe buffer' | head -2000 >> adr/ADR-9001-x.md; step c9
base="$base3" expect "large record closed with both fields passes (no SIGPIPE)" 0
git reset -q --hard "$base3"
rec complete inactive ""; yes 'padding line to push the record past the 64 KiB pipe buffer' | head -2000 >> adr/ADR-9001-x.md; step c10
base="$base3" expect "large record closed without fields fails (no SIGPIPE)" 1
exit $fails
