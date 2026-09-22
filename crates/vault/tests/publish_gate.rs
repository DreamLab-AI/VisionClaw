//! The publish gate, end to end: a credential on a page selected for
//! publication refuses the whole build and writes nothing.
//!
//! The unit tests in `build::publish` prove the staging logic. This one proves
//! the *consequence* — that `build::run` propagates the refusal, that the error
//! is the typed one the CLI turns into exit 2, and that a failed build leaves
//! the output tree absent rather than half-written. A gate that refuses but has
//! already written the bytes is not a gate.
//!
//! Every credential here is synthetic and has never been live.

use std::path::{Path, PathBuf};

use vault::build;
use vault_core::vocabulary::Vocabulary;

/// A synthetic OpenAI-shaped key. Not a real credential.
const SYNTHETIC_KEY: &str = "sk-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/fixture")
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("create destination");
    for entry in walkdir::WalkDir::new(from) {
        let entry = entry.expect("walk the fixture");
        let rel = entry.path().strip_prefix(from).expect("relative path");
        let target = to.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target).expect("create directory");
        } else {
            std::fs::copy(entry.path(), &target).expect("copy file");
        }
    }
}

/// Migrate the golden fixture into a scratch repo and return `(scratch, repo)`.
fn migrated_repo() -> (tempfile::TempDir, PathBuf) {
    let scratch = tempfile::tempdir().expect("scratch dir");
    let repo = scratch.path().join("repo");
    copy_tree(&fixture(), &repo);
    let vocab = Vocabulary::load(repo.join("ontology/vocabulary.yaml")).expect("vocabulary");
    vault::migrate::run(
        &vault::migrate::Options {
            scopes: vec![vault::migrate::Scope {
                vault: "knowledge".to_owned(),
                dir: repo.join("knowledge/pages"),
                journals: false,
            }],
            repo_root: repo.clone(),
            allow_type_fallback: false,
            dry_run: false,
            now: "2026-09-22T00:00:00Z".to_owned(),
        },
        &vocab,
        false,
    )
    .expect("the fixture migrates");
    (scratch, repo)
}

fn options(repo: &Path, out: &Path) -> build::Options {
    build::Options {
        vault_root: repo.join("knowledge"),
        out: out.to_path_buf(),
        repo_root: repo.to_path_buf(),
        with_rvdb: false,
        with_markdown_mirror: false,
        with_working_publish: false,
        publish_out: None,
        embed_endpoint: build::rvdb::DEFAULT_ENDPOINT.to_owned(),
        stale_after: None,
    }
}

/// The first `public: true` page in the migrated knowledge vault.
fn a_public_page(repo: &Path) -> PathBuf {
    let pages = repo.join("knowledge/pages");
    let mut candidates: Vec<PathBuf> = vault_core::page::page_files(&pages)
        .expect("walk the pages")
        .into_iter()
        .filter(|p| {
            std::fs::read_to_string(p)
                .map(|t| t.contains("public: true"))
                .unwrap_or(false)
        })
        .collect();
    candidates.sort();
    candidates.pop().expect("the fixture has a public page")
}

#[test]
fn the_fixture_builds_and_stages_its_public_pages() {
    let (scratch, repo) = migrated_repo();
    let out = scratch.path().join("out");
    let vocab = Vocabulary::load(repo.join("ontology/vocabulary.yaml")).expect("vocabulary");
    let report = build::run(&options(&repo, &out), &vocab).expect("a clean fixture builds");

    // The publish staging exists, is counted per vault, and is non-empty.
    let counts = &report.published["knowledge"];
    assert!(counts.published > 0, "{counts:?}");
    assert!(counts.published <= counts.considered, "{counts:?}");
    assert!(
        out.join("publish/index.md").is_file(),
        "the OKF §8 index is always written"
    );
    let index = std::fs::read_to_string(out.join("publish/index.md")).unwrap();
    assert!(
        index.contains(&format!("published_count: {}", counts.published)),
        "{index}"
    );
}

#[test]
fn a_credential_on_a_published_page_refuses_the_build_and_writes_nothing() {
    let (scratch, repo) = migrated_repo();
    let page = a_public_page(&repo);
    let original = std::fs::read_to_string(&page).expect("read the page");
    std::fs::write(&page, format!("{original}\nleaked: {SYNTHETIC_KEY}\n")).expect("plant the key");

    let out = scratch.path().join("out");
    let vocab = Vocabulary::load(repo.join("ontology/vocabulary.yaml")).expect("vocabulary");
    let error = build::run(&options(&repo, &out), &vocab).expect_err("the build must refuse");

    // The typed error, which is what the CLI turns into exit 2.
    let secrets = error
        .downcast_ref::<build::publish::SecretsFound>()
        .unwrap_or_else(|| panic!("expected SecretsFound, got: {error:#}"));
    assert_eq!(secrets.findings.len(), 1, "{:?}", secrets.findings);
    assert_eq!(
        secrets.findings[0].kind,
        vault_core::secrets::CredentialKind::OpenAiKey
    );

    // The value never appears in the message.
    let message = secrets.to_string();
    assert!(!message.contains("AAAA"), "{message}");
    assert!(message.contains("openai_key"), "{message}");

    // And nothing was written: a failed build leaves the previous bundle
    // intact, so a re-publish cannot pick up a partial tree.
    assert!(
        !out.exists(),
        "the refused build must not leave an output tree behind"
    );
}

#[test]
fn a_credential_on_a_private_page_does_not_refuse_the_build() {
    let (scratch, repo) = migrated_repo();
    // Hold one page back, then plant the same key in it. The fixture is 50
    // public pages, so the private one has to be made.
    let private = a_public_page(&repo);
    let original = std::fs::read_to_string(&private).expect("read");
    let held = original.replacen("public: true", "public: false", 1);
    assert_ne!(
        held, original,
        "the page must have been public to hold back"
    );
    std::fs::write(&private, format!("{held}\nleaked: {SYNTHETIC_KEY}\n")).expect("plant");

    let out = scratch.path().join("out");
    let vocab = Vocabulary::load(repo.join("ontology/vocabulary.yaml")).expect("vocabulary");
    // `vault validate` reports it as an error, so the build still refuses —
    // but on the VALIDATION error, not the publish gate. The distinction
    // matters: the publish gate is about what leaves the machine.
    let error = build::run(&options(&repo, &out), &vocab).expect_err("validation still refuses");
    assert!(
        error
            .downcast_ref::<build::publish::SecretsFound>()
            .is_none(),
        "a private page must not trip the PUBLISH gate: {error:#}"
    );
    assert!(
        error.to_string().contains("validation rejected"),
        "{error:#}"
    );
}

#[test]
fn a_build_reports_what_it_wrote() {
    // "Nothing built" must be distinguishable from "built successfully". The
    // CLI turns `written == 0` into a non-zero exit; the library reports the
    // count so it can.
    let (scratch, repo) = migrated_repo();
    let out = scratch.path().join("out");
    let vocab = Vocabulary::load(repo.join("ontology/vocabulary.yaml")).expect("vocabulary");
    let report = build::run(&options(&repo, &out), &vocab).expect("builds");
    assert!(report.written > 0, "a real build writes artefacts");
    // Every counted artefact is on disk.
    let on_disk = walkdir::WalkDir::new(&out)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_file())
        .count();
    assert_eq!(
        report.written, on_disk,
        "the reported count must be the number of files actually written"
    );
}

#[test]
fn publish_out_is_counted_and_lands_outside_the_bundle() {
    let (scratch, repo) = migrated_repo();
    let out = scratch.path().join("out");
    let published = scratch.path().join("published");
    let vocab = Vocabulary::load(repo.join("ontology/vocabulary.yaml")).expect("vocabulary");
    let mut opts = options(&repo, &out);
    opts.publish_out = Some(published.clone());
    let report = build::run(&opts, &vocab).expect("builds");

    assert!(report.written > 0);
    // The markdown went to `--publish-out`, not into the bundle.
    assert!(
        !out.join("publish").exists(),
        "publish/ must not be in the bundle"
    );
    assert!(published.join("index.md").is_file());
}

/// The `--publish-out` tree is the published site's URL contract (C3):
/// `pages/**` for knowledge (Quartz slug `pages/<Title>`), `working/**` keeping
/// subdirectories, and the OKF home page at the root. A `misc/` scratch page is
/// held back however it is flagged.
#[test]
fn publish_out_matches_the_site_url_contract() {
    let (scratch, repo) = migrated_repo();
    let public_title = a_public_page(&repo)
        .file_name()
        .expect("a file name")
        .to_owned();
    let note = "---\ntype: Note\nstatus: draft\npublic: true\n---\nA public working note.\n";
    for rel in ["podcast-evidence/Episode One.md", "misc/Scratch Note.md"] {
        let path = repo.join("working/pages").join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, note).unwrap();
    }
    let out = scratch.path().join("out");
    let published = scratch.path().join("published");
    let vocab = Vocabulary::load(repo.join("ontology/vocabulary.yaml")).expect("vocabulary");
    let mut opts = options(&repo, &out);
    opts.publish_out = Some(published.clone());
    opts.with_working_publish = true;
    build::run(&opts, &vocab).expect("builds");

    assert!(published.join("pages").join(&public_title).is_file());
    assert!(published
        .join("working/podcast-evidence/Episode One.md")
        .is_file());
    assert!(published.join("index.md").is_file());
    assert!(!published.join("knowledge").exists());
    assert!(!published.join("working/misc").exists());
    assert!(!published.join("working/Episode One.md").exists());
}
