//! Codescape ingest: the graphicle façade.
//!
//! This is the only Codescape crate that depends on the
//! [graphicle](https://github.com/Graphicle-AI/graphicle) code-graph engine,
//! and no graphicle type crosses its public API. An [`Ingestor`] extracts a
//! repository with graphicle, maps the node-link graph into
//! [`codescape_model`] types, builds the LOD tiers, computes Leiden
//! communities (stable-matched across recomputes) and betweenness
//! importance, then keeps the result current as files change.
//!
//! # Where things are written
//!
//! The repository root may be read-only and is never written. graphicle's
//! per-file AST cache, its build config (`.graphicle_build.json`, which
//! carries the configured excludes), the cached `graph.json` and a file
//! manifest all live under `<out_dir>/.graphicle/`. A second
//! [`Ingestor::open`] with an unchanged tree and configuration loads that
//! `graph.json` instead of extracting ([`IngestReport::from_cache`]).
//!
//! # Example
//!
//! ```no_run
//! use codescape_ingest::{IngestConfig, Ingestor};
//! use codescape_model::IdInterner;
//!
//! # fn main() -> Result<(), codescape_ingest::IngestError> {
//! let cfg = IngestConfig::new("visionclaw", "/repo", "/data/codescape/visionclaw");
//! let (mut ingest, report) = Ingestor::open(cfg, IdInterner::new(1))?;
//! println!("{} nodes, {} communities", report.nodes, report.communities);
//! if let Some(hit) = ingest.search("parse_config", 1).first() {
//!     let why = ingest.explain(hit.0, 2_000)?;
//!     println!("{}", why.summary);
//! }
//! if let Some(delta) = ingest.apply_changed(&["src/lib.rs".into()])? {
//!     println!("revision {} → {}", delta.from_revision, delta.to_revision);
//! }
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod analytics;
mod engine;
mod map;
mod view;
mod watch;

#[cfg(test)]
mod tests;

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use codescape_model::{
    GraphDelta, GraphSnapshot, IdInterner, InternError, Lang, NodeKind, Relation, Span,
};
use serde::{Deserialize, Serialize};

pub use watch::{WatchHandle, DEBOUNCE, MAX_BATCH_WAIT};

/// How to ingest one repository.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IngestConfig {
    /// Repository label carried into [`GraphSnapshot::repo`].
    pub repo_label: String,
    /// Repository root. Read, never written.
    pub root: PathBuf,
    /// Writable directory outside the repository for graphicle's cache and output.
    pub out_dir: PathBuf,
    /// Include prose (Markdown, reStructuredText, PDFs…) as well as code.
    pub include_docs: bool,
    /// Extra gitignore-style exclude patterns, relative to `root`.
    pub excludes: Vec<String>,
    /// Extraction worker cap (graphicle's default when `None`).
    pub max_workers: Option<usize>,
    /// Leiden resolution (1.0 = standard modularity).
    pub leiden_resolution: f64,
    /// Betweenness sample size used above 20,000 nodes (512 when `None`).
    pub centrality_sample: Option<usize>,
}

impl IngestConfig {
    /// A configuration with the defaults: code only, no extra excludes,
    /// default workers, resolution 1.0, default centrality sample.
    pub fn new(
        repo_label: impl Into<String>,
        root: impl Into<PathBuf>,
        out_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            repo_label: repo_label.into(),
            root: root.into(),
            out_dir: out_dir.into(),
            include_docs: false,
            excludes: Vec::new(),
            max_workers: None,
            leiden_resolution: 1.0,
            centrality_sample: None,
        }
    }
}

/// What [`Ingestor::open`] did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IngestReport {
    /// Files in the code graph (after ignore rules, excludes and the docs filter).
    pub files: usize,
    /// Inputs graphicle had no extractor for.
    pub unsupported: usize,
    /// Nodes in the snapshot, directory aggregates included.
    pub nodes: usize,
    /// Edges in the snapshot, synthetic containment edges included.
    pub edges: usize,
    /// Distinct non-zero communities.
    pub communities: usize,
    /// Wall time of the whole open, in milliseconds.
    pub duration_ms: u64,
    /// File count per language.
    pub languages: HashMap<Lang, usize>,
    /// `true` when the cached `graph.json` was loaded instead of extracting.
    pub from_cache: bool,
}

/// A slice of source text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snippet {
    /// Repository-relative file path.
    pub path: String,
    /// Lines covered by `text` (inclusive, 1-based).
    pub span: Span,
    /// The source text, possibly truncated to the caller's budget.
    pub text: String,
}

/// Source and graph facts about one node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Explanation {
    /// The node explained.
    pub node: u32,
    /// The node's own source (empty for directories and external symbols).
    pub snippets: Vec<Snippet>,
    /// Deterministic summary built from graph facts only.
    pub summary: String,
}

/// Errors from ingestion and queries.
#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    /// A filesystem operation failed.
    #[error("i/o error on {path}: {source}")]
    Io {
        /// The path involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// graphicle reported an error (message preserved; the type stays private).
    #[error("graph engine: {0}")]
    Engine(String),
    /// The wire-id space ran out.
    #[error(transparent)]
    Intern(#[from] InternError),
    /// The file watcher could not be started.
    #[error("watcher: {0}")]
    Watch(String),
    /// No node has this id.
    #[error("unknown node id {0}")]
    UnknownNode(u32),
    /// The configuration cannot be used.
    #[error("invalid configuration: {0}")]
    Config(String),
    /// A produced snapshot failed [`GraphSnapshot::validate`].
    #[error("snapshot invariant violated: {0}")]
    Invariant(String),
}

/// A live, queryable code graph of one repository.
pub struct Ingestor {
    cfg: IngestConfig,
    root: PathBuf,
    out: PathBuf,
    filter: Arc<engine::PathFilter>,
    interner: IdInterner,
    graph: engine::Graph,
    /// Resolution context of `graph`, kept between live patches.
    context: engine::ContextCache,
    /// Files live-patched since the last exact rebuild.
    unsettled: BTreeSet<String>,
    /// Line count per repository-relative file path.
    loc: HashMap<String, u32>,
    snapshot: GraphSnapshot,
    view: view::View,
}

impl std::fmt::Debug for Ingestor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ingestor")
            .field("repo", &self.cfg.repo_label)
            .field("root", &self.root)
            .field("revision", &self.snapshot.revision)
            .field("nodes", &self.snapshot.nodes.len())
            .field("edges", &self.snapshot.edges.len())
            .finish()
    }
}

// The server moves the ingestor onto its own thread and shares it behind a lock.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Ingestor>();
    assert_send_sync::<WatchHandle>();
};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

impl Ingestor {
    /// Cold extract (or load graphicle's cached `graph.json` under the out
    /// dir) into a snapshot with tiers built and communities and centrality
    /// computed. The snapshot starts at revision 1 in `interner`'s epoch.
    pub fn open(
        cfg: IngestConfig,
        interner: IdInterner,
    ) -> Result<(Self, IngestReport), IngestError> {
        let started = Instant::now();
        if !(cfg.leiden_resolution.is_finite() && cfg.leiden_resolution > 0.0) {
            return Err(IngestError::Config(format!(
                "leiden_resolution must be positive, got {}",
                cfg.leiden_resolution
            )));
        }
        let root = cfg.root.canonicalize().map_err(|e| IngestError::Io {
            path: cfg.root.clone(),
            source: e,
        })?;
        if !root.is_dir() {
            return Err(IngestError::Config(format!(
                "{} is not a directory",
                root.display()
            )));
        }
        // Judge the out dir before creating anything: a bad one must not leave
        // a directory behind inside the repository.
        if engine::resolve_lenient(&cfg.out_dir).starts_with(&root) {
            return Err(IngestError::Config(format!(
                "out_dir {} must be outside the repository {}",
                cfg.out_dir.display(),
                root.display()
            )));
        }
        std::fs::create_dir_all(&cfg.out_dir).map_err(|e| IngestError::Io {
            path: cfg.out_dir.clone(),
            source: e,
        })?;
        let out = cfg.out_dir.canonicalize().map_err(|e| IngestError::Io {
            path: cfg.out_dir.clone(),
            source: e,
        })?;
        if out.starts_with(&root) {
            return Err(IngestError::Config(format!(
                "out_dir {} must be outside the repository {}",
                out.display(),
                root.display()
            )));
        }
        let graphicle_out = graphicle_out_dir(&out);
        let filter = Arc::new(engine::PathFilter::new(
            &root,
            &out,
            &graphicle_out,
            cfg.include_docs,
            &cfg.excludes,
        )?);
        let files = filter.collect()?;
        let mut manifest = engine::Manifest::of(&root, &files, cfg.include_docs, &cfg.excludes)?;
        let (graph, unsupported, from_cache) = match engine::load_cached(&graphicle_out, &manifest)
        {
            Some((graph, unsupported)) => (graph, unsupported, true),
            None => {
                let (graph, unsupported) =
                    engine::extract_full(&root, &out, &files, cfg.max_workers)?;
                manifest.unsupported = unsupported;
                engine::store_cached(&graphicle_out, &graph, &manifest)?;
                (graph, unsupported, false)
            }
        };
        let rels: Vec<String> = files
            .iter()
            .filter_map(|f| engine::rel_of(&root, f))
            .collect();
        let loc = engine::line_counts(&root, rels.iter().map(String::as_str));
        let mut languages: HashMap<Lang, usize> = HashMap::new();
        for r in &rels {
            *languages.entry(Lang::from_path(r)).or_default() += 1;
        }

        let mut me = Self {
            root,
            out,
            filter,
            interner,
            graph,
            context: engine::ContextCache::default(),
            unsettled: BTreeSet::new(),
            loc,
            snapshot: GraphSnapshot::default(),
            view: view::View::default(),
            cfg,
        };
        let mut snapshot = map::assemble(&me.graph, &mut me.interner, None, &me.loc)?.snapshot;
        snapshot.repo = Arc::from(me.cfg.repo_label.as_str());
        snapshot.epoch = me.interner.epoch();
        snapshot.revision = 1;
        snapshot.generated_at_ms = now_ms();
        me.analyse(&mut snapshot)?;
        snapshot.validate().map_err(IngestError::Invariant)?;
        // Built now so the first live edit does not pay for it.
        me.context.ensure(&me.graph);
        me.view = view::View::build(&snapshot);
        me.snapshot = snapshot;

        let report = IngestReport {
            files: rels.len(),
            unsupported,
            nodes: me.snapshot.nodes.len(),
            edges: me.snapshot.edges.len(),
            communities: me.community_count(),
            duration_ms: started.elapsed().as_millis() as u64,
            languages,
            from_cache,
        };
        tracing::info!(repo = %me.cfg.repo_label, ?report, "codescape ingest opened");
        Ok((me, report))
    }

    /// The current snapshot.
    pub fn snapshot(&self) -> &GraphSnapshot {
        &self.snapshot
    }

    /// The id table every snapshot id comes from (the server persists it).
    pub fn interner(&self) -> &IdInterner {
        &self.interner
    }

    /// The canonical repository root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The configuration this ingestor was opened with.
    pub fn config(&self) -> &IngestConfig {
        &self.cfg
    }

    fn community_count(&self) -> usize {
        self.snapshot
            .nodes
            .iter()
            .map(|n| n.community)
            .filter(|&c| c != 0)
            .collect::<BTreeSet<_>>()
            .len()
    }

    /// Leiden communities (stable-matched against the labels already on
    /// `snapshot`) and normalised betweenness. Returns the retained fraction.
    fn analyse(&self, snapshot: &mut GraphSnapshot) -> Result<f32, IngestError> {
        let labels: Vec<(u32, u32)> = engine::communities(&self.graph, self.cfg.leiden_resolution)?
            .into_iter()
            .filter_map(|(key, label)| self.interner.get(&key).map(|id| (id, label)))
            .collect();
        let importance = analytics::betweenness(snapshot, self.cfg.centrality_sample);
        for (n, v) in snapshot.nodes.iter_mut().zip(importance) {
            n.importance = v;
        }
        Ok(analytics::apply_communities(snapshot, &labels))
    }

    /// Move to `next` (revision already set) through a delta, so the stored
    /// snapshot keeps the node order every delta consumer reproduces.
    fn commit(&mut self, next: &GraphSnapshot) -> Result<Option<GraphDelta>, IngestError> {
        let delta = GraphDelta::diff(&self.snapshot, next);
        if delta.is_empty() {
            return Ok(None);
        }
        let mut applied = self.snapshot.clone();
        delta.apply(&mut applied).map_err(IngestError::Invariant)?;
        applied.validate().map_err(IngestError::Invariant)?;
        self.view = view::View::build(&applied);
        self.snapshot = applied;
        Ok(Some(delta))
    }

    /// Absolute, canonical form of a changed path (absolute or root-relative).
    fn normalise(&self, path: &Path) -> PathBuf {
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        engine::resolve_lenient(&abs)
    }

    /// Re-extract these files only (a live patch of the resident graph),
    /// re-map the nodes of those files and return the delta, or `None` when
    /// nothing changed. Paths may be absolute or relative to the root; paths
    /// outside the root, ignored, excluded or unsupported are skipped. A
    /// deleted file removes its nodes; a directory stands for every code file
    /// below it (moved in) or every indexed file that was below it (moved out). Communities and importance of existing
    /// nodes are kept (see [`Ingestor::recompute_analytics`]); new nodes join
    /// the most common community of their neighbours. Revision + 1.
    pub fn apply_changed(&mut self, paths: &[PathBuf]) -> Result<Option<GraphDelta>, IngestError> {
        let mut changed: BTreeSet<PathBuf> = BTreeSet::new();
        for path in paths {
            let abs = self.normalise(path);
            if abs.is_dir() {
                // A directory moved in (or created whole): every code file below it.
                if self.filter.admits_dir(&abs) {
                    changed.extend(self.filter.collect_under(&abs)?);
                }
            } else if self.filter.admits(&abs) || self.indexed(&abs) {
                changed.insert(abs);
            } else if !abs.exists() {
                // A directory moved out or deleted: every file indexed below it.
                if let Some(rel) = engine::rel_of(&self.root, &abs) {
                    let prefix = format!("{rel}/");
                    changed.extend(
                        self.view
                            .files
                            .keys()
                            .filter(|f| f.starts_with(&prefix))
                            .map(|f| self.root.join(&**f)),
                    );
                }
            }
        }
        if changed.is_empty() {
            return Ok(None);
        }
        let Some(touched) = engine::patch(
            &mut self.graph,
            &mut self.context,
            &self.root,
            &self.out,
            &self.filter,
            &changed,
            self.cfg.max_workers,
        )?
        else {
            return Ok(None);
        };
        self.unsettled.extend(touched.iter().cloned());
        for rel in &touched {
            self.loc.remove(rel);
        }
        let present: Vec<&str> = touched
            .iter()
            .map(String::as_str)
            .filter(|r| self.root.join(r).is_file())
            .collect();
        self.loc.extend(engine::line_counts(&self.root, present));

        let assembly = map::assemble(
            &self.graph,
            &mut self.interner,
            Some((&self.snapshot, &touched)),
            &self.loc,
        )?;
        let mut next = assembly.snapshot;
        let mut new_ids: Vec<u32> = assembly.fresh.into_iter().collect();
        new_ids.sort_unstable();
        analytics::fill_new(&mut next, &new_ids);
        analytics::derive_aggregates(&mut next);
        next.repo = self.snapshot.repo.clone();
        next.epoch = self.snapshot.epoch;
        next.revision = self.snapshot.revision + 1;
        next.generated_at_ms = now_ms();
        Ok(self.commit(&next)?.map(|mut d| {
            d.changed_paths = touched.into_iter().collect();
            d
        }))
    }

    /// Whether a path currently has a file node (lets a now-ignored file be removed).
    fn indexed(&self, abs: &Path) -> bool {
        engine::rel_of(&self.root, abs).is_some_and(|r| self.view.files.contains_key(r.as_str()))
    }

    /// Recompute Leiden communities and betweenness, stable-match the labels
    /// against the current ones, and return the delta of changed nodes (its
    /// revision moves forward by one even when nothing changed) and the
    /// fraction of nodes that kept their community.
    ///
    /// If files were live-patched since the last exact build, the graph is
    /// first settled: graphicle re-assembles the whole corpus from its per-file
    /// cache (only changed files are parsed again), which resolves what a
    /// single-file patch cannot see (a `use crate::other` that pointed at an
    /// external stub until the corpus was re-assembled), and the cached
    /// `graph.json` is refreshed. That correction travels in the same delta.
    pub fn recompute_analytics(&mut self) -> Result<(GraphDelta, f32), IngestError> {
        let mut next = if self.unsettled.is_empty() {
            self.snapshot.clone()
        } else {
            self.settle()?
        };
        let retained = self.analyse(&mut next)?;
        next.repo = self.snapshot.repo.clone();
        next.epoch = self.snapshot.epoch;
        next.revision = self.snapshot.revision + 1;
        next.generated_at_ms = now_ms();
        let delta = match self.commit(&next)? {
            Some(d) => d,
            None => {
                let d = GraphDelta {
                    from_revision: self.snapshot.revision,
                    to_revision: next.revision,
                    generated_at_ms: next.generated_at_ms,
                    ..Default::default()
                };
                self.snapshot.revision = next.revision;
                self.snapshot.generated_at_ms = next.generated_at_ms;
                d
            }
        };
        Ok((delta, retained))
    }

    /// Exact rebuild after live patches: re-extract the corpus through the
    /// per-file cache, replace the resident graph, store `graph.json`, and map
    /// every node again (carrying communities and importance by id).
    fn settle(&mut self) -> Result<GraphSnapshot, IngestError> {
        let files = self.filter.collect()?;
        let mut manifest = engine::Manifest::of(
            &self.root,
            &files,
            self.cfg.include_docs,
            &self.cfg.excludes,
        )?;
        let (graph, unsupported) =
            engine::extract_full(&self.root, &self.out, &files, self.cfg.max_workers)?;
        manifest.unsupported = unsupported;
        engine::store_cached(&graphicle_out_dir(&self.out), &graph, &manifest)?;
        let rels: Vec<String> = files
            .iter()
            .filter_map(|f| engine::rel_of(&self.root, f))
            .collect();
        self.loc = engine::line_counts(&self.root, rels.iter().map(String::as_str));
        self.graph = graph;
        self.context.invalidate();
        self.context.ensure(&self.graph);
        // Every path counts as touched, so every node is mapped from the new graph.
        let mut every: BTreeSet<String> = rels.into_iter().collect();
        every.extend(std::mem::take(&mut self.unsettled));
        let assembly = map::assemble(
            &self.graph,
            &mut self.interner,
            Some((&self.snapshot, &every)),
            &self.loc,
        )?;
        Ok(assembly.snapshot)
    }

    /// Start a debounced ([`DEBOUNCE`], 250 ms; at most [`MAX_BATCH_WAIT`],
    /// 1 s, after the first relevant change) watcher on the root that sends
    /// batches of changed paths (absolute) that pass the same filter as the
    /// cold walk: `.gitignore`, the configured excludes, supported extensions.
    /// Dropping the returned handle stops it.
    pub fn watch(
        &self,
        tx: crossbeam_channel::Sender<Vec<PathBuf>>,
    ) -> Result<WatchHandle, IngestError> {
        watch::spawn(&self.root, self.filter.clone(), tx)
    }

    /// Ranked `(id, score)` matches of `query` against labels and file paths.
    pub fn search(&self, query: &str, limit: usize) -> Vec<(u32, f32)> {
        self.view.search(&self.snapshot, query, limit)
    }

    /// Neighbours of `id` as `(neighbour id, relation)`, optionally limited to
    /// some relations and to one direction.
    pub fn neighbors(
        &self,
        id: u32,
        relations: Option<&[Relation]>,
        incoming: bool,
        outgoing: bool,
    ) -> Vec<(u32, Relation)> {
        self.view
            .neighbors(&self.snapshot, id, relations, incoming, outgoing)
    }

    /// Nodes that depend on `id` within `depth` hops, as `(node, hop depth)`:
    /// graphicle's reverse walk over calls, references, imports and
    /// inheritance (members of a file or class are seeded at depth 0). A
    /// directory walks from each of its files.
    pub fn affected(&self, id: u32, depth: u32) -> Vec<(u32, u32)> {
        let Some(&k) = self.view.pos.get(&id) else {
            return Vec::new();
        };
        let node = &self.snapshot.nodes[k];
        let seeds: Vec<Arc<str>> = if node.kind == NodeKind::Directory {
            self.snapshot
                .nodes
                .iter()
                .filter(|n| n.parent == id && n.kind == NodeKind::File)
                .map(|n| n.key.clone())
                .collect()
        } else {
            vec![node.key.clone()]
        };
        let mut best: HashMap<u32, u32> = HashMap::new();
        for seed in seeds {
            let hits = match engine::affected(&self.graph, &seed, depth) {
                Ok(h) => h,
                Err(e) => {
                    tracing::warn!(%e, seed = %seed, "affected walk failed");
                    continue;
                }
            };
            for (key, d) in hits {
                if let Some(hit) = self
                    .interner
                    .get(&key)
                    .filter(|h| *h != id && self.view.pos.contains_key(h))
                {
                    let e = best.entry(hit).or_insert(d);
                    *e = (*e).min(d);
                }
            }
        }
        let mut out: Vec<(u32, u32)> = best.into_iter().collect();
        out.sort_unstable_by_key(|&(n, d)| (d, n));
        out
    }

    /// Shortest undirected path between two nodes (bidirectional BFS,
    /// directory aggregates not traversed), endpoints included.
    pub fn shortest_path(&self, from: u32, to: u32) -> Option<Vec<u32>> {
        self.view.shortest_path(&self.snapshot, from, to)
    }

    /// Undirected BFS from `from` up to `max_depth` hops, as
    /// `(node, hop distance, parent id)` for the SSSP fields of the wire
    /// record. The source comes first with parent 0.
    pub fn bfs_from(&self, from: u32, max_depth: u32) -> Vec<(u32, f32, i32)> {
        self.view.bfs_from(&self.snapshot, from, max_depth)
    }

    /// The node's source (read from the root, truncated to `budget_chars`
    /// characters) and a deterministic summary: kind, location, degree by
    /// relation, community, importance and the top callers and callees.
    pub fn explain(&self, id: u32, budget_chars: usize) -> Result<Explanation, IngestError> {
        let k = *self.view.pos.get(&id).ok_or(IngestError::UnknownNode(id))?;
        let node = &self.snapshot.nodes[k];
        let mut snippets = Vec::new();
        if let (Some(path), Some(span), false) =
            (&node.path, node.span, node.kind == NodeKind::Directory)
        {
            let file = self.root.join(&**path);
            let text = std::fs::read_to_string(&file).map_err(|e| IngestError::Io {
                path: file.clone(),
                source: e,
            })?;
            let lines: Vec<&str> = text.lines().collect();
            let start = span.start_line.max(1) as usize;
            let end = (span.end_line as usize).min(lines.len());
            if start <= end {
                let body = lines[start - 1..end].join("\n");
                let (text, shown_end) = truncate_lines(&body, budget_chars, start);
                snippets.push(Snippet {
                    path: path.to_string(),
                    span: Span {
                        start_line: span.start_line,
                        end_line: shown_end,
                    },
                    text,
                });
            }
        }
        Ok(Explanation {
            node: id,
            snippets,
            summary: self.summary(k),
        })
    }

    fn summary(&self, k: usize) -> String {
        let s = &self.snapshot;
        let node = &s.nodes[k];
        let mut out = format!("{} `{}`", node.kind.as_str(), node.label);
        match (&node.path, node.span) {
            (Some(p), Some(sp)) if node.kind != NodeKind::Directory => {
                let _ = write!(out, " at {p}:{}-{}", sp.start_line, sp.end_line);
            }
            (Some(p), _) => {
                let _ = write!(out, " at {p}");
            }
            _ => {}
        }
        if node.lang != Lang::Other {
            let _ = write!(out, " ({})", node.lang.as_str());
        }
        out.push('.');
        let degree = |edges: &[u32], incoming: bool| {
            let mut by: std::collections::BTreeMap<String, usize> =
                std::collections::BTreeMap::new();
            for &e in edges {
                let e = &s.edges[e as usize];
                let other = if incoming { e.source } else { e.target };
                if self
                    .view
                    .pos
                    .get(&other)
                    .is_some_and(|&o| s.nodes[o].kind != NodeKind::Directory)
                {
                    *by.entry(e.relation.as_str().to_string()).or_default() += 1;
                }
            }
            if by.is_empty() {
                "none".to_string()
            } else {
                by.iter()
                    .map(|(r, c)| format!("{r} {c}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        };
        let _ = write!(
            out,
            " Out: {}. In: {}.",
            degree(&self.view.out[k], false),
            degree(&self.view.inc[k], true)
        );
        let _ = write!(
            out,
            " Community {}, importance {:.2}.",
            node.community, node.importance
        );
        if let Some(parent) = self.view.pos.get(&node.parent).map(|&p| &s.nodes[p]) {
            let _ = write!(out, " Inside {} `{}`.", parent.kind.as_str(), parent.label);
        }
        let ranked = |edges: &[u32], incoming: bool| {
            let mut v: Vec<&codescape_model::CodeNode> = edges
                .iter()
                .map(|&e| &s.edges[e as usize])
                .filter(|e| e.relation == Relation::Calls)
                .filter_map(|e| {
                    self.view
                        .pos
                        .get(&if incoming { e.source } else { e.target })
                        .map(|&p| &s.nodes[p])
                })
                .collect();
            v.sort_by(|a, b| {
                b.importance
                    .total_cmp(&a.importance)
                    .then(a.label.cmp(&b.label))
                    .then(a.id.cmp(&b.id))
            });
            v.dedup_by_key(|n| n.id);
            v.iter()
                .take(5)
                .map(|n| format!("{} ({:.2})", n.label, n.importance))
                .collect::<Vec<_>>()
        };
        let callers = ranked(&self.view.inc[k], true);
        let callees = ranked(&self.view.out[k], false);
        if !callers.is_empty() {
            let _ = write!(out, " Top callers: {}.", callers.join(", "));
        }
        if !callees.is_empty() {
            let _ = write!(out, " Top callees: {}.", callees.join(", "));
        }
        out
    }

    /// The file node of a repository-relative path (`./` and `\` tolerated).
    pub fn file_id(&self, rel_path: &str) -> Option<u32> {
        let p = rel_path.replace('\\', "/");
        let p = p.trim_start_matches("./");
        self.view.files.get(p).copied()
    }
}

/// Where graphicle puts its cache for `cache_root = out`: `<out>/.graphicle`,
/// or `GRAPHICLE_OUT` when that names an absolute directory.
fn graphicle_out_dir(out: &Path) -> PathBuf {
    let name = Path::new(graphicle::paths::OUT_DIR.as_str());
    if name.is_absolute() {
        name.to_path_buf()
    } else {
        out.join(name)
    }
}

/// Cut `body` (starting at line `first`) to at most `budget` characters on a
/// character boundary, preferring a line boundary. Returns the text and the
/// last line it covers.
fn truncate_lines(body: &str, budget: usize, first: usize) -> (String, u32) {
    let total_lines = body.lines().count().max(1);
    if body.chars().count() <= budget {
        return (body.to_string(), (first + total_lines - 1) as u32);
    }
    let cut: String = body.chars().take(budget).collect();
    let text = match cut.rfind('\n') {
        Some(nl) if nl > 0 => cut[..nl].trim_end().to_string(),
        _ => cut.trim_end().to_string(),
    };
    let shown = text.lines().count().max(1);
    (text, (first + shown - 1) as u32)
}
