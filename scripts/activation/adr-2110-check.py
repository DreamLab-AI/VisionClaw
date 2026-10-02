#!/usr/bin/env python3
"""ADR-2110 post-rebuild acceptance probes — runs INSIDE the VisionClaw dev container.

Invoked by `scripts/activation/adr-2110-check.sh` as
`docker exec -i <container> python3 - < adr-2110-check.py`, so every request goes
through the container's own nginx (:3001) and backend (:4000), and every secret
the probes need (VISIONCLAW_AGENT_KEY) stays in the container's environment: it
is read here and sent as a header, and is never printed or written to a receipt.

Each probe returns PASS or FAIL with evidence. Nothing here passes by finding
nothing: an empty trace, a missing KPI tile, an unreachable route or an absent
log line is a FAIL. The probes that need data seed their own, clearly labelled
fixtures and delete them afterwards:

  * trace   — three `kpi_agent_events` rows under a probe-only did:nostr, read
              back through GET /api/trace?agent=…; deleted before the KPI probe.
  * broker  — one `high`-tier pending case in `enrichment_proposals`, used ONLY
              to collect refusals. A decision that is accepted instead is itself
              the failure; any decision row it left is reported and removed.

Output: one JSON object on stdout. Diagnostics go to stderr.
"""

import json
import os
import re
import secrets
import sqlite3
import sys
import time
import urllib.error
import urllib.request

NGINX = "http://localhost:3001"
BACKEND = "http://localhost:4000"
APP_ROOT = "/app"
LOG_PATH = "/app/logs/rust.log"
RUST_BINARY = os.environ.get("RUST_BINARY", "/app/target/dev-runtime/visionclaw-server")
MIN_RATIONALE_CHARS = 20  # must equal enrichment_proposals_handler::MIN_RATIONALE_CHARS

RUN_ID = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime()) + "-" + secrets.token_hex(3)
# 64-hex pubkey that cannot collide with a real identity in practice.
PROBE_PUBKEY = "ad2110" + secrets.token_hex(29)
PROBE_DID = f"did:nostr:{PROBE_PUBKEY}"
PROBE_CASE = f"adr2110-acceptance-{RUN_ID}"


def data_dir():
    d = os.environ.get("DATA_DIR", "./data")
    return d if d.startswith("/") else os.path.normpath(os.path.join(APP_ROOT, d))


def http(method, url, body=None, headers=None, timeout=30):
    """Return (status, parsed-json-or-text). Never raises on HTTP status."""
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("Accept", "application/json")
    if data is not None:
        req.add_header("Content-Type", "application/json")
    for k, v in (headers or {}).items():
        req.add_header(k, v)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            raw = r.read().decode("utf-8", "replace")
            status = r.status
    except urllib.error.HTTPError as e:
        raw = e.read().decode("utf-8", "replace")
        status = e.code
    except Exception as e:  # connection refused, timeout
        return None, f"{type(e).__name__}: {e}"
    try:
        return status, json.loads(raw)
    except ValueError:
        return status, raw[:500]


def result(name, ok, evidence, notes=None):
    return {"probe": name, "verdict": "PASS" if ok else "FAIL", "evidence": evidence,
            "notes": notes or []}


# ---------------------------------------------------------------------------
# 0. The running binary is the source on disk
# ---------------------------------------------------------------------------

def probe_binding():
    ev = {"binary": RUST_BINARY}
    if not os.path.isfile(RUST_BINARY):
        return result("binding", False, ev, ["dev-runtime binary not found — has the wrapper built it?"])
    bin_mtime = os.path.getmtime(RUST_BINARY)
    ev["binary_mtime_utc"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(bin_mtime))
    newer = []
    for root in ("src", "crates"):
        for dirpath, _, files in os.walk(os.path.join(APP_ROOT, root)):
            if "/target" in dirpath:
                continue
            for f in files:
                if f.endswith((".rs", ".toml")):
                    p = os.path.join(dirpath, f)
                    if os.path.getmtime(p) > bin_mtime:
                        newer.append(p[len(APP_ROOT) + 1:])
    for f in ("Cargo.toml", "Cargo.lock", "build.rs"):
        p = os.path.join(APP_ROOT, f)
        if os.path.isfile(p) and os.path.getmtime(p) > bin_mtime:
            newer.append(f)
    ev["sources_newer_than_binary"] = sorted(newer)[:20]
    ev["sources_newer_count"] = len(newer)
    s_api, b_api = http("GET", f"{BACKEND}/api/health")
    s_ngx, _ = http("GET", f"{NGINX}/api/health")
    ev["backend_health_status"] = s_api
    ev["nginx_health_status"] = s_ngx
    ok = not newer and s_api == 200 and s_ngx == 200
    notes = []
    if newer:
        notes.append("binary is older than mounted source: the running server is NOT this tree")
    if s_api != 200 or s_ngx != 200:
        notes.append(f"health: backend={s_api} nginx={s_ngx} ({b_api if s_api != 200 else ''})")
    return result("binding", ok, ev, notes)


# ---------------------------------------------------------------------------
# 1. /api/trace — intent persisted verbatim and matched (FR5.1–5.2)
# ---------------------------------------------------------------------------

TRACE_SEEDS = [
    # (event_id offset, action, target, intent, expected intent_match)
    (1, "graph_update", "urn:kg:node-7", "op=graph_update target=urn:kg:node-7", True),
    # EXP-AC-005 auditor's counter-example: node-7 must not match node-70.
    (2, "graph_update", "urn:kg:node-70", "op=graph_update target=urn:kg:node-7", False),
    # No declaration: no intent and no intent_match on the wire (null ≠ false).
    (3, "graph_update", "urn:kg:node-7", None, None),
]


def probe_trace():
    db = os.path.join(data_dir(), "kpi.sqlite3")
    ev = {"db": db, "probe_agent": PROBE_DID}
    if not os.path.isfile(db):
        return result("trace_intent", False, ev, ["kpi.sqlite3 not found"])
    base_event = 9_200_000_000 + int(time.time()) % 100_000_000
    now_ms = int(time.time() * 1000)
    con = sqlite3.connect(db, timeout=10)
    try:
        cols = {r[1] for r in con.execute("PRAGMA table_info(kpi_agent_events)")}
        if "intent" not in cols:
            return result("trace_intent", False, ev, ["kpi_agent_events has no `intent` column"])
        ev["real_rows_with_intent_30d"] = con.execute(
            "SELECT COUNT(*) FROM kpi_agent_events WHERE intent IS NOT NULL AND observed_at_ms >= ? "
            "AND (agent_did IS NULL OR agent_did NOT LIKE 'did:nostr:ad2110%')",
            (now_ms - 30 * 86_400_000,)).fetchone()[0]
        for off, action, target, intent, _ in TRACE_SEEDS:
            con.execute(
                "INSERT INTO kpi_agent_events (event_id, source_agent_id, action_type, observed_at_ms, "
                "agent_did, action_type_name, source_urn, target_urn, intent) VALUES (?,?,?,?,?,?,?,?,?)",
                (base_event + off, 0, 0, now_ms - off, PROBE_DID, action,
                 f"urn:kg:adr2110-probe-{off}", target, intent))
        con.commit()
        status, body = http("GET", f"{NGINX}/api/trace?agent={PROBE_DID}&window_ms=3600000")
        ev["http_status"] = status
        notes = []
        ok = status == 200 and isinstance(body, dict)
        records = [r for r in (body.get("records", []) if isinstance(body, dict) else [])
                   if r.get("agent_did") == PROBE_DID]
        ev["records_returned"] = len(records)
        if len(records) != len(TRACE_SEEDS):
            ok = False
            notes.append(f"expected {len(TRACE_SEEDS)} probe records, got {len(records)}")
        checks = []
        for off, action, target, intent, expect in TRACE_SEEDS:
            match = [r for r in records if r.get("intent") == intent
                     and (r.get("intent_match") if "intent_match" in r else None) == expect]
            # Absence must be absence: a None-intent record carries neither key.
            if intent is None:
                match = [r for r in records if "intent" not in r and "intent_match" not in r]
            got = bool(match)
            checks.append({"intent": intent, "target": target, "expected_intent_match": expect,
                           "observed": got})
            if not got:
                ok = False
                notes.append(f"seed {off}: no record with intent={intent!r} intent_match={expect!r}")
        ev["checks"] = checks
        return result("trace_intent", ok, ev, notes)
    finally:
        removed = con.execute("DELETE FROM kpi_agent_events WHERE agent_did = ?", (PROBE_DID,)).rowcount
        con.commit()
        con.close()
        ev["seed_rows_removed"] = removed


# ---------------------------------------------------------------------------
# 2. HITL Precision with its denominator (FR5.3–5.4)
# ---------------------------------------------------------------------------

def probe_hitl():
    status, body = http("GET", f"{NGINX}/api/kpi/summary", timeout=60)
    ev = {"http_status": status}
    if status != 200 or not isinstance(body, dict):
        return result("hitl_precision", False, ev, [f"GET /api/kpi/summary → {status}: {str(body)[:200]}"])
    data = body.get("data", body)
    ev["sha"] = data.get("sha")
    tiles = data.get("tiles", [])
    tile = next((t for t in tiles if t.get("kpi") == "hitl_precision"), None)
    if tile is None:
        return result("hitl_precision", False, ev, ["no hitl_precision tile"])
    ev["tile"] = {k: tile.get(k) for k in
                  ("status", "value", "numerator", "denominator", "sample_count", "snapshot_id", "source")}
    st, val, num, den = tile.get("status"), tile.get("value"), tile.get("numerator"), tile.get("denominator")
    notes = []
    ok = True
    if st == "awaiting_data_source":
        ok = False
        notes.append("HITL Precision is still the awaiting_data_source stub")
    elif den is None:
        ok = False
        notes.append("denominator not reported")
    elif st == "no_decided_cases":
        if val is not None or den != 0:
            ok = False
            notes.append("no_decided_cases must carry value=null and denominator=0")
        notes.append("no human-decided case in the window yet — the exit test still needs one")
    elif st == "computed":
        if not den or val is None or num is None or abs(val - num / den) > 1e-9:
            ok = False
            notes.append("computed value is not numerator ÷ denominator over a non-zero denominator")
    else:
        ok = False
        notes.append(f"unknown status {st!r}")
    return result("hitl_precision", ok, ev, notes)


# ---------------------------------------------------------------------------
# 3. Broker inbox + the server-side 422 rationale gate (FR2.2–2.4)
# ---------------------------------------------------------------------------

def probe_broker():
    db = os.path.join(data_dir(), "enrichment.sqlite3")
    ev = {"db": db, "probe_case": PROBE_CASE}
    notes = []
    if not os.path.isfile(db):
        return result("broker_rationale_gate", False, ev, ["enrichment.sqlite3 not found"])
    con = sqlite3.connect(db, timeout=10)
    proposal = {
        "title": "ADR-2110 acceptance probe — refusal-only, removed after the run",
        "target_path": "pages/adr2110-acceptance-probe.md",
        "content": "probe",
        "enrichment_type": "acceptance_probe",
        "reasoning_summary": "Seeded by scripts/activation/adr-2110-check.sh to collect 422 refusals.",
        "proposed_by": PROBE_DID,
        "risk_tier": "high",
    }
    ok = True
    try:
        con.execute(
            "INSERT INTO enrichment_proposals (case_id, category, source_iri, proposal_json, status) "
            "VALUES (?, 'knowledge_enrichment', NULL, ?, 'pending')",
            (PROBE_CASE, json.dumps(proposal)))
        con.commit()

        # Inbox: the probe case is listed with its declared tier and an ABSENT confidence.
        s, inbox = http("GET", f"{NGINX}/api/broker/inbox")
        ev["inbox_status"] = s
        case = None
        if s == 200 and isinstance(inbox, dict):
            ev["inbox_total"] = inbox.get("total")
            case = next((c for c in inbox.get("cases", []) if c.get("id") == PROBE_CASE), None)
        if case is None:
            ok = False
            notes.append(f"probe case not served by GET /api/broker/inbox (status {s}); "
                         "a 401/403 means the dev bypass is off — sign NIP-98 as a power user")
        else:
            md = case.get("metadata", {})
            ev["inbox_case_metadata"] = {"risk_tier": md.get("risk_tier"),
                                         "confidence": md.get("confidence")}
            if md.get("risk_tier") != "high" or md.get("confidence") is not None:
                ok = False
                notes.append("inbox metadata: expected risk_tier=high and confidence=null")

        # Operator route: three refusals, each with the structured body.
        attempts = [("absent", None, 0), ("short", "too short", 9),
                    ("whitespace-padded", "        x          ", 1)]
        refusals = []
        for label, reasoning, expect_chars in attempts:
            body = {"outcome": "approve"}
            if reasoning is not None:
                body["reasoning"] = reasoning
            s, rb = http("POST", f"{NGINX}/api/broker/cases/{PROBE_CASE}/decide", body)
            rec = {"route": "operator", "rationale": label, "status": s,
                   "body": rb if isinstance(rb, dict) else str(rb)[:200]}
            good = (s == 422 and isinstance(rb, dict) and rb.get("code") == "rationale_required"
                    and rb.get("tier") == "high" and rb.get("min_chars") == MIN_RATIONALE_CHARS
                    and rb.get("received_chars") == expect_chars and "reasoning" not in rb)
            rec["verdict"] = "PASS" if good else "FAIL"
            ok = ok and good
            refusals.append(rec)

        # Service route (agentbox bridge): same core, X-Agent-Key credential.
        key = os.environ.get("VISIONCLAW_AGENT_KEY", "")
        if key:
            s, rb = http("POST", f"{NGINX}/api/enrichment-proposals/{PROBE_CASE}/decide",
                         {"outcome": "reject", "reasoning": "nope"}, {"X-Agent-Key": key})
            good = s == 422 and isinstance(rb, dict) and rb.get("code") == "rationale_required"
            refusals.append({"route": "service", "rationale": "short", "status": s,
                             "body": rb if isinstance(rb, dict) else str(rb)[:200],
                             "verdict": "PASS" if good else "FAIL"})
            ok = ok and good
        else:
            notes.append("service route not exercised: VISIONCLAW_AGENT_KEY unset in the container")
        ev["refusals"] = refusals

        # Refused means refused: nothing minted, nothing persisted.
        n_dec = con.execute("SELECT COUNT(*) FROM enrichment_decisions WHERE case_id = ?",
                            (PROBE_CASE,)).fetchone()[0]
        st = con.execute("SELECT status FROM enrichment_proposals WHERE case_id = ?",
                         (PROBE_CASE,)).fetchone()
        ev["decision_rows_after"] = n_dec
        ev["status_after"] = st[0] if st else None
        if n_dec or (st and st[0] != "pending"):
            ok = False
            notes.append("a refused decision left state behind — the gate did not hold")
        return result("broker_rationale_gate", ok, ev, notes)
    finally:
        d = con.execute("DELETE FROM enrichment_decisions WHERE case_id = ?", (PROBE_CASE,)).rowcount
        p = con.execute("DELETE FROM enrichment_proposals WHERE case_id = ?", (PROBE_CASE,)).rowcount
        con.commit()
        con.close()
        ev["cleanup"] = {"decision_rows_removed": d, "proposal_rows_removed": p}


# ---------------------------------------------------------------------------
# 4. ElevationActor boot reconciliation (FR4.5)
# ---------------------------------------------------------------------------

def probe_elevation():
    ev = {"log": LOG_PATH}
    if not os.path.isfile(LOG_PATH):
        return result("elevation_boot_reconciliation", False, ev, ["rust.log not found"])
    with open(LOG_PATH, "r", errors="replace") as f:
        lines = f.read().splitlines()
    starts = [i for i, l in enumerate(lines) if "Starting Rust backend" in l]
    seg = lines[starts[-1]:] if starts else lines
    ev["segment"] = "since last 'Starting Rust backend'" if starts else "whole log (no start marker)"
    pick = lambda pat: [l.strip()[:300] for l in seg if pat in l]
    started = pick("ElevationActor started")
    disabled = pick("ElevationActor disabled")
    connect_failed = pick("[Elevation] ACSP connect failed")
    recon = pick("[Elevation] boot reconciliation:")
    recon_err = pick("[Elevation] boot reconciliation read failed")
    ev.update({"started": started[-1:], "disabled": disabled[-1:], "connect_failed": connect_failed[-1:],
               "reconciliation": recon[-1:], "reconciliation_read_failed": recon_err[-1:]})
    notes = []
    m = re.search(r"(\d+) case\(s\) re-armed, (\d+) timed out", recon[-1]) if recon else None
    if m:
        ev["re_armed"], ev["timed_out"] = int(m.group(1)), int(m.group(2))
    ok = bool(started) and bool(m) and not recon_err
    if disabled:
        notes.append("ElevationActor disabled: needs FORUM_RELAY_URL + ACSP_PANEL_NOSTR_PRIVKEY")
    if connect_failed:
        notes.append("ACSP connect failed — reconciliation only runs after a live relay connection")
    if not recon:
        notes.append("no boot-reconciliation line since the last backend start")
    return result("elevation_boot_reconciliation", ok, ev, notes)


def main():
    probes = []
    for fn in (probe_binding, probe_trace, probe_hitl, probe_broker, probe_elevation):
        try:
            probes.append(fn())
        except Exception as e:  # a crashed probe is a failed probe
            probes.append(result(fn.__name__.replace("probe_", ""), False,
                                 {"exception": f"{type(e).__name__}: {e}"}))
    json.dump({"run_id": RUN_ID, "probes": probes}, sys.stdout, indent=2, default=str)


if __name__ == "__main__":
    main()
