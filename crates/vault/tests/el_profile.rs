//! ADR-2126 Decision 2: the emitted OWL stays inside the EL profile.
//!
//! `turtle::build_graph` over a representative corpus — the golden fixture,
//! which carries existential restrictions, transitive properties, inverse
//! *hints* and the skos-stub long tail — must not contain one triple whose
//! predicate or object is a non-EL OWL construct. The list below is written out
//! independently of `vault_core::vocabulary::NON_EL_OWL_TERMS` on purpose: the
//! test is the specification, not a mirror of the implementation.

use std::path::{Path, PathBuf};

use vault::build::turtle::{build_graph, Graph, Term};
use vault::Corpus;
use vault_core::page::load_vault;
use vault_core::vocabulary::Vocabulary;

const OWL: &str = "http://www.w3.org/2002/07/owl#";

/// Constructs outside OWL 2 EL (or outside what Whelk implements) that the
/// emitter must never write.
const NON_EL: &[&str] = &[
    "allValuesFrom",
    "cardinality",
    "minCardinality",
    "maxCardinality",
    "qualifiedCardinality",
    "minQualifiedCardinality",
    "maxQualifiedCardinality",
    "hasValue",
    "oneOf",
    "propertyDisjointWith",
    "inverseOf",
    "complementOf",
    "SymmetricProperty",
    "unionOf",
];

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/fixture")
}

/// Every triple of `g` that names a non-EL construct as predicate or object.
fn non_el_triples(g: &Graph) -> Vec<String> {
    let forbidden: Vec<String> = NON_EL.iter().map(|t| format!("{OWL}{t}")).collect();
    g.iter()
        .filter(|(_, p, o)| {
            forbidden.iter().any(|f| f == p) || matches!(o, Term::Iri(i) if forbidden.contains(i))
        })
        .map(|(s, p, o)| format!("{s:?} {p} {o:?}"))
        .collect()
}

fn graph_of(root: &Path, public_only: bool) -> Graph {
    let vocab = Vocabulary::load(root.join("ontology/vocabulary.yaml")).expect("vocabulary");
    let vault = load_vault(root.join("knowledge")).expect("vault");
    let corpus = Corpus::build(&vault, &vocab);
    build_graph(&corpus, &vocab, public_only)
}

#[test]
fn the_golden_fixture_graph_contains_no_non_el_construct() {
    for public_only in [true, false] {
        let g = graph_of(&fixture_root(), public_only);
        // Positive control: the fixture really exercises the restriction and
        // characteristic branches, so an empty result is not vacuous.
        assert!(
            g.iter()
                .any(|(_, p, _)| p == format!("{OWL}someValuesFrom")),
            "fixture must emit at least one existential restriction"
        );
        assert!(
            g.iter().any(
                |(_, _, o)| matches!(o, Term::Iri(i) if *i == format!("{OWL}TransitiveProperty"))
            ),
            "fixture must emit at least one transitive property"
        );
        let bad = non_el_triples(&g);
        assert!(
            bad.is_empty(),
            "non-EL triples emitted:\n{}",
            bad.join("\n")
        );
    }
}

/// The real corpus root: `VAULT_CORPUS_DIR`, else the agentbox checkout.
fn corpus_dir() -> PathBuf {
    std::env::var_os("VAULT_CORPUS_DIR").map_or_else(
        || PathBuf::from("/home/devuser/workspace/visionGraph"),
        PathBuf::from,
    )
}

#[test]
fn the_real_corpus_graph_contains_no_non_el_construct() {
    let root = corpus_dir();
    let root = root.as_path();
    if !root.join("ontology/vocabulary.yaml").is_file() || !root.join("knowledge").is_dir() {
        eprintln!(
            "SKIP the_real_corpus_graph_contains_no_non_el_construct: no corpus at {} \
             (set VAULT_CORPUS_DIR to a visionGraph checkout)",
            root.display()
        );
        return;
    }
    let g = graph_of(root, true);
    assert!(
        g.len() > 1000,
        "real corpus graph is suspiciously small: {}",
        g.len()
    );
    let bad = non_el_triples(&g);
    assert!(
        bad.is_empty(),
        "non-EL triples emitted:\n{}",
        bad.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
    );
}
