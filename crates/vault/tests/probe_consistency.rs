//! ADR-2125 Decision item 3: the Whelk consistency gate on the `vault propose`
//! path is *provably armed*.
//!
//! The probe is the smallest EL++ inconsistency there is: `P ⊑ A`, `P ⊑ B`,
//! `A disjoint-with B`, with A and B siblings under one parent. Proposing the
//! second parent onto P must collapse P to `owl:Nothing`, and the real
//! `vault propose` assessment (`turtle::build_graph` then `whelk::reason`,
//! `crates/vault/src/propose.rs`) must report it as a `WHELK_INCONSISTENT`
//! blocker naming P. The control is the same corpus without the disjointness:
//! it must stay clean, so the probe proves the axiom is what arms the gate.
//!
//! The probe was red until disjointness landed on this path: the emitter now
//! writes `owl:disjointWith` and `vault::whelk` extracts it into
//! `DisjointClasses`. A probe that passes cleanly (no blocker) fails CI.
//!
//! The fixture lives here, never in `knowledge/`.

use vault::model::Corpus;
use vault::propose::{self, Options};
use vault_core::page::{Page, Vault, VaultKind};
use vault_core::proposal::{Level, PatchProposal};
use vault_core::vocabulary::Vocabulary;

/// The fixture vocabulary. `disjoint-with` is registered as the relation
/// ADR-2125 item 1 names; if the signed registration settles on a different
/// shape, this line follows it — the probe's assertions do not change.
const VOCABULARY: &str = r#"
version: 1
namespace: "urn:ngm:class:"
types:
  Class: { owl: "owl:Class", required: [resource, status] }
relations:
  is-a: { owl: "rdfs:subClassOf" }
  disjoint-with: { owl: "owl:disjointWith" }
scalars:
  quality: { type: number, min: 0, max: 1 }
"#;

const P_IRI: &str = "urn:ngm:class:p";

fn vocab() -> Vocabulary {
    Vocabulary::from_yaml_str(VOCABULARY).expect("the fixture vocabulary parses")
}

fn class(id: &str, extra: &str) -> String {
    format!(
        "---\ntype: Class\npublic: true\nresource: urn:ngm:class:{}\nstatus: stable\nquality: 0.35\n{extra}---\nbody\n",
        id.to_lowercase()
    )
}

fn vault_of(pages: &[(&str, String)]) -> Vault {
    Vault {
        root: std::path::PathBuf::new(),
        kind: VaultKind::Knowledge,
        journals: Vec::new(),
        skipped: Vec::new(),
        pages: pages
            .iter()
            .map(|(id, text)| {
                Page::parse(
                    format!("/v/pages/{id}.md"),
                    format!("pages/{id}.md"),
                    *id,
                    text,
                )
                .expect("fixture page parses")
            })
            .collect(),
    }
}

/// The base corpus: `Parent`, its children `A` and `B`, and `P ⊑ A`. When
/// `disjoint` is set, A carries `disjoint-with: [[B]]`.
fn base(disjoint: bool) -> Vault {
    let a_extra = if disjoint {
        "is-a: [\"[[Parent]]\"]\ndisjoint-with: [\"[[B]]\"]\n"
    } else {
        "is-a: [\"[[Parent]]\"]\n"
    };
    vault_of(&[
        ("Parent", class("Parent", "")),
        ("A", class("A", a_extra)),
        ("B", class("B", "is-a: [\"[[Parent]]\"]\n")),
        ("P", class("P", "is-a: [\"[[A]]\"]\n")),
    ])
}

/// Propose `P ⊑ A, P ⊑ B` through the real `vault propose` build.
fn propose_second_parent(vault: &Vault) -> PatchProposal {
    let vocab = vocab();
    let corpus = Corpus::build(vault, &vocab);
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("P.md");
    std::fs::write(&path, class("P", "is-a: [\"[[A]]\", \"[[B]]\"]\n")).expect("write P");
    let options = Options {
        subject: "P".into(),
        title: None,
        level: Level::Content,
        hypothesis: "P is both an A and a B".into(),
        diff_source: Some(path),
        proposer: "process:vault/probe".into(),
        now: time::OffsetDateTime::from_unix_timestamp(1_790_000_000).expect("timestamp"),
    };
    propose::build(vault, &corpus, &vocab, "g", &options).expect("the proposal builds")
}

#[test]
fn probe_disjoint_parents_block_with_whelk_inconsistent_naming_p() {
    let p = propose_second_parent(&base(true));
    // Whelk names classes by their emitted URI; accept that or the IRI.
    let p_uri = vault::build::turtle::iri_to_uri(P_IRI);
    let whelk: Vec<_> = p
        .blockers
        .iter()
        .filter(|b| b.code == "WHELK_INCONSISTENT")
        .collect();
    assert!(
        whelk
            .iter()
            .any(|b| b.detail.contains(P_IRI) || b.detail.contains(&p_uri)),
        "P ⊑ A, P ⊑ B, A disjoint-with B must block as WHELK_INCONSISTENT naming {P_IRI}; \
         blockers: {:?}",
        p.blockers
    );
    assert!(
        !p.is_postable(),
        "an inconsistent proposal is never postable"
    );
}

/// The control: without the disjointness the same change is consistent, so a
/// green probe is caused by the axiom, not by something else in the fixture.
#[test]
fn control_without_disjointness_has_no_whelk_inconsistent() {
    let p = propose_second_parent(&base(false));
    assert!(
        !p.blockers.iter().any(|b| b.code == "WHELK_INCONSISTENT"),
        "no disjointness ⇒ no WHELK_INCONSISTENT: {:?}",
        p.blockers
    );
    assert!(p.is_postable(), "{:?}", p.blockers);
}
