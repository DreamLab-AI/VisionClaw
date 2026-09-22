//! ADR-2115 (supersedes ADR-2041) — the graph-type vocabulary.
//!
//! The knowledge graph is named `knowledge`. The former name `logseq` was
//! accepted as a read-only alias for one release; that release has passed and
//! the alias is gone — `logseq` is now an unknown graph type everywhere:
//! REST/WebSocket/query-string discriminators, `graphs` JSON object keys and
//! dotted settings paths.
//!
//! This module is the single place the vocabulary is spelled out.

/// Normalise an inbound graph-type value to the canonical vocabulary.
///
/// Unknown values are passed through unchanged so callers can still reject them.
pub fn normalise_graph_type(graph: &str) -> &str {
    match graph {
        "knowledge" => "knowledge",
        "visionclaw" | "agent" | "bots" => "visionclaw",
        other => other,
    }
}

/// Look up the knowledge-graph entry inside an inbound `graphs` JSON value.
pub fn knowledge_graph_value(graphs: &serde_json::Value) -> Option<&serde_json::Value> {
    graphs.get("knowledge")
}

/// Whether an inbound `graphs` JSON object carries the knowledge graph.
pub fn graphs_map_has_knowledge(graphs: &serde_json::Map<String, serde_json::Value>) -> bool {
    graphs.contains_key("knowledge")
}

/// Whether a dotted settings path addresses the knowledge graph.
pub fn path_targets_knowledge_graph(path: &str) -> bool {
    path.contains(".graphs.knowledge.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_retired_alias_is_no_longer_normalised() {
        // ADR-2115: a retired or unknown graph type is passed through for the
        // caller to reject rather than silently resolved to `knowledge`.
        assert_eq!(normalise_graph_type("retired-graph"), "retired-graph");
        assert_eq!(normalise_graph_type("knowledge"), "knowledge");
        assert_eq!(normalise_graph_type("visionclaw"), "visionclaw");
        assert_eq!(normalise_graph_type("agent"), "visionclaw");
        assert_eq!(normalise_graph_type("bots"), "visionclaw");
        assert_eq!(normalise_graph_type("nonsense"), "nonsense");
    }

    #[test]
    fn json_lookups_take_the_canonical_key_only() {
        let retired = serde_json::json!({ "retired-graph": { "physics": { "springK": 1 } } });
        let canonical = serde_json::json!({ "knowledge": { "physics": { "springK": 1 } } });
        assert!(
            knowledge_graph_value(&retired).is_none(),
            "an unknown graph key must not resolve"
        );
        assert_eq!(
            knowledge_graph_value(&canonical),
            canonical.get("knowledge")
        );
        assert!(knowledge_graph_value(&serde_json::json!({ "visionclaw": {} })).is_none());

        assert!(!graphs_map_has_knowledge(retired.as_object().unwrap()));
        assert!(graphs_map_has_knowledge(canonical.as_object().unwrap()));
        assert!(!graphs_map_has_knowledge(
            serde_json::json!({ "visionclaw": {} }).as_object().unwrap()
        ));
    }

    #[test]
    fn paths_match_the_canonical_segment_only() {
        assert!(!path_targets_knowledge_graph(
            "visualisation.graphs.retired-graph.physics.springK"
        ));
        assert!(path_targets_knowledge_graph(
            "visualisation.graphs.knowledge.physics.springK"
        ));
        assert!(!path_targets_knowledge_graph(
            "visualisation.graphs.visionclaw.physics.springK"
        ));
    }
}
