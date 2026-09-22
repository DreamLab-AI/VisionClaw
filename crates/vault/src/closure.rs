//! The structural subclass closure — a faithful port of
//! `pipeline/reason.py::compute_closure`.
//!
//! This is **not** the reasoner. Whelk computes the EL++ closure that
//! `ontology-inferred.ttl` carries (see [`crate::whelk`]); this module
//! reproduces the Python pipeline's plain transitive `subClassOf` BFS because
//! `scaffold-index.json`'s `sup` / `isup` fields encode its exact *ordering*,
//! and the build must stay byte-identical to what Loom already loads.
//!
//! The ordering rule, stated precisely:
//!
//! * level 1 is the direct parents in assertion order, deduplicated;
//! * every later level is sorted alphabetically;
//! * a node already visited is never re-emitted, so a `subClassOf` cycle
//!   yields its component minus the node itself rather than looping.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use vault_core::slug::{is_thing, ref_slug};

use crate::model::{ClassRecord, Corpus, Ref};

/// Cap on inherited targets per relation type, matching
/// `reason.py::INHERITED_RELATION_CAP`.
pub const INHERITED_RELATION_CAP: usize = 8;

/// The computed closure, keyed throughout by **class slug**.
#[derive(Debug, Clone, Default)]
pub struct Closure {
    /// Every transitive superclass, proximity-ordered. Never contains self or
    /// `owl:Thing`.
    pub ancestors: IndexMap<String, Vec<String>>,
    /// Direct parents, assertion order, deduplicated.
    pub direct_parents: IndexMap<String, Vec<String>>,
    /// Ancestors that are not direct parents, proximity-ordered.
    pub inferred_superclasses: IndexMap<String, Vec<String>>,
    /// Relations inherited from ancestors, keyed by camelCase relation key.
    pub inherited_relations: IndexMap<String, IndexMap<String, Vec<Ref>>>,
    /// Best-known label per slug (corpus class first, then ref metadata).
    pub labels: HashMap<String, String>,
    /// Best-known IRI per slug.
    pub iris: HashMap<String, String>,
}

impl Closure {
    /// Direct parents of `slug`, or an empty slice.
    #[must_use]
    pub fn parents_of(&self, slug: &str) -> &[String] {
        self.direct_parents.get(slug).map_or(&[], Vec::as_slice)
    }

    /// Inferred (non-direct) ancestors of `slug`, or an empty slice.
    #[must_use]
    pub fn inferred_of(&self, slug: &str) -> &[String] {
        self.inferred_superclasses
            .get(slug)
            .map_or(&[], Vec::as_slice)
    }

    /// The IRI for a slug, synthesising `urn:ngm:class:<slug>` when the slug
    /// is not in the corpus — exactly the Python fallback.
    #[must_use]
    pub fn iri_of(&self, slug: &str) -> String {
        self.iris
            .get(slug)
            .cloned()
            .unwrap_or_else(|| format!("urn:ngm:class:{slug}"))
    }

    /// The label for a slug, falling back to the slug itself.
    #[must_use]
    pub fn label_of(&self, slug: &str) -> String {
        self.labels
            .get(slug)
            .cloned()
            .unwrap_or_else(|| slug.to_owned())
    }
}

/// Compute the closure over every record that carries a class IRI.
///
/// The record set is **not** filtered to public pages, matching
/// `build.py`, which computes the closure after the public projection has
/// already been applied to `pages`.
#[must_use]
pub fn compute(corpus: &Corpus) -> Closure {
    let mut closure = Closure::default();
    let mut class_by_slug: IndexMap<String, &ClassRecord> = IndexMap::new();
    let mut parent_map: IndexMap<String, Vec<String>> = IndexMap::new();

    for record in &corpus.records {
        if !record.has_ontology || record.iri.is_empty() {
            continue;
        }
        let slug = record.class_slug();
        if class_by_slug.contains_key(&slug) {
            continue; // first definition wins
        }
        closure.labels.insert(
            slug.clone(),
            if record.label.is_empty() {
                if record.title.is_empty() {
                    slug.clone()
                } else {
                    record.title.clone()
                }
            } else {
                record.label.clone()
            },
        );
        closure.iris.insert(slug.clone(), record.iri.clone());

        let mut parents: Vec<String> = Vec::new();
        for parent in &record.sub_class_of {
            let p_slug = ref_slug(&parent.iri);
            if p_slug.is_empty() || is_thing(&p_slug) || p_slug == slug {
                continue;
            }
            if !parents.contains(&p_slug) {
                parents.push(p_slug.clone());
            }
            closure.labels.entry(p_slug.clone()).or_insert_with(|| {
                if parent.label.is_empty() {
                    p_slug.clone()
                } else {
                    parent.label.clone()
                }
            });
            closure
                .iris
                .entry(p_slug)
                .or_insert_with(|| parent.iri.clone());
        }
        parent_map.insert(slug.clone(), parents);
        class_by_slug.insert(slug, record);
    }

    for slug in class_by_slug.keys() {
        let direct = parent_map.get(slug).cloned().unwrap_or_default();
        let mut visited: HashSet<String> = HashSet::new();
        visited.insert(slug.clone());
        let mut ordered: Vec<String> = Vec::new();
        let mut frontier: Vec<String> = direct
            .iter()
            .filter(|p| !visited.contains(*p))
            .cloned()
            .collect();

        while !frontier.is_empty() {
            for p in &frontier {
                visited.insert(p.clone());
                ordered.push(p.clone());
            }
            let mut next: Vec<String> = Vec::new();
            for p in &frontier {
                for gp in parent_map.get(p).map_or(&[][..], Vec::as_slice) {
                    if !visited.contains(gp) && !next.contains(gp) {
                        next.push(gp.clone());
                    }
                }
            }
            next.sort(); // alphabetical within a BFS level, for determinism
            frontier = next;
        }

        let direct_set: HashSet<&String> = direct.iter().collect();
        closure.inferred_superclasses.insert(
            slug.clone(),
            ordered
                .iter()
                .filter(|a| !direct_set.contains(*a))
                .cloned()
                .collect(),
        );
        closure.direct_parents.insert(slug.clone(), direct);
        closure.ancestors.insert(slug.clone(), ordered);
    }

    compute_inherited(&class_by_slug, &mut closure);
    closure
}

fn compute_inherited(class_by_slug: &IndexMap<String, &ClassRecord>, closure: &mut Closure) {
    for (slug, record) in class_by_slug {
        let mut inherited: IndexMap<String, Vec<Ref>> = IndexMap::new();
        for (_, json_key) in crate::model::RELATION_KEYS {
            let own: HashSet<String> = record.relation(json_key).iter().map(Ref::slug).collect();
            let mut seen = own;
            seen.insert(slug.clone());
            let mut collected: Vec<Ref> = Vec::new();
            'ancestors: for anc in closure.ancestors.get(slug).map_or(&[][..], Vec::as_slice) {
                let Some(anc_record) = class_by_slug.get(anc) else {
                    continue; // ancestor outside the corpus: nothing to inherit
                };
                let mut anc_refs: Vec<&Ref> = anc_record.relation(json_key).iter().collect();
                anc_refs.sort_by_key(|r| r.slug());
                for r in anc_refs {
                    let t = r.slug();
                    if seen.contains(&t) {
                        continue;
                    }
                    seen.insert(t);
                    collected.push((*r).clone());
                    if collected.len() >= INHERITED_RELATION_CAP {
                        break 'ancestors;
                    }
                }
            }
            if !collected.is_empty() {
                inherited.insert((*json_key).to_owned(), collected);
            }
        }
        if !inherited.is_empty() {
            closure.inherited_relations.insert(slug.clone(), inherited);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault_core::page::{Page, Vault, VaultKind};
    use vault_core::vocabulary::Vocabulary;

    fn vocab() -> Vocabulary {
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
        .unwrap()
    }

    fn page(id: &str, fm: &str) -> Page {
        Page::parse(
            format!("/v/pages/{id}.md"),
            format!("pages/{id}.md"),
            id,
            &format!("---\ntype: Class\npublic: true\n{fm}---\n"),
        )
        .unwrap()
    }

    fn closure_of(pages: Vec<Page>) -> Closure {
        let vault = Vault {
            root: std::path::PathBuf::new(),
            kind: VaultKind::Knowledge,
            journals: Vec::new(),
            skipped: Vec::new(),
            pages,
        };
        compute(&Corpus::build(&vault, &vocab()))
    }

    #[test]
    fn direct_parents_keep_assertion_order_and_dedupe() {
        let c = closure_of(vec![
            page("A", "is-a: [\"[[C]]\", \"[[B]]\", \"[[C]]\"]\n"),
            page("B", ""),
            page("C", ""),
        ]);
        assert_eq!(c.parents_of("a"), ["c", "b"]);
    }

    #[test]
    fn later_levels_are_alphabetical() {
        // A -> {D, B}; B -> {Z}; D -> {M}.  Level 2 must be [m, z].
        let c = closure_of(vec![
            page("A", "is-a: [\"[[D]]\", \"[[B]]\"]\n"),
            page("B", "is-a: [\"[[Z]]\"]\n"),
            page("D", "is-a: [\"[[M]]\"]\n"),
            page("M", ""),
            page("Z", ""),
        ]);
        assert_eq!(c.ancestors["a"], ["d", "b", "m", "z"]);
        assert_eq!(c.inferred_of("a"), ["m", "z"]);
    }

    #[test]
    fn owl_thing_and_self_references_are_dropped() {
        let c = closure_of(vec![page("A", "is-a: [\"[[owl:Thing]]\", \"[[A]]\"]\n")]);
        assert!(c.parents_of("a").is_empty());
        assert!(c.ancestors["a"].is_empty());
    }

    #[test]
    fn a_cycle_terminates_and_excludes_self() {
        let c = closure_of(vec![
            page("A", "is-a: [\"[[B]]\"]\n"),
            page("B", "is-a: [\"[[A]]\"]\n"),
        ]);
        assert_eq!(c.ancestors["a"], ["b"]);
        assert_eq!(c.ancestors["b"], ["a"]);
    }

    #[test]
    fn ancestors_outside_the_corpus_still_appear() {
        let c = closure_of(vec![page("A", "is-a: [\"[[Missing Parent]]\"]\n")]);
        assert_eq!(c.ancestors["a"], ["missing-parent"]);
        assert_eq!(c.iri_of("missing-parent"), "urn:ngm:class:missing-parent");
        assert_eq!(c.label_of("missing-parent"), "Missing Parent");
    }

    #[test]
    fn relations_are_inherited_in_proximity_order_and_deduped() {
        let c = closure_of(vec![
            page("Child", "is-a: [\"[[Parent]]\"]\nrequires: [\"[[Own]]\"]\n"),
            page(
                "Parent",
                "is-a: [\"[[Gp]]\"]\nrequires: [\"[[Own]]\", \"[[FromParent]]\"]\n",
            ),
            page("Gp", "requires: [\"[[FromGp]]\"]\n"),
            page("Own", ""),
            page("FromParent", ""),
            page("FromGp", ""),
        ]);
        let inh = &c.inherited_relations["child"]["requires"];
        let slugs: Vec<String> = inh.iter().map(Ref::slug).collect();
        // "Own" is already the child's own target, so it is not inherited.
        assert_eq!(slugs, ["fromparent", "fromgp"]);
    }

    #[test]
    fn inheritance_is_capped() {
        let mut pages = vec![page("Child", "is-a: [\"[[Parent]]\"]\n")];
        let targets: Vec<String> = (0..12).map(|i| format!("\"[[T{i:02}]]\"")).collect();
        pages.push(page("Parent", &format!("uses: [{}]\n", targets.join(", "))));
        for i in 0..12 {
            pages.push(page(&format!("T{i:02}"), ""));
        }
        let c = closure_of(pages);
        assert_eq!(
            c.inherited_relations["child"]["uses"].len(),
            INHERITED_RELATION_CAP
        );
    }

    #[test]
    fn a_class_with_no_inheritance_has_no_entry() {
        let c = closure_of(vec![page("Solo", "")]);
        assert!(!c.inherited_relations.contains_key("solo"));
    }
}
