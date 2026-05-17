# Architecture Self-Review: Enterprise Transformation

**Date**: 2026-04-14
**Reviewer**: Claude Opus 4.6 (GPT-5.4 Codex review attempted — API auth failed, 401)
**Scope**: ADR-040 through ADR-045, DDD enterprise contexts, platform coherence

---

## Overall Assessment: Strong Foundation, Honest Gaps

The architecture is directionally sound and internally consistent. The 6 ADRs form a coherent dependency graph aligned with the PRD delivery phases. The DDD model is well-structured with proper aggregate boundaries. The platform coherence work (ADR-036 through 039) provides a solid substrate.

**The biggest risk is not architecture — it's execution pace.** The 7 workstreams represent 44+ weeks of planned delivery. Shipping the Judgment Broker Workbench before enterprise identity exists will limit pilot reach. Shipping enterprise identity before platform coherence is complete risks trust erosion.

---

## Per-ADR Assessment

### ADR-040: Enterprise Identity — SOUND, Key Risk Identified

**Strength**: Dual-stack OIDC + Nostr is the correct approach. Preserving Nostr for provenance while adding OIDC for enterprise access avoids the false dilemma of choosing one. The ephemeral keypair delegation pattern is well-established (cf. Ethereum account abstraction, FIDO2 attestation delegation).

**Weakness**: Key lifecycle management is underspecified. What happens when an OIDC session expires? Is the ephemeral keypair revoked? What if a user has two concurrent sessions — two keypairs? The ADR needs a key rotation and revocation protocol.

**Gap**: SCIM provisioning deferred to "Phase 2" but enterprise IT teams will ask about it in Phase 1 pilots. At minimum, document the manual onboarding process.

### ADR-041: Judgment Broker Workbench — VIABLE, Cold Start Risk

**Strength**: The broker concept is not abstract — it maps directly to roles that already exist informally in every regulated organisation (programme leads, risk leads, transformation leads). The decision canvas with full provenance is genuinely differentiated from approval-queue tools.

**Weakness**: The "cold start" problem identified in PRD section 18(B) is real. On day 1, the broker inbox will be empty because the Discovery Engine has no data. The manual "Submit Workflow Proposal" primitive is essential and must ship with the broker MVP, not after.

**Gap**: No mention of notification strategy. How does a broker know something is in their inbox? Email? Push? WebSocket alone is insufficient for users not actively in the app.

### ADR-042: Workflow Proposal — SOLID, Complexity Appropriate

**Strength**: Append-only versioning via SUPERSEDES edges is the right pattern for auditable workflows. The status lifecycle is well-bounded. Rollback by pointer swap avoids data loss.

**Weakness**: The "diffable" requirement needs implementation detail. Structured JSON comparison is simple for step sequences but complex for graph-connected workflows with branch logic. The ADR should specify the diff algorithm (or explicitly scope it to linear step sequences for MVP).

### ADR-043: KPI Lineage — AMBITIOUS, Implementation Risk

**Strength**: Event-sourced KPIs with DERIVED_FROM lineage is architecturally clean. The four KPIs are well-defined and measurable.

**Weakness**: "Full lineage for snapshots with <= 500 source events" means KPI computation requires graph traversal at query time. At enterprise scale (thousands of decisions per month), this becomes a performance concern. Consider pre-computing lineage summaries.

**Gap**: No alerting or threshold-breach notification mechanism defined. KPIs without alerts are dashboards, not governance.

### ADR-044: Connector Governance — CORRECT CALL

**Strength**: GitHub-first is the right move. The "connector tarpit" warning is well-heeded. The redaction pipeline and legal review mode show maturity.

**Weakness**: GitHub Issues/PRs only surfaces engineering workflows. The thesis claims discovery across "knowledge-dense, compliance-sensitive organisations" — legal, pharma, finance teams don't live in GitHub. The gap between Tier 1 and Tier 2 connectors is larger than the ADR acknowledges.

### ADR-045: Policy Engine — RIGHT STARTING POINT

**Strength**: Embedded Rust engine is correct for bounded initial rules. Sub-millisecond evaluation matters for the broker hot path. TOML configuration is admin-friendly.

**Weakness**: The OPA migration path is mentioned but not designed. When does the team know it's time to migrate? Define the trigger (e.g., "more than 20 rules" or "rules require cross-resource joins").

---

## Critical Gaps for Enterprise Adoption

1. **No multi-tenancy model.** Enterprise deployments need tenant isolation. Neo4j doesn't natively support tenancy — this needs a namespace strategy (label prefixes, separate databases, or graph views).

2. **No rate limiting or quota management** for the broker API. Enterprise APIs need per-tenant and per-user rate limits.

3. **No backup/restore strategy** for workflow proposals and KPI data. Neo4j backup is operational, but the enterprise PRD implies these are compliance artifacts.

4. **No observability integration.** Enterprise platforms need OpenTelemetry, structured logging, and alerting. The current logging is `log` crate + `env_logger` — insufficient for production enterprise.

5. **No data migration tooling.** How does a pilot customer get their existing workflows, decisions, or policies into the platform?

---

## Priority Order for Implementation

1. **Enterprise Identity (ADR-040)** — Blocks all other enterprise features. A broker cannot log in without this.
2. **Judgment Broker MVP (ADR-041)** — The core differentiator. Include manual workflow submission from day 1.
3. **Policy Engine (ADR-045)** — Enables broker decisions to be policy-gated. Small scope, high value.
4. **Workflow Proposal (ADR-042)** — Builds on broker decisions. The promotion lifecycle is the compounding loop.
5. **KPI Observability (ADR-043)** — Instrumentation. Requires broker decisions and workflow data to exist first.
6. **Connector Governance (ADR-044)** — Last because it needs the full loop (discovery → broker → workflow) to exist before ingested signals have anywhere to go.

---

## Neo4j Suitability

Neo4j is appropriate for workflow proposals and KPI snapshots because:
- Proposals are graph-structured (linked to insights, brokers, patterns via typed relationships)
- KPI lineage is inherently a graph traversal (DERIVED_FROM chains)
- The existing codebase and team expertise is Neo4j-native

Consider adding a time-series sidecar (TimescaleDB or ClickHouse) only if KPI query volume exceeds Neo4j's comfort zone (>10K snapshots/day with complex aggregations). Not needed for MVP.

---

## Recommendation

The architecture is ready for implementation. The delivery order above minimises risk and maximises early value. The critical gaps (multi-tenancy, observability, backup) should be tracked as ADR candidates and addressed before Phase 4 (pilot release).
