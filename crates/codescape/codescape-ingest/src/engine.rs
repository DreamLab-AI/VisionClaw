//! The graphicle façade: every call into the engine lives in this module.
//!
//! Nothing here returns a graphicle type across the crate boundary. The module
//! owns the resident graphicle [`Graph`], the path filter and the on-disk cache
//! under the out dir. It never writes inside the repository root:
//!
//! * extraction runs with `root` = the repository and `cache_root` = the out dir,
//!   so graphicle's per-file AST cache lands in `<out>/.graphicle/cache`;
//! * the live patch is re-implemented here rather than taken from
//!   `serve_live::patch_with`, because that function pins `cache_root` to the
//!   watched root and so writes `.graphicle/` into the repository;
//! * `graph.json` and the file manifest are written to the out dir only.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::IngestError;

/// The resident graphicle graph type.
pub(crate) type Graph = graphicle::graph::Graph;

/// graphicle's extractors keep per-run global state (parse-cache run scopes,
/// the JS resolution table, the Markdown link index). Every extract and build
/// in the process is serialised through this lock, so concurrent ingestors and
/// parallel tests cannot interleave runs.
static ENGINE_LOCK: Mutex<()> = Mutex::new(());

/// File name of the cached node-link graph inside graphicle's out dir.
const GRAPH_FILE: &str = "graph.json";
/// File name of the manifest that decides whether `graph.json` is current.
const MANIFEST_FILE: &str = "codescape-manifest.json";
/// Bumped whenever the cached graph's shape or the manifest changes meaning.
const CACHE_FORMAT: u32 = 1;

/// Relations whose records travel as resolution context into a live patch;
/// the same set graphicle's own live path passes (`serve_live::context_where`).
const CONTEXT_RELATIONS: [&str; 5] = ["contains", "method", "inherits", "imports", "imports_from"];
/// Private node attributes the resolvers read from context records.
const CONTEXT_FLAGS: [&str; 10] = [
    "_callable",
    "_callable_class",
    "_elixir_module",
    "_rust_impl_key",
    "_rust_declaration_count",
    "_ts_public_twin",
    "_py_property",
    "_java_ret",
    "_ts_fields",
    "_ts_static_fields",
];
/// Metadata keys the resolvers read from context records.
const CONTEXT_METADATA: [&str; 11] = [
    "ruby_resolution_schema",
    "ruby_method_kind",
    "ruby_lookup_unsafe",
    "python_opaque_bases",
    "ruby_reopened",
    "ruby_external_method_owners",
    "language",
    "kind",
    "module",
    "name",
    "arity",
];

fn engine_err(e: impl std::fmt::Display) -> IngestError {
    IngestError::Engine(e.to_string())
}

fn io_err(path: &Path, source: std::io::Error) -> IngestError {
    IngestError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// The string form of a graphicle node id (always a string for code graphs).
pub(crate) fn node_key(id: &Value) -> String {
    match id {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// A string attribute of a graphicle node or edge record, `""` when absent.
pub(crate) fn attr_str<'a>(attrs: &'a graphicle::record::Attributes, key: &str) -> &'a str {
    attrs.get(key).and_then(Value::as_str).unwrap_or("")
}

/// Whether a record came from AST extraction (graphicle's `is_ast_tier`).
fn ast_tier(attrs: &graphicle::record::Attributes) -> bool {
    let mut probe = Map::new();
    for key in ["_origin", "source_location"] {
        if let Some(v) = attrs.get(key) {
            probe.insert(key.into(), v.clone());
        }
    }
    graphicle::build::is_ast_tier(&Value::Object(probe))
}

/// Whether graphicle's label rules make this node the node of its file.
pub(crate) fn is_file_node(attrs: &graphicle::record::Attributes) -> bool {
    match (attrs.get("label"), attrs.get("source_file")) {
        (Some(label), Some(source)) => graphicle::build::is_file_node_label(label, source),
        _ => false,
    }
}

/// Lexically normalise `path` (resolve `.` and `..` without touching the disk).
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Canonicalise a path that may not exist: the deepest existing ancestor is
/// resolved and the missing remainder appended.
pub(crate) fn resolve_lenient(path: &Path) -> PathBuf {
    let path = lexical(path);
    let mut existing = path.as_path();
    let mut rest: Vec<&std::ffi::OsStr> = Vec::new();
    loop {
        if let Ok(real) = existing.canonicalize() {
            let mut out = real;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name);
                existing = parent;
            }
            _ => return path,
        }
    }
}

/// Repository-relative path with `/` separators, if `path` is under `root`.
pub(crate) fn rel_of(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let s = rel.to_string_lossy().replace('\\', "/");
    (!s.is_empty()).then_some(s)
}

/// Directory names graphicle's corpus walk never enters: `_SKIP_DIRS` from
/// `assets/detect/constants.json` at the pinned graphicle revision (003f936).
/// graphicle keeps `is_noise_dir` crate-private, so the list is mirrored here
/// to keep the watcher and the cold walk in agreement. Its conditional rules
/// (`env`, `coverage`, `out`, `snapshots` judged by their contents) are not
/// mirrored; a file the cold walk would skip there is still dropped by
/// [`PathFilter::admits`] only if the other rules reject it.
const NOISE_DIRS: [&str; 39] = [
    ".angular",
    ".cache",
    ".eggs",
    ".git",
    ".graphicle",
    ".graphicle-graph",
    ".graphify",
    ".idea",
    ".mypy_cache",
    ".next",
    ".nox",
    ".nuxt",
    ".obsidian",
    ".parcel-cache",
    ".pytest_cache",
    ".ruff_cache",
    ".serverless",
    ".smart-env",
    ".svelte-kit",
    ".terraform",
    ".tox",
    ".turbo",
    ".venv",
    ".worktrees",
    "__pycache__",
    "__snapshots__",
    "build",
    "dist",
    "dist-protected",
    "graphify-out",
    "lcov-report",
    "lib64",
    "node_modules",
    "site-packages",
    "storybook-static",
    "target",
    "venv",
    "visual-test",
    "visual-tests",
];

/// Whether graphicle's walk skips a directory of this name.
pub(crate) fn is_noise_dir(name: &str) -> bool {
    name.ends_with(".egg-info") || NOISE_DIRS.contains(&name)
}

/// No hidden component and no noise directory on the path (relative to the root).
fn visible(rel: &Path) -> bool {
    let mut parts = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .peekable();
    while let Some(part) = parts.next() {
        // The last component of a file path is a file name, not a directory.
        if part.starts_with('.') || (parts.peek().is_some() && is_noise_dir(&part)) {
            return false;
        }
    }
    true
}

/// Decides which paths belong in the code graph, with graphicle's own ignore
/// semantics (nested `.gitignore`, `.graphicleignore`, noise directories) plus
/// the configured excludes.
pub(crate) struct PathFilter {
    root: PathBuf,
    out: PathBuf,
    include_docs: bool,
    ignore: Mutex<graphicle::detect::IgnoreState>,
}

impl PathFilter {
    /// Record the excludes in graphicle's build config under `graphicle_out`
    /// (the repository may be read-only, so the config cannot live beside it)
    /// and load the ignore rules from that config.
    pub(crate) fn new(
        root: &Path,
        out: &Path,
        graphicle_out: &Path,
        include_docs: bool,
        excludes: &[String],
    ) -> Result<Self, IngestError> {
        std::fs::create_dir_all(graphicle_out).map_err(|e| io_err(graphicle_out, e))?;
        graphicle::watch::write_config(graphicle_out, excludes, Some(true));
        let extra = graphicle::watch::build_excludes(graphicle_out);
        let gitignore = graphicle::watch::build_gitignore(graphicle_out);
        let state = graphicle::detect::IgnoreState::new(
            root,
            &extra,
            gitignore,
            &mut graphicle::detect::EncodingWarnings::default(),
        )
        .map_err(engine_err)?;
        Ok(Self {
            root: root.to_path_buf(),
            out: out.to_path_buf(),
            include_docs,
            ignore: Mutex::new(state),
        })
    }

    /// Whether graphicle has an extractor for `path` and it is code (or docs are on).
    pub(crate) fn supported(&self, path: &Path) -> bool {
        if graphicle::extract::get_extractor(path).is_none() {
            return false;
        }
        self.include_docs
            || matches!(
                graphicle::detect::classify_file(path),
                Some(graphicle::detect::FileType::Code)
            )
    }

    /// Whether an absolute path under the root is part of the code graph.
    /// Deleted paths are judged lexically, so a removal still gets through.
    pub(crate) fn admits(&self, path: &Path) -> bool {
        if path.starts_with(&self.out) {
            return false;
        }
        let Ok(rel) = path.strip_prefix(&self.root) else {
            return false;
        };
        if rel.as_os_str().is_empty() || !visible(rel) {
            return false;
        }
        if path.is_dir() || !self.supported(path) {
            return false;
        }
        !self
            .ignore
            .lock()
            .predicate(path, &mut graphicle::detect::EncodingWarnings::default())
            .unwrap_or(true)
    }

    /// Whether a path may be a directory of the code graph: an existing,
    /// visible, non-ignored directory under the root, or a path that no
    /// longer exists (a directory moved or deleted reports one event for the
    /// directory, none for its files).
    pub(crate) fn admits_dir(&self, path: &Path) -> bool {
        if path.starts_with(&self.out) {
            return false;
        }
        let Ok(rel) = path.strip_prefix(&self.root) else {
            return false;
        };
        if rel.as_os_str().is_empty() || !visible(rel) {
            return false;
        }
        if rel
            .file_name()
            .is_some_and(|n| is_noise_dir(&n.to_string_lossy()))
        {
            return false;
        }
        if !path.exists() {
            return true;
        }
        path.is_dir()
            && !self
                .ignore
                .lock()
                .predicate(path, &mut graphicle::detect::EncodingWarnings::default())
                .unwrap_or(true)
    }

    /// Every file of the code graph: graphicle's corpus walk, then [`Self::admits`].
    pub(crate) fn collect(&self) -> Result<Vec<PathBuf>, IngestError> {
        self.collect_under(&self.root)
    }

    /// The code-graph files under `dir` (inside the root).
    pub(crate) fn collect_under(&self, dir: &Path) -> Result<Vec<PathBuf>, IngestError> {
        let files =
            graphicle::extract::collect_files(dir, false, Some(&self.root)).map_err(engine_err)?;
        Ok(files.into_iter().filter(|p| self.admits(p)).collect())
    }
}

/// One file's entry in the cache manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct FileStamp {
    path: String,
    size: u64,
    mtime_ns: u64,
}

/// What `graph.json` was built from. The cached graph is reused only when
/// every field matches the current tree and configuration exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Manifest {
    format: u32,
    engine: String,
    include_docs: bool,
    excludes: Vec<String>,
    /// Inputs graphicle had no extractor for.
    pub(crate) unsupported: usize,
    files: Vec<FileStamp>,
}

impl Manifest {
    /// Stamp `files` (absolute, under `root`) for the given configuration.
    pub(crate) fn of(
        root: &Path,
        files: &[PathBuf],
        include_docs: bool,
        excludes: &[String],
    ) -> Result<Self, IngestError> {
        let mut stamps = Vec::with_capacity(files.len());
        for f in files {
            let meta = std::fs::metadata(f).map_err(|e| io_err(f, e))?;
            let mtime_ns = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos() as u64);
            stamps.push(FileStamp {
                path: rel_of(root, f).unwrap_or_default(),
                size: meta.len(),
                mtime_ns,
            });
        }
        stamps.sort_by(|a, b| a.path.cmp(&b.path));
        let mut excludes = excludes.to_vec();
        excludes.sort();
        Ok(Self {
            format: CACHE_FORMAT,
            engine: format!(
                "graphicle {} (upstream {}) grammars {}",
                graphicle::VERSION,
                graphicle::UPSTREAM_COMMIT,
                graphicle::cache::grammar_fingerprint()
            ),
            include_docs,
            excludes,
            unsupported: 0,
            files: stamps,
        })
    }

    /// Same inputs, ignoring the derived `unsupported` count.
    fn same_inputs(&self, other: &Self) -> bool {
        self.format == other.format
            && self.engine == other.engine
            && self.include_docs == other.include_docs
            && self.excludes == other.excludes
            && self.files == other.files
    }
}

/// The cached graph under the out dir, if its manifest matches `current`.
pub(crate) fn load_cached(graphicle_out: &Path, current: &Manifest) -> Option<(Graph, usize)> {
    let stored: Manifest =
        serde_json::from_slice(&std::fs::read(graphicle_out.join(MANIFEST_FILE)).ok()?).ok()?;
    if !stored.same_inputs(current) {
        return None;
    }
    let data: Value =
        serde_json::from_slice(&std::fs::read(graphicle_out.join(GRAPH_FILE)).ok()?).ok()?;
    let graph = graphicle::graph::Graph::from_node_link_owned(data, "links").ok()?;
    graph.is_directed().then_some((graph, stored.unsupported))
}

/// Write `graph.json` and its manifest atomically under the out dir.
pub(crate) fn store_cached(
    graphicle_out: &Path,
    graph: &Graph,
    manifest: &Manifest,
) -> Result<(), IngestError> {
    let write = |name: &str, bytes: Vec<u8>| -> Result<(), IngestError> {
        let dest = graphicle_out.join(name);
        let tmp = graphicle_out.join(format!(".{name}.tmp"));
        std::fs::write(&tmp, bytes).map_err(|e| io_err(&tmp, e))?;
        std::fs::rename(&tmp, &dest).map_err(|e| io_err(&dest, e))
    };
    let graph_bytes = serde_json::to_vec(&graph.to_node_link("links")).map_err(engine_err)?;
    write(GRAPH_FILE, graph_bytes)?;
    write(
        MANIFEST_FILE,
        serde_json::to_vec_pretty(manifest).map_err(engine_err)?,
    )
}

/// Cold extraction of `files` into a directed graphicle graph.
/// Returns the graph and the number of inputs graphicle had no extractor for.
pub(crate) fn extract_full(
    root: &Path,
    out: &Path,
    files: &[PathBuf],
    max_workers: Option<usize>,
) -> Result<(Graph, usize), IngestError> {
    let _run = ENGINE_LOCK.lock();
    let options = graphicle::extract::ExtractOptions {
        root: Some(root.to_path_buf()),
        cache_root: Some(out.to_path_buf()),
        max_workers,
        ..Default::default()
    };
    let (mut extraction, report) =
        graphicle::extract::extract_with_report(files, &options).map_err(engine_err)?;
    let graph = graphicle::build::build_from_json(
        &mut extraction,
        true,
        Some(root),
        &mut graphicle::build::Diagnostics::default(),
    )
    .map_err(engine_err)?;
    Ok((graph, report.unsupported))
}

/// The resolution-context records of every AST-tier record whose file
/// `keep` accepts: the fields graphicle's live path hands to `extract` so
/// that re-extracted files resolve calls into the unchanged corpus
/// (mirrors `serve_live::context_where`).
fn context_where(g: &Graph, keep: impl Fn(&str) -> bool) -> (Vec<Value>, Vec<Value>) {
    let live = |s: &str| !s.is_empty() && keep(s);
    let mut nodes = Vec::new();
    for node in g.nodes() {
        let a = &node.attributes;
        if node.id.as_str().is_none_or(str::is_empty)
            || !live(attr_str(a, "source_file"))
            || !ast_tier(a)
        {
            continue;
        }
        let get = |k: &str| a.get(k).cloned().unwrap_or(Value::Null);
        let mut c = json!({"id": node.id});
        for k in ["label", "source_file", "file_type", "type"] {
            c[k] = get(k);
        }
        if attr_str(a, "node_kind") == "route" {
            for k in ["node_kind", "route_method", "route_path"] {
                c[k] = get(k);
            }
        }
        for k in CONTEXT_FLAGS {
            if a.get(k).is_some_and(truthy) {
                c[k] = get(k);
            }
        }
        if let Some(m) = a.get("metadata").and_then(Value::as_object) {
            let meta: Map<String, Value> = CONTEXT_METADATA
                .iter()
                .filter_map(|k| m.get(*k).map(|v| ((*k).to_string(), v.clone())))
                .collect();
            if !meta.is_empty() {
                c["metadata"] = Value::Object(meta);
            }
        }
        nodes.push(c);
    }
    let mut edges = Vec::new();
    for (u, v, e) in g.iter_edges() {
        let a = &e.attributes;
        let relation = attr_str(a, "relation");
        if !CONTEXT_RELATIONS.contains(&relation)
            || !live(attr_str(a, "source_file"))
            || !ast_tier(a)
        {
            continue;
        }
        let mut c = json!({"source": g.nodes()[u].id, "target": g.nodes()[v].id, "relation": relation,
            "source_file": a.get("source_file").cloned().unwrap_or(Value::Null)});
        if let Some(m) = a.get("metadata").and_then(Value::as_object) {
            if let Some(sup) = m.get("ruby_superclass_ref").filter(|v| v.is_string()) {
                c["metadata"] = json!({"ruby_superclass_ref": sup});
                if let Some(scopes) = m
                    .get("ruby_lexical_scopes")
                    .filter(|v| v.as_array().is_some_and(|a| a.iter().all(Value::is_string)))
                {
                    c["metadata"]["ruby_lexical_scopes"] = scopes.clone();
                }
            }
        }
        edges.push(c);
    }
    (nodes, edges)
}

/// The resolution context of the whole resident graph, kept between live
/// patches (graphicle's live path does the same with its `ContextCache`):
/// a patch drops the records of the files it re-extracts, lends the rest to
/// the extraction, then adds the re-extracted files' records back. Rebuilding
/// the context from the graph on every patch costs more than the splice.
#[derive(Debug, Default)]
pub(crate) struct ContextCache {
    nodes: Vec<Value>,
    edges: Vec<Value>,
    valid: bool,
}

impl ContextCache {
    /// Forget everything; the next patch rebuilds from the graph.
    pub(crate) fn invalidate(&mut self) {
        *self = Self::default();
    }

    /// Build the records from `g` unless they are current.
    pub(crate) fn ensure(&mut self, g: &Graph) {
        if !self.valid {
            let (nodes, edges) = context_where(g, |_| true);
            *self = Self {
                nodes,
                edges,
                valid: true,
            };
        }
    }

    fn drop_files(&mut self, touched: &BTreeSet<String>) {
        let keep = |v: &Value| {
            !touched.contains(v.get("source_file").and_then(Value::as_str).unwrap_or(""))
        };
        self.nodes.retain(keep);
        self.edges.retain(keep);
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Files that share an edge with a node of a `touched` file (touched files excluded).
fn adjacent_files(g: &Graph, touched: &BTreeSet<String>) -> HashSet<String> {
    let mut near = HashSet::new();
    let source = |i: usize| attr_str(&g.nodes()[i].attributes, "source_file");
    for i in (0..g.node_count()).filter(|&i| touched.contains(source(i))) {
        for j in g
            .neighbor_indices(i, false)
            .chain(g.neighbor_indices(i, true))
        {
            let s = source(j);
            if !s.is_empty() && !touched.contains(s) {
                near.insert(s.to_string());
            }
        }
    }
    near
}

/// Re-extract `changed` (absolute paths under `root`) and splice the result
/// into `graph` in place, as graphicle's live path does, but with the
/// extraction cache under `out`.
///
/// Returns the repository-relative paths that were touched, or `None` when
/// none of the paths affects the graph. On error `graph` is left unchanged.
pub(crate) fn patch(
    graph: &mut Graph,
    context: &mut ContextCache,
    root: &Path,
    out: &Path,
    filter: &PathFilter,
    changed: &BTreeSet<PathBuf>,
    max_workers: Option<usize>,
) -> Result<Option<BTreeSet<String>>, IngestError> {
    let mut touched = BTreeSet::new();
    let mut targets = Vec::new();
    for path in changed {
        let Some(rel) = rel_of(root, path) else {
            continue;
        };
        if path.is_file() && filter.supported(path) {
            targets.push(path.clone());
        }
        touched.insert(rel);
    }
    let source_touched =
        |a: &graphicle::record::Attributes| touched.contains(attr_str(a, "source_file"));
    let removed: BTreeSet<usize> = graph
        .nodes()
        .iter()
        .enumerate()
        .filter(|(_, n)| source_touched(&n.attributes) && ast_tier(&n.attributes))
        .map(|(i, _)| i)
        .collect();
    if removed.is_empty() && targets.is_empty() {
        return Ok(None);
    }
    context.ensure(graph);
    context.drop_files(&touched);
    // Until the splice below succeeds the cache no longer matches the graph.
    context.valid = false;
    let fresh = if targets.is_empty() {
        graphicle::graph::Graph::new(true, false)
    } else {
        // Every node record is lent (the name table calls resolve against);
        // edge records only from the files adjacent to the touched ones.
        let near = adjacent_files(graph, &touched);
        let edges: Vec<Value> = context
            .edges
            .iter()
            .filter(|e| near.contains(e.get("source_file").and_then(Value::as_str).unwrap_or("")))
            .cloned()
            .collect();
        let _run = ENGINE_LOCK.lock();
        let mut options = graphicle::extract::ExtractOptions {
            root: Some(root.to_path_buf()),
            cache_root: Some(out.to_path_buf()),
            resolution_context_nodes: std::mem::take(&mut context.nodes),
            resolution_context_edges: edges,
            max_workers,
            ..Default::default()
        };
        let extracted = graphicle::extract::extract(&targets, &options);
        // `extract` only borrows the options: the node records come back intact.
        context.nodes = std::mem::take(&mut options.resolution_context_nodes);
        let mut extraction = extracted.map_err(engine_err)?;
        graphicle::build::build_from_json(
            &mut extraction,
            true,
            Some(root),
            &mut graphicle::build::Diagnostics::default(),
        )
        .map_err(engine_err)?
    };
    // Edges between a removed node and a surviving one that an unchanged file
    // declared (a caller in another file): re-attached if the endpoint returns.
    let mut kept = Vec::new();
    for (u, v, e) in graph.iter_edges() {
        if removed.contains(&u) != removed.contains(&v) && !source_touched(&e.attributes) {
            kept.push((
                graph.nodes()[u].id.clone(),
                graph.nodes()[v].id.clone(),
                e.attributes.clone(),
            ));
        }
    }
    let removed_ids: Vec<Value> = removed
        .iter()
        .map(|&i| graph.nodes()[i].id.clone())
        .collect();
    graph.remove_nodes_from(&removed_ids).map_err(engine_err)?;
    for node in fresh.nodes() {
        // Stubs of context nodes must not overwrite the resident records.
        if graph.node_index(&node.id).is_none() || source_touched(&node.attributes) {
            graph
                .add_node(node.id.clone(), node.attributes.clone())
                .map_err(engine_err)?;
        }
    }
    let mut add =
        |u: Value, v: Value, attrs: graphicle::record::Attributes| -> Result<(), IngestError> {
            if graph.node_index(&u).is_some() && graph.node_index(&v).is_some() {
                graph.add_edge(u, v, None, attrs).map_err(engine_err)?;
            }
            Ok(())
        };
    for (u, v, e) in fresh.iter_edges() {
        add(
            fresh.nodes()[u].id.clone(),
            fresh.nodes()[v].id.clone(),
            e.attributes.clone(),
        )?;
    }
    for (u, v, attrs) in kept {
        add(u, v, attrs)?;
    }
    // Follow the graph: unchanged files' edges into nodes that are gone go,
    // the re-extracted files' records come in.
    if !removed_ids.is_empty() {
        let exists = |v: &Value, k: &str| v.get(k).is_some_and(|id| graph.node_index(id).is_some());
        context
            .edges
            .retain(|e| exists(e, "source") && exists(e, "target"));
    }
    let (nodes, edges) = context_where(graph, |s| touched.contains(s));
    context.nodes.extend(nodes);
    context.edges.extend(edges);
    context.valid = true;
    Ok(Some(touched))
}

/// Leiden communities of the graph as `(node key, label ≥ 1)`. graphicle's
/// community ids start at 0, which the model reserves for "unassigned".
pub(crate) fn communities(
    graph: &Graph,
    resolution: f64,
) -> Result<Vec<(String, u32)>, IngestError> {
    let comms =
        graphicle::cluster::cluster(graph, resolution, None, "leiden").map_err(engine_err)?;
    let mut out = Vec::with_capacity(graph.node_count());
    for (cid, members) in comms {
        let label = u32::try_from(cid.max(0)).unwrap_or(u32::MAX - 1) + 1;
        out.extend(members.iter().map(|m| (node_key(m), label)));
    }
    Ok(out)
}

/// Dependents of `seed` (a node key), with graphicle's default relation set
/// walked in reverse, as `(node key, hop depth)`.
pub(crate) fn affected(
    graph: &Graph,
    seed: &str,
    depth: u32,
) -> Result<Vec<(String, u32)>, IngestError> {
    let relations: Vec<String> = graphicle::affected::DEFAULT_AFFECTED_RELATIONS
        .iter()
        .map(|s| s.to_string())
        .collect();
    let hits = graphicle::affected::affected_nodes(graph, seed, &relations, i64::from(depth))
        .map_err(engine_err)?;
    Ok(hits
        .into_iter()
        .map(|h| (h.node_id, u32::try_from(h.depth.max(0)).unwrap_or(u32::MAX)))
        .collect())
}

/// Line count of every file in `rels` under `root` (0 when unreadable).
pub(crate) fn line_counts<'a>(
    root: &Path,
    rels: impl IntoIterator<Item = &'a str>,
) -> HashMap<String, u32> {
    let mut out = HashMap::new();
    let mut seen = HashSet::new();
    for rel in rels {
        if !seen.insert(rel) {
            continue;
        }
        let n = std::fs::read(root.join(rel)).map_or(0, |b| {
            let lines = b.iter().filter(|&&c| c == b'\n').count();
            let tail = usize::from(!b.is_empty() && b.last() != Some(&b'\n'));
            u32::try_from(lines + tail).unwrap_or(u32::MAX)
        });
        out.insert(rel.to_string(), n);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_and_lenient_resolution() {
        assert_eq!(lexical(Path::new("/a/./b/../c")), PathBuf::from("/a/c"));
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().canonicalize().unwrap();
        assert_eq!(
            resolve_lenient(&dir.path().join("missing/x.rs")),
            real.join("missing/x.rs")
        );
        assert_eq!(
            rel_of(&real, &real.join("a/b.rs")).as_deref(),
            Some("a/b.rs")
        );
        assert_eq!(rel_of(&real, Path::new("/elsewhere")), None);
    }

    #[test]
    fn truthiness_matches_python() {
        assert!(!truthy(&Value::Null));
        assert!(!truthy(&json!("")));
        assert!(truthy(&json!(1)));
        assert!(!truthy(&json!(0.0)));
        assert!(truthy(&json!([1])));
    }

    #[test]
    fn noise_and_hidden_paths_are_not_visible() {
        assert!(visible(Path::new("src/lib.rs")));
        assert!(
            visible(Path::new("build.rs")),
            "a file named like a noise dir is fine"
        );
        assert!(!visible(Path::new("node_modules/x/index.ts")));
        assert!(!visible(Path::new("pkg/foo.egg-info/a.py")));
        assert!(!visible(Path::new("a/.hidden/b.rs")));
        assert!(is_noise_dir("target") && !is_noise_dir("src"));
    }
}
