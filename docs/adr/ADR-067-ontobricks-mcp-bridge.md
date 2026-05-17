# ADR-067: Ontobricks MCP Bridge & Reasoning Federation

**Status:** Proposed
**Date:** 2026-05-01
**Deciders:** jjohare, VisionClaw platform team
**Supersedes:** None
**Implements:** PRD-005 §6 Epic G
**Threat-modelled:** PRD-005 §19 (R-13 Databricks creds, R-25 stale ontology, R-27 inference poisoning, S-3 MCP impersonation, T-2 reasoning corruption, E-4 deserialisation, I-2 timing side-channel, E-6 AGPL coverage)

## Context

Databricks Labs **ontobricks** is a Databricks-native ontology framework with industrial-grade semantic reasoning (OWL 2 RL via `owlrl`, SWRL Horn-clause rules, SHACL shapes, R2RML mapping, GraphQL auto-schema, community detection). It exposes a FastMCP companion server (`mcp-ontobricks`) and stores triples in Delta tables. Combined with VC's strengths (multi-source federation, sovereign identity, GPU rendering, Logseq ingestion), the pair is complementary.

PRD-005 Epic G specifies a bidirectional bridge to ontobricks via MCP/REST — **without embedding ontobricks code or its Python runtime**. The bridge is **opt-in** and gated by user-configured Databricks credentials.

QE security review surfaced six critical concerns:

- **S-3** — User points the bridge at an attacker-controlled MCP endpoint (phishing, DNS poisoning, typo).
- **T-2** — Malicious imported ontology contains hostile axioms (e.g., `Customer subClassOf Suspect`); inferences pollute the canonical graph with `inferred=true` flag but no user gate.
- **R-13** — Databricks credential exfiltration if bridge code or transitive dep is compromised.
- **R-25** — Stale ontobricks data produces phantom inferences when ontology revision lags.
- **E-4** — Deserialisation gadgets in malformed ontobricks responses.
- **I-2** — SHACL-validation timing side-channels reveal private graph topology.

## Decision

**Bridge speaks to a user-pinned ontobricks MCP endpoint over TLS with TOFU certificate pinning. Inferred triples enter a quarantine layer until user accepts. Egress is allowlisted at OS level. Default off.**

### D1 — Bridge crate isolation

`crates/ontobricks-bridge` is the sole crate that touches ontobricks. It:

- Speaks MCP over HTTPS (no plaintext transport).
- Has zero outbound network egress except to the user-configured ontobricks URL — enforced at OS level via `cgroup` network namespace and Rust-level reqwest allowlist.
- Pins `ontobricks-mcp-rs` (or whatever client library lands) to specific git SHA in `Cargo.lock`.
- Emits `cargo-deny` and `cargo-audit` clean in CI; SBOM generated per release.

This addresses R-13 supply-chain risk.

### D2 — TLS certificate pinning (TOFU + change-warn)

On first connect to a configured ontobricks URL:

1. Bridge fetches the TLS cert chain.
2. User confirms a fingerprint pane showing: hostname, SHA-256 of the leaf cert, issuer, the Databricks workspace URL claimed by the MCP server.
3. Pin recorded in user's settings under `[analysis.ontobricks.pinned_servers.<host>] cert_sha256 = ...`.

Subsequent connects:
- If hostname's cert matches the pinned fingerprint → connect.
- If different → reject with `ServerIdentityChanged`; require manual re-pin via settings UI.
- Plaintext `http://` MCP rejected unconditionally.

Closes S-3 (MCP endpoint impersonation).

### D3 — Inferred-triple quarantine layer

Inferences from ontobricks land in a separate Neo4j namespace `(:Triple:Quarantine)`, never `(:Triple)`. Render layer surfaces them with a distinct visual treatment (dashed edge, low opacity, "PROPOSED" badge).

User accepts inferences via a side panel:
- **Per-axiom**: accept/reject/defer for each producing axiom.
- **Per-ontology**: accept all inferences from a specific ontology hash (whole-bundle trust).
- **Reject all**: clear quarantine.

Only accepted triples are signed into a bead per ADR-066. Quarantine state is persisted in AgentDB but not in pods (it's user-local, pre-decision content).

Every quarantined triple records:
- Source ontology hash.
- Producing axiom (Turtle snippet).
- Reasoning call URN.
- Producing peer / self.
- Timestamp.

Closes T-2, R-27 (inference poisoning).

### D4 — Ontology freshness validation

Bridge sends `If-None-Match: <local-ontology-etag>` on every reasoning request. Rejects responses where:

- The ontobricks server's reported ontology revision is older than the local cache's last seen revision (`ontology_stale` error), OR
- Server-side ontology age > 24h without a cache-hit signal.

UI surfaces a freshness banner showing last refresh timestamp. User can force a re-pull.

Closes R-25 (stale ontology phantom inferences).

### D5 — Strict response parsing (no deserialisation gadgets)

All bridge JSON parsing uses `serde_json` with:

- `deny_unknown_fields` on every struct.
- Bounded recursion (max nesting 32).
- Bounded total payload (max 100 MB; configurable per-endpoint, default 5 MB for `query_graphql`).
- `Value`-typed fields **not allowed** in bridge structs; every response shape is statically typed.

GraphQL responses are validated against the captured schema obtained from `get_graphql_schema` on connect; runtime drift triggers `SchemaDrift` error and forces re-pin.

Closes E-4 (deserialisation gadgets).

### D6 — Constant-time SHACL response

Bridge wraps SHACL validation calls with response-time padding:

- Measure baseline: median validation time on a synthetic noise dataset.
- Pad real responses to baseline + jitter ∈ [0, 200ms].
- Per-non-owner-peer rate limit: max 10 SHACL calls/min.

Output to non-owners reduced to `validation_attempted: bool`, no per-shape detail.

Closes I-2 (timing side-channel).

### D7 — Per-session cost budget

User configures Databricks DBU budget per session (default $5). Bridge tracks estimated cost per reasoning call (received from ontobricks's cost preview API where available, otherwise heuristic). Hard cutoff at 80% of budget with user prompt; rejection at 100%.

Cost telemetry exported per session: `ontobricks_cost_usd_total{owner_pubkey_hash, session_urn}`.

### D8 — AGPL coverage on bridge endpoints

Per existing ADR-052/AGPL invariant, every endpoint serving graph data emits the AGPL `Source-Code` header. Extended to bridge proxy endpoints — the ontobricks-derived data inherits the obligation when it flows through a `/lo/*` route.

CI invariant: every endpoint registered under `/lo/*` or proxied through the bridge serves the header. Test enumerates routes and asserts presence.

Legal review (PRD Q-11) is a **quality gate** before flag-flip — added to §8.2.

### D9 — Default off, gated config

```toml
[analysis.ontobricks]
enabled = false                     # default
mcp_url = ""                        # required when enabled
pinned_cert_sha256 = ""              # set on first-connect TOFU
budget_per_session_usd = 5.00
ontology_freshness_max_age_hours = 24
```

Bridge code paths are guarded by `if config.ontobricks.enabled` checks; when disabled, no network calls, no settings UI surface for ontobricks features.

## Consequences

### Positive

- Industrial-grade semantic reasoning (OWL 2 RL, SWRL, SHACL) without embedding Databricks runtime.
- Federated cognition pattern (VC ↔ ontobricks via MCP) generalises to other reasoning servers.
- Quarantine-by-default protects user from poisoned ontologies.
- Egress allowlist + TLS pinning + supply-chain hygiene close the credential exfil pathway.
- Cost budget keeps Databricks bills bounded.

### Negative

- Bridge is opt-in; users without Databricks access don't benefit.
- TOFU on first-connect is a phishing surface (user must verify out-of-band). Mitigation: link to user's Databricks workspace settings page that displays the canonical MCP URL.
- Quarantine UI is non-trivial; UX work required before flip.
- Constant-time SHACL response adds latency floor.

### Risks

- AGPL/GPL-related compliance still requires legal review per PRD Q-11.
- Ontobricks server version drift (PRD §19 versioning concern). Mitigated by pinned compat-test matrix.
- Bridge implementation must keep up with ontobricks evolution; risk of staleness on our side. Mitigation: nightly compat CI against pinned ontobricks versions.

## References

- PRD-005 §6 Epic G, §13.5, §19 (R-13, R-25, R-27, S-3, T-2, E-4, I-2, E-6)
- Ontobricks: `databricks.yml`, `app.yaml` (Databricks Asset Bundle), `src/back/core/reasoning/*`
- ADR-052 (Pod default WAC), ADR-053 (Solid-pod-rs)
- ADR-066 (Pod-Federated Graph Storage — quarantine accepted triples land here)
