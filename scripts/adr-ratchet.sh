#!/usr/bin/env bash
# adr-ratchet — no new decision record unless it closes a proposed one.
#
# The 2026-09-21 planning cycle's rule (VisionFlow estate plan, Track A item 4):
# the estate carries dozens of `decision_status: proposed` ADRs, and new ones
# kept landing faster than old ones were decided. The frontmatter checker
# validates FORMAT, so it could never see this. This script counts, over a git
# range, the ADR files ADDED UNDECIDED against the ADRs that LEFT `proposed`
# (accepted, rejected, superseded, withdrawn …), and fails when more were added
# than closed. A new proposal that closes a proposed one therefore passes; a new
# proposal on its own does not.
#
# A record added already decided (any `decision_status:` other than `proposed`,
# e.g. accepted with its implementation in the same change) is reported but not
# counted: it does not grow the backlog the rule exists to shrink, and counting
# it pushed finished decisions into amendments of unrelated records. A record
# with no `decision_status:` counts as undecided.
#
# The rule's one exception ("or documents the work above", i.e. the cycle's own
# tracks) is claimed per record with a commit trailer in the range, e.g.
#   ADR-Ratchet: ADR-2119 documents Track A item 5 (execution journal)
# which exempts exactly the record whose number it names, and is printed so the
# claim stays reviewable.
#
# Usage: scripts/adr-ratchet.sh <adr-dir> <base-rev> [<head-rev>]
#   <base-rev> of all zeros (a new branch's push) or an unknown rev: skipped.
#   ADR_RATCHET_UNTIL=YYYY-MM-DD  the rule's end date; after it the script
#                                 reports and exits 0. Empty means no end.
# Identical copies live in each estate repository that keeps a ledger; the
# frontmatter field it reads (`decision_status:`) is the estate's common one.
set -euo pipefail

dir="${1:?usage: adr-ratchet.sh <adr-dir> <base-rev> [<head-rev>]}"
base="${2:?usage: adr-ratchet.sh <adr-dir> <base-rev> [<head-rev>]}"
head="${3:-HEAD}"
until="${ADR_RATCHET_UNTIL-2026-10-20}"

if [[ "$base" =~ ^0+$ ]] || ! git cat-file -e "$base^{commit}" 2>/dev/null; then
  echo "adr-ratchet: base '$base' is not a known commit; nothing to compare (skipped)"
  exit 0
fi

# decision_status from the first frontmatter block of <rev>:<path>.
status_at() {
  git show "$1:$2" 2>/dev/null \
    | awk 'NR==1 && $0!="---"{exit} /^---$/{n++; if(n==2) exit; next} n==1 && /^decision_status:/{sub(/^decision_status:[ \t]*/,""); gsub(/["'"'"' \t\r]/,""); print; exit}'
}

is_adr() { [[ "$(basename "$1")" =~ ^ADR-[0-9]+.*\.md$ ]]; }

# Record numbers claimed as documenting cycle work, from ADR-Ratchet trailers.
exempt_ids=" $(git log --format='%(trailers:key=ADR-Ratchet,valueonly)' "$base..$head" \
  | { grep -oE 'ADR-[0-9]+' || true; } | sort -u | tr '\n' ' ')"
adr_id() { basename "$1" | grep -oE '^ADR-[0-9]+'; }

added=() closed=() exempt=() decided=()
while IFS=$'\t' read -r kind path; do
  is_adr "$path" || continue
  case "$kind" in
    A) if [[ "$exempt_ids" == *" $(adr_id "$path") "* ]]; then exempt+=("$path")
       elif s="$(status_at "$head" "$path")"; [[ -n "$s" && "$s" != proposed ]]; then decided+=("$path")
       else added+=("$path"); fi ;;
    M) [[ "$(status_at "$base" "$path")" == proposed && "$(status_at "$head" "$path")" != proposed ]] \
         && closed+=("$path") ;;
  esac
done < <(git diff --no-renames --name-status "$base" "$head" -- "$dir")

echo "adr-ratchet: $base..$head in $dir — added undecided ${#added[@]}, closed from proposed ${#closed[@]}, exempt ${#exempt[@]}, added decided ${#decided[@]}"
for f in "${added[@]}"; do echo "  + $f ($(status_at "$head" "$f"))"; done
for f in "${decided[@]}"; do echo "  · $f ($(status_at "$head" "$f") on arrival: not backlog)"; done
for f in "${exempt[@]}"; do echo "  = $f (ADR-Ratchet trailer: documents cycle work)"; done
for f in "${closed[@]}"; do echo "  ✓ $f ($(status_at "$base" "$f") → $(status_at "$head" "$f"))"; done

if (( ${#added[@]} <= ${#closed[@]} )); then
  echo "ADR-RATCHET-OK"
  exit 0
fi
if [[ -n "$until" && "$(date -u +%F)" > "$until" ]]; then
  echo "adr-ratchet: rule ended $until — reported, not enforced"
  echo "ADR-RATCHET-OK"
  exit 0
fi
echo "::error::adr-ratchet: ${#added[@]} undecided ADR(s) added but only ${#closed[@]} closed from proposed. Until $until a new proposal must close a proposed one (accept, reject, supersede or withdraw it in the same change), land already decided with its implementation, or claim that it documents cycle work with a commit trailer 'ADR-Ratchet: ADR-NNNN documents <track item>'."
echo "ADR-RATCHET-FAIL"
exit 1
