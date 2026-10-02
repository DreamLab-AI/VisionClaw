#!/usr/bin/env bash
# ADR-2110 activation acceptance — run AFTER the owner's `./scripts/launch.sh up dev`.
#
# Probes the LIVE dev stack (container `visionclaw_container`, nginx :3001,
# backend :4000) for every clause ADR-2110 governs, and writes a dated receipt:
#
#   binding                        the running binary is newer than every mounted source file
#   static                         no fabricated-rationale template in src/ or client/src/
#   trace_intent                   GET /api/trace echoes a declared intent verbatim and scores it
#                                  (true, the node-7/node-70 false, and absence-as-absence)
#   hitl_precision                 GET /api/kpi/summary carries HITL Precision with its denominator
#   broker_rationale_gate          GET /api/broker/inbox serves a high-tier case; decide with an
#                                  absent / short / whitespace rationale → 422 rationale_required,
#                                  and nothing is persisted
#   elevation_boot_reconciliation  rust.log shows the ElevationActor's boot reconciliation since
#                                  the last backend start
#
# The live probes run inside the container (docker exec … python3) so they hit the
# container's own nginx and never move a secret out of its environment. They seed
# labelled fixtures and remove them; see adr-2110-check.py.
#
# Exit 0 only when every probe PASSes. Any FAIL, any probe that could not run, or
# an unreachable container exits non-zero. Nothing passes on an empty result.
#
# Usage: scripts/activation/adr-2110-check.sh [--container NAME] [--receipt-dir DIR]
# Needs: bash, docker (exec on the container), git, python3 on the caller for the receipt.
# Never runs launch.sh, builds, or restarts anything.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONTAINER="${CONTAINER_NAME:-visionclaw_container}"
RECEIPT_DIR="$ROOT/.claude/evidence/activation"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --container) CONTAINER="$2"; shift 2 ;;
        --receipt-dir) RECEIPT_DIR="$2"; shift 2 ;;
        -h|--help) sed -n '2,28p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

die() { echo "ADR-2110 CHECK: FAIL — $*" >&2; exit 1; }

command -v docker >/dev/null || die "docker not on PATH"
command -v python3 >/dev/null || die "python3 not on PATH (needed to assemble the receipt)"

state="$(docker inspect -f '{{.State.Status}}' "$CONTAINER" 2>/dev/null || true)"
[[ "$state" == "running" ]] || die "container '$CONTAINER' is not running (state: ${state:-absent}). Run ./scripts/launch.sh up dev first."
docker exec "$CONTAINER" python3 -c 'import sqlite3, urllib.request' 2>/dev/null \
    || die "python3 with sqlite3 is not available inside '$CONTAINER'"

mkdir -p "$RECEIPT_DIR"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
receipt="$RECEIPT_DIR/ADR-2110-$stamp.json"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# --- static: the fabricated-rationale templates are gone (FR2.3, exit test item 1) ---
static_hits="$tmp/static.txt"
grep -rnE 'via governance UI|via control centre' "$ROOT/src" "$ROOT/client/src" \
    --include='*.rs' --include='*.ts' --include='*.tsx' 2>/dev/null \
    | grep -vE '^[^:]+:[0-9]+:[[:space:]]*(//|\*|/\*)' >"$static_hits" || true
# (comment lines are dropped: AcspCaseQueue.tsx records the deleted template in prose)

# --- live probes, inside the container ---
docker exec -i "$CONTAINER" python3 - <"$ROOT/scripts/activation/adr-2110-check.py" >"$tmp/live.json" \
    2>"$tmp/live.err" || die "in-container probe run crashed: $(tail -5 "$tmp/live.err")"

image_id="$(docker inspect -f '{{.Image}}' "$CONTAINER")"
image_created="$(docker image inspect -f '{{.Created}}' "$image_id" 2>/dev/null || echo unknown)"
container_started="$(docker inspect -f '{{.State.StartedAt}}' "$CONTAINER")"
head_sha="$(git -C "$ROOT" rev-parse HEAD)"
dirty="$(git -C "$ROOT" status --porcelain -- src crates Cargo.toml Cargo.lock | wc -l | tr -d ' ')"

python3 - "$tmp/live.json" "$static_hits" "$receipt" "$head_sha" "$dirty" "$CONTAINER" \
    "$image_id" "$image_created" "$container_started" "$ROOT" <<'PY'
import json, sys
live_p, static_p, receipt_p, head, dirty, container, image, image_created, started, root = sys.argv[1:]
live = json.load(open(live_p))
hits = [l.rstrip("\n").replace(root + "/", "") for l in open(static_p) if l.strip()]
probes = [{
    "probe": "static_no_fabricated_rationale",
    "verdict": "FAIL" if hits else "PASS",
    "evidence": {"pattern": "via governance UI|via control centre", "hits": hits[:20]},
    "notes": [],
}] + live["probes"]
expected = {"binding", "static_no_fabricated_rationale", "trace_intent", "hitl_precision",
            "broker_rationale_gate", "elevation_boot_reconciliation"}
seen = {p["probe"] for p in probes}
missing = sorted(expected - seen)
for m in missing:
    probes.append({"probe": m, "verdict": "FAIL", "evidence": {}, "notes": ["probe did not run"]})
overall = "PASS" if all(p["verdict"] == "PASS" for p in probes) else "FAIL"
hitl = next((p for p in probes if p["probe"] == "hitl_precision"), {})
receipt = {
    "adr": "ADR-2110",
    "kind": "activation-acceptance",
    "run_id": live.get("run_id"),
    "verdict": overall,
    "repo_head": head,
    "repo_dirty_rust_paths": int(dirty),
    "binary_reported_sha": hitl.get("evidence", {}).get("sha"),
    "container": container,
    "image": image,
    "image_created": image_created,
    "container_started": started,
    "probes": probes,
    "scope": "Live dev stack, unit fixtures seeded and removed by the probe. Does NOT cover: the "
             "owner's live high-tier 31403 on the edge relay (exit test item 4), the forum's "
             "effective_tier (VisionClaw does not ingest it; see ADR-2110 follow-on 5), or the "
             "case-card UI rendering (browser check).",
}
json.dump(receipt, open(receipt_p, "w"), indent=2)
print(f"ADR-2110 acceptance: {overall}  (receipt {receipt_p})")
for p in probes:
    print(f"  {p['verdict']:4}  {p['probe']}")
    for n in p.get("notes", []):
        print(f"        - {n}")
sys.exit(0 if overall == "PASS" else 1)
PY
