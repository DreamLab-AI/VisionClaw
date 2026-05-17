# ADR-043: KPI Lineage Model

## Status

Accepted

## Date

2026-04-14

## Context

The Coordination Collapse thesis proposes four organisational KPIs to measure whether an agentic mesh is compounding organisational capability or fragmenting it:

1. **Mesh Velocity** — time from first discovery signal to approved reusable workflow
2. **Augmentation Ratio** — proportion of decision/workflow volume resolved without human escalation
3. **Trust Variance** — rolling variance in decision quality, override rates, and policy exceptions across workflows and agents
4. **HITL Precision** — percentage of escalations where human intervention materially changed or improved the outcome

VisionClaw has no KPI infrastructure. The Analytics context (BC6) provides pagerank and clustering analytics on the graph, but no event-stream-based metric computation, no time-windowed aggregation, and no lineage from metric values back to the source events that produced them.

The PRD (Workstream 5, FR5) requires: "Every KPI is explainable from its source events. Metrics can be sliced by workflow, team, function, agent type, and time. KPI lineage must be auditable: users can click from a metric to the underlying decision events."

Lineage is the critical differentiator. Most dashboards show numbers; VisionClaw must show the provenance chain from a KPI value down to the individual broker decisions, workflow promotions, and escalation cases that produced it. This is what makes the KPIs auditable for regulated enterprises.

## Decision Drivers

- Each KPI must trace back to source events for auditability
- Metrics must be sliceable by team, function, workflow type, agent type, and time window
- Update frequency varies by KPI: hourly batch for velocity, near-real-time for HITL Precision
- The model must work with Neo4j as the primary store (consistent with all other graph entities)
- Auditors and transformation leaders must be able to drill from a dashboard number to the underlying events
- KPI computation must not degrade the broker workbench or graph query performance

## Considered Options

### Option 1: Neo4j materialized snapshots with DERIVED_FROM edges to source events (chosen)

Compute KPIs from domain event streams. Persist each computation as an `OrganisationalMetricSnapshot` node in Neo4j, linked to the source events via `DERIVED_FROM` edges. Snapshots are time-windowed and dimension-tagged for slicing.

- **Pros**: Full lineage as a graph traversal. Sliceable via Cypher. Consistent with the graph-native architecture. Auditors can traverse from metric to source events.
- **Cons**: High edge volume if every metric links to every contributing event. Requires careful snapshot granularity to control graph growth.

### Option 2: Time-series database (TimescaleDB/InfluxDB) with event references

Store KPI time-series in a dedicated time-series database. Each data point carries foreign keys to source events in Neo4j.

- **Pros**: Purpose-built for time-series queries. Excellent compression. Built-in downsampling.
- **Cons**: Introduces a third database (after Neo4j and PostgreSQL/RuVector). Cross-database lineage queries require application-level joins. Breaks the "Neo4j is primary for graph entities" storage strategy. Auditors cannot traverse lineage in a single query.

### Option 3: Compute KPIs on-demand from event queries

No materialized snapshots. KPIs computed live by querying the event graph.

- **Pros**: Always fresh. No storage overhead. No staleness.
- **Cons**: Expensive queries on every dashboard load. Cannot meet the 2-second load target (PRD NFR-3) for complex time-windowed aggregations across thousands of events. No historical trend without replaying the full event log.

## Decision

**Option 1: Neo4j materialized snapshots with DERIVED_FROM lineage edges.**

### KPI Definitions

#### Mesh Velocity

**Definition**: Median elapsed time (hours) from the `created_at` of an `Insight` to the `deployed_at` of the `WorkflowPattern` it became, within the measurement window.

**Source events**: `Insight.created_at`, `WorkflowProposal` status transitions, `WorkflowPattern.deployed_at`

**Computation**: For each `WorkflowPattern` deployed in the window, traverse `(pat:WorkflowPattern)<-[:PROMOTED_TO]-(wp:WorkflowProposal)<-[:CODIFIED_AS]-(i:Insight)` and compute `pat.deployed_at - i.created_at`. Report median, p75, p95.

**Update frequency**: Hourly (default). Configurable.

**Slice dimensions**: team, function, workflow type, time window.

#### Augmentation Ratio

**Definition**: Fraction of total workflow executions and decision events that completed without escalation to a human broker, within the measurement window.

**Source events**: Workflow execution completions (automated), `CaseEscalated` events (human intervention required)

**Computation**: `(total_completions - escalated_completions) / total_completions`

**Update frequency**: Hourly (default).

**Slice dimensions**: team, function, agent type, workflow type, time window.

#### Trust Variance

**Definition**: Rolling standard deviation of a composite trust score across workflows and agents, measuring consistency of decision quality. The composite score incorporates override rate (broker overrides automated suggestion), policy exception rate, and escalation frequency.

**Source events**: `BrokerDecisionMade` (track overrides of system suggestion), `PolicyExceptionRequested`, `CaseEscalated`

**Computation**: For each workflow/agent in the window, compute a trust score. Report variance across the population. High variance indicates inconsistent quality requiring attention.

**Update frequency**: Daily (default). Configurable.

**Slice dimensions**: team, function, workflow type, agent type, time window.

#### HITL Precision

**Definition**: Percentage of human escalations where the broker's decision materially differed from the system's automated suggestion, within the measurement window. A high HITL Precision means escalations are being routed well (humans add value). A low HITL Precision means the system is over-escalating (wasting broker attention).

**Source events**: `BrokerDecisionMade` (compare `decision.outcome` vs `case.suggested_decision`)

**Computation**: `count(decision != suggestion) / count(all_decisions)` for escalated cases.

**Update frequency**: Real-time (computed on each `BrokerDecisionMade` event, snapshot persisted at configurable interval, default: every 15 minutes).

**Slice dimensions**: broker, team, case category, time window.

### Data Model

```cypher
// OrganisationalMetricSnapshot — a point-in-time KPI value with dimensions
CREATE (s:OrganisationalMetricSnapshot {
  id: randomUUID(),
  kpi: "mesh_velocity",           // mesh_velocity | augmentation_ratio | trust_variance | hitl_precision
  value: 72.5,                    // the computed metric value
  unit: "hours",                  // hours | ratio | stddev | percentage
  confidence: 0.85,               // 0.0-1.0, based on sample size and data completeness
  window_start: datetime(),       // measurement window start
  window_end: datetime(),         // measurement window end
  dimensions: '{                  // JSON: slice dimensions for this snapshot
    "team": "regulatory-affairs",
    "workflow_type": "review",
    "agent_type": null
  }',
  sample_size: 47,                // number of source events contributing to this value
  computed_at: datetime()
})

// Lineage edges to source events
(s:OrganisationalMetricSnapshot)-[:DERIVED_FROM]->(d:BrokerDecision)
(s:OrganisationalMetricSnapshot)-[:DERIVED_FROM]->(wp:WorkflowProposal)
(s:OrganisationalMetricSnapshot)-[:DERIVED_FROM]->(pat:WorkflowPattern)
(s:OrganisationalMetricSnapshot)-[:DERIVED_FROM]->(i:Insight)
(s:OrganisationalMetricSnapshot)-[:DERIVED_FROM]->(c:BrokerCase)

// Snapshot time series
(s2:OrganisationalMetricSnapshot)-[:SUCCEEDS]->(s1:OrganisationalMetricSnapshot)
```

### Lineage Granularity

Full lineage (one `DERIVED_FROM` edge per contributing event) is stored for snapshots with `sample_size <= 500`. For larger samples, a summary approach is used:

1. The snapshot stores `sample_size` and `dimensions`
2. A `DERIVED_FROM_SUMMARY` edge links to a `MetricSourceBatch` node containing the Cypher query used to compute the metric and the event ID range
3. Auditors can re-execute the query to enumerate all contributing events

This prevents graph edge explosion for high-volume KPIs while maintaining full audit reproducibility.

### Computation Architecture

KPI computation runs as a background service within the existing actor system:

```
Domain Events (BrokerDecisionMade, WorkflowDeployed, CaseEscalated, ...)
    |
    v
KpiComputeActor (Rust, runs on configurable schedule per KPI)
    |
    +-- queries Neo4j for source events in the window
    +-- computes metric value
    +-- creates OrganisationalMetricSnapshot node
    +-- creates DERIVED_FROM edges to source events
    +-- emits MetricSnapshotCreated event (for dashboard push)
    |
    v
WebSocket push to subscribed dashboard clients
```

The `KpiComputeActor` is a new actor in the existing actix actor system. It does not block request processing. Computation runs on its own schedule, independent of the broker workbench or graph API.

### API

```
GET /api/mesh-metrics                           # List latest snapshot per KPI
GET /api/mesh-metrics/{kpi}                     # Latest value + trend for a specific KPI
GET /api/mesh-metrics/{kpi}/history             # Time series of snapshots
  ?from=2026-01-01&to=2026-04-14&granularity=daily
GET /api/mesh-metrics/{kpi}/lineage             # Source events for a specific snapshot
  ?snapshot_id=...
GET /api/mesh-metrics/{kpi}/slice               # Slice by dimensions
  ?team=regulatory-affairs&workflow_type=review
GET /api/mesh-metrics/export                    # Audit export (CSV/JSON)
  ?from=...&to=...&kpis=mesh_velocity,hitl_precision
```

### Confidence Model

Each snapshot carries a `confidence` score (0.0-1.0) based on:

- **Sample size**: below 10 events -> confidence capped at 0.5
- **Data completeness**: if source events are missing expected fields -> confidence reduced
- **Window coverage**: if the measurement window has gaps (e.g., system downtime) -> confidence reduced
- **Staleness**: if the last contributing event is older than 2x the expected update frequency -> confidence degraded

Dashboards display confidence alongside the metric value. Low-confidence metrics are visually distinguished.

## Consequences

### Positive

- Every KPI value traces back to the exact events that produced it, satisfying the auditability requirement
- DERIVED_FROM edges make lineage a native Cypher traversal, enabling the drill-down UX described in the PRD
- Materialized snapshots meet the 2-second dashboard load target without expensive live aggregation
- Dimension tagging enables slicing without schema changes; new dimensions are additive
- Confidence model gives auditors and leaders transparency about metric reliability
- SUCCEEDS edges between snapshots enable trend queries without index scans

### Negative

- DERIVED_FROM edges consume storage proportional to events per snapshot. Mitigation: summary batching for high-volume snapshots (>500 events).
- Snapshot staleness is possible if the compute actor fails or falls behind. Mitigation: health monitoring via BC6 (Analytics), alerting on compute lag.
- Four KPIs with multiple dimension combinations can produce many snapshot nodes. Mitigation: configurable retention policy; archive snapshots older than 12 months to cold storage with summary-only lineage.
- Confidence model requires tuning against real data. Initial defaults are heuristic. Mitigation: expose confidence parameters in configuration; refine based on pilot data.

### Neutral

- Existing Analytics context (BC6) is not modified; KPI computation is a new actor alongside existing analytics actors
- Neo4j query patterns for KPI snapshots are standard label + property index queries
- The client dashboard is a new view but uses the same WebSocket subscription pattern as the broker workbench

## Related Decisions

- ADR-041: Judgment Broker Workbench (BrokerDecisionMade events are primary source for HITL Precision and Trust Variance)
- ADR-042: Workflow Proposal Object Model (WorkflowProposal and WorkflowPattern lifecycle events feed Mesh Velocity)
- ADR-044: Connector Governance (Insight creation events from connectors feed Mesh Velocity source chain)
- ADR-045: Policy Engine Approach (PolicyEvaluated events contribute to Trust Variance)

## References

- PRD Workstream 5: Organisational KPI & Observability Layer
- PRD FR5: KPI Engine
- PRD Section 16: Success Metrics
- `presentation/report/chapters/08-new-kpis.tex`
- `docs/explanation/ddd-bounded-contexts.md` (BC6: Analytics & Monitoring)
