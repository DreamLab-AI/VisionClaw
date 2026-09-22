//! The json-ld fence reader — **one-shot, behind the `migrate` feature.**
//!
//! Before the migration every knowledge page carried two ` ```json-ld ` fences:
//! a `Page` block (identity, slug, public flag, curated outbound wikilinks) and
//! a `Class`/`Individual` block (the ontology). This module is a faithful port
//! of `pipeline/jsonld_parser.py` so that `vault migrate` reads exactly what
//! the Python pipeline read — including both schema spellings (v1's flat
//! `vc:`-prefixed keys and v2's nested `relations` object).
//!
//! It exists to be **deleted**. Once `knowledge/` is frontmatter-only, drop the
//! `migrate` feature, this file, and `vault migrate` with it.

use std::sync::OnceLock;

use indexmap::IndexMap;
use regex::Regex;
use serde_json::Value as Json;

/// The twelve relation attributes, in the order `reason.py::RELATION_TYPES`
/// declares them: `(canonical attr, v1 json-ld key, v2 nested key, camelCase
/// artefact key)`.
pub const RELATION_TYPES: &[(&str, &str, &str, &str)] = &[
    ("has_part", "vc:hasPart", "hasPart", "hasPart"),
    ("requires", "vc:requires", "requires", "requires"),
    ("enables", "vc:enables", "enables", "enables"),
    ("depends_on", "vc:depends-on", "dependsOn", "dependsOn"),
    ("implements", "vc:implements", "implements", "implements"),
    (
        "contrasts_with",
        "vc:contrasts-with",
        "contrastsWith",
        "contrastsWith",
    ),
    ("bridges_to", "vc:bridges-to", "bridgesTo", "bridgesTo"),
    ("uses", "vc:uses", "uses", "uses"),
    ("supports", "vc:supports", "supports", "supports"),
    (
        "standardized_by",
        "vc:standardizedBy",
        "standardizedBy",
        "standardizedBy",
    ),
    ("part_of", "vc:isPartOf", "partOf", "partOf"),
    ("related_to", "vc:relatedTo", "relatedTo", "relatedTo"),
];

/// The block `@type` values that carry an ontology entity.
const ENTITY_TYPES: &[&str] = &["OntologyClass", "Class", "Individual"];

fn fence_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?s)```json-ld\s*\n(.*?)```").expect("static fence regex"))
}

/// A `{ "@id": …, "vc:label": … }` reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    /// The referenced IRI.
    pub iri: String,
    /// Its human label, possibly empty.
    pub label: String,
}

/// The ontology block of a page.
#[derive(Debug, Clone, Default)]
pub struct FenceEntity {
    /// `@id`.
    pub iri: String,
    /// `label`, defaulting to the page title.
    pub label: String,
    /// `Class` or `Individual`.
    pub entity_type: String,
    /// `domain` (v2) or `vc:sourceDomain` (v1).
    pub domain: String,
    /// `definition`.
    pub definition: String,
    /// `subClassOf`.
    pub sub_class_of: Vec<Ref>,
    /// `instanceOf`.
    pub instance_of: Vec<Ref>,
    /// `quality` (v2 float) or `vc:qualityScore` (v1 typed object).
    pub quality_score: f64,
    /// `maturity`, defaulting to `draft`.
    pub maturity: String,
    /// Relations keyed by canonical attribute name.
    pub relations: IndexMap<&'static str, Vec<Ref>>,
    /// `provenance`, normalised across both schema versions.
    pub provenance: IndexMap<String, String>,
    /// The whole block, for the migration's exhaustiveness check.
    pub raw: Json,
}

impl FenceEntity {
    /// The refs under a canonical attribute name.
    #[must_use]
    pub fn relation(&self, attr: &str) -> &[Ref] {
        self.relations.get(attr).map_or(&[], Vec::as_slice)
    }
}

/// A page as the json-ld pipeline saw it.
#[derive(Debug, Clone)]
pub struct FencePage {
    /// `@id` of the Page block.
    pub page_iri: String,
    /// `vc:slug`.
    pub slug: String,
    /// `title`, defaulting to the file stem.
    pub title: String,
    /// `vc:public`.
    pub is_public: bool,
    /// `vc:schemaVersion`.
    pub schema_version: i64,
    /// `vc:outboundWikilinks` — the *curated* link set, not a body scan.
    pub wikilinks: Vec<Ref>,
    /// The ontology block, when the page has one.
    pub entity: Option<FenceEntity>,
    /// Everything after the last fence.
    pub body: String,
    /// The whole Page block, for the migration's exhaustiveness check.
    pub raw_page_block: Json,
    /// Frontmatter that already existed above the fences (`public`, `aliases`).
    pub existing_frontmatter: String,
}

fn parse_refs(v: Option<&Json>) -> Vec<Ref> {
    let Some(v) = v else { return Vec::new() };
    let items: Vec<&Json> = match v {
        Json::Array(a) => a.iter().collect(),
        Json::Object(_) => vec![v],
        _ => return Vec::new(),
    };
    items
        .into_iter()
        .filter_map(|item| {
            let obj = item.as_object()?;
            let iri = obj.get("@id")?.as_str()?;
            if iri.is_empty() {
                return None;
            }
            let label = obj
                .get("label")
                .or_else(|| obj.get("vc:label"))
                .and_then(Json::as_str)
                .unwrap_or_default();
            Some(Ref {
                iri: iri.to_owned(),
                label: label.to_owned(),
            })
        })
        .collect()
}

/// `_extract_float`: a bare number, a numeric string, or `{"@value": …}`.
fn extract_float(v: Option<&Json>) -> f64 {
    let Some(v) = v else { return 0.0 };
    let scalar = match v {
        Json::Object(o) => o.get("@value").unwrap_or(&Json::Null),
        other => other,
    };
    match scalar {
        Json::Number(n) => n.as_f64().unwrap_or(0.0),
        Json::String(s) => s.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn str_field(block: &Json, keys: &[&str]) -> String {
    for k in keys {
        if let Some(s) = block.get(*k).and_then(Json::as_str) {
            return s.to_owned();
        }
    }
    String::new()
}

fn extract_relations(block: &Json) -> IndexMap<&'static str, Vec<Ref>> {
    let mut out: IndexMap<&'static str, Vec<Ref>> = IndexMap::new();
    for (attr, v1_key, _, _) in RELATION_TYPES {
        let refs = parse_refs(block.get(*v1_key));
        if !refs.is_empty() {
            out.insert(attr, refs);
        }
    }
    if let Some(rel) = block.get("relations").filter(|v| v.is_object()) {
        for (attr, _, v2_key, _) in RELATION_TYPES {
            let refs = parse_refs(rel.get(*v2_key));
            if !refs.is_empty() {
                out.entry(attr).or_default().extend(refs);
            }
        }
    }
    out
}

/// Everything after the last fence, trimmed — `_extract_body`.
#[must_use]
pub fn extract_body(text: &str) -> String {
    match fence_re().find_iter(text).last() {
        Some(m) => text[m.end()..].trim().to_owned(),
        None => text.trim().to_owned(),
    }
}

/// The raw fence bodies, in document order.
#[must_use]
pub fn fence_blocks(text: &str) -> Vec<&str> {
    fence_re()
        .captures_iter(text)
        .filter_map(|c| c.get(1).map(|m| m.as_str()))
        .collect()
}

/// Build the [`FenceEntity`] from a decoded ontology block.
fn entity_from_block(block: &Json, title: &str) -> FenceEntity {
    let btype = block.get("@type").and_then(Json::as_str).unwrap_or("Class");
    let entity_type = if btype == "Individual" {
        "Individual"
    } else {
        "Class"
    };

    let label = block
        .get("label")
        .and_then(Json::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(title)
        .to_owned();

    let maturity = {
        let m = str_field(block, &["maturity", "vc:maturity"]);
        if m.is_empty() {
            "draft".to_owned()
        } else {
            m
        }
    };

    FenceEntity {
        iri: str_field(block, &["@id"]),
        label,
        entity_type: entity_type.to_owned(),
        domain: str_field(block, &["domain", "vc:sourceDomain"]),
        definition: str_field(block, &["definition"]),
        sub_class_of: parse_refs(block.get("subClassOf")),
        instance_of: parse_refs(block.get("instanceOf")),
        quality_score: extract_float(
            block
                .get("quality")
                .or_else(|| block.get("vc:qualityScore")),
        ),
        maturity,
        relations: extract_relations(block),
        provenance: extract_provenance(block),
        raw: block.clone(),
    }
}

/// Normalise provenance across both schema versions: v2 nests it under
/// `provenance`, v1 spreads it over `prov:` keys plus `vc:inferenceRule`.
fn extract_provenance(block: &Json) -> IndexMap<String, String> {
    let mut provenance: IndexMap<String, String> = IndexMap::new();
    if let Some(p) = block
        .get("provenance")
        .and_then(Json::as_object)
        .filter(|p| !p.is_empty())
    {
        for (k, v) in p {
            if let Some(s) = v.as_str() {
                provenance.insert(k.clone(), s.to_owned());
            }
        }
        return provenance;
    }
    if let Some(a) = block
        .get("prov:wasAttributedTo")
        .and_then(|v| v.get("@id"))
        .and_then(Json::as_str)
    {
        provenance.insert("attributedTo".into(), a.to_owned());
    }
    if let Some(g) = block
        .get("prov:generatedAtTime")
        .and_then(|v| v.get("@value"))
        .and_then(Json::as_str)
    {
        provenance.insert("generatedAt".into(), g.to_owned());
    }
    let ir = str_field(block, &["vc:inferenceRule"]);
    if !ir.is_empty() {
        provenance.insert("inferenceRule".into(), ir);
    }
    provenance
}

/// Parse a fenced page. Returns `None` when there are no decodable fences or
/// no `Page` block — exactly `parse_page`'s contract in Python.
#[must_use]
pub fn parse_fence_page(stem: &str, text: &str) -> Option<FencePage> {
    let parsed: Vec<Json> = fence_blocks(text)
        .into_iter()
        .filter_map(|raw| serde_json::from_str(raw).ok())
        .collect();
    if parsed.is_empty() {
        return None;
    }

    let mut page_block: Option<&Json> = None;
    let mut ontology_block: Option<&Json> = None;
    for b in &parsed {
        let btype = b.get("@type").and_then(Json::as_str).unwrap_or_default();
        if btype == "Page" && page_block.is_none() {
            page_block = Some(b);
        } else if ENTITY_TYPES.contains(&btype) && ontology_block.is_none() {
            ontology_block = Some(b);
        }
    }
    let page_block = page_block?;

    let title = page_block
        .get("title")
        .and_then(Json::as_str)
        .filter(|t| !t.is_empty())
        .unwrap_or(stem)
        .to_owned();

    Some(FencePage {
        page_iri: str_field(page_block, &["@id"]),
        slug: str_field(page_block, &["vc:slug"]),
        is_public: page_block
            .get("vc:public")
            .and_then(Json::as_bool)
            .unwrap_or(false),
        schema_version: page_block
            .get("vc:schemaVersion")
            .and_then(Json::as_i64)
            .unwrap_or(0),
        wikilinks: parse_refs(page_block.get("vc:outboundWikilinks")),
        entity: ontology_block.map(|b| entity_from_block(b, &title)),
        body: extract_body(text),
        raw_page_block: page_block.clone(),
        existing_frontmatter: crate::frontmatter::split(text)
            .yaml
            .unwrap_or_default()
            .to_owned(),
        title,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"---
public: true
aliases:
  - KG
---

# Knowledge Graph
```json-ld
{
  "@id": "urn:visionflow:page:abc",
  "@type": "Page",
  "vc:slug": "knowledge-graph",
  "title": "Knowledge Graph",
  "vc:public": true,
  "vc:schemaVersion": 2,
  "vc:outboundWikilinks": [
    { "@id": "urn:visionflow:linked:ontology", "vc:label": "Ontology" }
  ]
}
```
```json-ld
{
  "@id": "urn:ngm:class:knowledge-graph",
  "@type": "Class",
  "label": "Knowledge Graph",
  "domain": "spatial-computing",
  "definition": "A graph of entities.",
  "maturity": "established",
  "quality": 0.35,
  "subClassOf": [ { "@id": "urn:ngm:class:content-and-assets", "label": "Content and Assets" } ],
  "relations": { "requires": [ { "@id": "urn:ngm:class:ontology", "label": "Ontology" } ] },
  "provenance": { "attributedTo": "agent:enricher/1" }
}
```
- ### Current Landscape (2026)
  - Widely adopted.
"#;

    #[test]
    fn parses_both_blocks_and_the_trailing_body() {
        let p = parse_fence_page("Knowledge Graph", PAGE).unwrap();
        assert_eq!(p.slug, "knowledge-graph");
        assert!(p.is_public);
        assert_eq!(p.schema_version, 2);
        assert_eq!(p.wikilinks[0].label, "Ontology");
        assert!(p.body.starts_with("- ### Current Landscape"));
        assert!(p.existing_frontmatter.contains("aliases"));
        let e = p.entity.unwrap();
        assert_eq!(e.entity_type, "Class");
        assert_eq!(e.domain, "spatial-computing");
        assert!((e.quality_score - 0.35).abs() < f64::EPSILON);
        assert_eq!(e.sub_class_of[0].label, "Content and Assets");
        assert_eq!(e.relation("requires")[0].iri, "urn:ngm:class:ontology");
        assert_eq!(e.provenance["attributedTo"], "agent:enricher/1");
    }

    #[test]
    fn reads_the_v1_flat_spelling_too() {
        let text = r#"```json-ld
{"@id":"p","@type":"Page","vc:slug":"s","title":"T","vc:public":false}
```
```json-ld
{"@id":"c","@type":"OntologyClass","vc:sourceDomain":"ai",
 "vc:qualityScore":{"@value":"0.8"},"vc:maturity":"emerging",
 "vc:requires":[{"@id":"x","vc:label":"X"}],
 "prov:wasAttributedTo":{"@id":"agent:a/1"},
 "vc:inferenceRule":"r1"}
```
tail"#;
        let e = parse_fence_page("T", text).unwrap().entity.unwrap();
        assert_eq!(e.entity_type, "Class");
        assert_eq!(e.domain, "ai");
        assert!((e.quality_score - 0.8).abs() < f64::EPSILON);
        assert_eq!(e.maturity, "emerging");
        assert_eq!(e.relation("requires")[0].iri, "x");
        assert_eq!(e.provenance["attributedTo"], "agent:a/1");
        assert_eq!(e.provenance["inferenceRule"], "r1");
        assert_eq!(e.label, "T", "label falls back to the page title");
    }

    #[test]
    fn v1_and_v2_relations_concatenate() {
        let text = r#"```json-ld
{"@id":"p","@type":"Page","title":"T"}
```
```json-ld
{"@id":"c","@type":"Class","vc:uses":[{"@id":"a"}],"relations":{"uses":[{"@id":"b"}]}}
```"#;
        let e = parse_fence_page("T", text).unwrap().entity.unwrap();
        let uses: Vec<&str> = e.relation("uses").iter().map(|r| r.iri.as_str()).collect();
        assert_eq!(uses, vec!["a", "b"]);
    }

    #[test]
    fn a_page_without_a_page_block_is_skipped() {
        let text = "```json-ld\n{\"@id\":\"c\",\"@type\":\"Class\"}\n```";
        assert!(parse_fence_page("T", text).is_none());
    }

    #[test]
    fn undecodable_fences_are_ignored_not_fatal() {
        let text = "```json-ld\n{not json}\n```\n```json-ld\n{\"@id\":\"p\",\"@type\":\"Page\",\"title\":\"T\"}\n```";
        assert_eq!(parse_fence_page("T", text).unwrap().title, "T");
    }

    #[test]
    fn maturity_defaults_to_draft() {
        let text = "```json-ld\n{\"@type\":\"Page\",\"title\":\"T\"}\n```\n```json-ld\n{\"@id\":\"c\",\"@type\":\"Class\"}\n```";
        assert_eq!(
            parse_fence_page("T", text)
                .unwrap()
                .entity
                .unwrap()
                .maturity,
            "draft"
        );
    }

    #[test]
    fn body_is_everything_after_the_last_fence() {
        assert_eq!(extract_body("a\n```json-ld\n{}\n```\n  tail  "), "tail");
        assert_eq!(extract_body("no fences here"), "no fences here");
    }
}
