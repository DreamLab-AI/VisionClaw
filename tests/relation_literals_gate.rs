//! ADR-2204 E1 backstop — a relation weight, label table or fan-out default
//! must not reappear in code outside `crates/visionclaw-domain/data/relations.yaml`.
//!
//! Scans the production part (everything before the first `#[cfg(test)]`) of
//! the files that build relation edges, ignoring comment lines. It is a
//! tripwire for the S2 regression, not a parser: the producer/consumer tests
//! carry the real guarantee.

use std::path::Path;

/// Files that produce or weight relation edges.
const GUARDED: &[&str] = &[
    "src/services/github_sync_service.rs",
    "src/services/inferred_edge_materialiser.rs",
];

/// Substrings that mean a relation table or a weight scale is back in code.
const FORBIDDEN: &[(&str, &str)] = &[
    ("COCITE_WEIGHT", "a co-citation weight constant"),
    (
        "FANOUT_NODE_THRESHOLD",
        "the fan-out env var (schema owns it)",
    ),
    ("strength * 2", "the registry strength × 2 scale"),
    ("strength*2", "the registry strength × 2 scale"),
    ("fn predicate_to_edge_type", "the predicate → label map"),
    (
        "get_or_register_id",
        "registry auto-registration of unknown labels",
    ),
    (
        "SEMANTIC_TYPE_REGISTRY",
        "the registry as an edge-weight source",
    ),
];

/// Label arms such as `=> "hierarchical"` in a match.
const LABEL_ARMS: &[&str] = &[
    "hierarchical",
    "dependency",
    "structural",
    "associative",
    "bridge",
    "implements",
    "enhancement",
    "security",
    "provenance",
    "goal",
    "tracking",
    "similarity",
    "utilisation",
    "standardisation",
    "explicit_link",
];

fn production_lines(path: &str) -> Vec<(usize, String)> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
        .unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        if line.trim_start().starts_with("//") {
            continue;
        }
        out.push((i + 1, line.to_string()));
    }
    out
}

/// `weight: 1.0`, `weight: 0.5`… — a numeric literal where a schema value
/// belongs. `weight: weight` and `weight: RelationSchema…` are fine.
fn has_weight_literal(line: &str) -> bool {
    let mut rest = line;
    while let Some(at) = rest.find("weight:") {
        let after = rest[at + "weight:".len()..].trim_start();
        if after.starts_with(|c: char| c.is_ascii_digit()) {
            return true;
        }
        rest = &rest[at + "weight:".len()..];
    }
    false
}

#[test]
fn no_relation_weight_table_or_default_outside_the_schema() {
    let mut hits = Vec::new();
    for path in GUARDED {
        for (n, line) in production_lines(path) {
            for (needle, why) in FORBIDDEN {
                if line.contains(needle) {
                    hits.push(format!("{path}:{n}: {why}: {}", line.trim()));
                }
            }
            // A full predicate IRI constant (a `format!("…v1#{x}")` builder is
            // an axiom constructor, not a predicate → label table).
            if line.contains("narrativegoldmine.com/ns/v1#") && !line.contains("v1#{") {
                hits.push(format!(
                    "{path}:{n}: a predicate IRI constant: {}",
                    line.trim()
                ));
            }
            if has_weight_literal(&line) {
                hits.push(format!(
                    "{path}:{n}: a literal edge weight: {}",
                    line.trim()
                ));
            }
            if line.contains("Edge::new(") && line.contains(", 1.0)") {
                hits.push(format!(
                    "{path}:{n}: a literal edge weight: {}",
                    line.trim()
                ));
            }
            for label in LABEL_ARMS {
                if line.contains(&format!("=> \"{label}\"")) {
                    hits.push(format!("{path}:{n}: a label match arm: {}", line.trim()));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "relation knowledge outside crates/visionclaw-domain/data/relations.yaml:\n{}",
        hits.join("\n")
    );
}

/// The gate must be able to fail: a synthetic offender trips each detector.
#[test]
fn the_gate_detects_what_it_guards() {
    assert!(has_weight_literal("        weight: 1.5,"));
    assert!(has_weight_literal("Edge { weight: 0.5, id }"));
    assert!(!has_weight_literal(
        "weight: RelationSchema::builtin().x(),"
    ));
    assert!(!has_weight_literal("weight: w,"));
}

/// ADR-2204 D3 (S3): the co-citation fan-out boundary is the schema's one definition. Both sync
/// sites ask `admits_cocitation_at`; neither compares a referrer count to the threshold itself.
#[test]
fn both_sync_sites_use_the_one_fanout_boundary() {
    let lines = production_lines("src/services/github_sync_service.rs");
    let uses = lines
        .iter()
        .filter(|(_, l)| l.contains("admits_cocitation_at("))
        .count();
    assert_eq!(uses, 2, "the deferred pass and the stub fold");
    let hand_rolled: Vec<String> = lines
        .iter()
        .filter(|(_, l)| {
            (l.contains("cocite_threshold") || l.contains("threshold"))
                && (l.contains(" < ") || l.contains(" <= ") || l.contains(" > "))
                && !l.contains("admits_cocitation_at")
        })
        .map(|(n, l)| format!("{n}: {}", l.trim()))
        .collect();
    assert!(
        hand_rolled.is_empty(),
        "hand-rolled boundary: {hand_rolled:?}"
    );
}
