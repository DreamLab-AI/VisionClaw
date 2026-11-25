//! What `vault validate` does with a journal named as a source target.
//!
//! ontomark 0.10.0 indexes `journals/` as source targets: a `sources` entry
//! whose `resource` is a journal's page urn or `[[YYYY-MM-DD]]` resolves, and a
//! date nobody holds is `DANGLING_SOURCE`. VisionClaw has no such notion. It
//! does not resolve `sources` targets at all (its link-integrity check covers
//! the vocabulary's relation keys only), so a citation of a journal is neither
//! confirmed nor refused: the journal is read as `journals/` furniture, the
//! citing page as an ordinary working page. The one finding either draws is
//! `MISSING_DOMAIN` on the proposal (no `domain:`, VisionClaw's own rule, not
//! about the journal); neither draws a source finding. This test pins that, over ontomark's own fixture pages `JournalProposal` and
//! `journals/2026-11-25`, so a resolver added later has to change it knowingly.

use std::path::{Path, PathBuf};

use vault::{validate, Corpus};
use vault_core::page::{Vault, VaultKind};
use vault_core::roster::Roster;
use vault_core::vocabulary::Vocabulary;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../vault-core/tests/fixtures/ontomark-vocab-v3")
}

#[test]
fn a_journal_and_the_page_citing_it_draw_no_source_finding() {
    let root = fixture();
    let vocab = Vocabulary::load(root.join("ontology/vocabulary.yaml")).unwrap();
    let roster = Roster::load(&root).unwrap();
    let vault = Vault::load(root.join("working"), VaultKind::Working).unwrap();
    assert_eq!(vault.journals.len(), 1, "the journal is read");
    let corpus = Corpus::build(&vault, &vocab);
    let report = validate::validate_with(&vault, &corpus, &vocab, &roster);
    let about: Vec<(&str, &str)> = report
        .issues
        .iter()
        .filter(|i| i.path == "JournalProposal" || i.path == "2026-11-25")
        .map(|i| (i.path.as_str(), i.code.as_str()))
        .collect();
    assert_eq!(about, [("JournalProposal", "MISSING_DOMAIN")]);
    assert!(
        report.issues.iter().all(|i| !i.code.contains("SOURCE")),
        "VisionClaw has no source-target check: {:?}",
        report.issues
    );
}
