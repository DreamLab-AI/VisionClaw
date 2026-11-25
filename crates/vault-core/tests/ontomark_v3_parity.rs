//! VisionClaw reads vocabulary 3 headers the way ontomark does.
//!
//! The vault is read by two independent toolchains, so every rule that a
//! header carries can drift between them. This test pins the two together over
//! a shared fixture vault: ontomark's own vocabulary-3 fixture
//! (`crates/ontomark-cli/tests/vocab-v3-vault` at ontomark `v0.10.0`, vendored byte-for-byte; see
//! `tests/fixtures/ontomark-vocab-v3/README.md`) plus pages VisionClaw added for
//! the cases the fixture does not reach. `golden.json` holds **ontomark's**
//! answers for every page, produced by calling its predicates
//! (`is_public_under`, `Roster::resolve_principal`,
//! `OkfBlock::is_human_verified_under`, `is_recallable`), not by reading them
//! off the page. The test asserts VisionClaw's readers return the same.
//!
//! Where VisionClaw is deliberately different, the difference is named in
//! [`EXCEPTIONS`] with its reason; any other difference fails the build.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;
use vault_core::frontmatter::{self, Frontmatter};
use vault_core::lifecycle::is_recallable;
use vault_core::okf::OkfBlock;
use vault_core::roster::Roster;
use vault_core::visibility::{self, Rules};

/// A page where VisionClaw's answer for one field is intentionally not
/// ontomark's, with the reason. `(page, field)`.
const EXCEPTIONS: &[(&str, &str, &str)] = &[(
    "working/VcHumanName",
    "principal",
    "`human:jjohare` is a name, not a key. ontomark's reader leaves it unresolved until m0002 \
     rewrites it; VisionClaw resolves it through the roster label (as m0002 will), so the page \
     names the same principal before and after the migration.",
)];

/// Pages whose publication VisionClaw reads as NOT public where ontomark reads
/// them as public. `(page, vocabulary version, reason)`. The reverse (public in
/// VisionClaw, private in ontomark) is never allowed, with no exception list.
const FAIL_CLOSED: &[(&str, u32, &str)] = &[(
    "working/VcConflictPublic",
    3,
    "`public: false` with `visibility: public`: ontomark lets `visibility` decide; \
         VisionClaw fails closed and reports a conflict.",
)];

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ontomark-vocab-v3")
}

fn golden() -> Value {
    serde_json::from_str(&std::fs::read_to_string(fixture().join("golden.json")).unwrap()).unwrap()
}

/// Every page of the fixture, as `knowledge/Name` -> frontmatter. A page of
/// `working/journals/` is keyed by its date (`working/2026-11-25`), the id
/// ontomark's oracle and `Vault::load` both give a journal.
fn pages() -> BTreeMap<String, Frontmatter> {
    let mut out = BTreeMap::new();
    for (dir, sub) in [("knowledge", ""), ("working", ""), ("working", "journals")] {
        for entry in std::fs::read_dir(fixture().join(dir).join(sub)).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "md") {
                let text = std::fs::read_to_string(&path).unwrap();
                let fm = frontmatter::split(&text)
                    .yaml
                    .and_then(|y| Frontmatter::parse(y).ok())
                    .unwrap_or_default();
                let stem = path.file_stem().unwrap().to_string_lossy();
                out.insert(format!("{dir}/{stem}"), fm);
            }
        }
    }
    out
}

fn roster() -> Roster {
    Roster::load(&fixture()).unwrap()
}

fn is_exception(page: &str, field: &str) -> bool {
    EXCEPTIONS.iter().any(|(p, f, _)| *p == page && *f == field)
}

#[test]
fn the_upstream_fixture_is_vendored_unchanged() {
    use sha2::{Digest, Sha256};
    let manifest = std::fs::read_to_string(fixture().join("UPSTREAM.sha256")).unwrap();
    let mut checked = 0;
    for line in manifest.lines() {
        let (hash, path) = line.split_once("  ").unwrap();
        let bytes = std::fs::read(fixture().join(path)).unwrap();
        assert_eq!(
            hex::encode(Sha256::digest(&bytes)),
            hash,
            "{path} was edited"
        );
        checked += 1;
    }
    assert_eq!(checked, 39, "ontomark's fixture has 39 files");
}

#[test]
fn golden_covers_every_page_and_only_those() {
    let g = golden();
    let have: Vec<&str> = g["pages"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let want: Vec<String> = pages().into_keys().collect();
    assert_eq!(
        have, want,
        "re-run the oracle (see the README) after adding a page"
    );
}

#[test]
fn visibility_is_never_public_where_ontomark_keeps_a_page_private() {
    // Every page, under each vocabulary version, in its own vault: the reader
    // is given the rules ontomark's `is_public_under` applies, and must not
    // read as public anything ontomark does not. Where it reads LESS (the
    // fail-closed conflict) the page is named in FAIL_CLOSED.
    let g = golden();
    let mut compared = 0;
    for (id, fm) in pages() {
        for version in [2u32, 3] {
            let rules = Rules::for_path(&id, Some(version));
            let ours = visibility::is_public(&fm.map, rules);
            let want = g["pages"][&id][format!("v{version}")]["public"]
                .as_bool()
                .unwrap();
            assert!(
                !ours || want,
                "{id} under vocabulary {version}: VisionClaw reads it as public, ontomark does not"
            );
            if ours != want {
                assert!(
                    FAIL_CLOSED
                        .iter()
                        .any(|(p, v, _)| *p == id && *v == version),
                    "{id} under vocabulary {version}: ontomark says {want}, VisionClaw {ours}"
                );
            }
            compared += 1;
        }
    }
    assert_eq!(compared, pages().len() * 2);
}

#[test]
fn the_reviews_probe_pages_are_not_public_under_vocabulary_2() {
    // A working page with `public: false` + `visibility: public`, and one with
    // `visibility: public` and no `public`, in a vault whose vocabulary is 2.
    let fm = pages();
    for id in ["working/VcConflictPublic", "working/VcPublicV3"] {
        let rules = Rules::for_path(id, Some(2));
        assert_eq!(rules, Rules::PublicFlag);
        assert!(!visibility::is_public(&fm[id].map, rules), "{id}");
        assert!(!visibility::read(&fm[id].map, rules).public);
    }
    // Under vocabulary 3 the second is public and the first fails closed.
    assert!(visibility::is_public(
        &fm["working/VcPublicV3"].map,
        Rules::for_path("working/VcPublicV3", Some(3))
    ));
    let conflict = visibility::read(
        &fm["working/VcConflictPublic"].map,
        Rules::for_path("working/VcConflictPublic", Some(3)),
    );
    assert!(!conflict.public);
    assert_eq!(conflict.problem, Some(visibility::Problem::Conflict));
}

#[test]
fn a_knowledge_page_ignores_visibility_under_every_vocabulary() {
    let fm = pages();
    for version in [2u32, 3] {
        let rules = |id: &str| Rules::for_path(id, Some(version));
        let is = |id: &str| visibility::is_public(&fm[id].map, rules(id));
        assert!(!is("knowledge/VcKnowledgeVisibilityOnly"), "v{version}");
        assert!(!is("knowledge/VcKnowledgeVisibilityConflict"), "v{version}");
        assert!(is("knowledge/VcKnowledgePublicShared"), "v{version}");
        assert!(is("knowledge/VcKnowledgePublicAndVisible"), "v{version}");
    }
}

#[test]
fn a_page_answers_alike_in_either_form_where_ontomark_does() {
    // The same fact in both spellings is public in both tools, each under the
    // vocabulary that reads it.
    let fm = pages();
    assert!(visibility::is_public(
        &fm["working/VcPublicV2"].map,
        Rules::for_path("working/VcPublicV2", Some(2))
    ));
    assert!(visibility::is_public(
        &fm["working/VcPublicV3"].map,
        Rules::for_path("working/VcPublicV3", Some(3))
    ));
    let g = golden();
    assert!(g["pages"]["working/VcPublicV2"]["v2"]["public"]
        .as_bool()
        .unwrap());
    assert!(g["pages"]["working/VcPublicV3"]["v3"]["public"]
        .as_bool()
        .unwrap());
}

#[test]
fn principal_and_stamp_readability_match_ontomark() {
    let (g, roster) = (golden(), roster());
    for (id, fm) in pages() {
        let okf = OkfBlock::from_frontmatter(&fm);
        let want = &g["pages"][&id]["v3"];
        assert_eq!(
            okf.generated.is_some(),
            want["generated_readable"].as_bool().unwrap(),
            "{id}: generated readable"
        );
        assert_eq!(
            okf.verified.len() as u64,
            want["verified_readable"].as_u64().unwrap(),
            "{id}: verified entries readable"
        );
        let principal = okf
            .generated
            .as_ref()
            .and_then(|g| roster.principal_of(&g.by));
        if !is_exception(&id, "principal") {
            assert_eq!(
                principal.as_deref(),
                want["principal"].as_str(),
                "{id}: principal"
            );
        }
        let verified: Vec<Option<String>> = okf
            .verified
            .iter()
            .map(|s| roster.principal_of(&s.by))
            .collect();
        let want_verified: Vec<Option<String>> = want["verified_principals"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().map(str::to_owned))
            .collect();
        assert_eq!(verified, want_verified, "{id}: verified principals");
    }
}

#[test]
fn unreadable_stamps_are_surfaced_where_ontomark_cannot_read_them() {
    for (id, fm) in pages() {
        let okf = OkfBlock::from_frontmatter(&fm);
        let raw_verified = match fm.get("verified") {
            Some(serde_yaml::Value::Sequence(s)) => s.len(),
            Some(_) => 1,
            None => 0,
        };
        let unreadable = okf
            .problems
            .iter()
            .filter(|p| p.field != "sources[0]")
            .count();
        let expected = usize::from(fm.has("generated") && okf.generated.is_none())
            + (raw_verified - okf.verified.len());
        assert!(
            unreadable >= expected,
            "{id}: {expected} unreadable stamp(s) must each be listed, got {:?}",
            okf.problems
        );
    }
}

#[test]
fn human_certification_matches_ontomark() {
    let (g, roster) = (golden(), roster().under_vocabulary(Some(3)));
    for (id, fm) in pages() {
        let okf = OkfBlock::from_frontmatter(&fm);
        let want = g["pages"][&id]["v3"]["human_certified"].as_bool().unwrap();
        let ours = okf.is_human_verified_with(&roster);
        assert_eq!(ours, want, "{id}: ontomark {want}, VisionClaw {ours}");
    }
}

#[test]
fn the_old_human_spelling_certifies_under_version_2_only() {
    let (g, roster) = (golden(), roster().under_vocabulary(Some(2)));
    let all = pages();
    for (id, fm) in &all {
        let okf = OkfBlock::from_frontmatter(fm);
        let want = g["pages"][id]["v2"]["human_certified"].as_bool().unwrap();
        assert_eq!(okf.is_human_verified_with(&roster), want, "{id}: version 2");
    }
    // The two pages that carry only the `human:<npub>` spelling: certified under 2,
    // never under 3 (the golden above pins both from ontomark's own answer).
    for id in ["knowledge/HumanPrefixVerified", "knowledge/VcVerifiedNpub"] {
        assert_eq!(g["pages"][id]["v2"]["human_certified"], true, "{id}");
        assert_eq!(g["pages"][id]["v3"]["human_certified"], false, "{id}");
    }
}

/// What VisionClaw does with a journal named as a source target. ontomark
/// (v0.10.0) resolves `urn:visionflow:page:<id>` and `[[2026-11-25]]` to the
/// journal; VisionClaw has no source-target resolution at all. A journal is
/// `journals/` furniture: [`Vault::load`] keeps it in `journals`, outside the
/// ontology `pages`, so it has no id a link or an urn could reach, and nothing
/// in VisionClaw looks a `sources` entry up. The page that cites it is read as
/// any working page: both typed sources parse, with `resource`, `rel` and
/// `locator` kept as written.
#[test]
fn a_journal_is_furniture_and_a_source_citing_it_is_kept_not_resolved() {
    use vault_core::page::{Vault, VaultKind};
    let vault = Vault::load(fixture().join("working"), VaultKind::Working).unwrap();
    assert_eq!(
        vault
            .journals
            .iter()
            .map(|j| j.id.as_str())
            .collect::<Vec<_>>(),
        ["2026-11-25"],
        "the journal is read, in its own tree"
    );
    for id in ["2026-11-25", "journals/2026-11-25"] {
        assert!(vault.get(id).is_none(), "{id} is not an ontology page");
    }
    assert!(vault.get("JournalProposal").is_some());

    let fm = &pages()["working/JournalProposal"];
    let okf = OkfBlock::from_frontmatter(fm);
    assert!(okf.problems.is_empty(), "{:?}", okf.problems);
    let by_id: BTreeMap<_, _> = okf.sources.iter().map(|s| (s.id.as_str(), s)).collect();
    assert_eq!(by_id.len(), 2);
    assert_eq!(by_id["day"].rel.as_deref(), Some("origin"));
    assert!(by_id["day"]
        .resource
        .as_deref()
        .is_some_and(|r| r.starts_with("urn:visionflow:page:")));
    assert_eq!(by_id["day-link"].rel.as_deref(), Some("evidence"));
    assert_eq!(by_id["day-link"].locator.as_deref(), Some("line 1"));
    assert_eq!(by_id["day-link"].link().unwrap().target, "2026-11-25");
    // Neither the proposal nor the journal is public, as in ontomark's golden.
    for id in ["working/JournalProposal", "working/2026-11-25"] {
        let rules = Rules::for_path(id, Some(3));
        assert!(!visibility::is_public(&pages()[id].map, rules), "{id}");
    }
}

#[test]
fn recall_matches_ontomark() {
    let g = golden();
    let now = g["now"].as_i64().unwrap();
    for (id, fm) in pages() {
        let want = g["pages"][&id]["v3"]["recallable"].as_bool().unwrap();
        assert_eq!(is_recallable(&fm.map, now), want, "{id}");
    }
}

#[test]
fn the_exceptions_are_real_and_still_needed() {
    // An exception that no longer differs is dead weight; delete it.
    let (g, roster) = (golden(), roster());
    let all = pages();
    for (page, field, _reason) in EXCEPTIONS {
        assert_eq!(*field, "principal");
        let okf = OkfBlock::from_frontmatter(&all[*page]);
        let ours = okf
            .generated
            .as_ref()
            .and_then(|g| roster.principal_of(&g.by));
        assert_ne!(
            ours.as_deref(),
            g["pages"][*page]["v3"]["principal"].as_str(),
            "{page}"
        );
    }
}
