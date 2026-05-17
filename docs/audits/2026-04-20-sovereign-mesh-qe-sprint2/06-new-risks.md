# 06 — New Risks Introduced by Sprint-2

Scan of the six Sprint-2 commits for fresh concerns — things that didn't
exist before the debt-payoff sprint landed. Ratings: `low | medium |
high | blocker`. None are blockers.

---

## R1 — Best-effort kind-30100 audit drift

**Rating: medium.**

**Where**: `src/services/bridge_edge.rs:421-472` +
`src/services/metrics.rs:110-114`.

**Concern**: The B2 fix ships "promote commits Neo4j; signing is
sidecar." On signing failure the `bridge_kind30100_errors_total` counter
increments but the `:BRIDGE_TO` edge still exists. An operator who
isn't watching the counter can end up with promotions in Neo4j that
have no corresponding audit event on the relay — the inverse of the
pre-Sprint-2 gap (audit without edge), but the same observability
class. In principle a reconciliation job can sweep
`(k)-[r:BRIDGE_TO]->(o)` edges against the relay's kind-30100 query
and re-emit missing events; no such job is in the sprint-2 tree.

**Mitigations in place**: per-error-arm `error!` logs at
`src/services/bridge_edge.rs:450-455` and :461-466 carry the candidate
IRIs so forensics is possible without the counter alone.

**Recommendation**: add a nightly reconcile task post-MVP; not required
for merge-to-prod because the counter is wired and the operational
runbook can alert on a non-zero rate of change.

---

## R2 — Unbounded `:PodTombstone` growth

**Rating: low.**

**Where**: `src/sovereign/visibility.rs:223-239` — `write_tombstone`
does `MERGE (t:PodTombstone {path})`, no TTL, no archival path.

**Concern**: Every unpublish adds one node. A Pod re-publishing the
same path never deletes the tombstone (the saga writes the tombstone
for the *old* public path; re-publishing creates a new path under
`public/`, so the old tombstone remains forever). Over millions of
unpublish events the `:PodTombstone` label dominates the cold-read
footprint; the solid-proxy GET path runs a `MATCH` on every public-kg
GET (src/handlers/solid_proxy_handler.rs:578), so query cost grows
with tombstone cardinality.

**Mitigations**: (i) path-indexed lookup (Neo4j auto-indexes MERGE
keys); (ii) the GET check is only on the `public/` prefix, so
unrelated paths skip the lookup.

**Recommendation**: add a scheduled deletion of tombstones older than
90 days (or whatever the legal-hold horizon dictates) in a later
sprint. Safe to defer; no correctness risk.

---

## R3 — `corpus.jsonl` regenerated in full on every transition

**Rating: medium.**

**Where**: `src/services/ingest_saga.rs:669-765` +
`src/sovereign/visibility.rs:360-379`.

**Concern**: Each publish/unpublish of any single node triggers
`regenerate_corpus_jsonl(owner, None)` which scans all public nodes
for that owner, serialises them to JSON-Lines, and PUTs the full file
to the Pod. Cost is O(n) per transition in both Neo4j read time and
Pod PUT bandwidth where `n = public-node-count-for-owner`. For a
power user with 10k public nodes, a single publish is a 10k-row
Cypher read + a multi-MB upload.

**Mitigations**: (i) flag-gated (`URN_SOLID_ALIGNMENT=false` by
default); (ii) best-effort — logged, not propagated; (iii) the hot
path (publish → Neo4j flip → broadcast) completes before the corpus
regen even starts.

**Recommendation**: incremental corpus updates (append single line on
publish, rewrite on unpublish) are a follow-up; scoped for a Phase-3
ticket. For MVP the current full-regen is acceptable at expected user
scales.

---

## R4 — JSON-LD content negotiation profile parameters

**Rating: low.**

**Where**: `crates/solid-pod-rs/src/ldp.rs` — `negotiate_format`,
referenced in checklist tests but not deeply audited.

**Concern**: The Solid Protocol §3.1 specifies that `Accept:
application/ld+json; profile="https://www.w3.org/ns/activitystreams"`
should select a profile-specific representation. solid-pod-rs's
negotiation appears to match on media type only (the parity row says
`negotiate_prefers_explicit_turtle`, not profile-aware). External
Solid apps that send profile parameters may get a generic JSON-LD
payload where they expected a profiled one.

**Mitigations**: (i) present-in-checklist for base JSON-LD; (ii) the
JSS parity harness will catch regressions once profile-specific
fixtures are added.

**Recommendation**: add profile-parameter fixtures to `interop_jss.rs`
in a Phase-3 ticket. Low priority because the MVP consumer is the
VisionClaw UI, which sends plain `application/ld+json`.

---

## R5 — `APP_ENV=development` env-var poisoning in shared-host setups

**Rating: low.**

**Where**: `src/utils/auth.rs:27-41` — `is_production()` trusts the
process environment verbatim.

**Concern**: In a shared-host deployment (systemd unit with
`EnvironmentFile=` pointing at a user-writable path, or an older
Docker image that honours user-supplied `ENV` lines), an attacker with
file-system access can set `APP_ENV=development` and re-enable the
dev-mode bypass branch. This is the classic "container breakout via
env var" vector.

**Mitigations**: (i) the default is now fail-closed, so a missing var
is safe; (ii) the legacy X-Nostr-Pubkey path is the only thing
behind the gate, not arbitrary auth bypass; (iii) the NIP-98 body-hash
binding (B3) is unconditional, so signed-request replay is still
impossible even in dev.

**Recommendation**: the deployment runbook should document that
`APP_ENV` must be set via an immutable config source (Docker
`ARG`-baked image, sealed systemd unit); the code-level fix is
complete and appropriate.

---

## R6 — Lossy UTF-8 conversion on request body for NIP-98 binding

**Rating: low.**

**Where**: `src/utils/auth.rs:248-249` +
`src/handlers/solid_proxy_handler.rs:1595-1596`.

**Concern**: `String::from_utf8_lossy` replaces invalid UTF-8 bytes
with the Unicode replacement character (U+FFFD). If a malicious client
sends a body with invalid UTF-8, the server-side hash is computed on
the lossy version, and the client-side hash is computed on the raw
bytes. A careful attacker might be able to manufacture a body that
hashes differently on the wire than the client signed — breaking
the binding.

**Mitigations**: (i) the docstring at :240-247 acknowledges the
trade-off and argues that binary bodies are rare on authenticated
surfaces (JSON, RDF/TTL, form-encoded are all UTF-8 valid); (ii) even
if the hash differs, the attack direction is "server rejects a body
the client claims to have signed" — a denial-of-service, not a
bypass; the lossy conversion cannot make an unsigned body appear
signed.

**Recommendation**: when a binary-body authenticated surface is added
(signed file upload, image Pod write), switch that endpoint to
hash raw bytes directly. For MVP the current behaviour is safe.

---

## Summary

| # | Risk | Rating |
|---|------|--------|
| R1 | B2 best-effort creates audit/edge drift surface | medium |
| R2 | `:PodTombstone` unbounded growth | low |
| R3 | O(n) corpus.jsonl regen per transition | medium |
| R4 | JSON-LD profile negotiation gap | low |
| R5 | `APP_ENV` env-var poisoning | low |
| R6 | Lossy UTF-8 in NIP-98 body-hash binding | low |

None are blockers. R1 and R3 warrant operational runbook additions
before production soak-test; R5 warrants a deployment-hardening note;
the rest are backlog-grade.
