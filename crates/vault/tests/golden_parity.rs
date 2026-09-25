//! Golden parity: the migration must not change what Loom sees.
//!
//! The whole sovereign-corpus migration rests on one claim — folding the
//! `json-ld` fences into frontmatter is a *format* change, not a *content*
//! change. This test is that claim, executed: it takes 50 pages of the real
//! pre-migration corpus, runs `vault migrate --fences-to-properties` and
//! `vault build` over them, and compares the result against what the retired
//! Python pipeline produced from the same 50 pages.
//!
//! `scaffold-index.json` and `prose-index.json` must be **byte-identical**.
//! Loom's `loom-scaffold` loads the first of those directly, so a byte is a
//! contract.
//!
//! See `tests/golden/README.md` for how the reference output was produced.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use vault::{build, migrate};
use vault_core::vocabulary::Vocabulary;

/// The three documented divergences, stated once so a reader does not have to
/// infer them from assertion failures:
///
/// 1. `search-index.json`'s `labels` is a **superset**. Python read one
///    alternative label out of the fence's `vc:legacyProperties`; the migrated
///    corpus carries every alias, including the ones the migration derives
///    from reference labels so that `[[5G New Radio]]` still resolves.
/// 2. `ontology-inferred.ttl` is Whelk's EL++ closure, where Python emitted a
///    plain transitive `subClassOf` BFS. Contract C3 asks for the reasoner, so
///    this file is deliberately not compared.
/// 3. `prose-index.json`'s `cl` (Current Landscape) is a **superset**. Python
///    read the section only from a Logseq heading bullet
///    (`- ### Current Landscape`) and ended it at the next heading bullet.
///    `vault repair bodies` turns those bullets into markdown headings, so the
///    build reads both forms, ends the section at the next heading of its own
///    level or higher, and prefers the last section on a re-researched page.
///    Pages whose heading Python never saw gain a `cl`; no page loses one.
const DIVERGENCES: &str =
    "search-index labels (superset); ontology-inferred (Whelk, by contract); \
     prose-index cl (superset: markdown-form Current Landscape headings)";

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// Copy the fenced fixture into a scratch directory, migrate it, and build it.
fn migrate_and_build() -> (tempfile::TempDir, PathBuf) {
    let scratch = tempfile::tempdir().expect("scratch dir");
    let repo = scratch.path().join("repo");
    copy_tree(&golden_dir().join("fixture"), &repo);

    let vocab =
        Vocabulary::load(repo.join("ontology/vocabulary.yaml")).expect("fixture vocabulary");

    let report = migrate::run(
        &migrate::Options {
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
    .expect("migration runs");
    assert!(
        report.is_lossless(),
        "the fixture vocabulary must cover every fence key: {:?} / {:?}",
        report.uncovered_fence_fields,
        report.uncovered_logseq_keys
    );
    assert_eq!(report.pages_converted, 50);

    let out = scratch.path().join("out");
    build::run(
        &build::Options {
            vault_root: repo.join("knowledge"),
            out: out.clone(),
            repo_root: repo.clone(),
            with_rvdb: false,
            with_markdown_mirror: false,
            with_working_publish: false,
            publish_out: None,
            embed_endpoint: build::rvdb::DEFAULT_ENDPOINT.to_owned(),
            stale_after: None,
        },
        &vocab,
    )
    .expect("build succeeds on the migrated corpus");

    (scratch, out)
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("create target");
    for entry in std::fs::read_dir(from).expect("read source") {
        let entry = entry.expect("dir entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("copy file");
        }
    }
}

/// The `generated` timestamp, the one field that is allowed to move.
fn timestamp_re() -> &'static regex::Regex {
    static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r#""generated": ?"[^"]*""#).expect("static regex"))
}

/// A Logseq `key:: value` line — the residue the migration must remove.
fn logseq_re() -> &'static regex::Regex {
    static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        regex::Regex::new(r"(?m)^\s*(?:-\s*)?[A-Za-z0-9_.-]+::\s").expect("static regex")
    })
}

/// Every build-time timestamp, the only fields allowed to move.
fn timestamp_fields_re() -> &'static regex::Regex {
    static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        regex::Regex::new(r#""(generated|generatedAt|datasetDate)": ?"[^"]*""#)
            .expect("static regex")
    })
}

/// Blank the build-time timestamps so two builds can be compared byte for byte.
fn without_timestamp(s: &str) -> String {
    let once = timestamp_re().replace(s, r#""generated":"T""#).into_owned();
    timestamp_fields_re()
        .replace_all(&once, r#""$1":"T""#)
        .into_owned()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn assert_byte_identical(name: &str, golden: &Path, produced: &Path) {
    let expected = without_timestamp(&read(golden));
    let actual = without_timestamp(&read(produced));
    if expected == actual {
        return;
    }
    let at = expected
        .char_indices()
        .zip(actual.chars())
        .find(|((_, a), b)| a != b)
        .map_or(expected.len().min(actual.len()), |((i, _), _)| i);
    let window = |s: &str| {
        let start = at.saturating_sub(120);
        let end = (at + 120).min(s.len());
        s.get(start..end).unwrap_or(s).to_owned()
    };
    panic!(
        "{name} diverged from the Python pipeline at byte {at}\n  python: …{}…\n  vault:  …{}…\n\
         \n  (documented divergences: {DIVERGENCES})",
        window(&expected),
        window(&actual)
    );
}

/// The ground (blank-node-free) triple set, as `subject predicate object`
/// N-Triples lines. Blank nodes are excluded because their labels are
/// generated and carry no identity; the existential restrictions they belong
/// to are counted separately.
fn ground_triples(path: &Path) -> BTreeSet<String> {
    use oxttl::TurtleParser;

    let text = read(path);
    let mut out = BTreeSet::new();
    for triple in TurtleParser::new().for_reader(text.as_bytes()) {
        let triple = triple.expect("golden and produced turtle both parse");
        let subject = triple.subject.to_string();
        let object = triple.object.to_string();
        if subject.starts_with("_:") || object.starts_with("_:") {
            continue;
        }
        if subject.contains("narrativegoldmine.com/ontology") {
            continue; // the ontology header carries a build timestamp
        }
        out.insert(format!("{subject} {} {object}", triple.predicate));
    }
    out
}

fn blank_node_triples(path: &Path) -> usize {
    use oxttl::TurtleParser;

    let text = read(path);
    TurtleParser::new()
        .for_reader(text.as_bytes())
        .filter_map(Result::ok)
        .filter(|t| {
            t.subject.to_string().starts_with("_:") || t.object.to_string().starts_with("_:")
        })
        .count()
}

#[test]
fn scaffold_index_is_byte_identical_to_the_python_pipeline() {
    let (_scratch, out) = migrate_and_build();
    assert_byte_identical(
        "scaffold-index.json",
        &golden_dir().join("python/scaffold-index.json"),
        &out.join("data/scaffold-index.json"),
    );
}

#[test]
fn prose_index_matches_the_python_pipeline_but_for_landscape_superset() {
    let (_scratch, out) = migrate_and_build();
    let parse = |p: &Path| -> serde_json::Value {
        serde_json::from_str(&read(p)).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
    };
    let python = parse(&golden_dir().join("python/prose-index.json"));
    let vault = parse(&out.join("data/prose-index.json"));

    assert_eq!(
        python["counts"]["with_full_definition"], vault["counts"]["with_full_definition"],
        "definitions are not part of divergence 3 and must match exactly"
    );
    let count = |v: &serde_json::Value, k: &str| v["counts"][k].as_u64().unwrap_or(0);
    assert!(count(&vault, "with_landscape") >= count(&python, "with_landscape"));

    let (py_pages, vault_pages) = (
        python["pages"].as_object().expect("python pages"),
        vault["pages"].as_object().expect("vault pages"),
    );
    let without_cl = |v: &serde_json::Value| {
        let mut m = v.as_object().cloned().unwrap_or_default();
        m.remove("cl");
        m
    };
    for (slug, py) in py_pages {
        let ours = vault_pages
            .get(slug)
            .unwrap_or_else(|| panic!("{slug}: in python's prose index, missing from vault's"));
        assert_eq!(
            without_cl(py),
            without_cl(ours),
            "{slug}: a field other than cl diverged"
        );
        if py["cl"].as_str().is_some_and(|c| !c.is_empty()) {
            assert!(
                ours["cl"].as_str().is_some_and(|c| !c.is_empty()),
                "{slug}: python had a landscape and vault lost it ({DIVERGENCES})"
            );
        }
    }
    for (slug, ours) in vault_pages {
        if !py_pages.contains_key(slug) {
            assert!(
                without_cl(ours).is_empty(),
                "{slug}: an extra page may carry only a landscape ({DIVERGENCES})"
            );
        }
    }
}

#[test]
fn the_asserted_turtle_has_the_same_triples() {
    let (_scratch, out) = migrate_and_build();
    let golden = golden_dir().join("python/ontology.ttl");
    let produced = out.join("data/ontology.ttl");

    let expected = ground_triples(&golden);
    let actual = ground_triples(&produced);

    let missing: Vec<&String> = expected.difference(&actual).take(5).collect();
    let extra: Vec<&String> = actual.difference(&expected).take(5).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "turtle diverged: {} missing (first: {missing:?}), {} extra (first: {extra:?})",
        expected.difference(&actual).count(),
        actual.difference(&expected).count()
    );
    assert_eq!(
        blank_node_triples(&golden),
        blank_node_triples(&produced),
        "the existential restrictions must match in number"
    );
}

#[test]
fn the_search_index_matches_except_for_the_documented_label_superset() {
    let (_scratch, out) = migrate_and_build();
    let expected: Vec<serde_json::Value> =
        serde_json::from_str(&read(&golden_dir().join("python/search-index.json"))).expect("json");
    let actual: Vec<serde_json::Value> =
        serde_json::from_str(&read(&out.join("api/search-index.json"))).expect("json");

    assert_eq!(expected.len(), actual.len());
    for (e, a) in expected.iter().zip(&actual) {
        assert_eq!(e["id"], a["id"], "entry order must match");
        for key in [
            "title",
            "domain",
            "domain_name",
            "definition",
            "entityType",
            "qualityScore",
            "maturity",
            "iri",
            "is_subclass_of",
            "wikilinks",
        ] {
            assert_eq!(e[key], a[key], "{key} on {}", e["id"]);
        }
        // `labels` is a superset: every Python label must still be present.
        let python_labels = e["labels"].as_array().expect("labels array");
        let vault_labels = a["labels"].as_array().expect("labels array");
        for label in python_labels {
            assert!(
                vault_labels.contains(label),
                "{} lost the label {label} ({DIVERGENCES})",
                e["id"]
            );
        }
    }
}

#[test]
fn the_migrated_corpus_validates_clean() {
    let (scratch, _out) = migrate_and_build();
    let repo = scratch.path().join("repo");
    let vocab = Vocabulary::load(repo.join("ontology/vocabulary.yaml")).expect("vocabulary");
    let vault = vault_core::page::load_vault(repo.join("knowledge")).expect("load");
    let corpus = vault::Corpus::build(&vault, &vocab);
    let report = vault::validate::validate(&vault, &corpus, &vocab);
    assert!(
        report.errors().is_empty(),
        "migrated corpus has {} validation error(s); first: {}",
        report.errors().len(),
        report.errors()[0]
    );
}

#[test]
fn the_migration_leaves_no_residue() {
    let (scratch, _out) = migrate_and_build();
    let pages = scratch.path().join("repo/knowledge/pages");
    let mut checked = 0usize;
    for entry in std::fs::read_dir(&pages).expect("read pages") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let text = read(&path);
        let body = vault_core::frontmatter::split(&text).body;
        assert!(!text.contains("```json-ld"), "fence survived in {path:?}");
        assert!(!body.contains("{{embed"), "embed survived in {path:?}");
        assert!(
            !logseq_re().is_match(body),
            "logseq property survived in {path:?}"
        );
        checked += 1;
    }
    assert_eq!(checked, 50);
}

#[test]
fn the_build_is_reproducible() {
    let (_a, out_a) = migrate_and_build();
    let (_b, out_b) = migrate_and_build();
    for name in [
        "data/scaffold-index.json",
        "data/prose-index.json",
        "data/ontology.ttl",
        "api/search-index.json",
    ] {
        assert_eq!(
            without_timestamp(&read(&out_a.join(name))),
            without_timestamp(&read(&out_b.join(name))),
            "{name} is not reproducible"
        );
    }
}

// ── contract C3 amendment, 2026-09-22 ────────────────────────────────────
//
// The explorer's physics worker memory-maps the `.bin` tiers straight out of
// the published site, so "the same graph" is not good enough: the bytes must
// match. Two things made that achievable and are worth naming, because both
// are invisible until they bite:
//
//  * `overview.json`'s baked positions come out of **Python's** Mersenne
//    Twister. `crate::build::ngg1::PyRandom` reproduces MT19937 and
//    `random.uniform` so the layout is the same picture, not merely a similar
//    one.
//  * The layout must **not** use fused multiply-add. `a + b * c` rounds twice
//    in CPython and once under FMA; over 200 iterations that moved a
//    coordinate across a `round(x, 3)` boundary.

/// The NGG1 binary tiers, in emission order.
const BIN_TIERS: &[&str] = &[
    "full.bin",
    "domain-artificial-intelligence.bin",
    "domain-blockchain.bin",
    "domain-spatial-computing.bin",
    "domain-robotics.bin",
    "domain-distributed-collaboration.bin",
    "domain-infrastructure.bin",
];

fn assert_bytes_identical(name: &str, golden: &Path, produced: &Path) {
    let expected = std::fs::read(golden).unwrap_or_else(|e| panic!("{}: {e}", golden.display()));
    let actual = std::fs::read(produced).unwrap_or_else(|e| panic!("{}: {e}", produced.display()));
    assert_eq!(
        expected.len(),
        actual.len(),
        "{name}: {} bytes from the Python pipeline, {} from vault",
        expected.len(),
        actual.len()
    );
    if let Some(at) = expected.iter().zip(&actual).position(|(a, b)| a != b) {
        panic!(
            "{name}: first differing byte at offset {at} (python {:#04x}, vault {:#04x})",
            expected[at], actual[at]
        );
    }
}

#[test]
fn the_ngg1_binary_tiers_are_byte_identical() {
    let (_scratch, out) = migrate_and_build();
    for tier in BIN_TIERS {
        assert_bytes_identical(
            tier,
            &golden_dir().join("python/graph").join(tier),
            &out.join("data/graph").join(tier),
        );
    }
}

#[test]
fn every_bin_tier_carries_the_ngg1_header() {
    let (_scratch, out) = migrate_and_build();
    for tier in BIN_TIERS {
        let bytes = std::fs::read(out.join("data/graph").join(tier)).expect("tier");
        assert_eq!(&bytes[0..4], b"NGG1", "{tier} magic");
        assert_eq!(
            u16::from_le_bytes([bytes[4], bytes[5]]),
            1,
            "{tier} version"
        );
        let nodes = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let off_adjacency = u32::from_le_bytes(bytes[20..24].try_into().unwrap()) as usize;
        assert_eq!(off_adjacency, 32 + nodes * 24, "{tier} node stride");
    }
}

#[test]
fn stats_json_is_byte_identical() {
    let (_scratch, out) = migrate_and_build();
    assert_byte_identical(
        "graph/stats.json",
        &golden_dir().join("python/graph/stats.json"),
        &out.join("data/graph/stats.json"),
    );
}

#[test]
fn overview_and_bridges_are_byte_identical() {
    let (_scratch, out) = migrate_and_build();
    // Contract C3 asks only for JSON-equality on these two; they come out
    // byte-identical, so that is what is asserted.
    for name in ["overview.json", "bridges.json"] {
        assert_byte_identical(
            name,
            &golden_dir().join("python/graph").join(name),
            &out.join("data/graph").join(name),
        );
    }
}

#[test]
fn the_webvowl_graph_is_byte_identical() {
    let (_scratch, out) = migrate_and_build();
    assert_byte_identical(
        "ontology.json",
        &golden_dir().join("python/ontology.json"),
        &out.join("data/ontology.json"),
    );
}

#[test]
fn the_context_ships_at_both_served_paths() {
    let (_scratch, out) = migrate_and_build();
    let v1 = read(&out.join("context/v1.jsonld"));
    let v2 = read(&out.join("ns/v2.jsonld"));
    assert_eq!(v1, v2, "the two paths serve the same document");
    // The old site's pinned URL, required by the publish workflow.
    let schema = read(&out.join("api/schema/context.jsonld"));
    assert_eq!(
        schema, v2,
        "/api/schema/context.jsonld is the same document"
    );
    // The served path is /ns/v2.jsonld; the property IRIs inside still cite
    // narrativegoldmine.com/ns/v1#.
    assert!(
        v2.contains("https://narrativegoldmine.com/ns/v1#"),
        "the v1 property namespace must survive at the v2 path"
    );
}

#[test]
fn the_markdown_mirror_is_opt_in() {
    let (scratch, out) = migrate_and_build();
    assert!(
        !out.join("api/markdown").exists(),
        "124 MB with no consumer must not ship by default"
    );

    let repo = scratch.path().join("repo");
    let vocab = Vocabulary::load(repo.join("ontology/vocabulary.yaml")).expect("vocabulary");
    let mirrored = scratch.path().join("out-mirrored");
    build::run(
        &build::Options {
            vault_root: repo.join("knowledge"),
            out: mirrored.clone(),
            repo_root: repo,
            with_rvdb: false,
            with_markdown_mirror: true,
            with_working_publish: false,
            publish_out: None,
            embed_endpoint: build::rvdb::DEFAULT_ENDPOINT.to_owned(),
            stale_after: None,
        },
        &vocab,
    )
    .expect("build with the mirror");
    // Every indexed page's body is reachable by the TITLE the explorer
    // requests, and by its page id; a page whose title differs from its file
    // name therefore has two files.
    let index: serde_json::Value = serde_json::from_slice(
        &std::fs::read(mirrored.join("api/search-index.json")).expect("search index"),
    )
    .expect("search index JSON");
    let entries = index.as_array().expect("an array");
    assert_eq!(entries.len(), 50);
    for entry in entries {
        let title = entry["title"].as_str().expect("title").replace('/', "___");
        assert!(
            mirrored.join(format!("api/markdown/{title}.md")).is_file(),
            "no mirror file for the title {title}"
        );
    }
}

#[test]
fn the_graph_tiers_agree_with_the_class_count() {
    let (_scratch, out) = migrate_and_build();
    let stats: serde_json::Value =
        serde_json::from_str(&read(&out.join("data/graph/stats.json"))).expect("stats json");
    let scaffold: serde_json::Value =
        serde_json::from_str(&read(&out.join("data/scaffold-index.json"))).expect("scaffold json");
    assert_eq!(
        stats["classes"], scaffold["counts"]["classes"],
        "stats.json and scaffold-index.json must report the same class count"
    );
    assert_eq!(stats["domains"], 6);
    assert_eq!(stats["categories"], 34);
}
