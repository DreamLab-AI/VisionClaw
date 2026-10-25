//! VC-21-S1-01 host glue: build the bounded facts the opt-in Jev option may
//! see for one ingested page. The decision logic, wire shape and fallback live
//! in [`visionclaw_jev::ingest`]; this module only reads the page the ingest
//! path has already parsed and admitted.
//!
//! Draft suggestions only. Nothing here touches the §V4 inclusion gate, the
//! parsers, the node, the frontmatter or generation identity, and it runs only
//! for a page that has already passed the gate and declares no `type`.

use vault_core::frontmatter::split;
use vault_core::page::parse_page;
use vault_core::vocabulary::Vocabulary;
use visionclaw_jev::ingest::{PageFacts, RelationClaim};

/// Maximum body characters sent as the excerpt.
const EXCERPT_CHARS: usize = 2_000;
/// Maximum relation claims collected per page (the client trims again).
const MAX_CLAIMS: usize = 20;

/// The facts about `content` that may be sent, or `None` when no suggestion
/// is wanted: no vocabulary (no type candidates), unreadable frontmatter, or
/// the page already declares a `type`.
///
/// `path` is the source-relative page path. The caller must already have
/// applied the inclusion gate.
pub fn page_facts(content: &str, path: &str, vocab: Option<&Vocabulary>) -> Option<PageFacts> {
    let vocab = vocab?;
    let page = parse_page("", path, content).ok()?;
    if page.frontmatter.has("type") {
        return None;
    }
    let type_candidates: Vec<(String, String)> =
        vocab.types.iter().map(|(name, def)| (name.clone(), def.owl.clone())).collect();
    if type_candidates.is_empty() {
        return None;
    }
    let mut relations = Vec::new();
    'outer: for key in vocab.relations.keys() {
        for target in page.frontmatter.strings(key) {
            let target = target.trim().trim_start_matches("[[").trim_end_matches("]]").to_string();
            if target.is_empty() {
                continue;
            }
            relations.push(RelationClaim { key: key.clone(), target });
            if relations.len() >= MAX_CLAIMS {
                break 'outer;
            }
        }
    }
    let excerpt: String = page.body.trim().chars().take(EXCERPT_CHARS).collect();
    let frontmatter = split(content).yaml.unwrap_or("").to_string();
    Some(PageFacts { path: path.to_string(), excerpt, frontmatter, type_candidates, relations })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vocab() -> Vocabulary {
        Vocabulary::from_yaml_str(
            r#"
version: 1
types:
  Class: { owl: owl:Class }
  Individual: { owl: owl:NamedIndividual }
relations:
  is-subclass-of: { owl: "rdfs:subClassOf" }
"#,
        )
        .expect("vocabulary parses")
    }

    #[test]
    fn untyped_admitted_page_yields_facts_with_candidates_and_claims() {
        let md = "---\npublic: true\nis-subclass-of: \"[[Bar]]\"\n---\nFoo is a kind of Bar.\n";
        let f = page_facts(md, "working/Foo.md", Some(&vocab())).expect("facts");
        assert_eq!(f.path, "working/Foo.md");
        assert!(f.excerpt.contains("kind of Bar"));
        assert_eq!(f.type_candidates.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["Class", "Individual"]);
        assert_eq!(f.relations.len(), 1);
        assert_eq!((f.relations[0].key.as_str(), f.relations[0].target.as_str()), ("is-subclass-of", "Bar"));
        assert!(f.frontmatter.contains("public: true"));
    }

    #[test]
    fn typed_pages_no_vocab_and_bad_frontmatter_yield_nothing() {
        let typed = "---\npublic: true\ntype: Class\n---\nbody\n";
        assert!(page_facts(typed, "knowledge/A.md", Some(&vocab())).is_none());
        assert!(page_facts("---\npublic: true\n---\nbody\n", "working/A.md", None).is_none());
        assert!(page_facts("---\n: : bad: [\n---\nbody\n", "working/A.md", Some(&vocab())).is_none());
    }

    #[test]
    fn excerpt_is_bounded() {
        let md = format!("---\npublic: true\n---\n{}\n", "x".repeat(10_000));
        let f = page_facts(&md, "working/A.md", Some(&vocab())).unwrap();
        assert_eq!(f.excerpt.chars().count(), EXCERPT_CHARS);
    }
}
