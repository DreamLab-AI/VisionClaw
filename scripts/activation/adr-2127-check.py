# ADR-2127 live probes. Runs INSIDE visionclaw_container (piped to python3 by
# adr-2127-check.sh) so it hits the backend on its own loopback. Prints one JSON
# object {run_id, probes: [...]}; every probe is PASS or FAIL with its evidence.
#
# The class IRIs are real visionGraph classes chosen for a known shape:
#   1-inch is-a decentralized-exchange          (asserted)
#   decentralized-exchange is-a marketplace     (so 1-inch ⊑ marketplace is inferred)
#   360-video and decentralized-exchange share no path and no disjointness
# If the loaded corpus drops one of them, the probe FAILs and says so.
import json
import os
import time
import urllib.error
import urllib.request

BASE = "http://127.0.0.1:4000/api/ontology-agent/check"
ONE_INCH = "urn:ngm:class:1-inch"
DEX = "urn:ngm:class:decentralized-exchange"
MARKET = "urn:ngm:class:marketplace"
VIDEO = "urn:ngm:class:360-video"
probes = []


def post(body):
    req = urllib.request.Request(
        BASE, data=json.dumps(body).encode(), headers={"content-type": "application/json"}
    )
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            return r.status, json.loads(r.read() or b"{}")
    except urllib.error.HTTPError as e:
        raw = e.read()
        try:
            return e.code, json.loads(raw or b"{}")
        except ValueError:
            return e.code, {"raw": raw.decode(errors="replace")[:300]}


def check_of(body):
    return (body.get("data") or {}).get("check") or body.get("check") or {}


def probe(name, ok, evidence, notes=()):
    probes.append({"probe": name, "verdict": "PASS" if ok else "FAIL",
                   "evidence": evidence, "notes": list(notes)})


# --- binding: the running binary is newer than every mounted source file ---
binary = "/app/target/dev-runtime/visionclaw-server"
try:
    bin_mtime = os.path.getmtime(binary)
    newer = []
    for root in ("/app/src", "/app/crates"):
        for dirpath, _, files in os.walk(root):
            if "/target" in dirpath:
                continue
            for f in files:
                if f.endswith(".rs"):
                    p = os.path.join(dirpath, f)
                    if os.path.getmtime(p) > bin_mtime:
                        newer.append(p)
    probe("binding", not newer, {
        "binary": binary,
        "binary_mtime_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(bin_mtime)),
        "sources_newer_than_binary": sorted(newer)[:20],
        "sources_newer_count": len(newer),
    })
except OSError as e:
    probe("binding", False, {"binary": binary}, [f"cannot stat: {e}"])

# --- the three verdicts that today's corpus can produce ---
cases = [
    ("entailed_asserted", {"subject": ONE_INCH, "class": DEX}, "entailed", "asserted"),
    ("entailed_inferred", {"subject": ONE_INCH, "class": MARKET}, "entailed", "inferred"),
    ("not_asserted_is_silence", {"subject": VIDEO, "class": DEX}, "not_asserted", None),
]
for name, body, verdict, basis in cases:
    status, resp = post(body)
    c = check_of(resp)
    ok = status == 200 and c.get("verdict") == verdict and c.get("basis") == basis
    probe(name, ok, {"request": body, "status": status, "check": c})

# --- every answer names its open-world scope and generation ---
status, resp = post({"subject": ONE_INCH, "class": DEX})
scope = check_of(resp).get("scope") or {}
probe("scope_open_with_generation",
      status == 200 and scope.get("closure") == "open" and bool(scope.get("generation")),
      {"scope": scope})

# --- a label resolves to its class, and the answer echoes the IRI ---
status, resp = post({"subject": "1inch", "class": "decentralized exchange"})
c = check_of(resp)
probe("label_resolves_to_iri",
      status == 200 and c.get("subject") == ONE_INCH and c.get("class") == DEX
      and c.get("verdict") == "entailed",
      {"request": {"subject": "1inch", "class": "decentralized exchange"}, "status": status, "check": c})

# --- a term naming nothing is refused, never answered not_asserted ---
unknown = "zz-no-such-class-adr-2127"
status, resp = post({"subject": unknown, "class": DEX})
probe("unknown_term_refused",
      status == 400 and unknown in json.dumps(resp) and not check_of(resp),
      {"status": status, "body": resp})

print(json.dumps({"run_id": time.strftime("%Y%m%dT%H%M%SZ", time.gmtime()) + "-" + os.urandom(3).hex(),
                  "probes": probes}))
