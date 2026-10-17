//! The per-repository core loop: ingest thread, layout thread, analytics timer,
//! published snapshots and positions, and persistence.
//!
//! Threads never block actix workers. The ingest thread owns re-extraction
//! (watcher batches and hook-queued paths), the layout thread owns the engine
//! and steps it at up to 240 Hz until settled. Both publish immutable `Arc`s
//! that request handlers and per-client frame tasks read without waiting on
//! the work itself.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use codescape_collab::{ActorKind, ActorRegistry, TimelineLog};
use codescape_ingest::{IngestConfig, Ingestor};
use codescape_layout::{select_engine, LayoutEngine, LayoutParams, PositionSet};
use codescape_model::events::{PresenceEvent, ServerEvent};
use codescape_model::wire::{actor_record, record_for};
use codescape_model::{GraphDelta, GraphSnapshot, IdInterner, NodeKind, NodeRecord};
use codescape_server::cache::{ids_key, BoundedCache};
use codescape_server::config::{Config, DataLayout, RepoSpec};
use codescape_server::ingest_api::{truncate_chars, SHARED_TEXT_CHARS};
use codescape_server::now_ms;
use codescape_server::persist::{atomic_write, RotatingLog, WarmPositions};
use crossbeam_channel::{Receiver, Sender};
use parking_lot::{Mutex, RwLock};
use tokio::sync::broadcast;

/// Analytics (Leiden + centrality) recompute cadence while dirty.
pub const ANALYTICS_EVERY: Duration = Duration::from_secs(30);
/// Layout step-rate ceiling.
pub const MAX_STEP_HZ: f64 = 240.0;
/// Positions are published at most this often (frame tasks run at ≤ 30 Hz).
pub const PUBLISH_EVERY: Duration = Duration::from_millis(16);
/// Warm positions are persisted at most this often while the layout runs.
pub const PERSIST_POSITIONS_EVERY: Duration = Duration::from_secs(30);
/// Steps a focus-masked burst runs before the mask is lifted for a global settle.
pub const FOCUS_BURST_STEPS: u32 = 240;
/// Hop limit for per-client SSSP from the focus.
pub const SSSP_MAX_DEPTH: u32 = 8;
/// Hop distance and parent per node, from one focus.
pub type SsspOverlay = Arc<HashMap<u32, (f32, i32)>>;

/// Re-extraction batches queued before the oldest is dropped.
pub const CHANGE_QUEUE: usize = 1024;
/// Serialised snapshot bodies kept per repository.
pub const SNAPSHOT_CACHE: usize = 8;
/// Projections kept per repository (one per distinct tier + focus set).
pub const PROJECTION_CACHE: usize = 16;
/// SSSP overlays kept per repository (one per distinct focus).
pub const SSSP_CACHE: usize = 64;

/// Bounded queue of paths to re-extract. Hook ingest never blocks: when full, the
/// oldest batch is dropped and counted (the next watcher event or hook for those
/// files re-queues them). The file watcher sends through [`ChangeQueue::sender`]
/// and blocks when full, which is natural back-pressure on a separate thread.
pub struct ChangeQueue {
    tx: Sender<Vec<PathBuf>>,
    rx: Receiver<Vec<PathBuf>>,
    dropped: AtomicU64,
}

impl ChangeQueue {
    /// A queue of `cap` batches and its consuming end.
    pub fn new(cap: usize) -> (Self, Receiver<Vec<PathBuf>>) {
        let (tx, rx) = crossbeam_channel::bounded(cap.max(1));
        (
            Self {
                tx,
                rx: rx.clone(),
                dropped: AtomicU64::new(0),
            },
            rx,
        )
    }

    /// Queue a batch, dropping the oldest when full.
    pub fn push(&self, paths: Vec<PathBuf>) {
        let mut item = paths;
        loop {
            match self.tx.try_send(item) {
                Ok(()) => return,
                Err(crossbeam_channel::TrySendError::Full(back)) => {
                    item = back;
                    if self.rx.try_recv().is_ok() {
                        self.dropped.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Err(crossbeam_channel::TrySendError::Disconnected(_)) => return,
            }
        }
    }

    /// Batches dropped so far.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// A blocking sender for the file watcher.
    pub fn sender(&self) -> Sender<Vec<PathBuf>> {
        self.tx.clone()
    }
}

/// Most actors one repository's registry holds.
pub const MAX_ACTORS: usize = 1024;
/// Compaction evicts down to this many actors.
pub const EVICT_TO: usize = 768;
/// Only actors unseen for this long may be evicted.
pub const STALE_ACTOR_MS: u64 = 10 * 60 * 1000;
/// Broadcast channel depth for JSON events.
const EVENT_CHANNEL: usize = 1024;

/// An immutable published snapshot with lookup tables.
pub struct Published {
    /// The snapshot.
    pub snapshot: Arc<GraphSnapshot>,
    /// Node id → index in `snapshot.nodes`.
    pub index: HashMap<u32, usize>,
    /// Repo-relative file path → file node id.
    pub files: HashMap<Arc<str>, u32>,
}

impl Published {
    fn new(snapshot: Arc<GraphSnapshot>) -> Self {
        let index = snapshot.index();
        let files = snapshot
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::File)
            .filter_map(|n| n.path.clone().map(|p| (p, n.id)))
            .collect();
        Self {
            snapshot,
            index,
            files,
        }
    }
}

/// Positions published by the layout thread.
#[derive(Default)]
pub struct PositionsFrame {
    /// Bumped on every publish.
    pub version: u64,
    /// Node id → index into `pos`/`vel`.
    pub index: Arc<HashMap<u32, usize>>,
    /// Positions.
    pub pos: Vec<[f32; 3]>,
    /// Velocities.
    pub vel: Vec<[f32; 3]>,
}

/// Layout state for `/api/health` and `/api/graph/settlement`.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct LayoutStatus {
    /// `cpu` or `cuda`.
    pub engine: String,
    /// Why that engine was chosen.
    pub reason: String,
    /// Kinetic energy of the last step.
    pub energy: f32,
    /// Steps since boot.
    pub steps: u64,
    /// Settled for the current revision.
    pub settled: bool,
    /// Last step duration in ms.
    pub step_ms: f32,
}

/// An actor id and the node it is focused on.
type ActorFocus = (u32, Option<u32>);

/// Commands to the layout thread.
pub enum LayoutCmd {
    /// Topology changed.
    Delta(Arc<GraphSnapshot>, GraphDelta),
    /// Analytics changed node attributes (springs depend on communities).
    Reload(Arc<GraphSnapshot>),
    /// Burst-settle around these ids.
    Focus(Vec<u32>),
}

/// Mutable collaboration state of a repository.
pub struct Collab {
    /// Actors, focus and heat.
    pub registry: ActorRegistry,
    /// Bounded presence/revision log.
    pub timeline: TimelineLog,
}

/// Per-tick records shared by every client of a repository.
pub struct BaseRecords {
    /// Cache key: (positions version, revision, heat bucket, actor/focus signature).
    pub key: (u64, u64, u64, u64),
    /// Aligned with `published.snapshot.nodes`; `None` until the node has a position.
    pub records: Vec<Option<NodeRecord>>,
    /// Actor records (agent-flagged), placed at their focus.
    pub actors: Vec<NodeRecord>,
    /// The publication these records were built from.
    pub published: Arc<Published>,
}

/// Everything about one served repository.
pub struct RepoEngine {
    /// Label from `--repo`.
    pub label: String,
    /// Repository root.
    pub root: PathBuf,
    /// Data paths.
    pub data: DataLayout,
    /// The ingestor (graph queries go through it).
    pub ingest: RwLock<Ingestor>,
    /// Latest published snapshot.
    pub published: RwLock<Arc<Published>>,
    /// Latest published positions.
    pub positions: RwLock<Arc<PositionsFrame>>,
    /// Layout status.
    pub layout: RwLock<LayoutStatus>,
    /// Actors, heat and timeline.
    pub collab: Mutex<Collab>,
    /// JSON events for `/ws/events`.
    pub events: broadcast::Sender<Arc<ServerEvent>>,
    /// `0x23` frames for `/wss`.
    pub actions: broadcast::Sender<Arc<Vec<u8>>>,
    /// Paths to re-extract (watcher and hook ingest).
    pub changes: ChangeQueue,
    layout_tx: Sender<LayoutCmd>,
    base: Mutex<Option<Arc<BaseRecords>>>,
    /// Held while one caller builds the shared base, so concurrent clients wait and reuse it.
    base_build: Mutex<()>,
    snapshot_bodies: BoundedCache<(u64, u8, u64), bytes::Bytes>,
    initial_loads: BoundedCache<(u64, u64), Arc<String>>,
    projections: BoundedCache<(u64, u8, u64), Arc<Vec<u32>>>,
    sssp: BoundedCache<(u32, u64), SsspOverlay>,
    analytics_dirty: AtomicBool,
    timeline_file: Mutex<Option<RotatingLog>>,
    /// `--share-agent-text`: keep full presence text instead of a 280-character caption.
    share_agent_text: bool,
    /// Interner epoch.
    pub epoch: AtomicU64,
    /// Ids handed out by the interner (updated with every `ids.bin` write), so
    /// actor allocation never waits on the ingest lock from an async worker.
    interner_high: AtomicU32,
}

/// Receivers handed to the worker threads at spawn.
pub struct EngineThreads {
    changed_rx: Receiver<Vec<PathBuf>>,
    layout_rx: Receiver<LayoutCmd>,
    params: LayoutParams,
    prefer_cuda: bool,
}

fn load_interner(data: &DataLayout, reextract: bool) -> IdInterner {
    let existing = std::fs::read(data.ids()).ok().and_then(|b| match IdInterner::from_bytes(&b) {
        Ok(i) => Some(i),
        Err(e) => {
            tracing::warn!(error = %e, path = %data.ids().display(), "ignoring unreadable interner");
            None
        }
    });
    match (existing, reextract) {
        (Some(old), true) => {
            let next = old.next_epoch();
            tracing::info!(
                from = old.epoch(),
                to = next.epoch(),
                "re-extract: id epoch bumped"
            );
            next
        }
        (Some(old), false) => old,
        (None, _) => IdInterner::new(1),
    }
}

impl RepoEngine {
    /// Open a repository: load or bump the interner, extract (or load graphicle's
    /// cache), publish, and persist the ids. Blocking; call before serving.
    pub fn open(cfg: &Config, spec: &RepoSpec) -> anyhow::Result<(Arc<Self>, EngineThreads)> {
        let data = cfg.layout_for(&spec.label);
        std::fs::create_dir_all(&data.dir)?;
        if cfg.reextract {
            let g = data.graphicle();
            if g.exists() {
                std::fs::remove_dir_all(&g)?;
            }
            let _ = std::fs::remove_file(data.positions());
        }
        std::fs::create_dir_all(data.graphicle())?;
        let interner = load_interner(&data, cfg.reextract);
        let icfg = IngestConfig {
            repo_label: spec.label.clone(),
            root: spec.root.clone(),
            out_dir: data.graphicle(),
            include_docs: cfg.include_docs,
            excludes: cfg.excludes.clone(),
            max_workers: None,
            leiden_resolution: 1.0,
            centrality_sample: Some(512),
        };
        let started = Instant::now();
        let (ingestor, report) = Ingestor::open(icfg, interner)
            .map_err(|e| anyhow::anyhow!("ingest {}: {e}", spec.label))?;
        tracing::info!(
            repo = %spec.label,
            files = report.files,
            nodes = report.nodes,
            edges = report.edges,
            communities = report.communities,
            from_cache = report.from_cache,
            ms = started.elapsed().as_millis() as u64,
            "repository ingested"
        );
        let snapshot = Arc::new(ingestor.snapshot().clone());
        let epoch = ingestor.interner().epoch();
        let interner_len = ingestor.interner().len() as u32;
        atomic_write(&data.ids(), &ingestor.interner().to_bytes())?;

        let (changes, changed_rx) = ChangeQueue::new(CHANGE_QUEUE);
        let (layout_tx, layout_rx) = crossbeam_channel::unbounded();
        let (events, _) = broadcast::channel(EVENT_CHANNEL);
        let (actions, _) = broadcast::channel(EVENT_CHANNEL);
        let timeline_file = match RotatingLog::open(
            &data.timeline(),
            RotatingLog::DEFAULT_MAX_BYTES,
            RotatingLog::DEFAULT_KEEP,
        ) {
            Ok(l) => Some(l),
            Err(e) => {
                tracing::warn!(error = %e, "timeline.jsonl unavailable; timeline kept in memory only");
                None
            }
        };
        let engine = Arc::new(Self {
            label: spec.label.clone(),
            root: spec.root.clone(),
            data,
            ingest: RwLock::new(ingestor),
            published: RwLock::new(Arc::new(Published::new(snapshot))),
            positions: RwLock::new(Arc::new(PositionsFrame::default())),
            layout: RwLock::new(LayoutStatus::default()),
            collab: Mutex::new(Collab {
                registry: ActorRegistry::new(),
                timeline: TimelineLog::new(100_000),
            }),
            events,
            actions,
            changes,
            layout_tx,
            base: Mutex::new(None),
            base_build: Mutex::new(()),
            snapshot_bodies: BoundedCache::new(SNAPSHOT_CACHE),
            initial_loads: BoundedCache::new(SNAPSHOT_CACHE),
            projections: BoundedCache::new(PROJECTION_CACHE),
            sssp: BoundedCache::new(SSSP_CACHE),
            analytics_dirty: AtomicBool::new(false),
            timeline_file: Mutex::new(timeline_file),
            share_agent_text: cfg.share_agent_text,
            epoch: AtomicU64::new(epoch as u64),
            interner_high: AtomicU32::new(interner_len),
        });
        let params = LayoutParams::default();
        Ok((
            engine,
            EngineThreads {
                changed_rx,
                layout_rx,
                params,
                prefer_cuda: cfg.prefer_cuda,
            },
        ))
    }

    /// Current publication.
    pub fn current(&self) -> Arc<Published> {
        self.published.read().clone()
    }

    /// Broadcast a JSON event (no receivers is fine).
    pub fn emit(&self, ev: ServerEvent) {
        let _ = self.events.send(Arc::new(ev));
    }

    /// Ask the layout to burst-settle around `ids`.
    pub fn focus_layout(&self, ids: Vec<u32>) {
        let _ = self.layout_tx.send(LayoutCmd::Focus(ids));
    }

    /// Highest code id in use (actor ids must stay above it). Lock-free.
    pub fn interner_len(&self) -> u32 {
        self.interner_high.load(Ordering::Acquire)
    }

    /// Register `id` in this repo's registry. A new actor beyond [`MAX_ACTORS`]
    /// first evicts the least recently seen stale actors; `false` when every
    /// actor is still active and there is no room.
    pub fn admit_actor(&self, id: u32, label: &str, kind: ActorKind, now: u64) -> bool {
        let mut c = self.collab.lock();
        if c.registry.get(id).is_none() && c.registry.len() >= MAX_ACTORS {
            let evicted = evict_stale(&mut c.registry, now);
            if evicted > 0 {
                tracing::info!(repo = %self.label, evicted, kept = c.registry.len(), "stale actors evicted");
            }
            if c.registry.len() >= MAX_ACTORS {
                return false;
            }
        }
        c.registry.upsert(id, label, kind, now);
        true
    }

    /// Record a presence event: registry, timeline (memory + `timeline.jsonl`), broadcast.
    /// Unless `--share-agent-text`, the text is cut to one caption ([`SHARED_TEXT_CHARS`]).
    pub fn record_presence(&self, mut ev: PresenceEvent) {
        if !self.share_agent_text {
            ev.text = ev.text.map(|t| truncate_chars(&t, SHARED_TEXT_CHARS));
        }
        {
            let mut c = self.collab.lock();
            c.registry.record(ev.clone());
            c.timeline.push_presence(ev.clone());
        }
        // Focus moves are frequent and carry no history worth keeping: live only.
        if ev.kind != codescape_model::events::PresenceKind::Focus {
            self.append_timeline(&serde_json::json!({ "presence": &ev }));
        }
        self.emit(ServerEvent::Presence(ev));
    }

    fn append_timeline(&self, line: &serde_json::Value) {
        if let Some(f) = self.timeline_file.lock().as_mut() {
            if let Err(e) = f.append(&line.to_string()) {
                tracing::warn!(error = %e, "timeline append failed");
            }
        }
    }

    fn publish(&self, snapshot: Arc<GraphSnapshot>) {
        *self.published.write() = Arc::new(Published::new(snapshot));
    }

    fn persist_ids(&self) {
        let (bytes, len) = {
            let ing = self.ingest.read();
            (ing.interner().to_bytes(), ing.interner().len() as u32)
        };
        self.interner_high.store(len, Ordering::Release);
        if let Err(e) = atomic_write(&self.data.ids(), &bytes) {
            tracing::error!(error = %e, "persisting ids.bin failed");
        }
    }

    /// Re-extract `paths` now (ingest thread). Returns the delta when the graph changed.
    fn apply_paths(&self, paths: &[PathBuf]) -> Option<GraphDelta> {
        let (delta, snapshot) = {
            let mut ing = self.ingest.write();
            match ing.apply_changed(paths) {
                Ok(Some(d)) => (d, Arc::new(ing.snapshot().clone())),
                Ok(None) => return None,
                Err(e) => {
                    tracing::warn!(error = %e, n = paths.len(), "apply_changed failed");
                    return None;
                }
            }
        };
        self.after_change(snapshot, delta.clone(), true);
        Some(delta)
    }

    fn after_change(&self, snapshot: Arc<GraphSnapshot>, delta: GraphDelta, topology: bool) {
        let revision = snapshot.revision;
        self.publish(snapshot.clone());
        self.persist_ids();
        {
            let mut c = self.collab.lock();
            c.timeline
                .push_revision(delta.generated_at_ms.max(now_ms()), revision);
        }
        self.append_timeline(&serde_json::json!({ "revision": revision, "ts_ms": now_ms(), "changed_paths": &delta.changed_paths }));
        if topology {
            self.analytics_dirty.store(true, Ordering::Relaxed);
            let _ = self
                .layout_tx
                .send(LayoutCmd::Delta(snapshot, delta.clone()));
        } else {
            let _ = self.layout_tx.send(LayoutCmd::Reload(snapshot));
        }
        self.layout.write().settled = false;
        self.emit(ServerEvent::Delta(delta));
    }

    fn recompute_analytics(&self) {
        let res = {
            let mut ing = self.ingest.write();
            ing.recompute_analytics()
                .map(|(d, retained)| (d, retained, Arc::new(ing.snapshot().clone())))
        };
        match res {
            Ok((delta, retained, snapshot)) => {
                tracing::info!(repo = %self.label, changed = delta.changed_nodes.len(), retained, "analytics recomputed");
                if !delta.is_empty() {
                    self.after_change(snapshot, delta, false);
                }
            }
            Err(e) => tracing::warn!(error = %e, "analytics recompute failed"),
        }
    }

    /// Shared records for this tick, rebuilt only when positions, revision or the heat bucket changed.
    pub fn base_records(&self, now: u64) -> Arc<BaseRecords> {
        let positions = self.positions.read().clone();
        let published = self.current();
        let (heat, actors): (HashMap<u32, f32>, Vec<ActorFocus>) = {
            let c = self.collab.lock();
            (
                c.registry.heat(now),
                c.registry.actors().map(|a| (a.id, a.focus)).collect(),
            )
        };
        // The heat clock only matters while something is warm; a cold, settled
        // graph keeps one base until positions, revision or actors change.
        let heat_bucket = if heat.is_empty() { 0 } else { now / 250 };
        let actor_sig: Vec<u32> = actors
            .iter()
            .flat_map(|&(id, f)| [id, f.unwrap_or(0)])
            .collect();
        let key = (
            positions.version,
            published.snapshot.revision,
            heat_bucket,
            ids_key(&actor_sig),
        );
        let cached = |b: &Option<Arc<BaseRecords>>| b.as_ref().filter(|b| b.key == key).cloned();
        if let Some(b) = cached(&self.base.lock()) {
            return b;
        }
        // One builder at a time: the other clients of this tick wait here and reuse its result.
        let _building = self.base_build.lock();
        if let Some(b) = cached(&self.base.lock()) {
            return b;
        }
        let records = published
            .snapshot
            .nodes
            .iter()
            .map(|n| {
                positions.index.get(&n.id).map(|&i| {
                    record_for(
                        n,
                        positions.pos[i],
                        positions.vel[i],
                        (f32::INFINITY, -1),
                        heat.get(&n.id).copied().unwrap_or(0.0),
                    )
                })
            })
            .collect();
        let actor_recs = actors
            .into_iter()
            .filter_map(|(id, focus)| {
                let f = focus?;
                let &i = positions.index.get(&f)?;
                let p = positions.pos[i];
                // Hover just above the focused node.
                Some(actor_record(id, [p[0], p[1] + 4.0, p[2]], 1.0))
            })
            .collect();
        let b = Arc::new(BaseRecords {
            key,
            records,
            actors: actor_recs,
            published,
        });
        *self.base.lock() = Some(b.clone());
        b
    }

    /// The LOD projection for `(tier, focus_files)` at the current revision, shared by
    /// every client with the same view.
    pub fn projection_for(
        &self,
        tier: codescape_model::Tier,
        focus_files: &[u32],
    ) -> Arc<Vec<u32>> {
        let p = self.current();
        let key = (p.snapshot.revision, tier_code(tier), ids_key(focus_files));
        self.projections.get_or_insert_with(key, || {
            Arc::new(codescape_model::lod::project(
                &p.snapshot,
                tier,
                focus_files,
            ))
        })
    }

    /// SSSP overlay from `focus` at `revision`, computed once per distinct focus.
    /// `None` while a re-extract holds the graph (retry next tick).
    pub fn sssp_for(&self, focus: u32, revision: u64) -> Option<SsspOverlay> {
        let key = (focus, revision);
        if let Some(v) = self.sssp.get(&key) {
            return Some(v);
        }
        let ing = self.ingest.try_read()?;
        let v: SsspOverlay = Arc::new(
            ing.bfs_from(focus, SSSP_MAX_DEPTH)
                .into_iter()
                .map(|(n, d, p)| (n, (d, p)))
                .collect(),
        );
        drop(ing);
        self.sssp.insert(key, v.clone());
        Some(v)
    }

    /// The serialised projected snapshot for `(tier, focus_files)` at the current revision.
    pub fn snapshot_body(
        &self,
        tier: codescape_model::Tier,
        focus_files: &[u32],
    ) -> Result<bytes::Bytes, serde_json::Error> {
        let p = self.current();
        let key = (p.snapshot.revision, tier_code(tier), ids_key(focus_files));
        if let Some(b) = self.snapshot_bodies.get(&key) {
            return Ok(b);
        }
        let body = bytes::Bytes::from(serde_json::to_vec(&crate::routes::project_snapshot(
            &p.snapshot,
            tier,
            focus_files,
        ))?);
        self.snapshot_bodies.insert(key, body.clone());
        Ok(body)
    }

    /// The `initialGraphLoad` text frame for `ids` (sorted) at the current revision.
    pub fn initial_load_body(&self, ids: &[u32]) -> Arc<String> {
        let p = self.current();
        let key = (p.snapshot.revision, ids_key(ids));
        self.initial_loads.get_or_insert_with(key, || {
            let set: HashSet<u32> = ids.iter().copied().collect();
            Arc::new(
                codescape_server::xr::initial_graph_load(&p.snapshot, &set, now_ms()).to_string(),
            )
        })
    }

    /// Persist current positions (warm start for the next boot).
    fn persist_positions(&self, set: &PositionSet) {
        let w = WarmPositions {
            epoch: self.epoch.load(Ordering::Relaxed) as u32,
            entries: set
                .ids
                .iter()
                .zip(set.pos.iter().zip(set.vel.iter()))
                .map(|(&id, (p, v))| (id, *p, *v))
                .collect(),
        };
        if let Err(e) = atomic_write(&self.data.positions(), &w.to_bytes()) {
            tracing::warn!(error = %e, "persisting positions failed");
        }
    }

    fn warm_positions(&self) -> Option<PositionSet> {
        let bytes = std::fs::read(self.data.positions()).ok()?;
        match WarmPositions::from_bytes(&bytes, self.epoch.load(Ordering::Relaxed) as u32) {
            Ok(w) => {
                let mut set = PositionSet::default();
                for (id, p, v) in w.entries {
                    set.ids.push(id);
                    set.pos.push(p);
                    set.vel.push(v);
                }
                tracing::info!(n = set.len(), "warm positions loaded");
                Some(set)
            }
            Err(e) => {
                tracing::info!(reason = %e, "warm positions discarded");
                None
            }
        }
    }

    /// Start the ingest and layout threads. They exit when `shutdown` is set.
    pub fn spawn(
        self: &Arc<Self>,
        threads: EngineThreads,
        shutdown: Arc<AtomicBool>,
    ) -> Vec<std::thread::JoinHandle<()>> {
        let EngineThreads {
            changed_rx,
            layout_rx,
            params,
            prefer_cuda,
        } = threads;
        let me = self.clone();
        let sd = shutdown.clone();
        let ingest = std::thread::Builder::new()
            .name(format!("cs-ingest-{}", self.label))
            .spawn(move || me.ingest_loop(changed_rx, sd))
            .expect("spawn ingest thread");
        let me = self.clone();
        let layout = std::thread::Builder::new()
            .name(format!("cs-layout-{}", self.label))
            .spawn(move || me.layout_loop(layout_rx, params, prefer_cuda, shutdown))
            .expect("spawn layout thread");
        vec![ingest, layout]
    }

    fn ingest_loop(self: Arc<Self>, rx: Receiver<Vec<PathBuf>>, shutdown: Arc<AtomicBool>) {
        let watch = { self.ingest.read().watch(self.changes.sender()) };
        let _watch = match watch {
            Ok(h) => Some(h),
            Err(e) => {
                tracing::warn!(error = %e, "file watcher unavailable; relying on hook-queued paths");
                None
            }
        };
        let mut last_analytics = Instant::now();
        while !shutdown.load(Ordering::Relaxed) {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(first) => {
                    // Coalesce whatever else arrived within 50 ms.
                    let mut set: HashSet<PathBuf> = first.into_iter().collect();
                    let until = Instant::now() + Duration::from_millis(50);
                    while let Ok(more) = rx.recv_deadline(until) {
                        set.extend(more);
                    }
                    let paths: Vec<PathBuf> = set.into_iter().collect();
                    if let Some(d) = self.apply_paths(&paths) {
                        tracing::info!(repo = %self.label, rev = d.to_revision, added = d.added_nodes.len(), removed = d.removed_nodes.len(), changed = d.changed_nodes.len(), "graph updated");
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }
            if self.analytics_dirty.load(Ordering::Relaxed)
                && last_analytics.elapsed() >= ANALYTICS_EVERY
            {
                self.analytics_dirty.store(false, Ordering::Relaxed);
                last_analytics = Instant::now();
                self.recompute_analytics();
            }
        }
        tracing::info!(repo = %self.label, "ingest thread stopped");
    }

    fn publish_positions(
        &self,
        engine: &dyn LayoutEngine,
        index: &Arc<HashMap<u32, usize>>,
        version: &mut u64,
    ) {
        let set = engine.positions();
        *version += 1;
        *self.positions.write() = Arc::new(PositionsFrame {
            version: *version,
            index: index.clone(),
            pos: set.pos.clone(),
            vel: set.vel.clone(),
        });
    }

    fn layout_loop(
        self: Arc<Self>,
        rx: Receiver<LayoutCmd>,
        params: LayoutParams,
        prefer_cuda: bool,
        shutdown: Arc<AtomicBool>,
    ) {
        let (mut engine, reason) = select_engine(params, prefer_cuda);
        tracing::info!(repo = %self.label, engine = engine.name(), %reason, "layout engine selected");
        {
            let mut s = self.layout.write();
            s.engine = engine.name().to_string();
            s.reason = reason;
        }
        let mut snapshot = self.current().snapshot.clone();
        let warm = self.warm_positions();
        if let Err(e) = engine.load(&snapshot, warm.as_ref()) {
            tracing::error!(error = %e, "layout load failed; positions unavailable");
            return;
        }
        let index_of = |e: &dyn LayoutEngine| -> Arc<HashMap<u32, usize>> {
            Arc::new(
                e.positions()
                    .ids
                    .iter()
                    .enumerate()
                    .map(|(i, &id)| (id, i))
                    .collect(),
            )
        };
        let mut index = index_of(engine.as_ref());
        let mut version = 0u64;
        self.publish_positions(engine.as_ref(), &index, &mut version);

        let step_period = Duration::from_secs_f64(1.0 / MAX_STEP_HZ);
        let mut last_publish = Instant::now();
        let mut last_persist = Instant::now();
        let mut settled_reported = false;
        let mut mask_steps_left: Option<u32> = None;
        let mut total_steps: u64 = 0;

        while !shutdown.load(Ordering::Relaxed) {
            // Drain commands; block briefly only when settled.
            let settled = engine.settled() && mask_steps_left.is_none();
            let first = if settled {
                rx.recv_timeout(Duration::from_millis(250)).ok()
            } else {
                rx.try_recv().ok()
            };
            let mut cmds: Vec<LayoutCmd> = first.into_iter().collect();
            cmds.extend(rx.try_iter());
            for cmd in cmds {
                match cmd {
                    LayoutCmd::Delta(snap, delta) => {
                        let res = engine.apply_delta(&snap, &delta);
                        let res = match res {
                            Ok(()) => Ok(()),
                            Err(e) => {
                                tracing::warn!(error = %e, "apply_delta failed; reloading warm");
                                let warm = engine.positions().clone();
                                engine.load(&snap, Some(&warm))
                            }
                        };
                        if let Err(e) = res {
                            tracing::error!(error = %e, "layout reload failed");
                            continue;
                        }
                        snapshot = snap;
                        index = index_of(engine.as_ref());
                        let dirty = delta.dirty_ids();
                        if !dirty.is_empty() {
                            engine.set_focus_mask(Some(&dirty));
                            mask_steps_left = Some(FOCUS_BURST_STEPS);
                        }
                        settled_reported = false;
                    }
                    LayoutCmd::Reload(snap) => {
                        let warm = engine.positions().clone();
                        if let Err(e) = engine.load(&snap, Some(&warm)) {
                            tracing::error!(error = %e, "layout reload failed");
                            continue;
                        }
                        snapshot = snap;
                        index = index_of(engine.as_ref());
                        settled_reported = false;
                    }
                    LayoutCmd::Focus(ids) => {
                        if !ids.is_empty() {
                            engine.set_focus_mask(Some(&ids));
                            mask_steps_left = Some(FOCUS_BURST_STEPS);
                            settled_reported = false;
                        }
                    }
                }
                self.publish_positions(engine.as_ref(), &index, &mut version);
            }

            let running = !engine.settled() || mask_steps_left.is_some();
            if running {
                let t0 = Instant::now();
                let stats = engine.step(1);
                total_steps += u64::from(stats.steps);
                if let Some(left) = mask_steps_left.as_mut() {
                    *left = left.saturating_sub(stats.steps.max(1));
                    if *left == 0 || engine.settled() {
                        engine.set_focus_mask(None);
                        mask_steps_left = None;
                    }
                }
                {
                    let mut s = self.layout.write();
                    s.energy = stats.energy;
                    s.steps = total_steps;
                    s.step_ms = stats.millis;
                    s.settled = false;
                }
                if last_publish.elapsed() >= PUBLISH_EVERY {
                    self.publish_positions(engine.as_ref(), &index, &mut version);
                    last_publish = Instant::now();
                }
                if last_persist.elapsed() >= PERSIST_POSITIONS_EVERY {
                    self.persist_positions(engine.positions());
                    last_persist = Instant::now();
                }
                let spent = t0.elapsed();
                if spent < step_period {
                    std::thread::sleep(step_period - spent);
                }
            } else if !settled_reported {
                settled_reported = true;
                self.publish_positions(engine.as_ref(), &index, &mut version);
                self.persist_positions(engine.positions());
                self.layout.write().settled = true;
                tracing::info!(repo = %self.label, revision = snapshot.revision, steps = total_steps, "layout settled");
                self.emit(ServerEvent::Settled {
                    revision: snapshot.revision,
                });
            }
        }
        self.persist_positions(engine.positions());
        tracing::info!(repo = %self.label, "layout thread stopped; positions persisted");
    }

    /// Absolute path for a repo-relative or absolute path under this root.
    pub fn absolute(&self, rel_or_abs: &str) -> PathBuf {
        let p = Path::new(rel_or_abs);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.root.join(p)
        }
    }

    /// Repo-relative form of an absolute path under the root, if it is under it.
    pub fn relative(&self, abs: &str) -> Option<String> {
        let root = self.root.to_string_lossy();
        let root = root.trim_end_matches('/');
        abs.strip_prefix(root)
            .and_then(|r| r.strip_prefix('/'))
            .filter(|r| !r.is_empty())
            .map(str::to_string)
    }
}

/// A stable small code for a tier, for cache keys.
fn tier_code(t: codescape_model::Tier) -> u8 {
    match t {
        codescape_model::Tier::L0 => 0,
        codescape_model::Tier::L1 => 1,
        codescape_model::Tier::L2 => 2,
    }
}

/// Actors to evict so that at most `target` remain: least recently seen first,
/// and only those unseen for `stale_ms`. Active actors are never chosen, so the
/// result may leave more than `target`.
pub fn choose_evictions(
    seen: &[(u32, u64)],
    now: u64,
    target: usize,
    stale_ms: u64,
) -> HashSet<u32> {
    let excess = seen.len().saturating_sub(target);
    let mut stale: Vec<(u32, u64)> = seen
        .iter()
        .copied()
        .filter(|&(_, t)| now.saturating_sub(t) >= stale_ms)
        .collect();
    stale.sort_by_key(|&(id, t)| (t, id));
    stale.into_iter().take(excess).map(|(id, _)| id).collect()
}

/// Remove the least recently seen stale actors down to [`EVICT_TO`]; returns how many.
/// Their heat is left to decay (see `ActorRegistry::remove`).
fn evict_stale(registry: &mut ActorRegistry, now: u64) -> usize {
    let seen: Vec<(u32, u64)> = registry.actors().map(|a| (a.id, a.last_seen_ms)).collect();
    choose_evictions(&seen, now, EVICT_TO, STALE_ACTOR_MS)
        .into_iter()
        .filter(|&id| registry.remove(id).is_some())
        .count()
}

#[cfg(test)]
mod tests {
    use codescape_model::events::PresenceKind;

    use super::*;

    #[test]
    fn change_queue_drops_the_oldest_batch_when_full() {
        let (q, rx) = ChangeQueue::new(2);
        for i in 0..5 {
            q.push(vec![PathBuf::from(format!("f{i}"))]);
        }
        assert_eq!(q.dropped(), 3);
        let left: Vec<_> = rx.try_iter().map(|b| b[0].clone()).collect();
        assert_eq!(
            left,
            vec![PathBuf::from("f3"), PathBuf::from("f4")],
            "the newest survive"
        );
    }

    #[test]
    fn evictions_take_the_oldest_stale_actors_only() {
        let now = 100 * 60_000;
        let seen = vec![
            (1, now - 50 * 60_000),
            (2, now - 20 * 60_000),
            (3, now - 1_000),
            (4, now - 30 * 60_000),
        ];
        let e = choose_evictions(&seen, now, 2, STALE_ACTOR_MS);
        assert_eq!(
            e,
            [1, 4].into_iter().collect(),
            "oldest first, two over target"
        );
        let e = choose_evictions(&seen, now, 0, STALE_ACTOR_MS);
        assert!(!e.contains(&3), "an active actor is never evicted");
        assert!(choose_evictions(&seen, now, 10, STALE_ACTOR_MS).is_empty());
    }

    #[test]
    fn eviction_removes_stale_actors_and_keeps_the_rest_intact() {
        let mut r = ActorRegistry::new();
        let ev = |actor: u32, node: u32, ts: u64| PresenceEvent {
            actor,
            actor_label: format!("a{actor}"),
            actor_kind: "agent".into(),
            kind: PresenceKind::Update,
            node_ids: vec![node],
            paths: vec![],
            text: None,
            ts_ms: ts,
        };
        let now = STALE_ACTOR_MS * 10;
        // EVICT_TO active actors plus three stale ones.
        for a in 0..EVICT_TO as u32 {
            r.record(ev(1_000 + a, 10, now - 1_000));
        }
        for a in 1..=3 {
            r.record(ev(a, 20, now - STALE_ACTOR_MS - a as u64));
        }
        r.record(ev(1_000, 11, now - 500));
        assert_eq!(evict_stale(&mut r, now), 3);
        assert_eq!(r.len(), EVICT_TO);
        assert!(r.get(1).is_none() && r.recent(1, 10).is_empty());
        assert_eq!(r.focus_of(1_000), Some(11), "kept actors are untouched");
        assert_eq!(r.recent(1_000, 10).len(), 2);
        assert_eq!(evict_stale(&mut r, now), 0, "nothing stale left");
    }
}
