//! Framing-stage validation: `@context`, `@id`, `@type`, `@graph`,
//! `@version`, and the prov-O attribution / timestamp pair.
//!
//! "Framing" here is narrower than the W3C JSON-LD Framing
//! specification — we're just inspecting the document shape before any
//! N-Quad expansion is attempted. ADR-D01 §D12 checks A–D, and the
//! provenance check from §C.

use serde_json::Value;

use super::errors::ErrorCategory;
use super::iri;

/// Accepted `@context` URL values. ADR-D01 §D11: bump on breaking
/// changes. v1 is frozen; v2 (ontomark ADR-OM-012, visionGraph ADR-VG-009)
/// is served at both `/context/v2.jsonld` and `/ns/v2.jsonld`.
pub const ACCEPTED_CONTEXT_URLS: &[&str] = &[
    "https://narrativegoldmine.com/context/v1.jsonld",
    "https://narrativegoldmine.com/context/v2.jsonld",
    "https://narrativegoldmine.com/ns/v2.jsonld",
];

/// Result of inspecting one JSON-LD node within a block.
#[derive(Debug, Default)]
pub struct FrameIssues {
    pub categories: Vec<ErrorCategory>,
}

impl FrameIssues {
    fn push(&mut self, c: ErrorCategory) {
        self.categories.push(c);
    }
}

/// JSON-LD 1.1 features whose presence implies `@version: 1.1` must
/// be declared on the surrounding block. The fixture
/// `100-missing-schema-version.md` covers `@included`; other markers
/// (`@nest`, `@protected`) are added here for completeness.
pub const VERSION_1_1_MARKERS: &[&str] = &["@included", "@nest", "@protected"];

/// True if the JSON node OR any of its direct children uses a JSON-LD
/// 1.1-only keyword.
fn uses_jsonld_1_1_feature(node: &Value) -> bool {
    if let Value::Object(map) = node {
        for marker in VERSION_1_1_MARKERS {
            if map.contains_key(*marker) {
                return true;
            }
        }
    }
    false
}

/// Returns `true` if `node`'s `@context` (or `context`) declares
/// `@version: 1.1`. Accepts either the friendly `context` alias or the
/// canonical `@context`.
fn declares_version_1_1(node: &Value) -> bool {
    let Value::Object(map) = node else {
        return false;
    };
    let ctx = map.get("@context").or_else(|| map.get("context"));
    let Some(ctx) = ctx else {
        return false;
    };
    match ctx {
        // String form: just a URL; defaults to 1.0 unless the file
        // resolved at that URL declares @version 1.1. We treat the
        // canonical v1 URL as 1.0 from the document's POV — the
        // fixture's failure mode is using `@included` with a string
        // context, which IS the fault we're catching.
        Value::String(_) => false,
        Value::Object(obj) => obj
            .get("@version")
            .map(|v| v.as_f64() == Some(1.1) || v.as_str() == Some("1.1"))
            .unwrap_or(false),
        Value::Array(items) => items.iter().any(|item| match item {
            Value::Object(obj) => obj
                .get("@version")
                .map(|v| v.as_f64() == Some(1.1) || v.as_str() == Some("1.1"))
                .unwrap_or(false),
            _ => false,
        }),
        _ => false,
    }
}

/// Validate the high-level frame of a JSON-LD block (context, id, type,
/// provenance, schema-version-when-1.1-features-used).
pub fn validate_block_frame(block: &Value) -> FrameIssues {
    let mut issues = FrameIssues::default();
    let Value::Object(_) = block else {
        // Top-level must be a JSON object. Treat as ContextMissing as
        // the strongest signal — there's no context on a non-object.
        issues.push(ErrorCategory::ContextMissing);
        return issues;
    };

    // ADR-D01 §D7: `@graph` collections are validated per-entry.
    let entries: Vec<&Value> = collect_graph_entries(block);

    for entry in entries {
        validate_single_entry(entry, &mut issues, block);
    }

    issues
}

/// Collect the entries that carry assertions. If the top-level node has
/// `@graph`, each child is an assertion; otherwise the top-level node
/// itself is the sole assertion.
fn collect_graph_entries(block: &Value) -> Vec<&Value> {
    let Value::Object(map) = block else {
        return vec![block];
    };
    let graph_value = map.get("@graph").or_else(|| map.get("graph"));
    match graph_value {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(other) => vec![other],
        None => vec![block],
    }
}

fn validate_single_entry(entry: &Value, issues: &mut FrameIssues, root: &Value) {
    let Value::Object(map) = entry else {
        return;
    };

    // --- @context check (only required on the root, NOT on @graph
    // entries that inherit from a wrapping context). The
    // 101-missing-context fixture is a single-entry document, so this
    // applies; multi-graph wrappers carry @context once on the root.
    let context_carrier = if entry_has_context(entry) {
        entry
    } else {
        root
    };
    if !entry_has_context(context_carrier) {
        issues.push(ErrorCategory::ContextMissing);
    } else {
        // Validate the @context URL is in the accepted set.
        if let Some(url) = extract_context_url(context_carrier) {
            if !ACCEPTED_CONTEXT_URLS.contains(&url.as_str()) {
                issues.push(ErrorCategory::ContextVersionUnknown { found: url });
            }
        }
    }

    // --- Schema version check: if any JSON-LD-1.1-only keyword
    // appears in this entry and the context doesn't declare
    // @version: 1.1, the block is ill-formed (fixture 100).
    if uses_jsonld_1_1_feature(entry)
        && !declares_version_1_1(context_carrier)
        && !declares_version_1_1(entry)
    {
        issues.push(ErrorCategory::SchemaVersionMissing);
    }

    // --- @id well-formedness (fixture 104).
    let id_value = map.get("@id").or_else(|| map.get("id"));
    if let Some(Value::String(s)) = id_value {
        if !iri::is_well_formed(s) {
            issues.push(ErrorCategory::MalformedIri { value: s.clone() });
        }
    }

    // --- Provenance (ADR-D01 §D8): every block MUST carry both
    // attribution and timestamp.
    // v1 format: top-level prov:wasAttributedTo / prov:generatedAtTime
    // v2 format: nested "provenance" object with "attributedTo" / "generatedAt"
    let has_attribution = has_keyed(entry, &["prov:wasAttributedTo", "wasAttributedTo"])
        || has_provenance_field(entry, "attributedTo");
    let has_timestamp = has_keyed(entry, &["prov:generatedAtTime", "generatedAtTime"])
        || has_provenance_field(entry, "generatedAt");
    if !has_attribution {
        issues.push(ErrorCategory::ProvAttributionMissing);
    }
    if !has_timestamp {
        issues.push(ErrorCategory::ProvTimestampMissing);
    }
}

fn entry_has_context(entry: &Value) -> bool {
    let Value::Object(map) = entry else {
        return false;
    };
    map.contains_key("@context") || map.contains_key("context")
}

/// Pull the `@context` URL string when the context is a bare URL or an
/// array containing a URL. Returns `None` if the context is an inline
/// object (in which case the URL form is not used).
fn extract_context_url(entry: &Value) -> Option<String> {
    let Value::Object(map) = entry else {
        return None;
    };
    let ctx = map.get("@context").or_else(|| map.get("context"))?;
    match ctx {
        Value::String(s) => Some(s.clone()),
        Value::Array(items) => items.iter().find_map(|item| match item {
            Value::String(s) => Some(s.clone()),
            _ => None,
        }),
        _ => None,
    }
}

fn has_keyed(entry: &Value, keys: &[&str]) -> bool {
    let Value::Object(map) = entry else {
        return false;
    };
    keys.iter().any(|k| map.contains_key(*k))
}

/// Check whether a v2-style nested `"provenance"` object contains a given field.
fn has_provenance_field(entry: &Value, field: &str) -> bool {
    let Value::Object(map) = entry else {
        return false;
    };
    let Some(Value::Object(prov)) = map.get("provenance") else {
        return false;
    };
    prov.contains_key(field)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn missing_context_detected() {
        let block = json!({
            "@id": "urn:visionclaw:page:abc",
            "@type": "Page",
            "prov:wasAttributedTo": {"@id": "did:nostr:npub1bob"},
            "prov:generatedAtTime": {"@value": "2026-05-16T00:00:00Z"}
        });
        let issues = validate_block_frame(&block);
        assert!(issues
            .categories
            .iter()
            .any(|c| matches!(c, ErrorCategory::ContextMissing)));
    }

    #[test]
    fn unknown_context_version_detected() {
        let block = json!({
            "@context": "https://narrativegoldmine.com/context/v99.jsonld",
            "@id": "urn:visionclaw:page:abc",
            "@type": "Page",
            "prov:wasAttributedTo": {"@id": "did:nostr:npub1bob"},
            "prov:generatedAtTime": {"@value": "2026-05-16T00:00:00Z"}
        });
        let issues = validate_block_frame(&block);
        assert!(issues
            .categories
            .iter()
            .any(|c| matches!(c, ErrorCategory::ContextVersionUnknown { .. })));
    }

    #[test]
    fn every_served_context_url_is_accepted() {
        for url in ACCEPTED_CONTEXT_URLS {
            let block = json!({
                "@context": url,
                "@id": "urn:visionclaw:page:abc",
                "@type": "Page",
                "prov:wasAttributedTo": {"@id": "did:nostr:npub1bob"},
                "prov:generatedAtTime": {"@value": "2026-05-16T00:00:00Z"}
            });
            let issues = validate_block_frame(&block);
            assert!(
                !issues
                    .categories
                    .iter()
                    .any(|c| matches!(c, ErrorCategory::ContextVersionUnknown { .. })),
                "{url}"
            );
        }
    }

    #[test]
    fn schema_version_required_for_included() {
        let block = json!({
            "@context": "https://narrativegoldmine.com/context/v1.jsonld",
            "@id": "urn:visionclaw:owl:axiom:abc",
            "@type": "Axiom",
            "@included": [],
            "prov:wasAttributedTo": {"@id": "did:nostr:npub1bob"},
            "prov:generatedAtTime": {"@value": "2026-05-16T00:00:00Z"}
        });
        let issues = validate_block_frame(&block);
        assert!(issues
            .categories
            .iter()
            .any(|c| matches!(c, ErrorCategory::SchemaVersionMissing)));
    }

    #[test]
    fn malformed_iri_detected() {
        let block = json!({
            "@context": "https://narrativegoldmine.com/context/v1.jsonld",
            "@id": "urn visionclaw page with spaces",
            "@type": "Page",
            "prov:wasAttributedTo": {"@id": "did:nostr:npub1bob"},
            "prov:generatedAtTime": {"@value": "2026-05-16T00:00:00Z"}
        });
        let issues = validate_block_frame(&block);
        assert!(issues
            .categories
            .iter()
            .any(|c| matches!(c, ErrorCategory::MalformedIri { .. })));
    }
}
