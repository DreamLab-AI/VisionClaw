# ADR-044: Connector Governance and Privacy Boundaries

## Status

Accepted

## Date

2026-04-14

## Context

The Insight Ingestion Loop (PRD Workstream 4) requires discovery signals from enterprise collaboration and work systems. The Discovery Engine cannot function on graph data alone; it needs to observe real coordination patterns in the tools where work actually happens -- issue trackers, code review, project boards, messaging.

The PRD (Workstream 6) lists Tier 1 connectors (Slack, Teams, Jira, Confluence, Notion, Google Workspace, GitHub) and Tier 2 connectors (ServiceNow, Salesforce, Linear, Zendesk, meeting transcripts, incident management). The PRD analysis (risk A, "Discovery Connectors Tarpit") warns that building reliable, enterprise-compliant integrations for all these platforms simultaneously will consume all engineering bandwidth.

Enterprise connector ingestion is a legal and privacy minefield. European works councils may require notification or approval before workplace monitoring tools are deployed. GDPR requires data minimisation, purpose limitation, and lawful basis. PII from chat messages, issue descriptions, and document content must not be stored without justification. Legal counsel in regulated industries may require human review of all ingested content before it enters the knowledge graph.

VisionClaw has no connector infrastructure. The existing data ingestion path is Neo4j-native (ontology import, Logseq graph import). There is no framework for authenticating to external APIs, managing sync state, handling rate limits, or applying privacy controls.

## Decision Drivers

- The PRD analysis explicitly warns against trying to build a universal ingestion engine
- Privacy and legal compliance are prerequisites for enterprise deployment, not afterthoughts
- Connector reliability (rate limits, API changes, auth token refresh) is notoriously difficult
- Scoped ingestion (admin chooses which repos/channels/projects) is mandatory
- PII redaction before storage protects against data sovereignty violations
- Legal review mode is required for deployments in jurisdictions with works-council constraints
- The MVP connector must demonstrate the full privacy pipeline, not just API integration

## Considered Options

### Option 1: Tiered approach, GitHub Issues/PRs first, with full privacy pipeline (chosen)

Build exactly one Tier 1 connector (GitHub Issues and Pull Requests) with the complete governance pipeline: scoped ingestion, PII redaction, legal review mode, sync state tracking. Design the connector interface so that subsequent connectors (Jira, Slack) plug into the same framework.

- **Pros**: Avoids the tarpit. GitHub's API is well-documented and stable. Issues/PRs are high-signal for workflow discovery (repeated patterns, approval bottlenecks, handoff failures). The privacy pipeline is validated end-to-end on a single connector before scaling to others.
- **Cons**: Organisations whose workflows live primarily in Slack or Jira will not benefit until Tier 2. Mitigation: the manual submission path (ADR-041) covers the gap.

### Option 2: Build all Tier 1 connectors simultaneously

Implement Slack, Teams, Jira, Confluence, Notion, Google Workspace, and GitHub connectors in parallel.

- **Pros**: Broadest coverage from day one.
- **Cons**: Each connector has different auth models (OAuth2, API keys, service accounts), different rate limit patterns, different data shapes. Testing matrix explodes. Privacy pipeline must be validated against seven different PII patterns simultaneously. Engineering team is spread thin. This is the tarpit the PRD analysis warns against.

### Option 3: Use a third-party integration platform (Airbyte, Fivetran)

Delegate connector management to an existing integration platform.

- **Pros**: Pre-built connectors. Managed rate limiting. Schema normalisation.
- **Cons**: Introduces a heavy external dependency. PII redaction and legal review mode must still be built on VisionClaw's side. The integration platform stores a copy of the data, creating an additional data sovereignty concern. Cost at enterprise scale. Loss of control over sync behaviour.

## Decision

**Option 1: Tiered connector approach. GitHub Issues/PRs as the first connector, with the full governance pipeline.**

### Connector Architecture

```
External System (GitHub)
    |
    v
ConnectorAdapter (Rust, per-source implementation)
    |-- authenticates via OAuth2 / personal access token
    |-- fetches events within configured scope
    |-- applies rate limiting and backoff
    |
    v
RawSignal (normalised event envelope)
    |
    v
RedactionPipeline
    |-- configurable PII detection (regex + heuristic)
    |-- field-level redaction or pseudonymisation
    |-- redaction logged for audit
    |
    v
LegalReviewQueue (optional, if legal_review_mode = true)
    |-- signals queued for human review before indexing
    |-- reviewer approves, redacts further, or rejects
    |
    v
IngestionService
    |-- creates Insight nodes in Neo4j
    |-- links to ConnectorSource
    |-- emits InsightCreated event
```

### ConnectorSource Entity

```cypher
CREATE (cs:ConnectorSource {
  id: randomUUID(),
  connector_type: "github",       // github | jira | slack | ...
  display_name: "GitHub: acme/regulatory-workflows",
  config: '{                       // encrypted JSON: auth tokens, scope
    "owner": "acme",
    "repos": ["regulatory-workflows", "compliance-docs"],
    "event_types": ["issues", "pull_requests", "reviews"],
    "auth_method": "oauth2"
  }',
  sync_state: '{                   // cursor/offset for incremental sync
    "last_sync_at": "2026-04-14T10:00:00Z",
    "last_event_id": "12345",
    "cursor": "abc123"
  }',
  status: "active",                // active | paused | error | disabled
  error_message: null,
  legal_review_mode: false,
  redaction_policy: "standard",    // none | standard | aggressive | custom
  created_at: datetime(),
  updated_at: datetime()
})

// Relationship to ingested Insights
(i:Insight)-[:DISCOVERED_FROM]->(cs:ConnectorSource)
```

### Scoped Ingestion

Administrators configure the exact scope of each connector:

```toml
[connectors.github_regulatory]
type = "github"
owner = "acme"
repos = ["regulatory-workflows", "compliance-docs"]
event_types = ["issues", "pull_requests", "reviews"]
sync_interval_minutes = 15
legal_review_mode = false
redaction_policy = "standard"

[connectors.github_regulatory.scope]
# Only ingest from these labels/states
issue_labels = ["workflow-candidate", "process-improvement"]
pr_states = ["merged"]  # Only completed PRs, not drafts
```

Scope configuration is stored on the `ConnectorSource` node and modifiable via an admin API endpoint. Changes to scope are logged as provenance events.

### Redaction Pipeline

The redaction pipeline runs before any content is stored in Neo4j:

| Policy | Behaviour |
|--------|-----------|
| `none` | No redaction. For internal-only, pre-approved deployments. |
| `standard` | Detect and replace: email addresses, phone numbers, IP addresses, credit card patterns, national ID patterns. Replace with `[REDACTED:{type}]` tokens. |
| `aggressive` | Standard + proper nouns not in the ontology, URLs not in the approved domain list, file paths, @mentions resolved to names. |
| `custom` | Admin-defined regex patterns + deny lists + allow lists. |

Redaction is logged: for each redacted field, a `RedactionRecord` is created linking the `Insight` to the redaction type, field, and policy applied. This enables auditors to verify that redaction is working and to understand what was removed.

```cypher
CREATE (r:RedactionRecord {
  id: randomUUID(),
  insight_id: "...",
  field: "body",
  redaction_type: "email",
  policy_applied: "standard",
  created_at: datetime()
})
(i:Insight)-[:REDACTED_BY]->(r:RedactionRecord)
```

### Legal Review Mode

When `legal_review_mode = true` on a `ConnectorSource`:

1. Ingested signals are stored in a `LegalReviewQueue` (Neo4j nodes with status `pending_review`)
2. Signals are **not** indexed as `Insight` nodes until reviewed
3. A reviewer (Admin or Auditor role, ADR-040) accesses the queue via `/api/connectors/{id}/review-queue`
4. For each signal, the reviewer can: `approve` (index as Insight), `redact-and-approve` (additional manual redaction), `reject` (discard, with reason logged)
5. The review decision is recorded as a provenance event

Legal review mode is mandatory in jurisdictions with works-council requirements. It is optional but recommended for initial pilot deployments.

### Connector Interface (for future connectors)

```rust
#[async_trait]
pub trait ConnectorAdapter: Send + Sync {
    /// Fetch new events since the last sync cursor
    async fn fetch_events(
        &self,
        config: &ConnectorConfig,
        sync_state: &SyncState,
    ) -> Result<Vec<RawSignal>, ConnectorError>;

    /// Validate the connector configuration and credentials
    async fn validate_config(
        &self,
        config: &ConnectorConfig,
    ) -> Result<(), ConnectorError>;

    /// Health check: is the external system reachable?
    async fn health_check(&self) -> Result<ConnectorHealth, ConnectorError>;
}
```

Tier 2 connectors (Jira, Slack) implement this trait. The redaction pipeline, legal review queue, and ingestion service are shared infrastructure.

### API

```
GET    /api/connectors                        # List configured connectors
POST   /api/connectors                        # Create new connector (Admin)
GET    /api/connectors/{id}                   # Connector detail + sync state
PUT    /api/connectors/{id}                   # Update connector config (Admin)
DELETE /api/connectors/{id}                   # Disable and archive connector (Admin)
POST   /api/connectors/{id}/sync              # Trigger manual sync (Admin)
GET    /api/connectors/{id}/sync-history      # Sync run history
GET    /api/connectors/{id}/review-queue      # Legal review queue (if enabled)
POST   /api/connectors/{id}/review-queue/{signal_id}/decide  # Review decision
GET    /api/connectors/{id}/redaction-log     # Redaction audit log
```

All connector management endpoints require the `Admin` role. Review queue endpoints also accept `Auditor`.

### Privacy-by-Design Principles

| Principle | Implementation |
|-----------|---------------|
| **Minimisation** | Scoped ingestion: admin selects specific repos/channels/projects. Only configured event types are fetched. |
| **Purpose limitation** | Ingested data is used solely for workflow discovery. Signals that do not produce Insights are purged after configurable retention (default: 30 days). |
| **Transparency** | All active connectors, their scopes, and their redaction policies are visible to Auditors. Users in monitored systems can be notified via configurable notification mechanisms. |
| **Access control** | Connector configuration requires Admin role. Raw signals are not accessible to Contributors or Brokers; only processed Insights are visible. |
| **Right to erasure** | Insights linked to a specific user can be deleted via an admin erasure endpoint. Cascading deletion removes the Insight node, associated RedactionRecords, and DERIVED_FROM edges to downstream entities. |
| **Audit trail** | Every sync, redaction, review decision, and configuration change is logged with timestamp and actor identity. |

## Consequences

### Positive

- Single connector (GitHub) validates the full pipeline end-to-end before scaling to others
- Privacy pipeline is a first-class concern, not bolted on after connector implementation
- Legal review mode enables deployment in works-council jurisdictions from day one
- Scoped ingestion gives administrators precise control over what enters the system
- Redaction audit trail satisfies GDPR accountability requirements
- The `ConnectorAdapter` trait provides a clean extension point for Tier 2 connectors
- ConnectorSource as a Neo4j entity enables lineage: Insight -> ConnectorSource -> external system

### Negative

- Only GitHub is available at launch. Organisations whose workflow signals live in Slack or Jira must wait for Tier 2 or use manual submission. Mitigation: manual submission (ADR-041) and the tiered roadmap.
- PII detection via regex is imperfect; it will have false positives (over-redaction) and false negatives (missed PII). Mitigation: aggressive mode as default for external-facing deployments; legal review mode as a safety net; configurable patterns for domain-specific PII.
- Legal review mode adds latency to the ingestion pipeline. Signals may sit in the review queue for hours or days. Mitigation: this is intentional -- legal review is a governance choice, not a performance constraint.
- Rate limiting and API pagination logic must be implemented per connector. Mitigation: shared retry/backoff infrastructure; connector interface isolates this complexity.

### Neutral

- Neo4j remains the sole storage for Insights and ConnectorSource nodes; no new database
- Existing graph data (ontology, beads, agent nodes) is unaffected by connector ingestion
- The Insight entity defined in the PRD (Section 12) is created by the ingestion service, not by connectors directly

## Related Decisions

- ADR-040: Enterprise Identity Strategy (Admin role required for connector management)
- ADR-041: Judgment Broker Workbench (manual submission covers cold-start and connector gaps)
- ADR-042: Workflow Proposal Object Model (Insights from connectors feed DISCOVERED_FROM on proposals)
- ADR-043: KPI Lineage Model (Insight creation events feed Mesh Velocity source chain)
- ADR-045: Policy Engine Approach (connector ingestion respects policy-based access controls)

## References

- PRD Workstream 6: Discovery Connectors
- PRD FR7: Discovery Signal Ingestion
- PRD Section 18, Risk A: Discovery Connectors Tarpit
- PRD Section 11, NFR-6: Privacy (connector ingestion supports minimisation and legal review)
- GDPR Articles 5, 6, 17, 25 (data minimisation, purpose limitation, right to erasure, data protection by design)
- GitHub REST API v3 documentation
