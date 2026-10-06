#!/usr/bin/env bash
# ADR-2127 activation acceptance — run AFTER the owner's `./scripts/launch.sh up dev`.
#
# Probes the LIVE dev stack (container `visionclaw_container`, backend :4000)
# for the tri-valued check (POST /api/ontology-agent/check) and writes a dated
# receipt:
#   binding                     the running binary is newer than every mounted .rs source
#   entailed_asserted           1-inch ⊑ decentralized-exchange → entailed / asserted
#   entailed_inferred           1-inch ⊑ marketplace → entailed / inferred (Whelk closure)
#   not_asserted_is_silence     360-video ⊑ decentralized-exchange → not_asserted
#   scope_open_with_generation  every answer carries closure "open" and a generation
#   label_resolves_to_iri       class labels resolve; the answer echoes the IRIs
#   unknown_term_refused        a term naming no class → 400, never not_asserted
#
# NOT covered: `entailed_false`. It needs an owl:disjointWith in the loaded
# ontology, and the corpus has none until the first disjoint-with proposal is
# approved (31403) and built. Until then this receipt proves two of the three
# verdicts plus the refusal, and says so in `scope`.
#
# Exit 0 only when every probe PASSes.
# Usage: scripts/activation/adr-2127-check.sh [--container NAME] [--receipt-dir DIR]
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

die() { echo "ADR-2127 CHECK: FAIL — $*" >&2; exit 1; }

command -v docker >/dev/null || die "docker not on PATH"
command -v python3 >/dev/null || die "python3 not on PATH (needed to assemble the receipt)"

state="$(docker inspect -f '{{.State.Status}}' "$CONTAINER" 2>/dev/null || true)"
[[ "$state" == "running" ]] || die "container '$CONTAINER' is not running (state: ${state:-absent}). Run ./scripts/launch.sh up dev first."
docker exec "$CONTAINER" python3 -c 'import urllib.request' 2>/dev/null \
    || die "python3 is not available inside '$CONTAINER'"

mkdir -p "$RECEIPT_DIR"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
receipt="$RECEIPT_DIR/ADR-2127-$stamp.json"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# --- live probes, inside the container ---
docker exec -i "$CONTAINER" python3 - <"$ROOT/scripts/activation/adr-2127-check.py" >"$tmp/live.json" \
    2>"$tmp/live.err" || die "in-container probe run crashed: $(tail -5 "$tmp/live.err")"

image_id="$(docker inspect -f '{{.Image}}' "$CONTAINER")"
image_created="$(docker image inspect -f '{{.Created}}' "$image_id" 2>/dev/null || echo unknown)"
container_started="$(docker inspect -f '{{.State.StartedAt}}' "$CONTAINER")"
head_sha="$(git -C "$ROOT" rev-parse HEAD)"
dirty="$(git -C "$ROOT" status --porcelain -- src crates Cargo.toml Cargo.lock | wc -l | tr -d ' ')"

python3 - "$tmp/live.json" "$receipt" "$head_sha" "$dirty" "$CONTAINER" \
    "$image_id" "$image_created" "$container_started" "$ROOT" <<'PY'
import json, sys
live_p, receipt_p, head, dirty, container, image, image_created, started, root = sys.argv[1:]
live = json.load(open(live_p))
probes = live["probes"]
expected = {"binding", "entailed_asserted", "entailed_inferred", "not_asserted_is_silence",
            "scope_open_with_generation", "label_resolves_to_iri", "unknown_term_refused"}
seen = {p["probe"] for p in probes}
missing = sorted(expected - seen)
for m in missing:
    probes.append({"probe": m, "verdict": "FAIL", "evidence": {}, "notes": ["probe did not run"]})
overall = "PASS" if all(p["verdict"] == "PASS" for p in probes) else "FAIL"
receipt = {
    "adr": "ADR-2127",
    "kind": "activation-acceptance",
    "run_id": live.get("run_id"),
    "verdict": overall,
    "repo_head": head,
    "repo_dirty_rust_paths": int(dirty),
    "container": container,
    "image": image,
    "image_created": image_created,
    "container_started": started,
    "probes": probes,
    "scope": "Live dev stack against the loaded visionGraph corpus; read-only, nothing seeded. "
             "Does NOT cover entailed_false: the loaded ontology carries no owl:disjointWith until "
             "the first disjoint-with Schema proposal is approved (31403) and built.",
}
json.dump(receipt, open(receipt_p, "w"), indent=2)
print(f"ADR-2127 acceptance: {overall}  (receipt {receipt_p})")
for p in probes:
    print(f"  {p['verdict']:4}  {p['probe']}")
    for n in p.get("notes", []):
        print(f"        - {n}")
sys.exit(0 if overall == "PASS" else 1)
PY
