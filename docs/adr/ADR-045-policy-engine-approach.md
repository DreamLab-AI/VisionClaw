# ADR-045: Policy Engine Approach

## Status

Accepted

## Date

2026-04-14

## Context

The governed agentic mesh requires policy enforcement at multiple points: escalation routing, workflow deployment permissions, domain ownership, confidence thresholds, separation of duty, and regulatory constraints. Without a policy layer, the Judgment Broker Workbench (ADR-041) has no rules governing when cases should escalate, the Workflow Proposal lifecycle (ADR-042) has no deployment permissions, and the KPI layer (ADR-043) has no trust boundaries to measure against.

The PRD (Workstream 7) specifies: "Introduce a reusable policy model for escalation rules, domain ownership, confidence thresholds, regulatory workflows, separation-of-duty rules, workflow deployment permissions."

The core question is whether to build an embedded rule engine within the Rust application or adopt an external policy engine (Open Policy Agent, Cedar, Casbin). The answer depends on rule complexity, team capability, operational overhead, and the platform's current maturity.

VisionClaw is in its first enterprise phase. The initial policy requirements are known and bounded: escalation thresholds, domain ownership checks, confidence gates, and separation-of-duty constraints. These are evaluable as simple predicate logic over a `DecisionContext` struct. They do not require a Turing-complete policy language, complex policy composition, or cross-service policy distribution (yet).

### Current State

- No policy infrastructure exists in the codebase
- ADR-011 enforces authentication at the middleware level, but authorization beyond "authenticated or not" is not implemented
- Role-based access is defined in ADR-040 but not enforced by a policy engine
- The bead provenance system (ADR-034) records events but does not evaluate policies against them
- The existing Rust codebase uses trait-based abstractions extensively (e.g., `BeadStore` in ADR-034)

## Decision Drivers

- Initial policy rules are bounded and well-defined (6 built-in rule types)
- The team is Rust-native; an embedded engine avoids operational overhead of running a sidecar
- Policy evaluation must be fast (< 5ms per evaluation) to avoid degrading broker inbox load times
- Policy evaluation must produce provenance events for the KPI lineage model (ADR-043)
- Rules must be configurable by administrators without code changes
- The approach must have a viable migration path to a more powerful engine if rule complexity grows
- The platform does not yet have multi-service deployment; policies are evaluated in a single process

## Considered Options

### Option 1: Embedded Rust trait-based policy engine with TOML/YAML configuration (chosen)

Define a `PolicyRule` trait in Rust. Implement built-in rules as structs implementing this trait. Rules are parameterised via TOML/YAML configuration files. Policy evaluation is a synchronous function call within the application process. Every evaluation is logged as a `PolicyEvaluated` provenance event.

- **Pros**: Zero external dependencies. Sub-millisecond evaluation. Full type safety. Rules are testable with standard Rust unit tests. Configuration changes do not require recompilation. The trait boundary is the future migration seam to an external engine.
- **Cons**: Policy logic is coupled to the Rust application. Cannot share policies across services written in other languages (not relevant today, but relevant if the platform goes multi-service). TOML/YAML is less expressive than Rego or Cedar for complex policy composition.

### Option 2: Open Policy Agent (OPA) sidecar with Rego policies

Run OPA as a sidecar. Policies written in Rego. Application sends decision requests to OPA over HTTP or gRPC.

- **Pros**: Industry-standard. Rego is purpose-built for policy. Supports complex policy composition. Policies shareable across services. Large ecosystem.
- **Cons**: Adds a sidecar process to deploy and monitor. 1-5ms network overhead per evaluation (acceptable but unnecessary when policies are simple). Rego has a learning curve. Debugging Rego policies is harder than debugging Rust code. Operational overhead for a team that is Rust-native and deploying a single binary.

### Option 3: Cedar (AWS policy language)

Use Cedar for policy definitions. Evaluate via the `cedar-policy` Rust crate (embedded, no sidecar).

- **Pros**: Embedded in Rust (no sidecar). Cedar is designed for authorization. Type-safe policy schemas. Formal verification of policy properties.
- **Cons**: Relatively new ecosystem. Cedar's entity model is optimised for resource-action-principal patterns, which maps well to authorization but less naturally to threshold-based escalation rules or confidence gates. Adds a dependency on the Cedar crate and its policy format. Team must learn Cedar syntax.

### Option 4: No formal policy engine; hardcode rules in application logic

Write policy checks as `if` statements in handler code.

- **Pros**: Simplest implementation. No abstractions.
- **Cons**: Rules not configurable without code changes. Not testable in isolation. Not auditable. Scattered through the codebase. No migration path. Violates the PRD requirement that policies be configurable.

## Decision

**Option 1: Embedded Rust trait-based policy engine with TOML/YAML configuration.**

### PolicyRule Trait

```rust
/// Result of a policy evaluation
pub struct PolicyResult {
    pub rule_id: String,
    pub outcome: PolicyOutcome,
    pub reasoning: String,
    pub confidence: f64,        // 0.0-1.0
}

pub enum PolicyOutcome {
    Allow,
    Deny,
    Escalate,                   // requires human review
    Warn { message: String },   // allow but flag
}

/// Context provided to every policy evaluation
pub struct DecisionContext {
    pub actor: ActorIdentity,       // who is performing the action
    pub action: PolicyAction,       // what action is being performed
    pub resource: ResourceRef,      // what resource is being acted on
    pub metadata: HashMap<String, serde_json::Value>,  // additional context
}

pub enum PolicyAction {
    ApproveWorkflow,
    DeployWorkflow,
    EscalateCase,
    OverrideDecision,
    AccessConnector,
    ModifyPolicy,
}

/// Every policy rule implements this trait
#[async_trait]
pub trait PolicyRule: Send + Sync {
    /// Unique identifier for this rule type
    fn rule_id(&self) -> &str;

    /// Human-readable description
    fn description(&self) -> &str;

    /// Evaluate the rule against the given context
    async fn evaluate(&self, context: &DecisionContext) -> Result<PolicyResult, PolicyError>;
}
```

### Built-in Rules

| Rule ID | Description | Configuration Parameters |
|---------|-------------|------------------------|
| `escalation_threshold` | Escalate to broker when automated confidence drops below threshold | `threshold: 0.7`, `applies_to: [workflow_type]` |
| `domain_ownership` | Only the domain owner (or delegate) can approve workflows in their domain | `domains: {regulatory: [pubkey1, pubkey2]}` |
| `confidence_threshold` | Deny automated actions when model confidence is below minimum | `min_confidence: 0.5`, `action: deny\|escalate` |
| `separation_of_duty` | The proposer of a workflow cannot also be its approver | `applies_to: [approve_workflow, deploy_workflow]` |
| `deployment_scope` | Restrict workflow deployment to permitted scopes (team, department, org-wide) | `max_scope_without_escalation: "department"` |
| `rate_limit` | Limit the number of automated actions per time window per agent | `max_actions: 100`, `window_minutes: 60` |

### Configuration Format

Policies are defined in TOML files, loaded at startup and reloadable without restart:

```toml
[policies]

[policies.escalation_threshold]
enabled = true
threshold = 0.7
applies_to = ["regulatory_review", "compliance_check"]
action_on_breach = "escalate"

[policies.domain_ownership]
enabled = true

[policies.domain_ownership.domains]
regulatory = ["npub1abc...", "npub1def..."]
engineering = ["npub1ghi..."]
finance = ["npub1jkl..."]

[policies.separation_of_duty]
enabled = true
applies_to = ["approve_workflow", "deploy_workflow"]

[policies.confidence_threshold]
enabled = true
min_confidence = 0.5
action_on_breach = "escalate"

[policies.deployment_scope]
enabled = true
max_scope_without_escalation = "department"
# org-wide deployment requires explicit broker approval

[policies.rate_limit]
enabled = true
max_actions_per_agent = 100
window_minutes = 60
action_on_breach = "deny"
```

Configuration changes are detected via file watching or admin API:

```
GET  /api/policy                              # List all active policies and their config
GET  /api/policy/{rule_id}                    # Detail for a specific policy rule
PUT  /api/policy/{rule_id}                    # Update policy configuration (Admin only)
GET  /api/policy/evaluations                  # Recent evaluation log
GET  /api/policy/evaluations?resource={id}    # Evaluations for a specific resource
```

### Evaluation Pipeline

```
Action Request (e.g., broker approves workflow)
    |
    v
PolicyEngine::evaluate_all(context) -> Vec<PolicyResult>
    |
    +-- for each enabled PolicyRule:
    |     rule.evaluate(context) -> PolicyResult
    |
    +-- aggregate results:
    |     any Deny -> block action
    |     any Escalate -> route to broker (if not already broker action)
    |     all Allow -> proceed
    |     any Warn -> proceed but log warning
    |
    +-- emit PolicyEvaluated provenance event (all results, regardless of outcome)
    |
    v
Action proceeds or is blocked/escalated
```

### Provenance Integration

Every policy evaluation is recorded as a provenance event:

```cypher
CREATE (pe:PolicyEvaluated {
  id: randomUUID(),
  rule_id: "escalation_threshold",
  outcome: "escalate",
  reasoning: "Automated confidence 0.62 below threshold 0.70",
  context_action: "approve_workflow",
  context_resource_id: "...",
  context_actor_pubkey: "...",
  evaluated_at: datetime()
})

// Link to the resource being acted on
(pe:PolicyEvaluated)-[:EVALUATED_FOR]->(wp:WorkflowProposal)

// Link to the resulting case if escalated
(pe:PolicyEvaluated)-[:TRIGGERED]->(c:BrokerCase)
```

This enables the KPI lineage model (ADR-043) to trace Trust Variance back to specific policy evaluations, and enables auditors to answer "which policies were evaluated for this decision?"

### Migration Path to OPA

The `PolicyRule` trait is the migration seam. If rule complexity exceeds what the embedded engine handles well (indicators: rules that need cross-resource reasoning, temporal logic, or policy composition beyond simple conjunction), the migration path is:

1. Implement an `OpaPolicyRule` struct that implements `PolicyRule` by calling OPA over HTTP
2. Move complex rules to Rego; keep simple rules embedded
3. Both embedded and OPA-backed rules coexist in the same evaluation pipeline
4. Eventually, migrate all rules to OPA if warranted

This is a gradual migration, not a rewrite. The trait boundary ensures that the evaluation pipeline, provenance logging, and API are unchanged.

## Consequences

### Positive

- Zero external dependencies; policy evaluation is a function call in the same process
- Sub-millisecond evaluation latency; no impact on broker workbench performance
- Full Rust type safety; policy bugs caught at compile time
- TOML configuration enables admin-managed rules without recompilation
- Every evaluation is logged as provenance, feeding the KPI and audit systems
- The `PolicyRule` trait provides a clean migration path to OPA or Cedar if needed
- Built-in rules cover all six policy types specified in the PRD

### Negative

- Policy logic is Rust-only; cannot be shared with services in other languages (not relevant today but noted)
- TOML/YAML configuration is less expressive than Rego for complex conditional logic. Mitigation: the migration path to OPA exists if this becomes a limitation.
- New built-in rule types require Rust code (implementing the trait). Mitigation: the six built-in types cover the known requirements; custom rules are the migration trigger for OPA.
- No formal verification of policy properties (Cedar offers this). Mitigation: standard Rust unit and integration tests for policy rules.

### Neutral

- Existing `RequireAuth` middleware (ADR-011) is not replaced; it continues to enforce authentication. The policy engine enforces authorization and domain-specific rules above the authentication layer.
- The policy engine is a new Rust module alongside existing modules; no existing code is modified.
- Policy configuration files are additive; they do not replace any existing configuration.

## Related Decisions

- ADR-040: Enterprise Identity Strategy (role model referenced by domain ownership and separation-of-duty rules)
- ADR-041: Judgment Broker Workbench (PolicyExceptionRequested events feed the broker inbox; escalation rules route cases to brokers)
- ADR-042: Workflow Proposal Object Model (deployment scope and separation-of-duty rules govern workflow lifecycle transitions)
- ADR-043: KPI Lineage Model (PolicyEvaluated events feed Trust Variance KPI)
- ADR-011: Universal Authentication Enforcement (authentication layer, below the policy engine)
- ADR-034: Needle Bead Provenance (policy evaluations recorded as provenance events)
- ADR-049: Insight-migration broker workflow — uses the policy engine for candidate-surfacing thresholds and suppression rules

## References

- PRD Workstream 7: Provenance, Policy, and Digital Twin Reframing
- PRD Section 12: Policy Model requirements
- PRD Section 18, Decision 6: Policy language (embedded rules only or reusable DSL)
- Open Policy Agent: https://www.openpolicyagent.org/
- Cedar Policy Language: https://www.cedarpolicy.com/
- `src/settings/auth_extractor.rs` (existing auth middleware)
