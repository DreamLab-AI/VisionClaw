//! OWL 2 EL profile boundary check.
//!
//! ADR-D01 §D4 lists the forbidden constructs. Authors get a
//! structured error pointing at OWL 2 EL §3, Table 1 so the failure is
//! actionable rather than a silent reasoner divergence.

use serde_json::Value;

use super::errors::ErrorCategory;

/// OWL constructs that the EL profile rejects. Sourced from
/// ADR-D01 §D4 and the W3C OWL 2 EL specification §3 Table 1.
pub const FORBIDDEN_CONSTRUCTS: &[&str] = &[
    // Predicates / class expressions outside EL.
    "owl:complementOf",
    "owl:unionOf",
    "owl:allValuesFrom",
    "owl:disjointWith",
    "owl:AllDisjointClasses",
    "owl:hasValue",
    "owl:hasSelf",
    "owl:minCardinality",
    "owl:maxCardinality",
    "owl:cardinality",
    "owl:qualifiedMinCardinality",
    "owl:qualifiedMaxCardinality",
    "owl:qualifiedCardinality",
    "owl:AsymmetricProperty",
    "owl:IrreflexiveProperty",
];

/// Walk a JSON-LD value recursively and return the first OWL construct
/// that exceeds the EL profile, if any.
pub fn scan_for_forbidden(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in map.keys() {
                if FORBIDDEN_CONSTRUCTS.contains(&key.as_str()) {
                    return Some(key.clone());
                }
                // Type declarations: `@type: owl:AllDisjointClasses` etc.
                if key == "@type" || key == "type" {
                    if let Some(found) = scan_type_for_forbidden(&map[key]) {
                        return Some(found);
                    }
                }
            }
            for v in map.values() {
                if let Some(found) = scan_for_forbidden(v) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(scan_for_forbidden),
        _ => None,
    }
}

fn scan_type_for_forbidden(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => {
            if FORBIDDEN_CONSTRUCTS.contains(&s.as_str()) {
                Some(s.clone())
            } else {
                None
            }
        }
        Value::Array(items) => items.iter().find_map(scan_type_for_forbidden),
        _ => None,
    }
}

/// Validate the EL profile boundary on the entry. Returns an empty
/// `Vec` when in-profile.
pub fn validate_entry_profile(entry: &Value) -> Vec<ErrorCategory> {
    match scan_for_forbidden(entry) {
        Some(construct) => vec![ErrorCategory::OutsideOwl2ElProfile { construct }],
        None => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn union_of_rejected() {
        let entry = json!({
            "@id": "urn:visionclaw:owl:axiom:abc",
            "@type": "Axiom",
            "vc:object": {
                "@type": "owl:Class",
                "owl:unionOf": { "@list": [
                    {"@id": "urn:visionclaw:owl:class:a"},
                    {"@id": "urn:visionclaw:owl:class:b"}
                ]}
            }
        });
        let issues = validate_entry_profile(&entry);
        assert!(matches!(
            issues.first(),
            Some(ErrorCategory::OutsideOwl2ElProfile { construct }) if construct == "owl:unionOf"
        ));
    }

    #[test]
    fn disjoint_with_rejected() {
        let entry = json!({
            "@id": "urn:visionclaw:owl:class:a",
            "@type": "OntologyClass",
            "owl:disjointWith": {"@id": "urn:visionclaw:owl:class:b"}
        });
        let issues = validate_entry_profile(&entry);
        assert!(matches!(
            issues.first(),
            Some(ErrorCategory::OutsideOwl2ElProfile { construct }) if construct == "owl:disjointWith"
        ));
    }

    #[test]
    fn subclass_of_accepted() {
        let entry = json!({
            "@id": "urn:visionclaw:owl:class:a",
            "@type": "OntologyClass",
            "subClassOf": {"@id": "urn:visionclaw:owl:class:b"}
        });
        assert!(validate_entry_profile(&entry).is_empty());
    }
}
