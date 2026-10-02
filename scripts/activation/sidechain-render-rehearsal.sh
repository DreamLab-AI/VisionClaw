#!/usr/bin/env bash
# Sidechain render rehearsal (stream S5) — diffs what VisionClaw shows against
# the S4b witness receipt's independent replay. Starts nothing, builds nothing,
# restarts nothing.
#
# Reads:
#   receipt   the witness receipt JSON (schema "sidechain-witness/1"), by default
#             the newest .claude/evidence/sidechain/*.json under this repository.
#             Fields used: replay.height, replay.balances[{did, settled_sats}],
#             payments[{txid, payer, payee, amount_sats, block_height, found}],
#             checkpoint{parent_height} (null while unanchored), verdict.
#             Shape: agentbox scripts/activation/sidechain-demo-witness.sh.
#   screen    the `chain` object of GET /api/bots/data on the running VisionClaw
#             (fetched inside the container, so no secret leaves it), or a saved
#             body given with --screen-json.
#
# FAILs (exit 1) on any of:
#   * a receipt balance whose DID is not on screen, or whose settled sats differ;
#   * the screen's fold height differing from replay.height (unless
#     --allow-height-drift, which then compares the sats alone);
#   * a receipt payment missing from the screen, or shown with another payer,
#     payee or amount, or shown unsettled although the screen tip covers it;
#   * the screen saying "anchored" when the receipt has no parent checkpoint
#     (owner decision 2026-10-02, SC5), or "not anchored" when it has one;
#   * no chain view on screen at all (route not deployed, or no payments read).
# Exit 2 on a usage or environment error. Exit 0 only when everything matches.
# A JSON diff report is written to .claude/evidence/activation/.
#
# Usage:
#   scripts/activation/sidechain-render-rehearsal.sh [--receipt FILE] [--receipt-dir DIR]
#       [--container NAME] [--screen-json FILE] [--allow-height-drift]
#       [--report-dir DIR] [--self-test]
# --self-test runs the diff against the repository fixtures (one matching pair,
# then three mutated mismatches) and exits non-zero if any verdict is wrong.
#
# Screenshot: with the stack up, drive the browser skill (browser-gpu MCP) at
# http://visionclaw_container:3001 and open Agents → Sidechain payments.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONTAINER="${CONTAINER_NAME:-visionclaw_container}"
RECEIPT=""
RECEIPT_DIR="$ROOT/.claude/evidence/sidechain"
SCREEN_JSON=""
REPORT_DIR="$ROOT/.claude/evidence/activation"
ALLOW_DRIFT=0
SELF_TEST=0
FIX="$ROOT/tests/fixtures/sidechain"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --receipt) RECEIPT="$2"; shift 2 ;;
        --receipt-dir) RECEIPT_DIR="$2"; shift 2 ;;
        --container) CONTAINER="$2"; shift 2 ;;
        --screen-json) SCREEN_JSON="$2"; shift 2 ;;
        --report-dir) REPORT_DIR="$2"; shift 2 ;;
        --allow-height-drift) ALLOW_DRIFT=1; shift ;;
        --self-test) SELF_TEST=1; shift ;;
        -h|--help) sed -n '2,37p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

usage_die() { echo "SIDECHAIN REHEARSAL: ERROR — $*" >&2; exit 2; }
command -v python3 >/dev/null || usage_die "python3 not on PATH"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# The diff itself. Args: receipt screen allow_drift report_path(or "-").
cat > "$tmp/diff.py" <<'PY'
import json, sys

receipt_path, screen_path, allow_drift, report_path = sys.argv[1:5]
allow_drift = allow_drift == "1"
receipt = json.load(open(receipt_path))
body = json.load(open(screen_path))
chain = body.get("chain") or (body.get("data") or {}).get("chain")
snap = (chain or {}).get("snapshot")
fails, checks = [], []

def fail(msg):
    fails.append(msg)

if receipt.get("schema") != "sidechain-witness/1":
    fail(f"receipt schema {receipt.get('schema')!r} is not sidechain-witness/1")
if receipt.get("verdict", "PASS") != "PASS":
    fail(f"receipt verdict {receipt.get('verdict')!r}: a failed witness is no baseline")
if not snap:
    fail("no chain view on screen (route not deployed, or no payments read yet)")
else:
    if snap.get("chain") != receipt.get("chain"):
        fail(f"chain differs: screen {snap.get('chain')!r}, receipt {receipt.get('chain')!r}")
    replay = receipt.get("replay") or {}
    rh = replay.get("height")
    screen_bal = {b["did"]: b for b in snap.get("balances", [])}
    for b in replay.get("balances", []):
        s = screen_bal.get(b["did"])
        if s is None:
            fail(f"balance {b['did']}: on the receipt, not on screen")
            continue
        if s["settled_sats"] != b["settled_sats"]:
            fail(f"balance {b['did']}: screen {s['settled_sats']} sats, receipt {b['settled_sats']} sats")
        if s["fold_height"] != rh and not allow_drift:
            fail(f"balance {b['did']}: screen fold @ {s['fold_height']}, receipt replay @ {rh}")
        if not s.get("tier", "").startswith("settled (chain fold @ "):
            fail(f"balance {b['did']}: screen tier {s.get('tier')!r} is not the settled tier")
        checks.append({"balance": b["did"], "screen": s["settled_sats"], "receipt": b["settled_sats"]})
    tip = (snap.get("tip") or {}).get("height", -1)
    screen_pay = {p["txid"]: p for p in snap.get("payments", [])}
    for p in receipt.get("payments", []):
        if p.get("found") is False:
            # The witness looked for this txid and it is not in the chain.
            checks.append({"payment": p.get("txid"), "skipped": "not in chain (witness found:false)"})
            continue
        s = screen_pay.get(p["txid"])
        if s is None:
            fail(f"payment {p['txid']}: on the receipt, not on screen")
            continue
        for k in ("payer", "payee", "amount_sats"):
            if s.get(k) != p.get(k):
                fail(f"payment {p['txid']}: {k} screen {s.get(k)!r}, receipt {p.get(k)!r}")
        bh = p.get("block_height")
        if bh is not None and bh <= tip and not s.get("settled"):
            fail(f"payment {p['txid']}: in block {bh} <= screen tip {tip} but shown unsettled")
        checks.append({"payment": p["txid"], "amount_sats": p.get("amount_sats")})
    anchored = (snap.get("anchor") or {}).get("state") == "anchored"
    # A checkpoint unconfirmed on the parent (parent_height null) anchors
    # nothing yet, so the screen must not say anchored for it either.
    cp = receipt.get("checkpoint")
    has_cp = isinstance(cp, dict) and cp.get("parent_height") is not None
    if anchored and not has_cp:
        fail("screen says anchored, receipt has no parent checkpoint (SC5: must not imply anchoring)")
    if has_cp and not anchored:
        fail("receipt carries a parent checkpoint, screen says not anchored")
    checks.append({"anchor": "anchored" if anchored else "not_anchored", "receipt_checkpoint": has_cp})

report = {"receipt": receipt_path, "verdict": "FAIL" if fails else "PASS",
          "failures": fails, "checks": checks, "allow_height_drift": allow_drift}
if report_path != "-":
    json.dump(report, open(report_path, "w"), indent=2)
for f in fails:
    print(f"  MISMATCH  {f}")
print(f"SIDECHAIN REHEARSAL: {report['verdict']} ({len(checks)} checks, {len(fails)} mismatches)")
sys.exit(1 if fails else 0)
PY

if [[ "$SELF_TEST" == 1 ]]; then
    ok=1
    expect() { # expect <0|1> <label> <receipt> <screen>
        local want="$1" label="$2" got=0
        python3 "$tmp/diff.py" "$3" "$4" 0 - >"$tmp/out" 2>&1 || got=$?
        if [[ "$got" == "$want" ]]; then echo "self-test ok    $label"; else echo "self-test WRONG $label (exit $got, wanted $want)"; cat "$tmp/out"; ok=0; fi
    }
    expect 0 "fixture receipt matches fixture screen" "$FIX/witness-receipt.v1.json" "$FIX/bots-data-chain.v1.json"
    python3 - "$FIX" "$tmp" <<'PY'
import json, sys
fix, tmp = sys.argv[1:3]
r = json.load(open(f"{fix}/witness-receipt.v1.json"))
s = json.load(open(f"{fix}/bots-data-chain.v1.json"))
r1 = json.loads(json.dumps(r)); r1["replay"]["balances"][0]["settled_sats"] += 1
json.dump(r1, open(f"{tmp}/r-balance.json", "w"))
s2 = json.loads(json.dumps(s)); s2["chain"]["snapshot"]["anchor"] = {"state": "anchored", "label": "anchored"}
json.dump(s2, open(f"{tmp}/s-anchored.json", "w"))
r = json.load(open(f"{fix}/witness-receipt.v1.json"))
r2 = json.loads(json.dumps(r)); r2["verdict"] = "FAIL"
json.dump(r2, open(f"{tmp}/r-failed.json", "w"))
r3 = json.loads(json.dumps(r)); r3["checkpoint"] = {"parent_txid": "e" * 64, "covers_height": 940, "hash": "f" * 64, "replayed_hash_at_height": "f" * 64, "parent_height": 120000, "confirmations": 6}
json.dump(r3, open(f"{tmp}/r-anchored.json", "w"))
r4 = json.loads(json.dumps(r)); r4["checkpoint"] = dict(r3["checkpoint"], parent_height=None, confirmations=0)
r4["payments"].append({"txid": "9" * 64, "found": False, "payer": None, "payee": None, "amount_sats": None, "block_height": None})
json.dump(r4, open(f"{tmp}/r-unconfirmed.json", "w"))
s3 = json.loads(json.dumps(s))
for p in s3["chain"]["snapshot"]["payments"]:
    p["settled"] = False
json.dump(s3, open(f"{tmp}/s-unsettled.json", "w"))
PY
    expect 1 "balance off by one sat"            "$tmp/r-balance.json"            "$FIX/bots-data-chain.v1.json"
    expect 1 "screen claims anchoring"           "$FIX/witness-receipt.v1.json"   "$tmp/s-anchored.json"
    expect 1 "witness verdict FAIL"              "$tmp/r-failed.json"             "$FIX/bots-data-chain.v1.json"
    expect 1 "receipt anchored, screen not"      "$tmp/r-anchored.json"           "$FIX/bots-data-chain.v1.json"
    expect 0 "unconfirmed checkpoint, found:false" "$tmp/r-unconfirmed.json"      "$FIX/bots-data-chain.v1.json"
    expect 1 "settled payment shown unsettled"   "$FIX/witness-receipt.v1.json"   "$tmp/s-unsettled.json"
    [[ "$ok" == 1 ]] && { echo "SIDECHAIN REHEARSAL self-test: PASS"; exit 0; }
    echo "SIDECHAIN REHEARSAL self-test: FAIL"; exit 1
fi

if [[ -z "$RECEIPT" ]]; then
    RECEIPT="$(ls -t "$RECEIPT_DIR"/*.json 2>/dev/null | head -1 || true)"
    [[ -n "$RECEIPT" ]] || usage_die "no witness receipt in $RECEIPT_DIR (pass --receipt FILE)"
fi
[[ -f "$RECEIPT" ]] || usage_die "receipt not found: $RECEIPT"

if [[ -z "$SCREEN_JSON" ]]; then
    command -v docker >/dev/null || usage_die "docker not on PATH (or pass --screen-json FILE)"
    state="$(docker inspect -f '{{.State.Status}}' "$CONTAINER" 2>/dev/null || true)"
    [[ "$state" == "running" ]] || usage_die "container '$CONTAINER' is not running (state: ${state:-absent}); this script never starts it"
    SCREEN_JSON="$tmp/screen.json"
    docker exec "$CONTAINER" python3 -c '
import sys, urllib.request
for url in ("http://localhost:4000/api/bots/data", "http://localhost:3001/api/bots/data"):
    try:
        sys.stdout.write(urllib.request.urlopen(url, timeout=10).read().decode()); sys.exit(0)
    except Exception as e:
        last = e
sys.stderr.write(f"fetch failed: {last}\n"); sys.exit(1)
' > "$SCREEN_JSON" || usage_die "could not read /api/bots/data inside '$CONTAINER'"
fi

mkdir -p "$REPORT_DIR"
report="$REPORT_DIR/sidechain-render-$(date -u +%Y%m%dT%H%M%SZ).json"
echo "receipt: $RECEIPT"
set +e
python3 "$tmp/diff.py" "$RECEIPT" "$SCREEN_JSON" "$ALLOW_DRIFT" "$report"
rc=$?
set -e
echo "report:  $report"
exit "$rc"
