//! Whelk reasoning over one caller's view of the ontology (ADR-2184 Q1, reasoner residual).
//!
//! The shared Whelk closure is computed once over the whole store, so a subsumption that
//! passes *through* a class hidden from a caller (`A ⊑ H ⊑ B` gives `A ⊑ B`) survives
//! [`HiddenClasses::pairs`], which only drops pairs that name `H`, and would tell the caller
//! where `H` sits. For a gated caller the inferred reads instead use [`ViewReasoner`]: a fresh
//! Whelk run over only the axioms that caller may see, so every inference served is derived
//! from visible axioms alone.
//!
//! The per-view input is built exactly as the post-sync reasoner builds the whole-store one
//! (class declarations, asserted axioms, the NarrativeGoldmine property hierarchy), minus every
//! hidden class and every axiom naming one. Its closure is cached per view: one slot per
//! deny-list (a SHA-256 of the hidden set), holding the closure of one store state (a SHA-256
//! over the visible input). A store change replaces the view's slot, so a stale closure is
//! neither served nor kept. Callers intersect it with the
//! whole-store closure a full-access caller is served, so a gated answer never states more than
//! the full-access one. Replies derived from a view and the stored reasoner run (the gated
//! `/api/ontology/inference/results` and `/inferred` reads) are cached the same way, their
//! state being the view's and the stored run id ([`ViewReasoner::derived`]).
//!
//! Every failure (an unreadable store, a reasoner error or panic) is an `Err` whose message
//! starts with [`VIEW_REASONING_UNAVAILABLE`]; routes answer it with 503 and never fall back to
//! the whole-store closure.

use std::collections::VecDeque;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::adapters::whelk_inference_engine::WhelkInferenceEngine;
use crate::services::github_sync_service::GitHubSyncService;
use crate::services::ontology_visibility::HiddenClasses;
use sha2::{Digest, Sha256};
use tokio::sync::OnceCell;
use visionclaw_domain::ports::inference_engine::InferenceEngine;
use visionclaw_domain::ports::ontology_repository::{
    AxiomType, InferenceResults, OntologyRepository, OwlAxiom, OwlClass,
};

/// The prefix of every per-view reasoning error. A route that sees it answers 503.
pub const VIEW_REASONING_UNAVAILABLE: &str = "view reasoning unavailable";

/// Whether `error` is a per-view reasoning failure (see [`VIEW_REASONING_UNAVAILABLE`]).
pub fn is_view_reasoning_unavailable(error: &str) -> bool {
    error.starts_with(VIEW_REASONING_UNAVAILABLE)
}

fn unavailable(detail: impl std::fmt::Display) -> String {
    format!("{VIEW_REASONING_UNAVAILABLE}: {detail}")
}

/// The reasoner a [`ViewReasoner`] runs: the inferred axioms of a class and axiom set. It runs
/// on a blocking thread, so it may be synchronous and slow.
pub type Reasoner = fn(Vec<OwlClass>, Vec<OwlAxiom>) -> Result<Vec<OwlAxiom>, String>;

/// How many views each cache keeps, one entry per view (the oldest view is dropped). Views are
/// few: anonymous, plus one per distinct deny-list.
const CACHE_CAPACITY: usize = 32;

/// One view's closure.
#[derive(Debug, Clone)]
pub struct ViewClosure {
    /// The view's identity: a digest of its hidden set.
    pub view: [u8; 32],
    /// The store state it was reasoned over: a digest of the hidden set and the visible input.
    pub state: [u8; 32],
    /// The inferred axioms (the reasoner's output over the visible input).
    pub inferred_axioms: Vec<OwlAxiom>,
    /// How long the reasoner took on this view, in milliseconds.
    pub inference_time_ms: u64,
}

impl ViewClosure {
    /// The `(sub, sup)` pairs of the closure's `SubClassOf` axioms, as
    /// `InferenceEngine::get_subclass_hierarchy` reports them.
    pub fn hierarchy(&self) -> Vec<(String, String)> {
        self.inferred_axioms
            .iter()
            .filter(|a| a.axiom_type == AxiomType::SubClassOf)
            .map(|a| (a.subject.clone(), a.object.clone()))
            .collect()
    }
}

type Slot<T> = Arc<OnceCell<Arc<T>>>;

/// One computed-once slot per view, holding the value for one state of that view. A request
/// for a view in another state replaces its slot, so a superseded value is dropped as soon as
/// no request still holds it. Views beyond [`CACHE_CAPACITY`] evict the oldest.
struct SlotCache<T>(Mutex<VecDeque<SlotEntry<T>>>);

struct SlotEntry<T> {
    view: [u8; 32],
    state: [u8; 32],
    slot: Slot<T>,
}

impl<T> SlotCache<T> {
    fn new() -> Self {
        Self(Mutex::new(VecDeque::new()))
    }

    /// The slot for `view` in `state`. A failed computation leaves its slot empty, so the next
    /// request retries.
    fn slot(&self, view: [u8; 32], state: [u8; 32]) -> Slot<T> {
        let mut cache = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = cache.iter_mut().find(|e| e.view == view) {
            if entry.state != state {
                entry.state = state;
                entry.slot = Slot::default();
            }
            return entry.slot.clone();
        }
        if cache.len() >= CACHE_CAPACITY {
            cache.pop_front();
        }
        let slot = Slot::default();
        cache.push_back(SlotEntry {
            view,
            state,
            slot: slot.clone(),
        });
        slot
    }

    /// How many views are cached.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// The value for `view` in `state`, computed by `compute` on a miss. Concurrent misses on
    /// one view and state compute once.
    async fn get_or_compute<F, Fut>(
        &self,
        view: [u8; 32],
        state: [u8; 32],
        compute: F,
    ) -> Result<Arc<T>, String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, String>>,
    {
        let slot = self.slot(view, state);
        let value = slot
            .get_or_try_init(|| async { compute().await.map(Arc::new) })
            .await?;
        Ok(value.clone())
    }
}

/// Per-view Whelk reasoning with bounded per-view caches. One instance is shared by every
/// ontology route so a view is reasoned over once per store state.
pub struct ViewReasoner {
    reasoner: Reasoner,
    closures: SlotCache<ViewClosure>,
    results: SlotCache<Option<InferenceResults>>,
    graphs: SlotCache<serde_json::Value>,
    runs: AtomicU64,
}

impl std::fmt::Debug for ViewReasoner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewReasoner")
            .field("runs", &self.runs())
            .finish_non_exhaustive()
    }
}

impl Default for ViewReasoner {
    fn default() -> Self {
        Self::new()
    }
}

impl ViewReasoner {
    /// A reasoner that runs a fresh [`WhelkInferenceEngine`] per view.
    pub fn new() -> Self {
        Self::with_reasoner(whelk_closure)
    }

    /// A reasoner that runs `reasoner` per view (tests inject a failing one).
    pub fn with_reasoner(reasoner: Reasoner) -> Self {
        Self {
            reasoner,
            closures: SlotCache::new(),
            results: SlotCache::new(),
            graphs: SlotCache::new(),
            runs: AtomicU64::new(0),
        }
    }

    /// How many times the reasoner has run (cache misses).
    pub fn runs(&self) -> u64 {
        self.runs.load(Ordering::Relaxed)
    }

    /// The closure of what `hidden`'s caller may see in `repo`: class declarations and
    /// asserted axioms with every hidden class and every axiom naming one removed, plus the
    /// NarrativeGoldmine property hierarchy. Cached per view; concurrent misses on one view
    /// run the reasoner once.
    pub async fn closure(
        &self,
        repo: &dyn OntologyRepository,
        hidden: &HiddenClasses,
    ) -> Result<Arc<ViewClosure>, String> {
        let mut class_iris = repo.class_iris().await.map_err(unavailable)?;
        class_iris.retain(|iri| !hidden.contains(iri));
        let axioms = hidden.axioms(repo.get_axioms().await.map_err(unavailable)?);
        let view = view_identity(hidden);
        let state = view_digest(view, &class_iris, &axioms);
        self.closures
            .get_or_compute(view, state, || async {
                self.runs.fetch_add(1, Ordering::Relaxed);
                let classes: Vec<OwlClass> = class_iris
                    .into_iter()
                    .map(|iri| OwlClass {
                        iri,
                        ..OwlClass::default()
                    })
                    .collect();
                let mut input = axioms;
                input.extend(GitHubSyncService::ngm_property_hierarchy_axioms());
                let reasoner = self.reasoner;
                let started = Instant::now();
                let inferred_axioms = tokio::task::spawn_blocking(move || reasoner(classes, input))
                    .await
                    .map_err(unavailable)?
                    .map_err(unavailable)?;
                let inference_time_ms = started.elapsed().as_millis() as u64;
                log::info!(
                    "per-view Whelk run: {} inferred axioms in {inference_time_ms}ms",
                    inferred_axioms.len()
                );
                Ok(ViewClosure {
                    view,
                    state,
                    inferred_axioms,
                    inference_time_ms,
                })
            })
            .await
    }

    /// The caches of replies derived from a view and the stored reasoner run.
    pub fn derived(&self) -> Derived<'_> {
        Derived(self)
    }
}

/// Replies derived from one view's closure and the stored whole-store run, each computed once
/// per view state and run id ([`ViewReasoner::derived`]) and replaced when either moves. The
/// run id is content-addressed over the stored run, so a new run is a new state.
pub struct Derived<'a>(&'a ViewReasoner);

impl Derived<'_> {
    /// The view's inference results for stored run `run_id`, computed by `compute` on a miss.
    pub async fn inference_results<F, Fut>(
        &self,
        view: &ViewClosure,
        run_id: Option<&str>,
        compute: F,
    ) -> Result<Arc<Option<InferenceResults>>, String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Option<InferenceResults>, String>>,
    {
        self.0
            .results
            .get_or_compute(view.view, derived_state(view, run_id), compute)
            .await
    }

    /// The view's inferred graph for stored run `run_id`, computed by `compute` on a miss.
    pub async fn inferred_graph<F, Fut>(
        &self,
        view: &ViewClosure,
        run_id: Option<&str>,
        compute: F,
    ) -> Result<Arc<serde_json::Value>, String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<serde_json::Value, String>>,
    {
        self.0
            .graphs
            .get_or_compute(view.view, derived_state(view, run_id), compute)
            .await
    }
}

/// SHA-256 over a view's state and a stored run id (absent and empty kept apart).
fn derived_state(view: &ViewClosure, run_id: Option<&str>) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(view.state);
    match run_id {
        Some(id) => {
            h.update([1]);
            h.update((id.len() as u64).to_le_bytes());
            h.update(id.as_bytes());
        }
        None => h.update([0]),
    }
    h.finalize().into()
}

/// A fresh [`WhelkInferenceEngine`] over `classes` and `axioms`: the same load and classify
/// the post-sync reasoner runs on the whole store. Its async methods never yield, so they are
/// driven to completion on the calling (blocking) thread.
pub fn whelk_closure(
    classes: Vec<OwlClass>,
    axioms: Vec<OwlAxiom>,
) -> Result<Vec<OwlAxiom>, String> {
    futures::executor::block_on(async {
        let mut engine = WhelkInferenceEngine::new();
        engine
            .load_ontology(classes, axioms)
            .await
            .map_err(|e| format!("whelk load: {e}"))?;
        engine
            .infer()
            .await
            .map(|r| r.inferred_axioms)
            .map_err(|e| format!("whelk infer: {e}"))
    })
}

/// `s` length-prefixed into `h`, so field boundaries cannot be shifted.
fn field(h: &mut Sha256, s: &str) {
    h.update((s.len() as u64).to_le_bytes());
    h.update(s.as_bytes());
}

/// SHA-256 over the sorted hidden set: which view this is.
fn view_identity(hidden: &HiddenClasses) -> [u8; 32] {
    let mut h = Sha256::new();
    let mut hidden: Vec<&String> = hidden.as_set().iter().collect();
    hidden.sort();
    h.update((hidden.len() as u64).to_le_bytes());
    hidden.iter().for_each(|iri| field(&mut h, iri));
    h.finalize().into()
}

/// SHA-256 over a view's identity and its visible input, each field length-prefixed and each
/// set sorted, so two states agree only when they would reason over the same input.
fn view_digest(view: [u8; 32], class_iris: &[String], axioms: &[OwlAxiom]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(view);
    let mut iris: Vec<&String> = class_iris.iter().collect();
    iris.sort();
    h.update((iris.len() as u64).to_le_bytes());
    iris.iter().for_each(|iri| field(&mut h, iri));
    let mut lines: Vec<Vec<u8>> = axioms
        .iter()
        .map(|a| {
            let mut line = Sha256::new();
            field(&mut line, &format!("{:?}", a.axiom_type));
            field(&mut line, &a.subject);
            field(&mut line, &a.object);
            let mut annotations: Vec<(&String, &String)> = a.annotations.iter().collect();
            annotations.sort();
            for (k, v) in annotations {
                field(&mut line, k);
                field(&mut line, v);
            }
            line.finalize().to_vec()
        })
        .collect();
    lines.sort();
    h.update((lines.len() as u64).to_le_bytes());
    lines.iter().for_each(|l| h.update(l));
    h.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    fn sub(s: &str, o: &str) -> OwlAxiom {
        OwlAxiom {
            id: None,
            axiom_type: AxiomType::SubClassOf,
            subject: s.to_string(),
            object: o.to_string(),
            annotations: HashMap::new(),
        }
    }

    fn hiding(iris: &[&str]) -> HiddenClasses {
        HiddenClasses::from(iris.iter().map(|s| s.to_string()).collect::<HashSet<_>>())
    }

    #[test]
    fn the_digest_separates_views_and_store_states_and_ignores_order() {
        let iris = vec!["a".to_string(), "b".to_string()];
        let axioms = vec![sub("a", "b"), sub("b", "c")];
        let base = view_digest(view_identity(&hiding(&["h"])), &iris, &axioms);
        let reordered: Vec<OwlAxiom> = axioms.iter().rev().cloned().collect();
        let iris_rev: Vec<String> = iris.iter().rev().cloned().collect();
        assert_eq!(
            base,
            view_digest(view_identity(&hiding(&["h"])), &iris_rev, &reordered)
        );
        assert_ne!(
            base,
            view_digest(view_identity(&hiding(&["g"])), &iris, &axioms)
        );
        assert_ne!(
            base,
            view_digest(view_identity(&hiding(&["h"])), &iris, &axioms[..1])
        );
        assert_ne!(
            base,
            view_digest(view_identity(&hiding(&["h"])), &iris[..1], &axioms)
        );
        // Field boundaries are length-prefixed: ("ab","c") is not ("a","bc").
        assert_ne!(
            view_digest(view_identity(&hiding(&[])), &[], &[sub("ab", "c")]),
            view_digest(view_identity(&hiding(&[])), &[], &[sub("a", "bc")])
        );
    }

    #[test]
    fn a_new_state_replaces_the_view_slot_and_a_new_view_adds_one() {
        let cache: SlotCache<u8> = SlotCache::new();
        let (a, b) = ([1; 32], [2; 32]);
        let first = cache.slot(a, [10; 32]);
        assert!(
            Arc::ptr_eq(&first, &cache.slot(a, [10; 32])),
            "same state, same slot"
        );
        let weak = Arc::downgrade(&first);
        drop(first);
        let second = cache.slot(a, [11; 32]);
        assert!(weak.upgrade().is_none(), "the superseded state is dropped");
        assert_eq!(cache.len(), 1, "one view, one entry");
        cache.slot(b, [10; 32]);
        assert_eq!(cache.len(), 2);
        for i in 0..CACHE_CAPACITY as u8 {
            cache.slot([100 + i; 32], [0; 32]);
        }
        assert_eq!(cache.len(), CACHE_CAPACITY, "bounded");
        assert!(
            !Arc::ptr_eq(&second, &cache.slot(a, [11; 32])),
            "oldest view evicted"
        );
    }

    #[test]
    fn whelk_closure_derives_the_chain_and_nothing_through_a_missing_link() {
        let class = |iri: &str| OwlClass {
            iri: iri.to_string(),
            ..OwlClass::default()
        };
        let pairs = |axioms: Vec<OwlAxiom>| -> HashSet<(String, String)> {
            axioms
                .into_iter()
                .filter(|a| a.axiom_type == AxiomType::SubClassOf)
                .map(|a| (a.subject, a.object))
                .collect()
        };
        let whole = whelk_closure(
            vec![class("ex:A"), class("ex:H"), class("ex:B")],
            vec![sub("ex:A", "ex:H"), sub("ex:H", "ex:B")],
        )
        .expect("classifies");
        assert!(pairs(whole).contains(&("ex:A".to_string(), "ex:B".to_string())));
        let view =
            whelk_closure(vec![class("ex:A"), class("ex:B")], Vec::new()).expect("classifies");
        assert!(!pairs(view).contains(&("ex:A".to_string(), "ex:B".to_string())));
    }
}
