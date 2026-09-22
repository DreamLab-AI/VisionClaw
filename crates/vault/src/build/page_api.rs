//! `api/pages/<slug>.json` and `api/pages/_domain-index.json` — a port of
//! `pipeline/jsonld_to_page_api.py`.
//!
//! Each public page gets one closure-enriched JSON document. The twelve
//! `relationships` keys are **always present**, empty lists included: this is
//! the opposite of `scaffold-index.json`'s omit-empty rule, and both are v1
//! contracts that consumers already depend on.

use std::collections::HashMap;

use serde_json::{json, Map, Value};

use crate::build::indexes::{inherited_to_json, refs_to_json};
use crate::closure::Closure;
use crate::model::{ClassRecord, Corpus, RELATION_KEYS};

/// The frozen `relationships` key order of the page API.
pub const PAGE_API_REL_ORDER: &[&str] = &[
    "hasPart",
    "requires",
    "enables",
    "dependsOn",
    "implements",
    "contrastsWith",
    "bridgesTo",
    "uses",
    "supports",
    "standardizedBy",
    "partOf",
    "relatedTo",
];

/// One page's API document, plus the slug it is written under.
#[derive(Debug, Clone)]
pub struct PageDocument {
    /// The file stem: `<slug>.json`.
    pub slug: String,
    /// The document.
    pub value: Value,
}

/// Build every public page document and the domain index.
///
/// Returns `(documents, domain_index)`.
///
/// # Panics
/// Never: the domain index is only ever populated with JSON arrays, and the
/// `expect` asserts that invariant rather than hiding it.
#[must_use]
#[allow(clippy::implicit_hasher, clippy::too_many_lines)]
pub fn build(
    corpus: &Corpus,
    closure: &Closure,
    backlinks: &HashMap<String, Vec<String>>,
) -> (Vec<PageDocument>, Value) {
    let public: Vec<&ClassRecord> = corpus.public().collect();
    let slug_to_title: HashMap<&str, &str> = public
        .iter()
        .map(|r| (r.slug.as_str(), r.title.as_str()))
        .collect();

    let mut domain_index: Map<String, Value> = Map::new();
    let mut documents = Vec::with_capacity(public.len());

    for record in &public {
        let mut entry = Map::new();
        entry.insert("id".into(), json!(record.page_iri));
        entry.insert("title".into(), json!(record.title));
        entry.insert("slug".into(), json!(record.slug));
        entry.insert("public".into(), json!(true));

        if record.has_ontology {
            entry.insert("classIri".into(), json!(record.iri));
            entry.insert("domain".into(), json!(record.domain));
            entry.insert("definition".into(), json!(record.definition));
            entry.insert("subClassOf".into(), refs_to_json(&record.sub_class_of));
            entry.insert("entityType".into(), json!(record.entity_type.as_str()));
            entry.insert("qualityScore".into(), json!(record.quality));
            entry.insert("maturity".into(), json!(record.maturity));

            let mut relationships = Map::new();
            for key in PAGE_API_REL_ORDER {
                let canonical = RELATION_KEYS
                    .iter()
                    .find(|(_, j)| j == key)
                    .map_or(*key, |(_, j)| *j);
                relationships.insert((*key).to_owned(), refs_to_json(record.relation(canonical)));
            }
            entry.insert("relationships".into(), Value::Object(relationships));

            let class_slug = record.class_slug();
            entry.insert(
                "inferredSuperClasses".into(),
                Value::Array(
                    closure
                        .inferred_of(&class_slug)
                        .iter()
                        .map(|a| {
                            json!({
                                "id": closure.iri_of(a),
                                "label": closure.label_of(a),
                                "slug": a,
                            })
                        })
                        .collect(),
                ),
            );
            entry.insert(
                "inheritedRelations".into(),
                closure
                    .inherited_relations
                    .get(&class_slug)
                    .map_or_else(|| Value::Object(Map::new()), inherited_to_json),
            );

            let domain = if record.domain.is_empty() {
                "unclassified"
            } else {
                record.domain.as_str()
            };
            domain_index
                .entry(domain.to_owned())
                .or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut()
                .expect("domain index entries are arrays")
                .push(json!({
                    "slug": record.slug,
                    "title": record.label,
                    "qualityScore": record.quality,
                }));
        }

        entry.insert(
            "wikilinks".into(),
            Value::Array(
                record
                    .links
                    .iter()
                    .map(|r| json!({ "slug": r.slug(), "label": r.label }))
                    .collect(),
            ),
        );
        entry.insert(
            "backlinks".into(),
            Value::Array(
                backlinks
                    .get(&record.slug)
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                    .iter()
                    .map(|s| {
                        json!({
                            "slug": s,
                            "label": slug_to_title.get(s.as_str()).copied().unwrap_or(s.as_str()),
                        })
                    })
                    .collect(),
            ),
        );

        documents.push(PageDocument {
            slug: record.slug.clone(),
            value: Value::Object(entry),
        });
    }

    (documents, Value::Object(domain_index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EntityType, Ref};
    use indexmap::IndexMap;

    fn record(id: &str) -> ClassRecord {
        ClassRecord {
            page_id: id.to_owned(),
            slug: vault_core::slug::slugify(id),
            title: id.to_owned(),
            public: true,
            page_iri: format!("urn:visionflow:page:{}", vault_core::slug::slugify(id)),
            iri: format!("urn:ngm:class:{}", vault_core::slug::slugify(id)),
            label: id.to_owned(),
            entity_type: EntityType::Class,
            domain: "blockchain".into(),
            definition: "d".into(),
            maturity: "draft".into(),
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
    fn every_relationship_key_is_present_even_when_empty() {
        let (docs, _) = build(
            &corpus_of(vec![record("A")]),
            &Closure::default(),
            &HashMap::new(),
        );
        let rel = docs[0].value["relationships"].as_object().unwrap();
        assert_eq!(rel.len(), PAGE_API_REL_ORDER.len());
        let keys: Vec<&String> = rel.keys().collect();
        assert_eq!(keys, PAGE_API_REL_ORDER);
        assert!(rel["hasPart"].as_array().unwrap().is_empty());
    }

    #[test]
    fn private_pages_get_no_document() {
        let mut r = record("Secret");
        r.public = false;
        let (docs, _) = build(&corpus_of(vec![r]), &Closure::default(), &HashMap::new());
        assert!(docs.is_empty());
    }

    #[test]
    fn the_domain_index_groups_by_domain() {
        let mut other = record("B");
        other.domain = String::new();
        let (_, index) = build(
            &corpus_of(vec![record("A"), other]),
            &Closure::default(),
            &HashMap::new(),
        );
        assert_eq!(index["blockchain"].as_array().unwrap().len(), 1);
        assert_eq!(index["unclassified"][0]["slug"], "b");
    }

    #[test]
    fn closure_enrichment_lands_on_the_document() {
        let mut closure = Closure::default();
        closure
            .inferred_superclasses
            .insert("a".into(), vec!["ancestor".into()]);
        closure.labels.insert("ancestor".into(), "Ancestor".into());
        let mut inherited = IndexMap::new();
        inherited.insert(
            "requires".to_owned(),
            vec![Ref {
                iri: "urn:ngm:class:x".into(),
                label: "X".into(),
            }],
        );
        closure.inherited_relations.insert("a".into(), inherited);

        let (docs, _) = build(&corpus_of(vec![record("A")]), &closure, &HashMap::new());
        let v = &docs[0].value;
        assert_eq!(v["inferredSuperClasses"][0]["slug"], "ancestor");
        assert_eq!(v["inferredSuperClasses"][0]["label"], "Ancestor");
        assert_eq!(v["inheritedRelations"]["requires"][0]["label"], "X");
    }

    #[test]
    fn backlinks_resolve_titles_and_fall_back_to_the_slug() {
        let mut backlinks = HashMap::new();
        backlinks.insert("a".to_owned(), vec!["b".to_owned(), "ghost".to_owned()]);
        let (docs, _) = build(
            &corpus_of(vec![record("A"), record("B")]),
            &Closure::default(),
            &backlinks,
        );
        let bl = docs[0].value["backlinks"].as_array().unwrap();
        assert_eq!(bl[0]["label"], "B");
        assert_eq!(bl[1]["label"], "ghost");
    }

    #[test]
    fn a_plain_note_gets_identity_and_links_only() {
        let mut r = record("Note");
        r.has_ontology = false;
        let (docs, index) = build(&corpus_of(vec![r]), &Closure::default(), &HashMap::new());
        let obj = docs[0].value.as_object().unwrap();
        assert!(!obj.contains_key("relationships"));
        assert!(obj.contains_key("wikilinks"));
        assert!(index.as_object().unwrap().is_empty());
    }
}
