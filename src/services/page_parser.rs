// src/services/page_parser.rs
//! The single page-parsing seam for corpus ingest (ADR-2114).
//!
//! Ingest calls exactly one function to turn a page's markdown into a
//! [`CanonicalEntity`]: [`parse_page`]. It currently delegates to the
//! JSON-LD-fence parser the corpus is authored against today
//! ([`jsonld_ingest::parse_canonical_entity`]). When the sovereign-corpus
//! migration folds those fences into frontmatter properties, `vault_core`'s
//! parser replaces the body of this one function and nothing else in the sync
//! pipeline moves.

use crate::services::jsonld_ingest;
use visionclaw_domain::models::canonical_entity::CanonicalEntity;

/// Parse one corpus page into its canonical entity.
///
/// * `Ok(Some(entity))` — the page carries structured ontology content.
/// * `Ok(None)` — no structured content; the caller falls back to the plain
///   markdown/wikilink path that populates the working graph.
/// * `Err(_)` — the page has structured content that failed to parse; the
///   caller skips it and logs.
///
/// `source_path` is the source-relative page path, used for provenance and
/// for the domain derivation.
pub fn parse_page(content: &str, source_path: &str) -> Result<Option<CanonicalEntity>, String> {
    jsonld_ingest::parse_canonical_entity(content, source_path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FENCED: &str = r#"---
public: true
---

# Alpha

```json-ld
{
  "@context": "https://narrativegoldmine.com/ns/v2.jsonld",
  "@id": "urn:ngm:class:alpha",
  "@type": "Class",
  "label": "Alpha",
  "definition": "A fixture class.",
  "domain": "data",
  "maturity": "established",
  "subClassOf": [],
  "quality": 0.5
}
```
"#;

    #[test]
    fn a_page_with_structured_content_yields_an_entity() {
        let entity = parse_page(FENCED, "knowledge/pages/Alpha.md")
            .expect("a well-formed page parses")
            .expect("a Class block is structured content");
        assert_eq!(entity.slug, "alpha");
    }

    /// ADR-2112: the corpus is frontmatter-only. A `key:: value` line carries
    /// no ontology meaning — it is body text, and a page whose only "metadata"
    /// is such a block yields no canonical entity.
    #[test]
    fn a_logseq_property_line_is_body_text_not_metadata() {
        let page = "public:: true\nowl:class:: mv:Alpha\nterm-id:: 20067\n\n# Alpha\n\nProse.\n";
        assert!(
            parse_page(page, "knowledge/pages/Alpha.md")
                .expect("parsing must not fail")
                .is_none(),
            "a `key:: value` block contributes no structured content"
        );
    }

    /// The same line inside a page that *does* carry structured content does
    /// not leak into the entity either.
    #[test]
    fn a_logseq_property_line_beside_a_fence_is_ignored() {
        let page = FENCED.replace("# Alpha\n", "# Alpha\n\nowl:class:: mv:Impostor\n");
        let entity = parse_page(&page, "knowledge/pages/Alpha.md")
            .expect("a well-formed page parses")
            .expect("the fence is still structured content");
        assert_eq!(
            entity.slug, "alpha",
            "identity comes from the fence, never from a `key::` line"
        );
    }
}
