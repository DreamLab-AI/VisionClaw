// src/services/page_parser.rs
//! The single page-parsing seam for corpus ingest (ADR-2114).
//!
//! Ingest turns a page's markdown into structured ontology content through
//! exactly one function, [`parse_page`], which delegates to
//! [`vault_core::page::parse_page`] — the parser `vault build` uses, so the
//! graph and the published ontology read the corpus through one
//! implementation (ADR-2113). The corpus is frontmatter-only (ADR-2112): a
//! page's ontology identity is its `type`, `resource`, `title` and `slug`
//! properties, its links are the curated `links` list (else the body's
//! wikilinks), and its relations are the list-of-wikilink keys that
//! `ontology/vocabulary.yaml` declares. Neither `key:: value` lines nor
//! `json-ld` fences carry meaning.

use std::collections::HashMap;

use vault_core::vocabulary::Vocabulary;
use visionclaw_domain::models::canonical_entity::{CanonicalEntity, EntityKind, OutboundLink};
use visionclaw_domain::ports::ontology_repository::{AxiomType, OwlAxiom, OwlClass};

pub use vault_core::page::Page;

/// The class namespace resources are minted in (vocabulary.yaml `namespace`).
/// A migrated page always declares its `resource`; this is only the fallback
/// `namespace + slug` for a page that does not.
pub const CLASS_NAMESPACE: &str = "urn:ngm:class:";

/// The individual namespace (vocabulary.yaml `individual_namespace`), the
/// fallback for an `Individual` page with no declared `resource`.
pub const INDIVIDUAL_NAMESPACE: &str = "urn:ngm:individual:";

/// The vault root whose pages may declare ontology (PRD-sovereign-corpus Q16:
/// "Governed ontology bundle = `type: Class|Property|Individual` in
/// `knowledge/` only").
pub const ONTOLOGY_VAULT_PREFIX: &str = "knowledge/";

/// The OKF `type` values that make a page part of the ontology bundle.
pub const ONTOLOGY_PAGE_TYPES: [&str; 3] = ["Class", "Property", "Individual"];

/// A page that declares ontology content: the canonical entity the graph node
/// is built from, the parsed page its relations are read from, and the
/// declared OKF type.
#[derive(Debug, Clone)]
pub struct ParsedPage {
    /// The page's graph identity, IRIs, publication flag and outbound links.
    pub entity: CanonicalEntity,
    /// The page as `vault_core` parsed it — frontmatter plus body.
    pub page: Page,
    /// The declared ontology type: one of [`ONTOLOGY_PAGE_TYPES`].
    pub ontology_type: String,
}

/// Parse one corpus page.
///
/// * `Ok(Some(parsed))` — the page declares ontology content: it lives under
///   `knowledge/` and its frontmatter `type` is one of
///   [`ONTOLOGY_PAGE_TYPES`].
/// * `Ok(None)` — any other page. The caller ingests it on the plain
///   markdown/wikilink path that populates the working graph.
/// * `Err(_)` — the page opens a frontmatter block that is not valid YAML.
///   The caller skips it and logs: a page whose metadata cannot be read has no
///   readable publication flag either, so skipping is the fail-closed answer.
///
/// `source_path` is the source-relative page path (`knowledge/pages/Ns/Foo.md`);
/// the page id is its path under the `pages/` directory, without `.md`.
pub fn parse_page(content: &str, source_path: &str) -> Result<Option<ParsedPage>, String> {
    let path = source_path.trim_start_matches("./").trim_start_matches('/');
    let page =
        vault_core::page::parse_page(pages_dir(path), path, content).map_err(|e| e.to_string())?;
    let Some(ontology_type) = declared_ontology_type(path, &page) else {
        return Ok(None);
    };
    let entity = canonical_entity(&page, &ontology_type, path);
    Ok(Some(ParsedPage {
        entity,
        page,
        ontology_type,
    }))
}

/// The ontology type a page DECLARES, or `None` if it declares none.
///
/// Two conditions, both necessary (PRD-sovereign-corpus Q16):
///   1. the page lives under `knowledge/` — `working/` is OKF-conformant with
///      its own types (Q9) and never contributes classes or axioms;
///   2. its frontmatter `type` is one of [`ONTOLOGY_PAGE_TYPES`].
///
/// Deriving this from the path ALONE would re-admit the non-class files Q16
/// moves out of `knowledge/`; deriving it from the `type` alone would admit a
/// `working/` page that happened to say `type: Class`. Both halves, or the
/// page is graph-only.
pub fn declared_ontology_type(source_path: &str, page: &Page) -> Option<String> {
    let path = source_path.trim_start_matches("./").trim_start_matches('/');
    if !path.starts_with(ONTOLOGY_VAULT_PREFIX) {
        return None;
    }
    let page_type = page.frontmatter.text("type")?;
    let page_type = page_type.trim();
    ONTOLOGY_PAGE_TYPES
        .contains(&page_type)
        .then(|| page_type.to_string())
}

/// The `pages/` directory a source path sits in (`knowledge/pages`), so the
/// page id is the path beneath it. A path with no `pages/` segment is its own
/// id.
fn pages_dir(path: &str) -> &str {
    if path.starts_with("pages/") {
        return "pages";
    }
    path.find("/pages/")
        .map_or("", |i| &path[..i + "/pages".len()])
}

fn canonical_entity(page: &Page, ontology_type: &str, source_path: &str) -> CanonicalEntity {
    let (kind, namespace) = if ontology_type == "Individual" {
        (EntityKind::OntologyIndividual, INDIVIDUAL_NAMESPACE)
    } else {
        (EntityKind::OntologyClass, CLASS_NAMESPACE)
    };
    let outbound_links = page
        .outbound_links()
        .into_iter()
        .map(|link| OutboundLink {
            target_iri: format!(
                "{CLASS_NAMESPACE}{}",
                vault_core::slug::slugify(&link.target)
            ),
            target_label: link.alias.clone().unwrap_or_else(|| link.target.clone()),
            target_slug: link.target,
        })
        .collect();
    CanonicalEntity {
        slug: page.slug(),
        page_iri: page.frontmatter.text("page_resource").unwrap_or_default(),
        class_iri: Some(page.resource(namespace)),
        title: page.title(),
        public: page.is_public(),
        kind,
        outbound_links,
        source_path: source_path.to_string(),
    }
}

/// The OWL classes and asserted `SubClassOf` axioms a set of ontology pages
/// declares, for the Oxigraph quad-store (`load_ontology`, ADR-2064).
#[derive(Debug, Clone, Default)]
pub struct OntologyProjection {
    /// One class per page, keyed by the page's `resource`.
    pub classes: Vec<OwlClass>,
    /// `class SubClassOf parent` for every target of the taxonomy relation.
    pub axioms: Vec<OwlAxiom>,
}

/// Project ontology pages to OWL classes and axioms through the vocabulary.
///
/// A relation target resolves to the `resource` of the page it names — by
/// page id, title or slug, case-insensitively — and otherwise to
/// `namespace + slug(target)`, the long-tail concept IRI `vault build` mints
/// too. The taxonomy relation (the key whose property is `rdfs:subClassOf`)
/// fills `parent_classes` and yields the `SubClassOf` axioms; the others fill
/// the class's relationship lists.
pub fn project_ontology(pages: &[ParsedPage], vocab: &Vocabulary) -> OntologyProjection {
    let mut by_name: HashMap<String, String> = HashMap::new();
    for parsed in pages {
        let Some(iri) = parsed.entity.class_iri.clone() else {
            continue;
        };
        for name in [
            parsed.page.id.clone(),
            parsed.entity.title.clone(),
            parsed.entity.slug.clone(),
        ] {
            by_name
                .entry(name.to_lowercase())
                .or_insert_with(|| iri.clone());
        }
    }
    let resolve = |target: &str| {
        by_name
            .get(&target.to_lowercase())
            .cloned()
            .unwrap_or_else(|| format!("{CLASS_NAMESPACE}{}", vault_core::slug::slugify(target)))
    };
    let taxonomy = vocab.taxonomy_key();

    let mut out = OntologyProjection::default();
    for parsed in pages {
        let Some(iri) = parsed.entity.class_iri.clone() else {
            continue;
        };
        let fm = &parsed.page.frontmatter;
        let mut class = OwlClass {
            iri: iri.clone(),
            term_id: fm.text("legacy-term-id"),
            preferred_term: Some(parsed.entity.title.clone()),
            label: Some(parsed.entity.title.clone()),
            source_domain: fm.text("domain"),
            class_type: Some(parsed.ontology_type.clone()),
            status: fm.text("status"),
            maturity: fm.text("maturity"),
            quality_score: fm.number("quality").map(|q| q as f32),
            public_access: Some(parsed.entity.public),
            source_file: Some(parsed.entity.source_path.clone()),
            markdown_content: Some(parsed.page.body.clone()),
            ..OwlClass::default()
        };
        let definition = parsed.page.leading_paragraph();
        if !definition.is_empty() {
            class.description = Some(definition);
        }
        for (key, targets) in parsed
            .page
            .relations(vocab.relations.keys().map(String::as_str))
        {
            let iris: Vec<String> = targets.iter().map(|t| resolve(&t.target)).collect();
            if Some(key.as_str()) == taxonomy {
                for parent in &iris {
                    out.axioms.push(OwlAxiom {
                        id: None,
                        axiom_type: AxiomType::SubClassOf,
                        subject: iri.clone(),
                        object: parent.clone(),
                        annotations: HashMap::new(),
                    });
                }
                class.parent_classes = iris;
                continue;
            }
            match key.as_str() {
                "has-part" => class.has_part = iris,
                "part-of" => class.is_part_of = iris,
                "requires" => class.requires = iris,
                "depends-on" => class.depends_on = iris,
                "enables" => class.enables = iris,
                "related-to" => class.relates_to = iris,
                "bridges-to" => class.bridges_to = iris,
                _ => {
                    class.other_relationships.insert(key, iris);
                }
            }
        }
        out.classes.push(class);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLASS_PAGE: &str = "---
type: Class
title: Alpha Beta
resource: urn:ngm:class:alpha-beta
page_resource: urn:visionflow:page:abc
slug: alpha-beta
public: true
status: stable
links:
- '[[gamma|Gamma]]'
is-a:
- '[[Delta]]'
---

A fixture class. See [[Body Only]].
";

    #[test]
    fn a_knowledge_class_page_yields_an_entity() {
        let parsed = parse_page(CLASS_PAGE, "knowledge/pages/Ns/Alpha Beta.md")
            .expect("valid frontmatter parses")
            .expect("a knowledge Class is ontology content");
        assert_eq!(parsed.ontology_type, "Class");
        assert_eq!(parsed.page.id, "Ns/Alpha Beta");
        let e = &parsed.entity;
        assert_eq!(e.slug, "alpha-beta");
        assert_eq!(e.title, "Alpha Beta");
        assert_eq!(e.class_iri.as_deref(), Some("urn:ngm:class:alpha-beta"));
        assert_eq!(e.page_iri, "urn:visionflow:page:abc");
        assert_eq!(e.kind, EntityKind::OntologyClass);
        assert!(e.public);
        // The curated `links` list wins over the body scan.
        let targets: Vec<&str> = e
            .outbound_links
            .iter()
            .map(|l| l.target_slug.as_str())
            .collect();
        assert_eq!(targets, vec!["gamma"]);
        assert_eq!(e.outbound_links[0].target_label, "Gamma");
    }

    #[test]
    fn the_same_type_under_working_is_not_ontology() {
        assert!(parse_page(CLASS_PAGE, "working/pages/Alpha Beta.md")
            .expect("parses")
            .is_none());
    }

    #[test]
    fn a_non_ontology_type_is_not_ontology() {
        let note = "---\ntype: Note\npublic: true\n---\n\nBody.\n";
        assert!(parse_page(note, "knowledge/pages/N.md")
            .expect("parses")
            .is_none());
    }

    #[test]
    fn an_individual_mints_in_the_individual_namespace() {
        let page = "---\ntype: Individual\ntitle: Ada\n---\n";
        let parsed = parse_page(page, "knowledge/pages/Ada.md")
            .expect("parses")
            .expect("ontology content");
        assert_eq!(parsed.entity.kind, EntityKind::OntologyIndividual);
        assert_eq!(
            parsed.entity.class_iri.as_deref(),
            Some("urn:ngm:individual:ada")
        );
    }

    #[test]
    fn invalid_frontmatter_is_an_error() {
        let broken = "---\ntype: [unclosed\n---\nbody\n";
        assert!(parse_page(broken, "knowledge/pages/B.md").is_err());
    }

    /// ADR-2112: the corpus is frontmatter-only. A `key:: value` line carries
    /// no ontology meaning — it is body text, and a page whose only "metadata"
    /// is such a block yields no entity.
    #[test]
    fn a_logseq_property_line_is_body_text_not_metadata() {
        let page = "type:: Class\npublic:: true\nowl:class:: mv:Alpha\n\n# Alpha\n\nProse.\n";
        assert!(
            parse_page(page, "knowledge/pages/Alpha.md")
                .expect("parsing must not fail")
                .is_none(),
            "a `key:: value` block contributes no structured content"
        );
    }

    /// The same lines inside a page that *does* carry frontmatter do not leak
    /// into the entity either.
    #[test]
    fn a_logseq_property_line_beside_frontmatter_is_ignored() {
        let page = CLASS_PAGE.replace(
            "A fixture class.",
            "resource:: urn:ngm:class:impostor\npublic:: false\n\nA fixture class.",
        );
        let parsed = parse_page(&page, "knowledge/pages/Alpha Beta.md")
            .expect("parses")
            .expect("frontmatter is still ontology content");
        assert_eq!(
            parsed.entity.class_iri.as_deref(),
            Some("urn:ngm:class:alpha-beta"),
            "identity comes from frontmatter, never from a `key::` line"
        );
        assert!(parsed.entity.public);
    }

    /// A `json-ld` fence is body text too: it neither declares nor overrides
    /// ontology identity.
    #[test]
    fn a_json_ld_fence_is_body_text() {
        let fenced = "---\npublic: true\n---\n\n```json-ld\n{ \"@type\": \"Class\", \"@id\": \"urn:ngm:class:x\" }\n```\n";
        assert!(parse_page(fenced, "knowledge/pages/X.md")
            .expect("parses")
            .is_none());
    }

    fn test_vocabulary() -> Vocabulary {
        Vocabulary::from_yaml_str(
            r#"
version: 1
namespace: "urn:ngm:class:"
relations:
  is-a:     { owl: "rdfs:subClassOf" }
  requires: { owl: "vc:requires" }
  uses:     { owl: "vc:uses" }
"#,
        )
        .expect("test vocabulary")
    }

    #[test]
    fn the_projection_resolves_relations_to_declared_resources() {
        let parent = "---\ntype: Class\ntitle: Device\nresource: urn:ngm:class:the-device\npublic: true\n---\nA thing.\n";
        let child = "---\ntype: Class\ntitle: Camera\nresource: urn:ngm:class:camera\npublic: true\ndomain: robotics\nquality: 0.7\nis-a:\n- '[[Device]]'\nrequires:\n- '[[Lens]]'\nuses:\n- '[[Light]]'\n---\nCaptures light.\n";
        let pages: Vec<ParsedPage> = [
            (parent, "knowledge/pages/Device.md"),
            (child, "knowledge/pages/Camera.md"),
        ]
        .into_iter()
        .map(|(text, path)| parse_page(text, path).unwrap().unwrap())
        .collect();

        let projection = project_ontology(&pages, &test_vocabulary());
        assert_eq!(projection.classes.len(), 2);
        let camera = projection
            .classes
            .iter()
            .find(|c| c.iri == "urn:ngm:class:camera")
            .expect("camera");
        assert_eq!(
            camera.parent_classes,
            vec!["urn:ngm:class:the-device".to_string()],
            "a page target resolves to that page's declared resource"
        );
        assert_eq!(camera.requires, vec!["urn:ngm:class:lens".to_string()]);
        assert_eq!(
            camera.other_relationships.get("uses"),
            Some(&vec!["urn:ngm:class:light".to_string()])
        );
        assert_eq!(camera.description.as_deref(), Some("Captures light."));
        assert_eq!(camera.source_domain.as_deref(), Some("robotics"));
        assert_eq!(camera.quality_score, Some(0.7));

        assert_eq!(projection.axioms.len(), 1);
        let axiom = &projection.axioms[0];
        assert_eq!(axiom.axiom_type, AxiomType::SubClassOf);
        assert_eq!(axiom.subject, "urn:ngm:class:camera");
        assert_eq!(axiom.object, "urn:ngm:class:the-device");
    }

    #[test]
    fn pages_dir_is_the_segment_holding_the_page() {
        assert_eq!(pages_dir("knowledge/pages/Ns/Foo.md"), "knowledge/pages");
        assert_eq!(pages_dir("pages/Foo.md"), "pages");
        assert_eq!(pages_dir("Foo.md"), "");
    }
}
