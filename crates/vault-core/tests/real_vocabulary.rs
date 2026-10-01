//! The model must load WS-B's real `ontology/vocabulary.yaml`, not merely a
//! fixture shaped like it.
//!
//! These assert **invariants**, not counts. WS-B is still authoring the file
//! and a test that pins `relations.len() == 47` would break on every edit
//! without telling anyone anything true. What must hold is: it parses, the
//! v1 predicates keep their OWL IRIs and nothing silently falls through.
//!
//! Skipped (not failed) when the corpus is not checked out beside this repo,
//! so the crate still builds on a clean machine.

use std::path::{Path, PathBuf};

use vault_core::vocabulary::Vocabulary;

fn real_vocabulary() -> Option<(PathBuf, Vocabulary)> {
    let candidates = [
        Path::new("/home/devuser/workspace/visionGraph/ontology/vocabulary.yaml"),
        Path::new("../../../visionGraph/ontology/vocabulary.yaml"),
    ];
    let path = candidates.iter().find(|p| p.is_file())?;
    Some((
        (*path).to_path_buf(),
        Vocabulary::load(path).unwrap_or_else(|e| panic!("loading {}: {e}", path.display())),
    ))
}

macro_rules! vocab_or_skip {
    () => {
        match real_vocabulary() {
            Some((_, v)) => v,
            None => {
                eprintln!("skipped: no visionGraph checkout");
                return;
            }
        }
    };
}

#[test]
fn it_loads_and_is_internally_consistent() {
    let vocab = vocab_or_skip!();
    assert!(vocab.version >= 1);
    assert_eq!(vocab.namespace, "urn:ngm:class:");
    assert_eq!(vocab.individual_namespace, "urn:ngm:individual:");
    assert_eq!(vocab.taxonomy_key(), Some("is-a"));
    assert!(!vocab.relations.is_empty());
    assert!(!vocab.scalars.is_empty());
}

#[test]
fn the_v1_predicates_keep_their_owl_iris() {
    let vocab = vocab_or_skip!();
    // OWL IRI stability is the vocabulary's own headline promise: these are
    // the same as the published ontology. A change here is
    // an ontology break, not a refactor.
    for (key, owl) in [
        ("is-a", "http://www.w3.org/2000/01/rdf-schema#subClassOf"),
        ("has-part", "https://narrativegoldmine.com/ns/v1#hasPart"),
        ("part-of", "https://narrativegoldmine.com/ns/v1#isPartOf"),
        ("requires", "https://narrativegoldmine.com/ns/v1#requires"),
        ("enables", "https://narrativegoldmine.com/ns/v1#enables"),
        (
            "depends-on",
            "https://narrativegoldmine.com/ns/v1#dependsOn",
        ),
        (
            "implements",
            "https://narrativegoldmine.com/ns/v1#implements",
        ),
        (
            "contrasts-with",
            "https://narrativegoldmine.com/ns/v1#contrastsWith",
        ),
        (
            "bridges-to",
            "https://narrativegoldmine.com/ns/v1#bridgesTo",
        ),
        ("uses", "https://narrativegoldmine.com/ns/v1#uses"),
        ("supports", "https://narrativegoldmine.com/ns/v1#supports"),
        (
            "standardized-by",
            "https://narrativegoldmine.com/ns/v1#standardizedBy",
        ),
        (
            "related-to",
            "https://narrativegoldmine.com/ns/v1#relatedTo",
        ),
        (
            "enabled-by",
            "https://narrativegoldmine.com/ns/v1#enabledBy",
        ),
    ] {
        let def = vocab
            .relations
            .get(key)
            .unwrap_or_else(|| panic!("{key} is not declared"));
        assert_eq!(vocab.expand(&def.owl), owl, "{key}");
        assert!(def.emitted, "{key} must reach ontology.ttl");
    }
}

#[test]
fn unemitted_relations_are_valid_frontmatter_and_absent_from_the_ttl() {
    let vocab = vocab_or_skip!();
    for key in ["same-as", "produces", "implemented-in-layer"] {
        assert!(vocab.is_relation(key), "{key} must be valid frontmatter");
        assert!(
            !vocab.is_emitted_relation(key),
            "{key} must stay out of ontology.ttl for parity"
        );
    }
    assert!(!vocab.emitted_relations().is_empty());
}

#[test]
fn restrictions_are_declared_on_requires_and_has_part_only() {
    let vocab = vocab_or_skip!();
    let mut r = vocab.restriction_relations();
    r.sort_unstable();
    assert_eq!(r, ["has-part", "requires"]);
}

#[test]
fn maturity_admits_mature() {
    let vocab = vocab_or_skip!();
    let maturity = &vocab.scalars["maturity"];
    assert!(
        maturity.r#enum.contains(&"mature".to_owned()),
        "`mature` must stop collapsing to `draft`"
    );
    assert!(maturity.r#enum.contains(&"deprecated".to_owned()));
}

#[test]
fn the_domain_scalar_is_free_text_with_a_census_not_an_enumeration() {
    // A7. `domain` has no closed `enum`; it has six `roots` and a 17-value
    // `observed` census. `known_values` is their union, which is what
    // `vault validate` checks against — so `economics` (84 pages, in the
    // census) is not reported, and a typo still is.
    let vocab = vocab_or_skip!();
    let domain = vocab
        .scalars
        .get("domain")
        .expect("the vocabulary declares `domain`");
    assert!(
        domain.r#enum.is_empty(),
        "`domain` is deliberately free text: normalising `ai` onto \
         `artificial-intelligence` is a governance decision, not a schema one"
    );
    assert_eq!(
        domain.roots.len(),
        8,
        "the six legacy and two space/EO roots"
    );
    assert!(domain.observed.len() >= 17, "{:?}", domain.observed.len());

    let known = domain.known_values();
    for value in [
        "artificial-intelligence",
        "blockchain",
        "economics",
        "machine-learning",
        "supply-chain",
        "space-science-and-systems",
        "earth-observation-and-geospatial-sensing",
    ] {
        assert!(known.contains(&value), "`{value}` is in the census");
    }
    assert!(
        !known.contains(&"not-a-domain"),
        "an unseen value is still reported"
    );
    // The roots are all present even if the census somehow omitted one.
    for root in &domain.roots {
        assert!(known.contains(&root.as_str()), "{root}");
    }
}
