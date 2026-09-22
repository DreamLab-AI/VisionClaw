//! `data/ontology.json` — the `WebVOWL` graph, ported from
//! `pipeline/jsonld_to_webvowl.py`.
//!
//! This is the explorer's build input. Two details are quirks of the format
//! rather than of the ontology, and both are preserved deliberately:
//!
//! * **`related-to` is emitted as `skos:related`**, not `vc:relatedTo`. The
//!   Turtle emitter uses `vc:relatedTo` for the same edge; `WebVOWL` has always
//!   used the SKOS spelling and consumers key on it.
//! * **Only edges whose target is itself a declared public node ship.** A
//!   dangling reference is dropped rather than drawn to a node that does not
//!   exist, which is what keeps the rendered graph well-founded.
//!
//! Property ids are `prop-{sub,type,rel}-<n>` where `n` is a single counter
//! shared across all three kinds, incremented in page order. That ordering is
//! observable in the output, so it is reproduced exactly.

use std::collections::BTreeSet;

use serde_json::{json, Map, Value};

use crate::model::{Corpus, EntityType};

/// Domain fill colours.
const DOMAIN_COLOURS: &[(&str, &str)] = &[
    ("artificial-intelligence", "#4CAF50"),
    ("spatial-computing", "#2196F3"),
    ("blockchain", "#FF9800"),
    ("infrastructure", "#9C27B0"),
    ("distributed-collaboration", "#00BCD4"),
    ("robotics", "#F44336"),
];
const DEFAULT_COLOUR: &str = "#607D8B";
const INDIVIDUAL_COLOUR: &str = "#FF5722";

/// The HTTP class namespace.
pub const BASE_CLASS_IRI: &str = "https://narrativegoldmine.com/class/";
/// The HTTP individual namespace.
pub const BASE_INDIVIDUAL_IRI: &str = "https://narrativegoldmine.com/individual/";

/// The twelve relation keys `WebVOWL` draws, with the property IRI it labels each
/// with. Note `related-to` to `skos:related`.
const REL_MAP: &[(&str, &str)] = &[
    ("has-part", "vc:hasPart"),
    ("requires", "vc:requires"),
    ("enables", "vc:enables"),
    ("depends-on", "vc:dependsOn"),
    ("implements", "vc:implements"),
    ("contrasts-with", "vc:contrastsWith"),
    ("bridges-to", "vc:bridgesTo"),
    ("uses", "vc:uses"),
    ("related-to", "skos:related"),
    ("supports", "vc:supports"),
    ("standardized-by", "vc:standardizedBy"),
    ("part-of", "vc:isPartOf"),
];

/// Map a corpus IRI onto its HTTP projection (`_remap_iri`).
///
/// Deliberately **not** [`crate::build::turtle::iri_to_uri`]: that one also
/// handles `owl:Thing` and falls back to the class namespace for a bare slug.
/// This one passes anything unrecognised through untouched, which is what keeps
/// a foreign IRI out of the rendered graph's namespace.
#[must_use]
pub fn remap_iri(iri: &str) -> String {
    const MAPPINGS: &[(&str, &str)] = &[
        ("urn:ngm:class:", BASE_CLASS_IRI),
        ("urn:ngm:individual:", BASE_INDIVIDUAL_IRI),
        ("urn:visionflow:owl:class:", BASE_CLASS_IRI),
        (
            "urn:visionflow:linked:",
            "https://narrativegoldmine.com/linked/",
        ),
        (
            "urn:visionflow:page:",
            "https://narrativegoldmine.com/page/",
        ),
    ];
    for (prefix, base) in MAPPINGS {
        if let Some(tail) = iri.strip_prefix(prefix) {
            return format!("{base}{tail}");
        }
    }
    iri.to_owned()
}

/// Build the `WebVOWL` document.
#[must_use]
#[allow(clippy::too_many_lines)] // One faithful port of one Python function.
pub fn build(corpus: &Corpus) -> Value {
    let public: Vec<_> = corpus
        .records
        .iter()
        .filter(|r| r.public && r.has_ontology && !r.iri.is_empty())
        .collect();

    let declared: BTreeSet<String> = public.iter().map(|r| remap_iri(&r.iri)).collect();

    let mut classes = Vec::new();
    let mut class_attrs = Vec::new();
    let mut properties = Vec::new();
    let mut prop_attrs = Vec::new();
    let mut counter = 0usize;
    let mut domains = BTreeSet::new();

    for record in &public {
        let node_id = remap_iri(&record.iri);
        let is_individual = record.entity_type == EntityType::Individual;

        classes.push(json!({
            "id": node_id,
            "type": if is_individual { "owl:NamedIndividual" } else { "owl:Class" },
        }));

        let bg = if is_individual {
            INDIVIDUAL_COLOUR
        } else {
            DOMAIN_COLOURS
                .iter()
                .find(|(d, _)| *d == record.domain)
                .map_or(DEFAULT_COLOUR, |(_, c)| *c)
        };
        domains.insert(record.domain.clone());

        let comment: String = record.definition.chars().take(200).collect();
        class_attrs.push(json!({
            "id": node_id,
            "iri": node_id,
            "baseIri": if is_individual { BASE_INDIVIDUAL_IRI } else { BASE_CLASS_IRI },
            "attributes": ["colored"],
            "backgroundColor": bg,
            "domain": record.domain,
            "entityType": record.entity_type.as_str(),
            "term_id": record.legacy_term_id,
            "label": { "en": record.label },
            "comment": { "en": comment },
        }));

        if is_individual {
            let refs = if record.instance_of.is_empty() {
                &record.sub_class_of
            } else {
                &record.instance_of
            };
            for r in refs {
                let target = remap_iri(&r.iri);
                if !declared.contains(&target) {
                    continue;
                }
                let prop_id = format!("prop-type-{counter}");
                counter += 1;
                properties.push(json!({ "id": prop_id, "type": "rdf:type" }));
                prop_attrs.push(json!({
                    "id": prop_id,
                    "attributes": ["object"],
                    "domain": node_id,
                    "range": target,
                    "label": { "en": "type" },
                }));
            }
        } else {
            for parent in &record.sub_class_of {
                let target = remap_iri(&parent.iri);
                if !declared.contains(&target) {
                    continue;
                }
                let prop_id = format!("prop-sub-{counter}");
                counter += 1;
                properties.push(json!({ "id": prop_id, "type": "rdfs:subClassOf" }));
                prop_attrs.push(json!({
                    "id": prop_id,
                    "attributes": ["subclass"],
                    "domain": node_id,
                    "range": target,
                }));
            }
        }

        for (fm_key, prop_iri) in REL_MAP {
            let json_key = crate::model::RELATION_KEYS
                .iter()
                .find(|(f, _)| f == fm_key)
                .map_or(*fm_key, |(_, j)| *j);
            for r in record.relation(json_key) {
                let target = remap_iri(&r.iri);
                if !declared.contains(&target) {
                    continue;
                }
                let prop_id = format!("prop-rel-{counter}");
                counter += 1;
                properties.push(json!({ "id": prop_id, "type": "owl:objectProperty" }));
                prop_attrs.push(json!({
                    "id": prop_id,
                    "iri": prop_iri,
                    "baseIri": "https://narrativegoldmine.com/ns/v1#",
                    "attributes": ["object"],
                    "domain": node_id,
                    "range": target,
                    "label": { "en": prop_iri.rsplit(':').next().unwrap_or(prop_iri) },
                }));
            }
        }
    }

    let mut header = Map::new();
    header.insert("languages".into(), json!(["en"]));
    header.insert(
        "title".into(),
        json!({ "en": "NarrativeGoldmine Ontology" }),
    );
    header.insert(
        "iri".into(),
        json!("https://narrativegoldmine.com/ontology"),
    );
    header.insert("version".into(), json!("3.1.0"));
    header.insert("author".into(), json!(["Dr John O'Hare", "LCR Swarm"]));
    header.insert(
        "description".into(),
        json!({
            "en": format!(
                "Knowledge graph ontology with {} nodes across {} domains",
                classes.len(),
                domains.len()
            )
        }),
    );

    json!({
        "header": Value::Object(header),
        "class": classes,
        "classAttribute": class_attrs,
        "property": properties,
        "propertyAttribute": prop_attrs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Ref;
    use indexmap::IndexMap;

    fn record(id: &str) -> crate::model::ClassRecord {
        crate::model::ClassRecord {
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

    fn corpus_of(records: Vec<crate::model::ClassRecord>) -> Corpus {
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
    fn remaps_every_urn_namespace_and_passes_others_through() {
        assert_eq!(remap_iri("urn:ngm:class:x"), format!("{BASE_CLASS_IRI}x"));
        assert_eq!(
            remap_iri("urn:ngm:individual:y"),
            format!("{BASE_INDIVIDUAL_IRI}y")
        );
        assert_eq!(
            remap_iri("urn:visionflow:linked:z"),
            "https://narrativegoldmine.com/linked/z"
        );
        // Unlike the Turtle mapper, a bare slug is NOT given a namespace.
        assert_eq!(remap_iri("bare"), "bare");
        assert_eq!(remap_iri("owl:Thing"), "owl:Thing");
    }

    #[test]
    fn emits_a_class_node_with_its_colour_and_comment() {
        let mut r = record("Bitcoin");
        r.definition = "x".repeat(300);
        let doc = build(&corpus_of(vec![r]));
        assert_eq!(doc["class"][0]["type"], "owl:Class");
        assert_eq!(doc["classAttribute"][0]["backgroundColor"], "#FF9800");
        assert_eq!(doc["classAttribute"][0]["baseIri"], BASE_CLASS_IRI);
        assert_eq!(
            doc["classAttribute"][0]["comment"]["en"]
                .as_str()
                .unwrap()
                .len(),
            200,
            "the comment is capped at 200 characters"
        );
    }

    #[test]
    fn an_unknown_domain_takes_the_default_colour() {
        let mut r = record("X");
        r.domain = "not-a-domain".into();
        let doc = build(&corpus_of(vec![r]));
        assert_eq!(doc["classAttribute"][0]["backgroundColor"], DEFAULT_COLOUR);
    }

    #[test]
    fn an_individual_is_coloured_and_typed_differently() {
        let mut r = record("Instance");
        r.entity_type = EntityType::Individual;
        let doc = build(&corpus_of(vec![r]));
        assert_eq!(doc["class"][0]["type"], "owl:NamedIndividual");
        assert_eq!(
            doc["classAttribute"][0]["backgroundColor"],
            INDIVIDUAL_COLOUR
        );
        assert_eq!(doc["classAttribute"][0]["baseIri"], BASE_INDIVIDUAL_IRI);
    }

    #[test]
    fn a_dangling_edge_is_not_drawn() {
        let mut a = record("A");
        a.sub_class_of = vec![Ref {
            iri: "urn:ngm:class:nowhere".into(),
            label: "Nowhere".into(),
        }];
        let doc = build(&corpus_of(vec![a]));
        assert!(doc["property"].as_array().unwrap().is_empty());
    }

    #[test]
    fn a_declared_parent_becomes_a_subclass_property() {
        let mut a = record("A");
        a.sub_class_of = vec![Ref {
            iri: "urn:ngm:class:b".into(),
            label: "B".into(),
        }];
        let doc = build(&corpus_of(vec![a, record("B")]));
        assert_eq!(doc["property"][0]["type"], "rdfs:subClassOf");
        assert_eq!(doc["property"][0]["id"], "prop-sub-0");
        assert_eq!(doc["propertyAttribute"][0]["attributes"][0], "subclass");
    }

    #[test]
    fn related_to_is_labelled_skos_related_not_vc_relatedto() {
        let mut a = record("A");
        a.relations.insert(
            "relatedTo",
            vec![Ref {
                iri: "urn:ngm:class:b".into(),
                label: "B".into(),
            }],
        );
        let doc = build(&corpus_of(vec![a, record("B")]));
        assert_eq!(doc["propertyAttribute"][0]["iri"], "skos:related");
        assert_eq!(doc["propertyAttribute"][0]["label"]["en"], "related");
    }

    #[test]
    fn the_property_counter_is_shared_across_kinds_and_page_ordered() {
        let mut a = record("A");
        a.sub_class_of = vec![Ref {
            iri: "urn:ngm:class:b".into(),
            label: "B".into(),
        }];
        a.relations.insert(
            "requires",
            vec![Ref {
                iri: "urn:ngm:class:b".into(),
                label: "B".into(),
            }],
        );
        let doc = build(&corpus_of(vec![a, record("B")]));
        let ids: Vec<&str> = doc["property"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["prop-sub-0", "prop-rel-1"]);
    }

    #[test]
    fn the_header_counts_nodes_and_domains() {
        let mut b = record("B");
        b.domain = "robotics".into();
        let doc = build(&corpus_of(vec![record("A"), b]));
        assert_eq!(
            doc["header"]["description"]["en"],
            "Knowledge graph ontology with 2 nodes across 2 domains"
        );
        assert_eq!(doc["header"]["version"], "3.1.0");
    }

    #[test]
    fn private_pages_contribute_nothing() {
        let mut r = record("Secret");
        r.public = false;
        let doc = build(&corpus_of(vec![r]));
        assert!(doc["class"].as_array().unwrap().is_empty());
    }

    #[test]
    fn the_legacy_term_id_is_carried_into_the_node_attributes() {
        let mut r = record("A");
        r.legacy_term_id = "BC-0042".into();
        let doc = build(&corpus_of(vec![r]));
        assert_eq!(doc["classAttribute"][0]["term_id"], "BC-0042");
    }
}
