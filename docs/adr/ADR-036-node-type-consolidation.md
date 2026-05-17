# ADR-036: Node Type System Consolidation

## Status

Accepted

## Context

VisionFlow classifies nodes into three graph populations (Knowledge, Ontology, Agent) to drive dual-graph X-axis separation on the GPU, per-client type filtering over WebSocket, colour assignment, and visual mode detection. This classification is implemented independently in 11 locations across the Rust server and TypeScript client, each with its own string-matching logic and its own subset of recognised type literals.

The root defect is a casing mismatch: `neo4j_adapter.rs` writes `node_type = Some("OwlClass")` (PascalCase, line 573) while every downstream consumer matches `"owl_class"` (snake_case). Ontology nodes ingested from Neo4j are silently misclassified as Knowledge, breaking population counts, X-axis separation, and client-side visual mode.

### Affected call sites (11 implementations)

| # | File | Function / Logic | Recognised literals |
|---|------|-----------------|---------------------|
| 1 | `src/adapters/neo4j_adapter.rs:573` | Writes `node_type` during ontology ingest | `"OwlClass"` (PascalCase) |
| 2 | `src/actors/graph_state_actor.rs:190` | `classify_node()` | `page`, `linked_page`, `owl_class`, `ontology_node`, `owl_individual`, `owl_property`, `agent`, `bot` |
| 3 | `src/actors/graph_state_actor.rs:234` | `reclassify_all_nodes()` inline match | Same as #2 (duplicated) |
| 4 | `src/actors/gpu/force_compute_actor.rs:487` | GPU upload population classification | `agent`, `bot`, `owl_class`, `ontology_node`, `owl_individual`, `owl_property`, `page`, `block`, `knowledge_node` |
| 5 | `src/utils/binary_protocol.rs:183` | `get_node_type()` via flag bits | Reads from flag bits set by #2/#3, not strings |
| 6 | `src/handlers/socket_flow_handler/position_updates.rs:515` | Per-client `nodeTypes` filter | Maps `NodeType` enum to `"agent"`, `"knowledge"`, `"ontology"`, `"unknown"` |
| 7 | `src/handlers/bots_visualization_handler.rs:377` | Knowledge node filtering for bot viz | `page`, `linked_page`, `None` |
| 8 | `src/gpu/semantic_forces.rs:452` | `node_type_to_int()` | `generic`, `person`, `organization`, `project`, `task`, `concept`, `class`, `individual` |
| 9 | `client/src/features/graph/hooks/useGraphVisualState.ts:147` | Visual mode detection | `ontology_node`, `owl_class`, `OwlClass`, `page`, `linked_page`, `agent`, `bot` + IRI heuristic |
| 10 | `client/src/features/graph/utils/graphComputations.ts:160` | Colour assignment by type | `property`, `datatype_property`, `object_property`, `instance`, `individual` |
| 11 | `tests/github_sync_fix_test.rs:232` | Test node filtering | `page`, `linked_page` |

Sites #2 through #8 all use snake_case. Site #1 writes PascalCase. Site #9 patches around the inconsistency by accepting both casings. No site performs case-insensitive comparison.

## Decision

Consolidate to a single authoritative classification function and a canonical set of type constants.

### 1. `NodePopulation` enum in `src/models/graph_types.rs`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodePopulation {
    Knowledge,
    Ontology,
    Agent,
}
```

This replaces the private `GraphPopulation` enum in `force_compute_actor.rs` and aligns with the three populations already used by the GPU pipeline.

### 2. Single classification function

```rust
pub fn classify_node_population(node_type: &str) -> NodePopulation {
    match node_type.to_ascii_lowercase().as_str() {
        "agent" | "bot" => NodePopulation::Agent,
        "owl_class" | "owlclass" | "ontology_node"
        | "owl_individual" | "owlnamedindividual"
        | "owl_property" | "owlproperty"
        | "owl_datatype_property" | "owldatatypeproperty"
        | "owl_object_property" | "owlobjectproperty" => NodePopulation::Ontology,
        _ => NodePopulation::Knowledge,
    }
}
```

Case-insensitive via `to_ascii_lowercase()`. Accepts both `"OwlClass"` and `"owl_class"`. Default is Knowledge (preserving current fallback behaviour).

### 3. Canonical constants module

```rust
pub mod node_types {
    pub const PAGE: &str = "page";
    pub const LINKED_PAGE: &str = "linked_page";
    pub const OWL_CLASS: &str = "owl_class";
    pub const OWL_INDIVIDUAL: &str = "owl_individual";
    pub const OWL_PROPERTY: &str = "owl_property";
    pub const ONTOLOGY_NODE: &str = "ontology_node";
    pub const AGENT: &str = "agent";
    pub const BOT: &str = "bot";
}
```

### 4. Neo4j adapter normalization

`neo4j_adapter.rs` line 573 changes from `Some("OwlClass".to_string())` to `Some(node_types::OWL_CLASS.to_string())`, eliminating the casing mismatch at source.

### 5. Client mirror

TypeScript `getNodePopulation(nodeType: string): NodePopulation` in `client/src/features/graph/types/graphTypes.ts` mirrors the server function with identical matching rules. Accepts both casings.

## Consequences

### Positive

- Eliminates silent misclassification of ontology nodes ingested from Neo4j
- Single function to audit, test, and extend when new node types are added
- Case-insensitive matching prevents future casing drift
- Canonical constants prevent typos in string literals across the codebase
- GPU `GraphPopulation` and server `NodePopulation` share the same three-variant shape

### Negative

- All 11 call sites require update in a single coordinated change
- Adding a new node type now requires updating the central function (intentional gate)
- `semantic_forces.rs` `node_type_to_int()` serves a different purpose (semantic clustering, not population classification) and retains its own mapping; this ADR does not consolidate it

### Neutral

- `binary_protocol.rs` flag-bit system is unchanged; it reads from pre-classified data set by `GraphStateActor`, which will call the new function
- Test file `github_sync_fix_test.rs` filtering logic is unrelated to population classification and is left as-is

## Options Considered

### Option 1: Fix neo4j_adapter casing only

- **Pros**: Minimal change, fixes the immediate bug
- **Cons**: 11 independent match blocks remain; next new type will re-introduce drift

### Option 2: Consolidate to single function (chosen)

- **Pros**: Single source of truth, case-insensitive, testable in isolation
- **Cons**: Larger changeset touching multiple actors

### Option 3: Derive classification from OWL IRI presence

- **Pros**: No string matching needed for ontology nodes
- **Cons**: Knowledge nodes with `owl_class_iri` set would be misclassified; does not handle agent nodes

## Related Decisions

- ADR-031: Layout Mode System (defines the visual modes that consume population data)
- ADR-014: Semantic Pipeline Unification (upstream ontology enrichment)

## References

- `src/actors/gpu/force_compute_actor.rs` lines 24-31 (current `GraphPopulation` enum)
- `src/actors/graph_state_actor.rs` lines 190-217 (current `classify_node`)
- `src/adapters/neo4j_adapter.rs` line 573 (PascalCase write site)
