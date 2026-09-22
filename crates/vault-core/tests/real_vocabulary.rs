//! The model must load WS-B's real `ontology/vocabulary.yaml`, not merely a
//! fixture shaped like it.
//!
//! These assert **invariants**, not counts. WS-B is still authoring the file
//! and a test that pins `relations.len() == 47` would break on every edit
//! without telling anyone anything true. What must hold is: it parses, the
//! twelve v1 predicates keep their OWL IRIs, every migration destination names
//! a key the same file declares, and nothing silently falls through.
//!
//! Skipped (not failed) when the corpus is not checked out beside this repo,
//! so the crate still builds on a clean machine.

use std::path::{Path, PathBuf};

use vault_core::vocabulary::{DefinitionPlacement, Destination, Vocabulary};

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
    // byte-identical to what jsonld_to_turtle.py emits today. A change here is
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
fn every_migration_destination_names_a_declared_key() {
    let vocab = vocab_or_skip!();
    let m = &vocab.migration;
    assert!(
        m.is_populated(),
        "the migration block must be populated; an empty one means the file \
         was read mid-write and the migration must refuse"
    );

    // `Vocabulary::check` already enforces this at load; assert it explicitly
    // so a regression names the offending key rather than failing to parse.
    for (source, destination) in m.all_destinations() {
        if let Some(target) = destination.frontmatter_key() {
            assert!(
                vocab.is_known_working_key(target),
                "migration maps {source:?} onto undeclared {target:?}"
            );
        }
    }
}

#[test]
fn the_dispositions_are_all_used() {
    let vocab = vocab_or_skip!();
    let m = &vocab.migration;
    let count = |p: fn(&Destination) -> bool| m.all_destinations().filter(|(_, d)| p(d)).count();
    assert!(count(Destination::is_drop) > 0, "drop");
    assert!(count(|d| *d == Destination::Prose) > 0, "prose");
    assert!(
        count(|d| d.frontmatter_key().is_some()) > 0,
        "a plain key lifts into frontmatter"
    );
    assert!(
        count(|d| matches!(
            d,
            Destination::SourceAppend | Destination::SourceField { .. }
        )) > 0,
        "sources[…]"
    );
    assert!(
        count(|d| matches!(d, Destination::Nested { .. })) > 0,
        "generated.by / generated.at"
    );
}

#[test]
fn the_identity_fields_are_mapped_and_resource_comes_from_the_fence_id() {
    let vocab = vocab_or_skip!();
    let m = &vocab.migration;
    // `resource` is immutable and copied verbatim from the Class fence `@id`;
    // recomputing it from the label or filename re-points 412 classes.
    assert_eq!(
        m.class_fence.get("@id"),
        Some(&Destination::Key("resource".into())),
        "the Class fence @id is the resource, verbatim"
    );
    assert!(
        m.page_fence.contains_key("@id"),
        "the Page fence @id is accounted for"
    );
    assert_eq!(
        m.class_fence.get("relations"),
        Some(&Destination::PerPredicate)
    );
}

#[test]
fn the_relation_aliases_all_fold_onto_declared_relations() {
    let vocab = vocab_or_skip!();
    for (spelling, canonical) in &vocab.migration.relation_aliases {
        assert!(
            vocab.is_relation(canonical),
            "{spelling:?} folds onto {canonical:?}, which is not a relation"
        );
    }
}

#[test]
fn the_maturity_aliases_all_fold_onto_the_enum() {
    let vocab = vocab_or_skip!();
    let permitted = &vocab.scalars["maturity"].r#enum;
    for (spelling, canonical) in &vocab.migration.maturity_aliases {
        assert!(
            permitted.contains(canonical),
            "{spelling:?} folds onto {canonical:?}, which is not in the maturity enum"
        );
    }
}

#[test]
fn definition_placement_is_a_declared_decision() {
    let vocab = vocab_or_skip!();
    // Either answer is legal; what matters is that the vocabulary states one,
    // because the migration reads it rather than hard-coding a choice.
    let placement = vocab.migration.body.definition_placement();
    assert!(matches!(
        placement,
        DefinitionPlacement::Frontmatter | DefinitionPlacement::LeadingParagraph
    ));
    assert!(
        vocab.migration.body.definition_placement.is_some(),
        "the vocabulary must declare definition_placement explicitly"
    );
}

#[test]
fn block_references_declare_where_to_resolve_from() {
    let vocab = vocab_or_skip!();
    let refs = &vocab.migration.block_refs;
    assert!(
        !refs.resolve_from.is_empty(),
        "block references resolve from somewhere, or they cannot be inlined"
    );
    assert!(refs.max_lines > 0);
}

#[test]
fn the_intentional_deltas_are_declared() {
    let vocab = vocab_or_skip!();
    let deltas = &vocab.migration.intentional_deltas;
    assert!(
        !deltas.is_empty(),
        "parity is modulo these; they must be stated"
    );
    for needle in ["mature", "qualityScore", "provisional"] {
        assert!(
            deltas.iter().any(|d| d.contains(needle)),
            "no declared delta mentions {needle}: {deltas:#?}"
        );
    }
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
    assert_eq!(domain.roots.len(), 6, "the six taxonomic domain roots");
    assert!(domain.observed.len() >= 17, "{:?}", domain.observed.len());

    let known = domain.known_values();
    for value in [
        "artificial-intelligence",
        "blockchain",
        "economics",
        "machine-learning",
        "supply-chain",
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
