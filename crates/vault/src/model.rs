//! The build's intermediate model: a [`Corpus`] of [`ClassRecord`]s projected
//! out of frontmatter pages.
//!
//! Every artefact `vault build` emits is a pure function of this model, which
//! is what makes the golden parity test meaningful: the Python pipeline's
//! `PageData` / `OntologyEntity` pair is reproduced here field for field, so a
//! divergence in output is a divergence in one clearly located projection.
//!
//! # Reference resolution
//!
//! A frontmatter relation is a list of wikilinks; the artefacts need IRIs. A
//! target is resolved in this order:
//!
//! 1. a page with that id, title or alias — its `resource` is the IRI;
//! 2. an entry in the vocabulary's `tail_iris` map — the 163 long-tail
//!    concepts whose IRI is genuinely not derivable from their label
//!    (`IDA*` → `urn:ngm:class:ida-star`);
//! 3. `namespace + slugify(target)`.
//!
//! Step 2 exists because the json-ld fences carried explicit IRIs and 0.14% of
//! them cannot be recomputed. Losing them would silently re-point those edges.

use std::collections::HashMap;

use indexmap::IndexMap;
use vault_core::page::{Page, Vault};
use vault_core::slug::{ref_slug, slugify, target_slug};
use vault_core::vocabulary::Vocabulary;

/// The twelve relation attributes in `pipeline/reason.py::RELATION_TYPES`
/// order: `(frontmatter key, camelCase artefact key)`.
///
/// This is the **frozen v1 artefact contract**, not a vocabulary derivative: a
/// consumer that already parses `relationships.hasPart` must keep working
/// across a vocabulary rename.
pub const RELATION_KEYS: &[(&str, &str)] = &[
    ("has-part", "hasPart"),
    ("requires", "requires"),
    ("enables", "enables"),
    ("depends-on", "dependsOn"),
    ("implements", "implements"),
    ("contrasts-with", "contrastsWith"),
    ("bridges-to", "bridgesTo"),
    ("uses", "uses"),
    ("supports", "supports"),
    ("standardized-by", "standardizedBy"),
    ("part-of", "partOf"),
    ("related-to", "relatedTo"),
];

/// The frontmatter key that carries the taxonomy (`rdfs:subClassOf`).
pub const IS_A_KEY: &str = "is-a";
/// The frontmatter key that carries `rdf:type` for individuals.
pub const INSTANCE_OF_KEY: &str = "instance-of";

/// An IRI plus the label the citing page used for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    /// The resolved IRI.
    pub iri: String,
    /// The display label from the wikilink.
    pub label: String,
}

impl Ref {
    /// The IRI's slug (last colon segment).
    #[must_use]
    pub fn slug(&self) -> String {
        ref_slug(&self.iri)
    }
}

/// Whether a record is an OWL class or a named individual.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityType {
    /// `owl:Class`.
    Class,
    /// `owl:NamedIndividual`.
    Individual,
}

impl EntityType {
    /// The artefact spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Class => "Class",
            Self::Individual => "Individual",
        }
    }
}

/// One page projected into the ontology model.
#[derive(Debug, Clone)]
pub struct ClassRecord {
    /// The page id (vault-relative path without `.md`).
    pub page_id: String,
    /// The page's publish slug.
    pub slug: String,
    /// The page's title.
    pub title: String,
    /// `public: true`?
    pub public: bool,
    /// The page's identity IRI (`urn:visionflow:page:…` historically; the
    /// class IRI when no separate page IRI exists).
    pub page_iri: String,
    /// The class/individual IRI (`resource`).
    pub iri: String,
    /// `rdfs:label`.
    pub label: String,
    /// Class or individual.
    pub entity_type: EntityType,
    /// `domain`.
    pub domain: String,
    /// `definition`.
    pub definition: String,
    /// `maturity`.
    pub maturity: String,
    /// `quality`.
    pub quality: f64,
    /// `legacy-term-id` — the pre-urn identifier (`AI-0376`, `RB-0162`) the
    /// `WebVOWL` node attributes carry as `term_id`. 2,466 pages have one.
    pub legacy_term_id: String,
    /// `is-a`.
    pub sub_class_of: Vec<Ref>,
    /// `instance-of`.
    pub instance_of: Vec<Ref>,
    /// The twelve typed relations, keyed by camelCase artefact key, in
    /// [`RELATION_KEYS`] order; empty lists are absent.
    pub relations: IndexMap<&'static str, Vec<Ref>>,
    /// Curated outbound wikilinks.
    pub links: Vec<Ref>,
    /// The page body.
    pub body: String,
    /// Whether the page declared an ontology identity at all. A page without
    /// `resource` and without `type` is a plain note: it still appears in the
    /// search index but contributes no OWL.
    pub has_ontology: bool,
}

impl ClassRecord {
    /// The class slug used to key every artefact — the IRI's tail, **not** the
    /// page slug. 96 pages in the corpus differ between the two.
    #[must_use]
    pub fn class_slug(&self) -> String {
        ref_slug(&self.iri)
    }

    /// The refs under a camelCase relation key.
    #[must_use]
    pub fn relation(&self, json_key: &str) -> &[Ref] {
        self.relations.get(json_key).map_or(&[], Vec::as_slice)
    }
}

/// The resolved corpus: every record plus the indexes the build needs.
#[derive(Debug, Clone)]
pub struct Corpus {
    /// Records in page order.
    pub records: Vec<ClassRecord>,
    /// Class slug to index into [`Corpus::records`], first definition winning.
    pub by_class_slug: IndexMap<String, usize>,
}

/// Index from lowercased name (id, title, alias) to page position.
struct NameIndex {
    by_name: HashMap<String, usize>,
}

impl NameIndex {
    fn build(pages: &[Page]) -> Self {
        let mut by_name = HashMap::new();
        for (i, page) in pages.iter().enumerate() {
            let mut add = |k: String| {
                by_name.entry(k).or_insert(i);
            };
            add(page.id.to_lowercase());
            add(page.title().to_lowercase());
            // A slug-targeted wikilink (what `vault migrate` emits for a
            // long-tail reference) must still connect if a page is later
            // authored for that concept.
            add(page.slug().to_lowercase());
            for a in page.frontmatter.strings("aliases") {
                add(a.to_lowercase());
            }
        }
        Self { by_name }
    }

    fn get(&self, name: &str) -> Option<usize> {
        self.by_name.get(&name.to_lowercase()).copied()
    }
}

/// Give every public record a unique publish slug.
///
/// The slug names each page's published files (`api/pages/<slug>.json`) and
/// is its search-index `id`, which is what the explorer fetches by. It is the
/// declared `slug`, else `slugify(title)` — and those two sources can collide:
/// `ML Experiment Tracking` declares `slug: experiment-tracking`, while
/// `Experiment Tracking` derives the same slug from its title. Left alone, the
/// second page's file silently overwrote the first's.
///
/// Among public records sharing a slug, one keeps it: the page that
/// **declares** it (the migration carried that slug over as the page's
/// published identity), else the one whose `resource` tail equals it, else the
/// first in page order. Every other claimant is re-keyed to its own `resource`
/// tail — unique, immutable, and already the IRI Loom and `VisionClaw` key on.
/// Private records are left alone: they publish nothing, and a public/private
/// clash is refused by [`crate::projection::project`] rather than resolved by
/// renaming a public page.
fn disambiguate_public_slugs(records: &mut [ClassRecord], declares_slug: &[bool]) {
    let mut claims: IndexMap<String, Vec<usize>> = IndexMap::new();
    for (i, r) in records.iter().enumerate() {
        if r.public && !r.slug.is_empty() {
            claims.entry(r.slug.clone()).or_default().push(i);
        }
    }
    for (slug, claimants) in claims {
        if claimants.len() < 2 {
            continue;
        }
        let keeper = claimants
            .iter()
            .copied()
            .find(|&i| declares_slug[i])
            .or_else(|| {
                claimants
                    .iter()
                    .copied()
                    .find(|&i| ref_slug(&records[i].iri) == slug)
            })
            .unwrap_or(claimants[0]);
        for i in claimants {
            if i != keeper {
                let tail = ref_slug(&records[i].iri);
                if !tail.is_empty() {
                    records[i].slug = tail;
                }
            }
        }
    }
}

impl Corpus {
    /// Project a loaded vault through its vocabulary.
    #[must_use]
    pub fn build(vault: &Vault, vocab: &Vocabulary) -> Self {
        let names = NameIndex::build(&vault.pages);
        let resources: Vec<String> = vault
            .pages
            .iter()
            .map(|p| p.resource(&vocab.namespace))
            .collect();

        let resolve = |target: &str, label: &str| -> Ref {
            let iri = names
                .get(target)
                .map(|i| resources[i].clone())
                .or_else(|| vocab.tail_iris.get(label).cloned())
                .or_else(|| vocab.tail_iris.get(target).cloned())
                .unwrap_or_else(|| format!("{}{}", vocab.namespace, slugify(target)));
            Ref {
                iri,
                label: label.to_owned(),
            }
        };

        let links_of = |page: &Page, key: &str| -> Vec<Ref> {
            page.frontmatter
                .wikilinks(key)
                .into_iter()
                .map(|w| resolve(&w.target, w.label()))
                .collect()
        };

        let mut records = Vec::with_capacity(vault.pages.len());
        let mut declares_slug = Vec::with_capacity(vault.pages.len());
        for (i, page) in vault.pages.iter().enumerate() {
            declares_slug.push(page.frontmatter.has("slug"));
            let fm = &page.frontmatter;
            let title = page.title();
            let mut relations: IndexMap<&'static str, Vec<Ref>> = IndexMap::new();
            for (fm_key, json_key) in RELATION_KEYS {
                let refs = links_of(page, fm_key);
                if !refs.is_empty() {
                    relations.insert(json_key, refs);
                }
            }
            let entity_type = if fm.text("type").as_deref() == Some("Individual") {
                EntityType::Individual
            } else {
                EntityType::Class
            };
            records.push(ClassRecord {
                page_id: page.id.clone(),
                slug: page.slug(),
                label: fm
                    .text("label")
                    .filter(|l| !l.trim().is_empty())
                    .unwrap_or_else(|| title.clone()),
                title,
                public: page.is_public(),
                page_iri: fm
                    .text("page_resource")
                    .unwrap_or_else(|| resources[i].clone()),
                iri: resources[i].clone(),
                entity_type,
                domain: fm.text("domain").unwrap_or_default(),
                // `definition_placement: leading-paragraph` (Q4): the fence
                // definition is prose in the body, not a frontmatter key. The
                // frontmatter form is still honoured for a vocabulary that
                // declares `frontmatter`, so this side stays data-driven too.
                definition: fm
                    .text("definition")
                    .filter(|d| !d.trim().is_empty())
                    .unwrap_or_else(|| page.leading_paragraph()),
                maturity: fm
                    .text("maturity")
                    .filter(|m| !m.trim().is_empty())
                    .unwrap_or_else(|| "draft".to_owned()),
                quality: fm.number("quality").unwrap_or(0.0),
                legacy_term_id: fm.text("legacy-term-id").unwrap_or_default(),
                sub_class_of: links_of(page, IS_A_KEY),
                instance_of: links_of(page, INSTANCE_OF_KEY),
                relations,
                // Curated links resolve **slug-verbatim**, not through the
                // name index. The backlink graph is keyed on the link's own
                // IRI tail, and 96 pages have a class slug that differs from
                // their page slug — resolving through the target page would
                // silently re-key every backlink list in the bundle.
                links: page
                    .outbound_links()
                    .into_iter()
                    .map(|w| Ref {
                        iri: format!("{}{}", vocab.namespace, target_slug(&w.target)),
                        label: w.label().to_owned(),
                    })
                    .collect(),
                body: page.body.clone(),
                has_ontology: fm.has("resource") || fm.has("type"),
            });
        }

        disambiguate_public_slugs(&mut records, &declares_slug);

        let mut by_class_slug = IndexMap::new();
        for (i, r) in records.iter().enumerate() {
            if r.has_ontology && !r.iri.is_empty() {
                by_class_slug.entry(r.class_slug()).or_insert(i);
            }
        }

        Self {
            records,
            by_class_slug,
        }
    }

    /// The records with `public: true`, in page order.
    pub fn public(&self) -> impl Iterator<Item = &ClassRecord> {
        self.records.iter().filter(|r| r.public)
    }

    /// The public records that are OWL classes with a non-empty IRI — the set
    /// `scaffold-index.json` and the class count are computed over.
    pub fn public_classes(&self) -> impl Iterator<Item = &ClassRecord> {
        self.records.iter().filter(|r| {
            r.public && r.has_ontology && !r.iri.is_empty() && r.entity_type == EntityType::Class
        })
    }

    /// Look a record up by class slug (first definition wins).
    #[must_use]
    pub fn by_slug(&self, slug: &str) -> Option<&ClassRecord> {
        self.by_class_slug.get(slug).map(|&i| &self.records[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault_core::page::VaultKind;

    #[test]
    fn a_declared_slug_keeps_its_url_and_the_derived_claimant_moves_to_its_resource() {
        let vault = Vault {
            root: std::path::PathBuf::new(),
            kind: VaultKind::Knowledge,
            journals: Vec::new(),
            skipped: Vec::new(),
            pages: [
                (
                    "Experiment Tracking",
                    "---\ntype: Class\npublic: true\nresource: urn:ngm:class:empirical-experimental-design-tracking\n---\n",
                ),
                (
                    "ML Experiment Tracking",
                    "---\ntype: Class\npublic: true\nslug: experiment-tracking\nresource: urn:ngm:class:experiment-tracking\n---\n",
                ),
            ]
            .into_iter()
            .map(|(id, text)| {
                Page::parse(format!("/v/pages/{id}.md"), format!("pages/{id}.md"), id, text)
                    .unwrap()
            })
            .collect(),
        };
        let vocab =
            Vocabulary::from_yaml_str("version: 1\nnamespace: \"urn:ngm:class:\"\n").unwrap();
        let corpus = Corpus::build(&vault, &vocab);
        let slug_of = |id: &str| {
            corpus
                .records
                .iter()
                .find(|r| r.page_id == id)
                .unwrap()
                .slug
                .clone()
        };
        assert_eq!(slug_of("ML Experiment Tracking"), "experiment-tracking");
        assert_eq!(
            slug_of("Experiment Tracking"),
            "empirical-experimental-design-tracking"
        );
    }

    fn vocab() -> Vocabulary {
        Vocabulary::from_yaml_str(
            r#"
version: 1
namespace: "urn:ngm:class:"
relations:
  is-a:      { owl: "rdfs:subClassOf" }
  requires:  { owl: "vc:requires" }
  has-part:  { owl: "vc:hasPart" }
scalars:
  domain: { type: text }
tail_iris:
  "IDA*": "urn:ngm:class:ida-star"
"#,
        )
        .unwrap()
    }

    fn page(id: &str, fm: &str, body: &str) -> Page {
        Page::parse(
            format!("/v/pages/{id}.md"),
            format!("pages/{id}.md"),
            id,
            &format!("---\n{fm}---\n{body}"),
        )
        .unwrap()
    }

    fn corpus(pages: Vec<Page>) -> Corpus {
        let vault = Vault {
            root: std::path::PathBuf::new(),
            kind: VaultKind::Knowledge,
            journals: Vec::new(),
            skipped: Vec::new(),
            pages,
        };
        Corpus::build(&vault, &vocab())
    }

    #[test]
    fn resolves_a_ref_through_the_target_pages_resource() {
        let c = corpus(vec![
            page(
                "Apple Vision Pro",
                "type: Class\npublic: true\nresource: urn:ngm:class:apple-inc-technology-corporation-vision-pro\n",
                "",
            ),
            page("Headset", "type: Class\npublic: true\nis-a: [\"[[Apple Vision Pro]]\"]\n", ""),
        ]);
        let r = &c.records[1].sub_class_of[0];
        assert_eq!(
            r.iri,
            "urn:ngm:class:apple-inc-technology-corporation-vision-pro"
        );
        assert_eq!(r.label, "Apple Vision Pro");
    }

    #[test]
    fn resolves_through_an_alias() {
        let c = corpus(vec![
            page(
                "Localisation",
                "type: Class\npublic: true\naliases: [Localization]\n",
                "",
            ),
            page(
                "LiDAR",
                "type: Class\npublic: true\nrequires: [\"[[Localization]]\"]\n",
                "",
            ),
        ]);
        assert_eq!(
            c.records[1].relation("requires")[0].iri,
            "urn:ngm:class:localisation"
        );
    }

    #[test]
    fn falls_back_to_a_tail_iri_then_to_slugification() {
        let c = corpus(vec![page(
            "Planner",
            "type: Class\npublic: true\nuses: []\nrequires: [\"[[IDA*]]\", \"[[Plain Concept]]\"]\n",
            "",
        )]);
        let refs = c.records[0].relation("requires");
        assert_eq!(refs[0].iri, "urn:ngm:class:ida-star");
        assert_eq!(refs[1].iri, "urn:ngm:class:plain-concept");
    }

    #[test]
    fn curated_links_keep_their_own_slug() {
        let c = corpus(vec![
            page(
                "Animation",
                "type: Class\npublic: true\nresource: urn:ngm:class:animation\nslug: 3-d-animation\n",
                "",
            ),
            page(
                "Viewer",
                "type: Class\npublic: true\nlinks: [\"[[3-d-animation|3D Animation]]\"]\n",
                "",
            ),
        ]);
        // Not `urn:ngm:class:animation`: the link carries its own identity.
        assert_eq!(c.records[1].links[0].iri, "urn:ngm:class:3-d-animation");
        assert_eq!(c.records[1].links[0].label, "3D Animation");
    }

    #[test]
    fn a_wikilink_alias_becomes_the_ref_label() {
        let c = corpus(vec![
            page("Localisation", "type: Class\npublic: true\n", ""),
            page(
                "LiDAR",
                "type: Class\npublic: true\nrequires: [\"[[Localisation|Localization]]\"]\n",
                "",
            ),
        ]);
        let r = &c.records[1].relation("requires")[0];
        assert_eq!(r.iri, "urn:ngm:class:localisation");
        assert_eq!(r.label, "Localization");
    }

    #[test]
    fn relations_keep_the_frozen_artefact_order() {
        let c = corpus(vec![page(
            "X",
            "type: Class\npublic: true\nrequires: [\"[[A]]\"]\nhas-part: [\"[[B]]\"]\n",
            "",
        )]);
        let keys: Vec<&str> = c.records[0].relations.keys().copied().collect();
        assert_eq!(keys, vec!["hasPart", "requires"]);
    }

    #[test]
    fn the_class_slug_comes_from_the_iri_not_the_page_slug() {
        let c = corpus(vec![page(
            "Bitcoin Cash",
            "type: Class\npublic: true\nslug: bitcoin-cash\nresource: urn:ngm:class:bitcoin-proof-of-work-protocol-cash\n",
            "",
        )]);
        assert_eq!(c.records[0].slug, "bitcoin-cash");
        assert_eq!(
            c.records[0].class_slug(),
            "bitcoin-proof-of-work-protocol-cash"
        );
        assert!(c.by_slug("bitcoin-proof-of-work-protocol-cash").is_some());
    }

    #[test]
    fn private_and_non_ontology_pages_are_excluded_from_the_class_set() {
        let c = corpus(vec![
            page("Pub", "type: Class\npublic: true\n", ""),
            page("Priv", "type: Class\npublic: false\n", ""),
            page("Note", "public: true\n", "just prose"),
            page("Ind", "type: Individual\npublic: true\n", ""),
        ]);
        let slugs: Vec<String> = c
            .public_classes()
            .map(super::ClassRecord::class_slug)
            .collect();
        assert_eq!(slugs, vec!["pub".to_owned()]);
        assert_eq!(c.public().count(), 3);
    }

    #[test]
    fn maturity_defaults_to_draft_and_quality_to_zero() {
        let c = corpus(vec![page("X", "type: Class\npublic: true\n", "")]);
        assert_eq!(c.records[0].maturity, "draft");
        assert!((c.records[0].quality - 0.0).abs() < f64::EPSILON);
    }
}
