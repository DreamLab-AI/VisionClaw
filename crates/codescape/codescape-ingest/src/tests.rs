//! Crate-level tests: isolation, golden fixture, incremental patching,
//! watching, the 50k-node timing run and the dogfood smoke test.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use codescape_model::{Confidence, IdInterner, Lang, NodeKind, Relation, Tier};

use super::*;

// ---------------------------------------------------------------- isolation

/// Every graphicle path this crate may name. Anything else fails the build.
const ALLOWED: &[&str] = &[
    "VERSION",
    "UPSTREAM_COMMIT",
    "affected::DEFAULT_AFFECTED_RELATIONS",
    "affected::affected_nodes",
    "build::Diagnostics",
    "build::build_from_json",
    "build::is_ast_tier",
    "build::is_file_node_label",
    "cache::grammar_fingerprint",
    "cluster::cluster",
    "detect::EncodingWarnings",
    "detect::FileType",
    "detect::IgnoreState",
    "detect::classify_file",
    "extract::ExtractOptions",
    "extract::collect_files",
    "extract::extract",
    "extract::extract_with_report",
    "extract::get_extractor",
    "graph::Graph",
    "paths::OUT_DIR",
    "record::Attributes",
    "watch::build_excludes",
    "watch::build_gitignore",
    "watch::write_config",
];

/// graphicle modules that must never be reached from Codescape (ADR-2143 §2).
/// Spelled in pieces so this list does not match itself.
const FORBIDDEN: &[&str] = &[
    concat!("ins", "tall"),
    concat!("ho", "oks"),
    concat!("ll", "m"),
    concat!("exporters", "::", "graphdb"),
    concat!("google", "_workspace"),
    concat!("disp", "atch"),
];

/// `graphicle::<path>` uses in `src`, with the path's identifier chain.
fn graphicle_paths(src: &str) -> Vec<String> {
    let needle = concat!("graph", "icle::");
    let mut out = Vec::new();
    let mut rest = src;
    while let Some(at) = rest.find(needle) {
        let before = rest[..at].chars().next_back();
        rest = &rest[at + needle.len()..];
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let path: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':')
            .collect();
        let path = path.trim_end_matches(':').to_string();
        out.push(path);
    }
    out
}

#[test]
fn only_allowlisted_graphicle_paths_are_used() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut used = BTreeSet::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(
                !text.contains(concat!("use graph", "icle::{")),
                "{}: grouped graphicle imports hide paths from this check",
                path.display()
            );
            for p in graphicle_paths(&text) {
                if p.is_empty() {
                    continue;
                }
                // An allowed type also allows its associated items (`Graph::new`).
                let allowed = ALLOWED.iter().any(|a| {
                    p == *a || p.strip_prefix(a).is_some_and(|rest| rest.starts_with("::"))
                });
                assert!(
                    allowed,
                    "{}: graphicle::{p} is not on the allowlist",
                    path.display()
                );
                for f in FORBIDDEN {
                    assert!(
                        !p.split("::").any(|seg| seg.starts_with(f)) && !p.starts_with(f),
                        "{}: forbidden graphicle path {p}",
                        path.display()
                    );
                }
                used.insert(p);
            }
        }
    }
    for allowed in ALLOWED {
        assert!(
            FORBIDDEN.iter().all(|f| !allowed.starts_with(f)),
            "allowlist entry {allowed} is forbidden"
        );
    }
    assert!(
        used.contains("extract::extract_with_report"),
        "the scan found nothing: {used:?}"
    );
}

#[test]
fn path_scanner_sees_every_form() {
    let src = concat!(
        "let a = graph",
        "icle::cluster::cluster(&g);\nlet b: graph",
        "icle::graph::Graph;\nnotgraph",
        "icle::x::y\n"
    );
    assert_eq!(
        graphicle_paths(src),
        vec!["cluster::cluster", "graph::Graph"]
    );
}

// ---------------------------------------------------------------- fixtures

pub(crate) fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// A small repository with known structure in three languages, a README,
/// a gitignored directory and an excluded one.
fn fixture(root: &Path) {
    write(
        root,
        "src/lib.rs",
        "mod util;\nuse crate::util::helper;\n\npub struct Engine { n: u32 }\n\nimpl Engine {\n    pub fn new() -> Self { Engine { n: helper(1) } }\n    pub fn run(&self) -> u32 { self.step() + helper(self.n) }\n    fn step(&self) -> u32 { 2 }\n}\n\npub fn start() -> u32 { Engine::new().run() }\n",
    );
    write(
        root,
        "src/util.rs",
        "pub fn helper(x: u32) -> u32 { x + inner() }\nfn inner() -> u32 { 1 }\n",
    );
    write(
        root,
        "web/app.ts",
        "import { format } from './fmt';\nexport class App {\n  render(): string { return format(this.title()); }\n  title(): string { return 'x'; }\n}\nexport function boot(): string { return new App().render(); }\n",
    );
    write(
        root,
        "web/fmt.ts",
        "export function format(s: string): string { return s.trim(); }\n",
    );
    write(
        root,
        "py/main.py",
        "from py.tools import clean\n\nclass Runner:\n    def go(self):\n        return clean(self.name())\n    def name(self):\n        return \"n\"\n\ndef main():\n    return Runner().go()\n",
    );
    write(root, "py/tools.py", "def clean(s):\n    return s.strip()\n");
    write(root, "README.md", "# Fixture\n\nSee `start`.\n");
    write(root, ".gitignore", "gen/\n");
    write(root, "gen/skip.rs", "pub fn generated() {}\n");
    write(root, "vendor/third.rs", "pub fn vendored() {}\n");
}

fn config(root: &Path, out: &Path) -> IngestConfig {
    let mut cfg = IngestConfig::new("fixture", root, out);
    cfg.excludes = vec!["vendor/".into()];
    cfg
}

/// Every file under `root` with its size and modification time.
fn tree_state(root: &Path) -> BTreeMap<PathBuf, (u64, std::time::SystemTime)> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap() {
            let e = e.unwrap();
            let meta = e.metadata().unwrap();
            if meta.is_dir() {
                stack.push(e.path());
            }
            out.insert(e.path(), (meta.len(), meta.modified().unwrap()));
        }
    }
    out
}

#[cfg(unix)]
fn set_read_only(root: &Path, read_only: bool) {
    use std::os::unix::fs::PermissionsExt;
    let mut dirs = vec![root.to_path_buf()];
    let mut all = Vec::new();
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                dirs.push(p.clone());
            }
            all.push(p);
        }
        all.push(d);
    }
    for p in all {
        let mode = match (p.is_dir(), read_only) {
            (true, true) => 0o555,
            (true, false) => 0o755,
            (false, true) => 0o444,
            (false, false) => 0o644,
        };
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
    }
}

fn id_of(ing: &Ingestor, key: &str) -> u32 {
    ing.interner()
        .get(key)
        .unwrap_or_else(|| panic!("no node {key}"))
}

fn node<'a>(ing: &'a Ingestor, key: &str) -> &'a codescape_model::CodeNode {
    let id = id_of(ing, key);
    ing.snapshot()
        .node(id)
        .unwrap_or_else(|| panic!("{key} interned but not in the snapshot"))
}

fn has_edge(ing: &Ingestor, from: &str, to: &str, rel: Relation) -> bool {
    let (s, t) = (id_of(ing, from), id_of(ing, to));
    ing.snapshot()
        .edges
        .iter()
        .any(|e| e.source == s && e.target == t && e.relation == rel)
}

fn edge_keys(ing: &Ingestor) -> BTreeSet<(String, String, String)> {
    let key = |id: u32| ing.interner().key_of(id).unwrap().to_string();
    ing.snapshot()
        .edges
        .iter()
        .map(|e| {
            (
                key(e.source),
                key(e.target),
                e.relation.as_str().to_string(),
            )
        })
        .collect()
}

// ---------------------------------------------------------------- golden

#[test]
fn golden_fixture_maps_structure_and_loads_from_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let out = tmp.path().join("out");
    fixture(&root);
    let before = tree_state(&root);
    #[cfg(unix)]
    set_read_only(&root, true);

    let (ing, report) = Ingestor::open(config(&root, &out), IdInterner::new(1)).unwrap();
    println!("golden report: {report:?}");
    assert!(!report.from_cache);
    assert_eq!(
        report.files, 6,
        "README, gen/ (gitignored) and vendor/ (excluded) are out"
    );
    assert_eq!(report.languages.get(&Lang::Rust), Some(&2));
    assert_eq!(report.languages.get(&Lang::TypeScript), Some(&2));
    assert_eq!(report.languages.get(&Lang::Python), Some(&2));
    assert_eq!(report.nodes, ing.snapshot().nodes.len());
    let snap = ing.snapshot();
    snap.validate().unwrap();
    assert_eq!(snap.revision, 1);
    assert_eq!(&*snap.repo, "fixture");
    for n in &snap.nodes {
        let p = n.path.as_deref().unwrap_or("");
        assert!(
            !p.ends_with(".md") && !p.starts_with("gen/") && !p.starts_with("vendor/"),
            "leaked {p}"
        );
        assert!(
            (0.0..=1.0).contains(&n.importance),
            "{} importance {}",
            n.key,
            n.importance
        );
    }
    assert!(
        snap.nodes.iter().any(|n| n.importance == 1.0),
        "betweenness is normalised to a maximum of 1"
    );

    // Kinds, labels, languages.
    assert_eq!(node(&ing, "src_lib").kind, NodeKind::File);
    assert_eq!(&*node(&ing, "src_lib").label, "lib.rs");
    assert_eq!(node(&ing, "src_lib").size, 12);
    assert_eq!(node(&ing, "src_lib_engine").kind, NodeKind::Struct);
    assert_eq!(node(&ing, "src_lib_engine_run").kind, NodeKind::Method);
    assert_eq!(&*node(&ing, "src_lib_engine_run").label, "run");
    assert_eq!(node(&ing, "src_lib_start").kind, NodeKind::Function);
    assert_eq!(node(&ing, "web_app_app").kind, NodeKind::Class);
    assert_eq!(node(&ing, "web_app_app").lang, Lang::TypeScript);
    assert_eq!(node(&ing, "py_main_runner").kind, NodeKind::Class);
    assert_eq!(node(&ing, "py_main_runner_go").kind, NodeKind::Method);
    assert_eq!(node(&ing, "web_fmt_format").kind, NodeKind::Function);

    // Spans: start lines from graphicle, ends derived from the next non-member symbol.
    let span = |k: &str| node(&ing, k).span.map(|s| (s.start_line, s.end_line));
    assert_eq!(
        span("src_lib_engine"),
        Some((4, 11)),
        "a struct spans its impl block's methods"
    );
    assert_eq!(span("src_lib_engine_run"), Some((8, 8)));
    assert_eq!(span("src_lib_start"), Some((12, 12)));
    assert_eq!(span("src_lib"), Some((1, 12)));

    // Parents and tiers: symbol → file → top-level directory.
    let lib = id_of(&ing, "src_lib");
    assert_eq!(
        node(&ing, "src_lib_engine_run").parent,
        lib,
        "methods hang off their file, not their class"
    );
    assert_eq!(node(&ing, "src_lib_start").parent, lib);
    let dir = snap.node(node(&ing, "src_lib").parent).unwrap();
    assert_eq!(dir.kind, NodeKind::Directory);
    assert_eq!(dir.tier, Tier::L0);
    assert_eq!(&*dir.label, "src");
    assert_eq!(dir.size, 2);
    assert_eq!(node(&ing, "src_lib").tier, Tier::L1);
    assert_eq!(node(&ing, "src_lib_start").tier, Tier::L2);
    assert_eq!(snap.tier_counts()[0], 3, "src, web, py");
    assert_eq!(node(&ing, "src_lib_start").cluster, dir.cluster);

    // Edges.
    assert!(has_edge(
        &ing,
        "src_lib_start",
        "src_lib_engine_run",
        Relation::Calls
    ));
    assert!(has_edge(
        &ing,
        "src_lib_engine",
        "src_lib_engine_run",
        Relation::Method
    ));
    assert!(has_edge(
        &ing,
        "py_main_runner_go",
        "py_tools_clean",
        Relation::Calls
    ));
    assert!(has_edge(&ing, "web_app", "web_fmt", Relation::ImportsFrom));
    assert!(has_edge(
        &ing,
        "web_app_app_render",
        "web_fmt_format",
        Relation::Calls
    ));
    assert!(
        has_edge(&ing, "src_lib", "src_lib_engine_run", Relation::Contains),
        "synthetic file → method containment"
    );
    let inferred = snap
        .edges
        .iter()
        .find(|e| {
            e.source == id_of(&ing, "src_lib_engine_new")
                && e.target == id_of(&ing, "src_util_helper")
        })
        .unwrap();
    assert_eq!(inferred.confidence, Confidence::Inferred);
    let mut seen = HashSet::new();
    for e in &snap.edges {
        assert!(
            seen.insert((e.source, e.target, e.relation.clone())),
            "duplicate edge {e:?}"
        );
        assert_eq!(e.weight, 1.0);
    }

    // Communities.
    assert!(report.communities > 1, "{} communities", report.communities);
    assert!(snap
        .nodes
        .iter()
        .filter(|n| n.kind != NodeKind::Directory)
        .all(|n| n.community != 0));
    assert_eq!(
        node(&ing, "src_lib").community,
        node(&ing, "src_lib_start").community
    );

    // Queries.
    assert_eq!(ing.file_id("src/lib.rs"), Some(lib));
    assert_eq!(ing.file_id("./src/lib.rs"), Some(lib));
    assert_eq!(ing.file_id("README.md"), None);
    assert_eq!(
        ing.search("engine", 3).first().map(|h| h.0),
        Some(id_of(&ing, "src_lib_engine"))
    );
    assert_eq!(ing.search("lib.rs", 3).first().map(|h| h.0), Some(lib));
    let callers = ing.neighbors(
        id_of(&ing, "src_lib_engine_run"),
        Some(&[Relation::Calls]),
        true,
        false,
    );
    assert_eq!(
        callers,
        vec![(id_of(&ing, "src_lib_start"), Relation::Calls)]
    );
    let affected = ing.affected(id_of(&ing, "src_util_helper"), 3);
    let depth_of = |k: &str| {
        affected
            .iter()
            .find(|(n, _)| *n == id_of(&ing, k))
            .map(|&(_, d)| d)
    };
    assert_eq!(depth_of("src_lib_engine_run"), Some(1), "{affected:?}");
    assert_eq!(depth_of("src_lib_start"), Some(2), "{affected:?}");
    assert_eq!(depth_of("web_fmt_format"), None);
    let path = ing
        .shortest_path(id_of(&ing, "src_lib_start"), id_of(&ing, "src_util_inner"))
        .unwrap();
    assert_eq!(path.len(), 4, "start → new|run → helper → inner: {path:?}");
    assert_eq!(path.first(), Some(&id_of(&ing, "src_lib_start")));
    let bfs = ing.bfs_from(id_of(&ing, "src_lib_start"), 1);
    assert_eq!(bfs[0], (id_of(&ing, "src_lib_start"), 0.0, 0));
    assert!(bfs.contains(&(
        id_of(&ing, "src_lib_engine_run"),
        1.0,
        id_of(&ing, "src_lib_start") as i32
    )));
    assert!(bfs.iter().all(|&(_, d, _)| d <= 1.0));

    // Explain.
    let why = ing.explain(id_of(&ing, "src_lib_start"), 1_000).unwrap();
    assert_eq!(why.snippets.len(), 1);
    assert_eq!(
        why.snippets[0].text,
        "pub fn start() -> u32 { Engine::new().run() }"
    );
    assert_eq!(why.snippets[0].path, "src/lib.rs");
    assert!(
        why.summary
            .starts_with("function `start` at src/lib.rs:12-12 (rust)."),
        "{}",
        why.summary
    );
    assert!(why.summary.contains("Top callees: "), "{}", why.summary);
    assert!(
        why.summary.contains("Inside file `lib.rs`"),
        "{}",
        why.summary
    );
    assert_eq!(
        why.summary,
        ing.explain(id_of(&ing, "src_lib_start"), 1_000)
            .unwrap()
            .summary,
        "deterministic"
    );
    let short = ing.explain(id_of(&ing, "src_lib_engine"), 20).unwrap();
    assert!(short.snippets[0].text.chars().count() <= 20);
    assert_eq!(short.snippets[0].text, "pub struct Engine {");
    assert!(matches!(
        ing.explain(9_999_999, 10),
        Err(IngestError::UnknownNode(_))
    ));

    // The repository was not written, and the second open comes from the cache.
    let first_keys: BTreeSet<String> = snap.nodes.iter().map(|n| n.key.to_string()).collect();
    let first_edges = edge_keys(&ing);
    drop(ing);
    let (again, report2) = Ingestor::open(config(&root, &out), IdInterner::new(1)).unwrap();
    assert!(report2.from_cache, "{report2:?}");
    assert_eq!(report2.files, report.files);
    assert_eq!(report2.nodes, report.nodes);
    assert_eq!(
        again
            .snapshot()
            .nodes
            .iter()
            .map(|n| n.key.to_string())
            .collect::<BTreeSet<_>>(),
        first_keys
    );
    assert_eq!(edge_keys(&again), first_edges);
    again.snapshot().validate().unwrap();

    // A changed configuration invalidates the cache.
    let mut cfg = config(&root, &out);
    cfg.include_docs = true;
    let (docs, report3) = Ingestor::open(cfg, IdInterner::new(1)).unwrap();
    assert!(!report3.from_cache);
    assert_eq!(report3.files, 7, "README joins with include_docs");
    assert!(docs
        .snapshot()
        .nodes
        .iter()
        .any(|n| n.kind == NodeKind::Document));

    #[cfg(unix)]
    set_read_only(&root, false);
    assert_eq!(
        tree_state(&root),
        before,
        "the repository root must never be written"
    );
    assert!(out.join(".graphicle/graph.json").is_file());
    assert!(out.join(".graphicle/.graphicle_build.json").is_file());
}

#[test]
fn open_rejects_bad_configuration() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    fixture(&root);
    let inside = IngestConfig::new("x", &root, root.join("out"));
    assert!(matches!(
        Ingestor::open(inside, IdInterner::new(1)),
        Err(IngestError::Config(_))
    ));
    assert!(
        !root.join("out").exists(),
        "a rejected out_dir was created inside the repository"
    );
    let mut bad = IngestConfig::new("x", &root, tmp.path().join("out"));
    bad.leiden_resolution = 0.0;
    assert!(matches!(
        Ingestor::open(bad, IdInterner::new(1)),
        Err(IngestError::Config(_))
    ));
    let missing = IngestConfig::new("x", tmp.path().join("nope"), tmp.path().join("out"));
    assert!(matches!(
        Ingestor::open(missing, IdInterner::new(1)),
        Err(IngestError::Io { .. })
    ));
}

// ---------------------------------------------------------------- incremental

#[test]
fn apply_changed_patches_only_the_touched_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let out = tmp.path().join("out");
    fixture(&root);
    let (mut ing, _) = Ingestor::open(config(&root, &out), IdInterner::new(1)).unwrap();
    let before = ing.snapshot().clone();

    // Unchanged content: nothing to report, revision stays.
    assert_eq!(
        ing.apply_changed(&[PathBuf::from("src/util.rs")]).unwrap(),
        None
    );
    assert_eq!(ing.snapshot().revision, 1);

    // A new function and a new call in one file.
    write(&root, "src/util.rs", "pub fn helper(x: u32) -> u32 { x + inner() + extra() }\nfn inner() -> u32 { 1 }\npub fn extra() -> u32 { inner() }\n");
    let delta = ing
        .apply_changed(&[root.join("src/util.rs")])
        .unwrap()
        .expect("a delta");
    assert_eq!((delta.from_revision, delta.to_revision), (1, 2));
    assert_eq!(delta.changed_paths, vec!["src/util.rs".to_string()]);
    assert_eq!(ing.snapshot().revision, 2);
    let extra = id_of(&ing, "src_util_extra");
    let added = delta
        .added_nodes
        .iter()
        .find(|n| n.id == extra)
        .expect("extra() added");
    assert_eq!(added.kind, NodeKind::Function);
    assert_eq!(added.parent, id_of(&ing, "src_util"));
    assert_ne!(
        added.community, 0,
        "a new node joins its neighbours' community"
    );
    assert!(delta
        .added_edges
        .iter()
        .any(|e| e.source == id_of(&ing, "src_util_helper")
            && e.target == extra
            && e.relation == Relation::Calls));
    for n in delta.changed_nodes.iter().chain(&delta.added_nodes) {
        let p = n.path.as_deref().unwrap_or("");
        assert!(
            p == "src/util.rs" || n.kind == NodeKind::Directory,
            "untouched file node in delta: {} ({p})",
            n.key
        );
    }
    // Cross-file edges into the patched file survive.
    assert!(has_edge(
        &ing,
        "src_lib_engine_new",
        "src_util_helper",
        Relation::Calls
    ));
    // The delta reproduces the stored snapshot exactly.
    let mut replay = before.clone();
    delta.apply(&mut replay).unwrap();
    assert_eq!(&replay, ing.snapshot());
    ing.snapshot().validate().unwrap();
    assert_eq!(ing.search("extra", 1).first().map(|h| h.0), Some(extra));

    // Deleting a file removes its nodes and every edge into them.
    std::fs::remove_file(root.join("py/tools.py")).unwrap();
    let tools = id_of(&ing, "py_tools");
    let clean = id_of(&ing, "py_tools_clean");
    let delta = ing
        .apply_changed(&[PathBuf::from("py/tools.py")])
        .unwrap()
        .expect("a delta");
    assert!(delta.removed_nodes.contains(&tools) && delta.removed_nodes.contains(&clean));
    assert!(ing.snapshot().node(clean).is_none());
    assert!(ing
        .snapshot()
        .edges
        .iter()
        .all(|e| e.target != clean && e.source != clean));
    assert_eq!(ing.file_id("py/tools.py"), None);
    assert_eq!(ing.snapshot().revision, 3);

    // A new file appears with a file node under its directory.
    write(&root, "web/extra.ts", "import { format } from './fmt';\nexport function shout(s: string): string { return format(s) + '!'; }\n");
    let delta = ing
        .apply_changed(&[PathBuf::from("web/extra.ts")])
        .unwrap()
        .expect("a delta");
    let file = ing.file_id("web/extra.ts").expect("file node");
    assert!(delta.added_nodes.iter().any(|n| n.id == file));
    assert!(has_edge(
        &ing,
        "web_extra_shout",
        "web_fmt_format",
        Relation::Calls
    ));

    // Ignored, excluded, unsupported and outside paths are skipped.
    write(&root, "gen/new.rs", "pub fn g() {}\n");
    write(&root, "vendor/new.rs", "pub fn v() {}\n");
    write(&root, "notes.md", "# notes\n");
    assert_eq!(
        ing.apply_changed(&[
            PathBuf::from("gen/new.rs"),
            PathBuf::from("vendor/new.rs"),
            PathBuf::from("notes.md"),
            PathBuf::from("/etc/hosts")
        ])
        .unwrap(),
        None
    );

    // The live preview around the patched files (TS and Rust), before the
    // settle re-assembles the corpus: compared with the exact result below.
    let around = |i: &Ingestor| {
        let ext: HashSet<u32> = i
            .snapshot()
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::External)
            .map(|n| n.id)
            .collect();
        let key = |id: u32| i.interner().key_of(id).unwrap().to_string();
        i.snapshot()
            .edges
            .iter()
            .filter(|e| !ext.contains(&e.source) && !ext.contains(&e.target))
            .map(|e| {
                (
                    key(e.source),
                    key(e.target),
                    e.relation.as_str().to_string(),
                )
            })
            .filter(|(s, t, _)| {
                ["src_util", "web_extra"]
                    .iter()
                    .any(|p| s.starts_with(p) || t.starts_with(p))
            })
            .collect::<BTreeSet<_>>()
    };
    let preview = around(&ing);
    assert!(preview.len() > 5, "{preview:?}");

    // Analytics recompute: stable labels, revision + 1.
    let rev = ing.snapshot().revision;
    let (delta, retained) = ing.recompute_analytics().unwrap();
    assert_eq!((delta.from_revision, delta.to_revision), (rev, rev + 1));
    assert_eq!(ing.snapshot().revision, rev + 1);
    assert!(
        retained > 0.5,
        "stable matching keeps most labels: {retained}"
    );
    ing.snapshot().validate().unwrap();
    let settled = around(&ing);
    assert_eq!(
        preview.difference(&settled).collect::<Vec<_>>(),
        Vec::<&(String, String, String)>::new(),
        "preview edges the exact build lacks"
    );
    for (s, t, rel) in settled.difference(&preview) {
        assert!(
            rel.starts_with("imports"),
            "preview missed {s} -{rel}-> {t}"
        );
    }

    // A directory moved: one path for where it was, one for where it went.
    let old_app = id_of(&ing, "web_app");
    std::fs::rename(root.join("web"), root.join("webapp")).unwrap();
    let delta = ing
        .apply_changed(&[root.join("web"), root.join("webapp")])
        .unwrap()
        .expect("a delta");
    assert!(delta.removed_nodes.contains(&old_app));
    assert_eq!(ing.file_id("web/app.ts"), None);
    let moved = ing.file_id("webapp/app.ts").expect("moved file indexed");
    assert!(delta.added_nodes.iter().any(|n| n.id == moved));
    ing.recompute_analytics().unwrap();

    // Live patches + settle converge on exactly what a cold build produces.
    let (cold, cold_report) = Ingestor::open(
        config(&root, &tmp.path().join("out-cold")),
        IdInterner::new(1),
    )
    .unwrap();
    assert!(!cold_report.from_cache);
    let keys = |i: &Ingestor| {
        i.snapshot()
            .nodes
            .iter()
            .map(|n| n.key.to_string())
            .collect::<BTreeSet<_>>()
    };
    assert_eq!(keys(&ing), keys(&cold));
    assert_eq!(edge_keys(&ing), edge_keys(&cold));
    let spans = |i: &Ingestor| {
        i.snapshot()
            .nodes
            .iter()
            .map(|n| {
                (
                    n.key.to_string(),
                    n.span.map(|s| (s.start_line, s.end_line)),
                    n.kind.as_str().to_string(),
                    n.size,
                )
            })
            .collect::<BTreeSet<_>>()
    };
    assert_eq!(spans(&ing), spans(&cold));

    // The settle refreshed graph.json: a reopen of the patched tree is a cache hit.
    drop(ing);
    let (_, reopened) = Ingestor::open(config(&root, &out), IdInterner::new(1)).unwrap();
    assert!(reopened.from_cache, "{reopened:?}");

    assert!(
        !root.join(".graphicle").exists(),
        "graphicle wrote into the repository"
    );
}

// ---------------------------------------------------------------- watch

#[test]
fn watcher_batches_filters_and_stops_on_drop() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let out = tmp.path().join("out");
    fixture(&root);
    let (mut ing, _) = Ingestor::open(config(&root, &out), IdInterner::new(1)).unwrap();
    let (tx, rx) = crossbeam_channel::unbounded();
    let handle = ing.watch(tx).unwrap();
    assert!(handle.is_running());
    std::thread::sleep(Duration::from_millis(200));
    let started = Instant::now();
    write(&root, "src/fresh.rs", "pub fn fresh() {}\n");
    write(&root, "src/fresh2.rs", "pub fn fresh2() {}\n");
    write(&root, "NOTES.md", "# no\n");
    write(&root, "gen/x.rs", "pub fn x() {}\n");
    write(&root, "vendor/y.rs", "pub fn y() {}\n");
    let mut got = BTreeSet::new();
    while got.len() < 2 && started.elapsed() < Duration::from_secs(5) {
        if let Ok(batch) = rx.recv_timeout(Duration::from_millis(500)) {
            got.extend(batch);
        }
    }
    let canon = root.canonicalize().unwrap();
    assert!(got.contains(&canon.join("src/fresh.rs")), "{got:?}");
    assert!(got.contains(&canon.join("src/fresh2.rs")), "{got:?}");
    assert!(
        got.iter().all(|p| p.extension().is_some_and(|e| e == "rs")
            && !p.starts_with(canon.join("gen"))
            && !p.starts_with(canon.join("vendor"))),
        "{got:?}"
    );
    let latency = started.elapsed();
    println!("watch: first batch after {latency:?} (debounce {DEBOUNCE:?})");
    assert!(latency >= DEBOUNCE, "batches wait for the debounce window");

    // A directory moved in from outside the root is one event for the
    // directory; applying the batch indexes the files below it.
    write(tmp.path(), "outside/moved/q.rs", "pub fn q() {}\n");
    std::fs::rename(tmp.path().join("outside/moved"), root.join("moved")).unwrap();
    let mut moved = Vec::new();
    let t = Instant::now();
    while !moved.contains(&canon.join("moved")) && t.elapsed() < Duration::from_secs(5) {
        if let Ok(batch) = rx.recv_timeout(Duration::from_millis(500)) {
            moved.extend(batch);
        }
    }
    assert!(moved.contains(&canon.join("moved")), "{moved:?}");
    ing.apply_changed(&moved).unwrap().expect("a delta");
    assert!(ing.file_id("moved/q.rs").is_some());

    drop(handle);
    match rx.recv_timeout(Duration::from_secs(2)) {
        Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {}
        other => panic!("watcher still alive after drop: {other:?}"),
    }

    // With a bounded channel nobody drains, dropping the handle still returns.
    let (tx, _rx_full) = crossbeam_channel::bounded(1);
    let handle = ing.watch(tx).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    for i in 0..3 {
        write(&root, &format!("src/burst{i}.rs"), "pub fn b() {}\n");
        std::thread::sleep(DEBOUNCE * 2);
    }
    let (done_tx, done_rx) = crossbeam_channel::bounded(1);
    std::thread::spawn(move || {
        drop(handle);
        let _ = done_tx.send(());
    });
    assert!(
        done_rx.recv_timeout(Duration::from_secs(3)).is_ok(),
        "dropping the handle hung on a full channel"
    );
}

/// An unreadable subdirectory (the agentbox `profiles/*/workspace/.tmp`
/// case, made visible here) must not stop the watcher: it is skipped, and
/// ignored, excluded and noise trees are never registered at all.
#[cfg(unix)]
#[test]
fn watcher_skips_unreadable_and_ignored_subtrees() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let out = tmp.path().join("out");
    fixture(&root);
    write(&root, "locked/inner/secret.rs", "pub fn s() {}\n");
    write(&root, "node_modules/pkg/index.ts", "export const x = 1;\n");
    let locked = root.join("locked");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    // Restores the mode however the test ends, so the tempdir can be removed.
    struct Unlock(PathBuf);
    impl Drop for Unlock {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }
    let _unlock = Unlock(locked.clone());
    if std::fs::read_dir(&locked).is_ok() {
        println!("skipped: running with privileges that ignore directory permissions (root)");
        return;
    }

    let (ing, report) = Ingestor::open(config(&root, &out), IdInterner::new(1)).unwrap();
    assert_eq!(
        report.files, 6,
        "the cold walk skips the unreadable subtree too"
    );
    let (tx, rx) = crossbeam_channel::unbounded();
    let handle = ing
        .watch(tx)
        .expect("an unreadable subtree must not fail the watcher");
    let canon = root.canonicalize().unwrap();
    assert!(handle.is_watching(&canon));
    assert!(handle.is_watching(&canon.join("src")));
    assert!(
        !handle.is_watching(&canon.join("locked")),
        "unwatchable directory skipped"
    );
    for skipped in ["gen", "vendor", "node_modules", "node_modules/pkg"] {
        assert!(
            !handle.is_watching(&canon.join(skipped)),
            "{skipped} registered"
        );
    }
    assert_eq!(handle.watched_dirs(), 4, "repo, src, web, py");

    std::thread::sleep(Duration::from_millis(200));
    write(&root, "src/sibling.rs", "pub fn sibling() {}\n");
    let mut got = Vec::new();
    let t = Instant::now();
    while !got.contains(&canon.join("src/sibling.rs")) && t.elapsed() < Duration::from_secs(5) {
        if let Ok(batch) = rx.recv_timeout(Duration::from_millis(500)) {
            got.extend(batch);
        }
    }
    assert!(got.contains(&canon.join("src/sibling.rs")), "{got:?}");

    // A directory created later is registered when its event arrives.
    write(&root, "extra/deep/new.rs", "pub fn n() {}\n");
    let t = Instant::now();
    while !handle.is_watching(&canon.join("extra/deep")) && t.elapsed() < Duration::from_secs(5) {
        let _ = rx.recv_timeout(Duration::from_millis(200));
    }
    assert!(handle.is_watching(&canon.join("extra/deep")));
    write(&root, "extra/deep/later.rs", "pub fn l() {}\n");
    let mut got = Vec::new();
    let t = Instant::now();
    while !got.contains(&canon.join("extra/deep/later.rs")) && t.elapsed() < Duration::from_secs(5)
    {
        if let Ok(batch) = rx.recv_timeout(Duration::from_millis(500)) {
            got.extend(batch);
        }
    }
    assert!(got.contains(&canon.join("extra/deep/later.rs")), "{got:?}");
}

#[test]
fn watcher_fails_only_for_an_unwatchable_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let out = tmp.path().join("out");
    fixture(&root);
    let (ing, _) = Ingestor::open(config(&root, &out), IdInterner::new(1)).unwrap();
    std::fs::remove_dir_all(&root).unwrap();
    let (tx, _rx) = crossbeam_channel::unbounded();
    assert!(matches!(ing.watch(tx), Err(IngestError::Watch(_))));
}

/// Rewrites `rel` under `root` every `every` until the returned guard drops.
struct Churn(
    Arc<std::sync::atomic::AtomicBool>,
    Option<std::thread::JoinHandle<()>>,
);

impl Churn {
    fn start(root: &Path, rels: &[&str], every: Duration) -> Self {
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (flag, root) = (stop.clone(), root.to_path_buf());
        let rels: Vec<String> = rels.iter().map(|r| r.to_string()).collect();
        let thread = std::thread::spawn(move || {
            let mut n = 0u64;
            while !flag.load(std::sync::atomic::Ordering::Relaxed) {
                for rel in &rels {
                    write(&root, rel, &format!("pub fn churn() -> u64 {{ {n} }}\n"));
                }
                n += 1;
                std::thread::sleep(every);
            }
        });
        Self(stop, Some(thread))
    }
}

impl Drop for Churn {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(t) = self.1.take() {
            let _ = t.join();
        }
    }
}

/// Receive until a batch contains `want`: when it arrived, and every path
/// received on the way.
fn wait_for(
    rx: &crossbeam_channel::Receiver<Vec<PathBuf>>,
    want: &Path,
    limit: Duration,
) -> (Option<Duration>, Vec<PathBuf>) {
    let t = Instant::now();
    let mut seen = Vec::new();
    while t.elapsed() < limit {
        if let Ok(batch) = rx.recv_timeout(Duration::from_millis(50)) {
            let hit = batch.iter().any(|p| p == want);
            seen.extend(batch);
            if hit {
                return (Some(t.elapsed()), seen);
            }
        }
    }
    (None, seen)
}

#[test]
fn ignored_churn_does_not_hold_back_a_batch() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    fixture(&root);
    let (ing, _) =
        Ingestor::open(config(&root, &tmp.path().join("out")), IdInterner::new(1)).unwrap();
    let (tx, rx) = crossbeam_channel::unbounded();
    let _handle = ing.watch(tx).unwrap();
    let canon = root.canonicalize().unwrap();
    // Gitignored and unsupported files rewritten every 100 ms, well inside the debounce.
    let _noise = Churn::start(
        &root,
        &["gen/noise.rs", "build.log"],
        Duration::from_millis(100),
    );
    std::thread::sleep(Duration::from_millis(400));
    let t = Instant::now();
    write(&root, "src/real.rs", "pub fn real() {}\n");
    let (took, seen) = wait_for(&rx, &canon.join("src/real.rs"), Duration::from_secs(3));
    println!("watch: batch under ignored churn after {took:?}");
    let took = took.expect("ignored churn suppressed the batch");
    assert!(
        took < MAX_BATCH_WAIT,
        "released by the quiet period, not the max wait: {took:?}"
    );
    assert!(t.elapsed() >= DEBOUNCE);
    assert_eq!(
        seen,
        vec![canon.join("src/real.rs")],
        "noise leaked into a batch"
    );
}

#[test]
fn continuous_real_writes_flush_at_the_max_wait() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    fixture(&root);
    let (ing, _) =
        Ingestor::open(config(&root, &tmp.path().join("out")), IdInterner::new(1)).unwrap();
    let (tx, rx) = crossbeam_channel::unbounded();
    let _handle = ing.watch(tx).unwrap();
    let canon = root.canonicalize().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    // A real source file rewritten every 100 ms: the quiet period never comes.
    let _hot = Churn::start(&root, &["src/hot.rs"], Duration::from_millis(100));
    let (took, _) = wait_for(&rx, &canon.join("src/hot.rs"), Duration::from_secs(4));
    println!("watch: batch under continuous writes after {took:?}");
    let took = took.expect("continuous writes starved the batch");
    assert!(
        took >= MAX_BATCH_WAIT - Duration::from_millis(150),
        "{took:?}"
    );
    assert!(
        took < MAX_BATCH_WAIT + Duration::from_millis(700),
        "{took:?}"
    );
    // And it keeps flushing, about once per max wait.
    let (again, _) = wait_for(&rx, &canon.join("src/hot.rs"), Duration::from_secs(3));
    let again = again.expect("a second batch");
    assert!(
        again < MAX_BATCH_WAIT + Duration::from_millis(700),
        "{again:?}"
    );
}

#[test]
fn drop_returns_promptly_while_events_stream() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    fixture(&root);
    let (ing, _) =
        Ingestor::open(config(&root, &tmp.path().join("out")), IdInterner::new(1)).unwrap();
    // Nobody drains this bounded channel, so the thread is also mid-send.
    let (tx, _rx) = crossbeam_channel::bounded(1);
    let handle = ing.watch(tx).unwrap();
    let _churn = Churn::start(
        &root,
        &["src/a.rs", "src/b.rs", "gen/c.rs"],
        Duration::from_millis(5),
    );
    std::thread::sleep(Duration::from_millis(2_500));
    // Dropped on a helper thread so a regression fails here instead of hanging.
    let (done_tx, done_rx) = crossbeam_channel::bounded(1);
    std::thread::spawn(move || {
        let t = Instant::now();
        drop(handle);
        let _ = done_tx.send(t.elapsed());
    });
    let took = done_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("WatchHandle::drop hung while events were streaming");
    println!("watch: drop under streaming events took {took:?}");
    assert!(took < Duration::from_millis(300), "drop took {took:?}");
}

// ---------------------------------------------------------------- timing

/// Generate `files` Rust files of `fns` functions each, chained within a
/// file and calling into the next file.
pub(crate) fn synthetic_repo(root: &Path, files: usize, fns: usize) {
    let mut lib = String::new();
    for i in 0..files {
        lib.push_str(&format!("pub mod m{i};\n"));
        let next = (i + 1) % files;
        let mut src = format!("use crate::m{next};\n\n");
        for j in 0..fns {
            let callee = if j + 1 < fns {
                format!("f{}() + ", j + 1)
            } else {
                String::new()
            };
            src.push_str(&format!(
                "pub fn f{j}() -> u32 {{ {callee}m{next}::f{j}() * 0 + {j} }}\n"
            ));
        }
        write(root, &format!("src/m{i}.rs"), &src);
    }
    write(root, "src/lib.rs", &lib);
}

#[test]
fn single_file_patch_on_a_50k_node_graph_is_fast() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let out = tmp.path().join("out");
    synthetic_repo(&root, 340, 150);
    let t = Instant::now();
    let (mut ing, report) = Ingestor::open(
        IngestConfig::new("synthetic", &root, &out),
        IdInterner::new(1),
    )
    .unwrap();
    println!("synthetic cold open: {report:?} wall {:?}", t.elapsed());
    assert!(report.nodes >= 50_000, "{} nodes", report.nodes);

    let externals = |i: &Ingestor| {
        i.snapshot()
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::External)
            .count()
    };
    let externals_cold = externals(&ing);
    let mut timings = Vec::new();
    for round in 0..3 {
        let rel = format!("src/m{}.rs", 17 + round);
        let mut text = std::fs::read_to_string(root.join(&rel)).unwrap();
        text.push_str(&format!("pub fn added{round}() -> u32 {{ f3() + f4() }}\n"));
        std::fs::write(root.join(&rel), text).unwrap();
        let t = Instant::now();
        let delta = ing
            .apply_changed(&[PathBuf::from(&rel)])
            .unwrap()
            .expect("a delta");
        let took = t.elapsed();
        timings.push(took);
        println!(
            "apply_changed({rel}) on {} nodes / {} edges: {took:?} (+{} nodes, ~{} nodes, +{} edges)",
            ing.snapshot().nodes.len(),
            ing.snapshot().edges.len(),
            delta.added_nodes.len(),
            delta.changed_nodes.len(),
            delta.added_edges.len()
        );
        assert!(
            delta
                .added_nodes
                .iter()
                .any(|n| *n.label == format!("added{round}")),
            "{:?}",
            delta.added_nodes
        );
    }
    let best = timings.iter().min().unwrap();
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    println!("single-file apply_changed best of 3 ({profile}): {best:?}");
    // The sub-second bar is for the build the server ships (release). In the
    // dev profile this crate and codescape-model run unoptimised (the
    // workspace's `[profile.dev.package."*"]` optimises dependencies only), so
    // debug gets a looser bound; both numbers are printed.
    let bar = if cfg!(debug_assertions) {
        Duration::from_secs(4)
    } else {
        Duration::from_secs(1)
    };
    assert!(
        *best < bar,
        "single-file patch took {best:?} ({profile}, bar {bar:?})"
    );

    // The preview's edges around the patched files equal the exact rebuild's
    // (stubs a single-file patch may mint for `use crate::mN` aside).
    let around = |i: &Ingestor| {
        let ext: HashSet<u32> = i
            .snapshot()
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::External)
            .map(|n| n.id)
            .collect();
        let key = |id: u32| i.interner().key_of(id).unwrap().to_string();
        i.snapshot()
            .edges
            .iter()
            .filter(|e| !ext.contains(&e.source) && !ext.contains(&e.target))
            .map(|e| {
                (
                    key(e.source),
                    key(e.target),
                    e.relation.as_str().to_string(),
                )
            })
            .filter(|(s, t, _)| {
                ["src_m17", "src_m18", "src_m19"]
                    .iter()
                    .any(|p| s.starts_with(p) || t.starts_with(p))
            })
            .collect::<BTreeSet<_>>()
    };
    let preview = around(&ing);

    // The settle (exact re-assembly from the per-file cache) removes those
    // stubs and recomputes analytics.
    let t = Instant::now();
    let (delta, retained) = ing.recompute_analytics().unwrap();
    println!(
        "recompute_analytics with settle: {:?} (removed {}, changed {}, retained {retained:.3})",
        t.elapsed(),
        delta.removed_nodes.len(),
        delta.changed_nodes.len()
    );
    assert_eq!(
        externals(&ing),
        externals_cold,
        "live-patch stubs survive the settle"
    );
    let settled = around(&ing);
    assert_eq!(
        preview.difference(&settled).collect::<Vec<_>>(),
        Vec::<&(String, String, String)>::new(),
        "preview edges the exact build lacks"
    );
    // A single-file extract has no corpus module map, so `use crate::mN`
    // points at a stub until the settle: those import edges are the only misses.
    let missed: Vec<_> = settled.difference(&preview).collect();
    println!(
        "preview: {} edges around the patched files, {} module imports deferred to the settle",
        preview.len(),
        missed.len()
    );
    for (s, t, rel) in &missed {
        assert!(
            rel.starts_with("imports")
                && node(&ing, t).kind == NodeKind::File
                && node(&ing, s).kind == NodeKind::File,
            "preview missed {s} -{rel}-> {t}"
        );
    }
    let t = Instant::now();
    ing.recompute_analytics().unwrap();
    println!("recompute_analytics without settle: {:?}", t.elapsed());
}

// ---------------------------------------------------------------- dogfood

/// Manual smoke test on VisionClaw's own crates:
/// `cargo test -p codescape-ingest dogfood -- --ignored --nocapture`.
#[test]
#[ignore = "reads the host checkout; run manually"]
fn dogfood_visionclaw_crates() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../crates")
        .canonicalize()
        .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let t = Instant::now();
    let (ing, report) = Ingestor::open(
        IngestConfig::new("visionclaw-crates", &crates, tmp.path()),
        IdInterner::new(1),
    )
    .unwrap();
    println!("dogfood report: {report:#?}");
    println!(
        "dogfood wall {:?}, tiers {:?}",
        t.elapsed(),
        ing.snapshot().tier_counts()
    );
    ing.snapshot().validate().unwrap();
    let t = Instant::now();
    let (mut again, report2) = Ingestor::open(
        IngestConfig::new("visionclaw-crates", &crates, tmp.path()),
        IdInterner::new(1),
    )
    .unwrap();
    println!(
        "dogfood reopen: from_cache {} in {:?} ({} ms reported)",
        report2.from_cache,
        t.elapsed(),
        report2.duration_ms
    );
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for n in &again.snapshot().nodes {
        *kinds.entry(n.kind.as_str().to_string()).or_default() += 1;
    }
    println!("dogfood kinds: {kinds:?}");
    if let Some(&(id, _)) = again.search("GraphSnapshot", 1).first() {
        println!(
            "dogfood explain: {}",
            again.explain(id, 400).unwrap().summary
        );
    }

    // The live path on real files, unmodified so nothing in `crates/` changes:
    // the largest Rust file and the largest TypeScript file, then the settle.
    let largest = |lang: Lang| {
        again
            .snapshot()
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::File && n.lang == lang)
            .max_by_key(|n| n.size)
            .and_then(|n| n.path.as_deref().map(str::to_string))
    };
    let picks: Vec<String> = [Lang::Rust, Lang::TypeScript]
        .into_iter()
        .filter_map(largest)
        .collect();
    for rel in &picks {
        let t = Instant::now();
        let delta = again.apply_changed(&[PathBuf::from(rel)]).unwrap();
        println!(
            "dogfood apply_changed({rel}, unmodified): {:?}, delta {}",
            t.elapsed(),
            delta.map_or("none".to_string(), |d| format!(
                "+{} -{} ~{} nodes, +{} -{} edges, added {:?}",
                d.added_nodes.len(),
                d.removed_nodes.len(),
                d.changed_nodes.len(),
                d.added_edges.len(),
                d.removed_edges.len(),
                d.added_nodes
                    .iter()
                    .map(|n| format!("{}:{}", n.kind.as_str(), n.key))
                    .collect::<Vec<_>>()
            ))
        );
    }
    let t = Instant::now();
    let (delta, retained) = again.recompute_analytics().unwrap();
    println!(
        "dogfood recompute_analytics with settle: {:?} (removed {}, added {}, changed {}, retained {retained:.3})",
        t.elapsed(),
        delta.removed_nodes.len(),
        delta.added_nodes.len(),
        delta.changed_nodes.len()
    );
    let t = Instant::now();
    again.recompute_analytics().unwrap();
    println!(
        "dogfood recompute_analytics without settle: {:?}",
        t.elapsed()
    );
}
