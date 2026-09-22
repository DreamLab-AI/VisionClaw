//! The OKF v0.2 bundle export and the JSON-LD context.
//!
//! `okf/` is the **exchange** form: an `index.md` (OKF §8) plus one markdown
//! concept file per public class, each carrying the lifecycle and trust block
//! in frontmatter. It is not a second copy of the vault — it is the
//! redacted, public projection, which is why it is generated from the
//! projected corpus rather than copied from disk.

use std::fmt::Write as _;

use serde_json::{json, Value};
use vault_core::vocabulary::Vocabulary;

use crate::model::{ClassRecord, Corpus, RELATION_KEYS};

/// One file of the OKF bundle.
#[derive(Debug, Clone)]
pub struct BundleFile {
    /// Path relative to `okf/`.
    pub path: String,
    /// File contents.
    pub content: String,
}

/// Build the JSON-LD context document (`context/v1.jsonld`).
///
/// Every relation the vocabulary declares gets a term mapping, so a consumer
/// can expand the page API's camelCase keys without a second lookup table.
#[must_use]
pub fn context(vocab: &Vocabulary) -> Value {
    let mut ctx = serde_json::Map::new();
    ctx.insert("vc".into(), json!(super::turtle::VC));
    ctx.insert("ngm".into(), json!(super::turtle::NGM));
    ctx.insert("ngmi".into(), json!(super::turtle::NGMI));
    ctx.insert("owl".into(), json!("http://www.w3.org/2002/07/owl#"));
    ctx.insert(
        "rdfs".into(),
        json!("http://www.w3.org/2000/01/rdf-schema#"),
    );
    ctx.insert("xsd".into(), json!("http://www.w3.org/2001/XMLSchema#"));
    ctx.insert("prov".into(), json!("http://www.w3.org/ns/prov#"));
    ctx.insert("skos".into(), json!("http://www.w3.org/2004/02/skos/core#"));
    for (key, def) in &vocab.relations {
        ctx.insert(
            vocab.json_key(key),
            json!({ "@id": vocab.expand(&def.owl), "@type": "@id" }),
        );
    }
    for key in vocab.scalars.keys() {
        ctx.insert(key.clone(), json!(format!("{}{key}", super::turtle::VC)));
    }
    json!({ "@context": Value::Object(ctx) })
}

/// Build the OKF bundle: `index.md` plus `concepts/<slug>.md` per public class.
#[must_use]
pub fn bundle(corpus: &Corpus, vocab: &Vocabulary, generation_id: &str) -> Vec<BundleFile> {
    let mut classes: Vec<&ClassRecord> = corpus.public_classes().collect();
    classes.sort_by_key(|r| r.class_slug());

    let mut files = Vec::with_capacity(classes.len() + 1);
    let mut index = String::new();
    let _ = writeln!(index, "---");
    let _ = writeln!(index, "okf_version: \"0.2\"");
    let _ = writeln!(index, "type: Index");
    let _ = writeln!(index, "title: NarrativeGoldmine Knowledge Bundle");
    let _ = writeln!(index, "generation: {generation_id}");
    let _ = writeln!(index, "vocabulary_version: {}", vocab.version);
    let _ = writeln!(index, "concept_count: {}", classes.len());
    let _ = writeln!(index, "---");
    let _ = writeln!(index);
    let _ = writeln!(index, "# NarrativeGoldmine Knowledge Bundle");
    let _ = writeln!(index);
    let _ = writeln!(
        index,
        "{} concepts, exported from generation `{generation_id}`.",
        classes.len()
    );
    let _ = writeln!(index);

    for record in &classes {
        let slug = record.class_slug();
        let _ = writeln!(
            index,
            "- [{}](concepts/{slug}.md) — `{}`",
            record.label, record.iri
        );
        files.push(BundleFile {
            path: format!("concepts/{slug}.md"),
            content: concept_file(record),
        });
    }

    files.insert(
        0,
        BundleFile {
            path: "index.md".to_owned(),
            content: index,
        },
    );
    files
}

fn concept_file(record: &ClassRecord) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "---");
    let _ = writeln!(out, "okf_version: \"0.2\"");
    let _ = writeln!(out, "type: {}", record.entity_type.as_str());
    let _ = writeln!(out, "title: {}", yaml_scalar(&record.label));
    let _ = writeln!(out, "resource: {}", record.iri);
    if !record.domain.is_empty() {
        let _ = writeln!(out, "domain: {}", yaml_scalar(&record.domain));
    }
    // OKF `description`: a SHORT form of the definition, derived here rather
    // than authored. The page's definition is its leading paragraph — 451
    // characters at the median — and the same character cap the scaffold index
    // uses applies, so the two artefacts agree on what "short" means and the
    // rule stays Python's (characters, not bytes).
    if !record.definition.is_empty() {
        let _ = writeln!(
            out,
            "description: {}",
            yaml_scalar(&crate::build::indexes::truncate_definition(
                &record.definition
            ))
        );
    }
    let _ = writeln!(out, "maturity: {}", yaml_scalar(&record.maturity));
    let _ = writeln!(out, "quality: {}", record.quality);
    if !record.sub_class_of.is_empty() {
        let _ = writeln!(out, "is-a:");
        for r in &record.sub_class_of {
            let _ = writeln!(out, "  - {}", r.iri);
        }
    }
    for (_, json_key) in RELATION_KEYS {
        let refs = record.relation(json_key);
        if refs.is_empty() {
            continue;
        }
        let _ = writeln!(out, "{json_key}:");
        for r in refs {
            let _ = writeln!(out, "  - {}", r.iri);
        }
    }
    let _ = writeln!(out, "---");
    let _ = writeln!(out);
    let _ = writeln!(out, "# {}", record.label);
    if !record.definition.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "{}", record.definition);
    }
    out
}

/// Quote a YAML scalar when it could otherwise be misread.
fn yaml_scalar(s: &str) -> String {
    let needs_quotes = s.is_empty()
        || s.starts_with(|c: char| c.is_whitespace())
        || s.ends_with(|c: char| c.is_whitespace())
        || s.contains(": ")
        || s.contains(" #")
        || s.starts_with([
            '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`', '-', '?', ':', '[', ']', '{', '}',
            ',',
        ])
        || matches!(
            s.to_ascii_lowercase().as_str(),
            "true" | "false" | "null" | "yes" | "no" | "on" | "off" | "~"
        );
    if needs_quotes {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EntityType, Ref};
    use indexmap::IndexMap;

    fn vocab() -> Vocabulary {
        Vocabulary::from_yaml_str(
            r#"
version: 3
namespace: "urn:ngm:class:"
relations:
  is-a:     { owl: "rdfs:subClassOf" }
  requires: { owl: "vc:requires", json_key: requires }
scalars:
  domain: { type: text }
"#,
        )
        .unwrap()
    }

    fn record(id: &str) -> ClassRecord {
        ClassRecord {
            page_id: id.to_owned(),
            slug: vault_core::slug::slugify(id),
            title: id.to_owned(),
            public: true,
            page_iri: String::new(),
            iri: format!("urn:ngm:class:{}", vault_core::slug::slugify(id)),
            label: id.to_owned(),
            entity_type: EntityType::Class,
            domain: "blockchain".into(),
            definition: "A definition.".into(),
            maturity: "established".into(),
            quality: 0.5,
            legacy_term_id: String::new(),
            sub_class_of: Vec::new(),
            instance_of: Vec::new(),
            relations: IndexMap::new(),
            links: Vec::new(),
            body: String::new(),
            has_ontology: true,
        }
    }

    fn corpus_of(records: Vec<ClassRecord>) -> Corpus {
        let mut by_class_slug = IndexMap::new();
        for (i, r) in records.iter().enumerate() {
            by_class_slug.entry(r.class_slug()).or_insert(i);
        }
        Corpus {
            records,
            by_class_slug,
        }
    }

    #[test]
    fn the_context_maps_every_declared_relation() {
        let ctx = context(&vocab());
        assert_eq!(
            ctx["@context"]["requires"]["@id"],
            "https://narrativegoldmine.com/ns/v1#requires"
        );
        assert_eq!(ctx["@context"]["requires"]["@type"], "@id");
        assert!(ctx["@context"]["domain"].is_string());
    }

    #[test]
    fn the_bundle_has_an_index_and_one_file_per_class() {
        let files = bundle(
            &corpus_of(vec![record("Zeta"), record("Alpha")]),
            &vocab(),
            "visionGraph@abc",
        );
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].path, "index.md");
        assert_eq!(files[1].path, "concepts/alpha.md");
        assert_eq!(files[2].path, "concepts/zeta.md");
        assert!(files[0].content.contains("concept_count: 2"));
        assert!(files[0].content.contains("visionGraph@abc"));
    }

    #[test]
    fn a_concept_file_carries_the_okf_block_and_relations() {
        let mut r = record("Graph");
        r.sub_class_of = vec![Ref {
            iri: "urn:ngm:class:thing".into(),
            label: "Thing".into(),
        }];
        r.relations.insert(
            "requires",
            vec![Ref {
                iri: "urn:ngm:class:store".into(),
                label: "Store".into(),
            }],
        );
        let files = bundle(&corpus_of(vec![r]), &vocab(), "g");
        let c = &files[1].content;
        assert!(c.contains("okf_version: \"0.2\""));
        assert!(c.contains("resource: urn:ngm:class:graph"));
        assert!(c.contains("is-a:\n  - urn:ngm:class:thing"));
        assert!(c.contains("requires:\n  - urn:ngm:class:store"));
        assert!(c.contains("# Graph"));
        assert!(c.contains("A definition."));
    }

    #[test]
    fn private_classes_never_reach_the_bundle() {
        let mut r = record("Secret");
        r.public = false;
        assert_eq!(bundle(&corpus_of(vec![r]), &vocab(), "g").len(), 1);
    }

    #[test]
    fn ambiguous_scalars_are_quoted() {
        assert_eq!(yaml_scalar("plain"), "plain");
        assert_eq!(yaml_scalar("true"), "\"true\"");
        assert_eq!(yaml_scalar("a: b"), "\"a: b\"");
        assert_eq!(yaml_scalar("- leading"), "\"- leading\"");
        assert_eq!(yaml_scalar(""), "\"\"");
    }
}
