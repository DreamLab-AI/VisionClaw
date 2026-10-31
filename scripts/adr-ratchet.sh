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
# Second rule, the evidence gate (ADR-2007): over the same range, any record
# whose `implementation_status` becomes `complete`, or whose `activation_status`
# becomes `live`, must carry a non-empty `evidence_class` and
# `acceptance_boundary` at <head-rev>. The rule governs the closing EVENT, so
# it needs no backfill and catches a pre-existing record being closed; the date
# gate in adr-index-gen.cjs stays as a backstop for newly dated records. It is
# not tied to ADR_RATCHET_UNTIL: the backlog rule may end, this one does not.
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

# A frontmatter field from the first frontmatter block of <rev>:<path>.
# awk drains its input rather than exiting early: an early exit closes the pipe and, under
# pipefail, a record larger than the pipe buffer then fails the command with SIGPIPE (141).
# Usage: field_at <rev> <path> <key>   (empty when absent)
field_at() {
  git show "$1:$2" 2>/dev/null \
    | awk -v key="$3" 'done{next} NR==1 && $0!="---"{done=1; next} /^---$/{n++; if(n==2) done=1; next} n==1 && index($0,key":")==1{sub("^"key":[ \t]*",""); gsub(/["'"'"'\r]/,""); sub(/[ \t]+$/,""); print; done=1}'
}
status_at() { field_at "$1" "$2" decision_status | tr -d ' \t'; }

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

# Evidence gate: closing transitions need evidence_class and acceptance_boundary.
closing=() unevidenced=()
while IFS=$'\t' read -r kind path; do
  is_adr "$path" || continue
  [[ "$kind" == A || "$kind" == M ]] || continue
  impl_now="$(field_at "$head" "$path" implementation_status | tr -d ' \t')"
  act_now="$(field_at "$head" "$path" activation_status | tr -d ' \t')"
  impl_was=""; act_was=""
  if [[ "$kind" == M ]]; then
    impl_was="$(field_at "$base" "$path" implementation_status | tr -d ' \t')"
    act_was="$(field_at "$base" "$path" activation_status | tr -d ' \t')"
  fi
  if [[ ( "$impl_now" == complete && "$impl_was" != complete ) || ( "$act_now" == live && "$act_was" != live ) ]]; then
    closing+=("$path")
    if [[ -z "$(field_at "$head" "$path" evidence_class)" || -z "$(field_at "$head" "$path" acceptance_boundary)" ]]; then
      unevidenced+=("$path")
    fi
  fi
done < <(git diff --no-renames --name-status "$base" "$head" -- "$dir")

echo "adr-ratchet: $base..$head in $dir — added undecided ${#added[@]}, closed from proposed ${#closed[@]}, exempt ${#exempt[@]}, added decided ${#decided[@]}"
for f in "${added[@]}"; do echo "  + $f ($(status_at "$head" "$f"))"; done
for f in "${decided[@]}"; do echo "  · $f ($(status_at "$head" "$f") on arrival: not backlog)"; done
for f in "${exempt[@]}"; do echo "  = $f (ADR-Ratchet trailer: documents cycle work)"; done
for f in "${closed[@]}"; do echo "  ✓ $f ($(status_at "$base" "$f") → $(status_at "$head" "$f"))"; done

echo "adr-ratchet: evidence gate — ${#closing[@]} record(s) became complete/live, ${#unevidenced[@]} without evidence_class/acceptance_boundary"
for f in "${closing[@]}"; do echo "  ⇒ $f (closing transition)"; done
if (( ${#unevidenced[@]} )); then
  for f in "${unevidenced[@]}"; do echo "::error file=$f::adr-ratchet: $f became implementation_status complete or activation_status live without evidence_class and acceptance_boundary (ADR-2007)"; done
  echo "ADR-RATCHET-FAIL"
  exit 1
fi

if (( ${#added[@]} <= ${#closed[@]} )); then
  echo "ADR-RATCHET-OK"
  exit 0
fi
if [[ -n "$until" && "$(date -u +%F)" > "$until" ]]; then
  echo "adr-ratchet: backlog rule ended $until — reported, not enforced"
  echo "ADR-RATCHET-OK"
  exit 0
fi
echo "::error::adr-ratchet: ${#added[@]} undecided ADR(s) added but only ${#closed[@]} closed from proposed. Until $until a new proposal must close a proposed one (accept, reject, supersede or withdraw it in the same change), land already decided with its implementation, or claim that it documents cycle work with a commit trailer 'ADR-Ratchet: ADR-NNNN documents <track item>'."
echo "ADR-RATCHET-FAIL"
exit 1
