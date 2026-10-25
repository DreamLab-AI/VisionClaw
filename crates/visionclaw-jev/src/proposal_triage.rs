//! VC-24-S1-01: triage the enrichment proposal queue and check evidence.
//!
//! State per proposal: the rationale, the requested change, the supporting
//! excerpt and references, and the exact current status. Questions: a
//! **Score** for completeness, a **Score** for reviewer relevance, and a
//! **Noul** that flags a rationale unsupported by the supplied evidence.
//!
//! Jev annotates and orders; it never approves, rejects, promotes or infers
//! authority. The decide route, signed decisions and mutation-owner receipts
//! are untouched. Only `pending`/`claimed` cases are eligible.
//!
//! The opportunity also names *novelty*. Judging it needs the target page's
//! current content, which the proposal row does not carry, so it is not
//! asked; no unsupported dimension is invented.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::json;

use crate::client::Jev;
use crate::wire::{Answers, Question, Request};
use crate::{clip, Fallback};

/// Question-set version, logged on every fallback.
pub const QSET_VERSION: &str = "vc-24-s1-01/1";

/// Completeness rubric (index = score).
pub const COMPLETENESS_LEVELS: [&str; 4] = [
    "Empty: the proposal states no concrete change or no rationale.",
    "Thin: a change is stated but the rationale or target is vague.",
    "Adequate: the change, target and rationale are each stated and consistent.",
    "Complete: the change, target, rationale and supporting evidence are all present and consistent.",
];

/// Reviewer-relevance rubric (index = score).
pub const RELEVANCE_LEVELS: [&str; 3] = [
    "Not actionable: nothing a reviewer could decide on from this proposal.",
    "Marginal: a decision is possible but the change is trivial or cosmetic.",
    "Needs a reviewer: a substantive knowledge change a reviewer should decide on.",
];

/// The facts about one proposal that may be sent. Build it from the
/// broker-facing projection; bound every field before sending.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ProposalFacts {
    /// Case id (not sent; used for caching).
    pub id: String,
    /// Exact current status (`pending` or `claimed`).
    pub status: String,
    /// Target page path.
    pub target_path: Option<String>,
    /// Enrichment type.
    pub enrichment_type: Option<String>,
    /// The requested change.
    pub content: Option<String>,
    /// The proposer's rationale.
    pub rationale: Option<String>,
    /// Supporting source excerpt from the provenance.
    pub source_excerpt: Option<String>,
}

/// Tunables.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    /// At most this many cases are triaged per call.
    pub max_cases: usize,
    /// Score confidence below this rejects the triage of that case.
    pub min_confidence: f64,
    /// Noul at or above this sets `unsupported_flag`.
    pub unsupported_flag_at: f64,
    /// How long to skip Jev after a failure (a short circuit breaker so an
    /// outage costs one deadline, not one per poll).
    pub cooldown: Duration,
}

impl Default for Params {
    fn default() -> Self {
        Self { max_cases: 20, min_confidence: 0.2, unsupported_flag_at: 0.7, cooldown: Duration::from_secs(60) }
    }
}

impl Params {
    /// Read `VISIONCLAW_JEV_TRIAGE_MAX`, `VISIONCLAW_JEV_TRIAGE_MIN_CONFIDENCE`,
    /// `VISIONCLAW_JEV_TRIAGE_UNSUPPORTED_AT`.
    pub fn from_lookup(lookup: &dyn Fn(&str) -> Option<String>) -> Self {
        let d = Self::default();
        Self {
            max_cases: crate::config::count(lookup, "VISIONCLAW_JEV_TRIAGE_MAX", d.max_cases, 50),
            min_confidence: crate::config::threshold(lookup, "VISIONCLAW_JEV_TRIAGE_MIN_CONFIDENCE", d.min_confidence),
            unsupported_flag_at: crate::config::threshold(lookup, "VISIONCLAW_JEV_TRIAGE_UNSUPPORTED_AT", d.unsupported_flag_at),
            cooldown: d.cooldown,
        }
    }
}

/// A reviewer-facing annotation. Advisory only.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Triage {
    /// Completeness, 0..=1.
    pub completeness: f64,
    /// Reviewer relevance, 0..=1.
    pub relevance: f64,
    /// Probability that the rationale is unsupported by the evidence supplied.
    pub unsupported_probability: f64,
    /// `unsupported_probability` crossed the flag threshold.
    pub unsupported_flag: bool,
    /// Ordering key: mean of completeness and relevance.
    pub priority: f64,
    /// Question-set version.
    pub question_set: &'static str,
}

fn state(f: &ProposalFacts) -> serde_json::Value {
    json!({
        "status": f.status,
        "target_path": f.target_path.as_deref().map(|s| clip(s, 300)),
        "enrichment_type": f.enrichment_type.as_deref().map(|s| clip(s, 80)),
        "requested_change": f.content.as_deref().map(|s| clip(s, 1500)),
        "rationale": f.rationale.as_deref().map(|s| clip(s, 1000)),
        "supporting_excerpt": f.source_excerpt.as_deref().map(|s| clip(s, 1000)),
    })
}

/// Build the three-question request for one proposal.
pub fn build_request(model: &str, f: &ProposalFacts) -> Request {
    let mut questions = BTreeMap::new();
    questions.insert(
        "completeness".into(),
        Question::Score {
            instructions: "How complete is this knowledge-enrichment proposal? Consider whether `requested_change`, \
                           `target_path`, `rationale` and `supporting_excerpt` are each present and consistent with one another."
                .into(),
            criteria: COMPLETENESS_LEVELS.iter().map(|s| s.to_string()).collect(),
        },
    );
    questions.insert(
        "relevance".into(),
        Question::Score {
            instructions: "How much does this proposal need a human reviewer's attention? Judge the substance of \
                           `requested_change` for the page at `target_path`."
                .into(),
            criteria: RELEVANCE_LEVELS.iter().map(|s| s.to_string()).collect(),
        },
    );
    questions.insert(
        "unsupported".into(),
        Question::Noul {
            instructions: "The `rationale` makes claims that are not supported by `supporting_excerpt` or by the \
                           `requested_change` itself. If `supporting_excerpt` is absent, answer from the rationale alone."
                .into(),
        },
    );
    Request { model: model.into(), state: state(f), questions }
}

/// Interpret one proposal's answers.
///
/// # Errors
///
/// [`Fallback::Malformed`] for missing/out-of-range answers;
/// [`Fallback::LowConfidence`] if either Score is under the threshold.
pub fn interpret(answers: &Answers, params: &Params) -> Result<Triage, Fallback> {
    let c = answers.score("completeness", COMPLETENESS_LEVELS.len())?;
    let r = answers.score("relevance", RELEVANCE_LEVELS.len())?;
    let u = answers.noul("unsupported")?;
    if c.confidence < params.min_confidence || r.confidence < params.min_confidence {
        return Err(Fallback::LowConfidence);
    }
    let completeness = c.score / (COMPLETENESS_LEVELS.len() - 1) as f64;
    let relevance = r.score / (RELEVANCE_LEVELS.len() - 1) as f64;
    Ok(Triage {
        completeness,
        relevance,
        unsupported_probability: u,
        unsupported_flag: u >= params.unsupported_flag_at,
        priority: 0.5 * (completeness + relevance),
        question_set: QSET_VERSION,
    })
}

/// Triage one proposal.
///
/// # Errors
///
/// Any [`Fallback`].
pub async fn triage_one(jev: &Jev, f: &ProposalFacts, params: &Params) -> Result<Triage, Fallback> {
    let answers = jev.ask(&build_request(jev.model(), f)).await?;
    interpret(&answers, params)
}

/// Memo of accepted triages plus a failure cool-down, shared across polls.
#[derive(Default)]
pub struct TriageCache {
    inner: Mutex<CacheInner>,
}

#[derive(Default)]
struct CacheInner {
    done: HashMap<ProposalFacts, Triage>,
    cool_until: Option<Instant>,
}

const CACHE_CAP: usize = 2_000;

impl TriageCache {
    /// An empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    fn get(&self, f: &ProposalFacts) -> Option<Triage> {
        self.inner.lock().ok()?.done.get(f).cloned()
    }

    fn put(&self, f: ProposalFacts, t: Triage) {
        if let Ok(mut g) = self.inner.lock() {
            if g.done.len() >= CACHE_CAP {
                g.done.clear();
            }
            g.done.insert(f, t);
        }
    }

    fn cooling(&self) -> bool {
        self.inner.lock().map(|g| g.cool_until.is_some_and(|t| Instant::now() < t)).unwrap_or(false)
    }

    fn cool(&self, d: Duration) {
        if let Ok(mut g) = self.inner.lock() {
            g.cool_until = Some(Instant::now() + d);
        }
    }
}

/// Annotate and order a queue, or return it unchanged.
///
/// `facts(item)` returns `None` for ineligible items (decided ones), which are
/// never sent and never move. Returns the items and, aligned with them, each
/// one's [`Triage`] (`None` where there is none). With `jev == None`, an
/// active cool-down, or no accepted triage, the returned items are
/// **identical** to the input and every annotation is `None`.
pub async fn triage_queue<T>(
    jev: Option<&Jev>,
    cache: &TriageCache,
    items: Vec<T>,
    facts: impl Fn(&T) -> Option<ProposalFacts>,
    params: &Params,
) -> (Vec<T>, Vec<Option<Triage>>) {
    let n = items.len();
    let none = |items: Vec<T>| (items, vec![None; n]);
    let Some(jev) = jev else { return none(items) };
    if cache.cooling() {
        return none(items);
    }
    let eligible: Vec<(usize, ProposalFacts)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, it)| facts(it).map(|f| (i, f)))
        .take(params.max_cases)
        .collect();
    if eligible.is_empty() {
        return none(items);
    }
    let results = futures_util::future::join_all(eligible.iter().map(|(_, f)| async move {
        if let Some(hit) = cache.get(f) {
            return Ok(hit);
        }
        let t = triage_one(jev, f, params).await?;
        cache.put(f.clone(), t.clone());
        Ok::<_, Fallback>(t)
    }))
    .await;
    let mut ann: Vec<Option<Triage>> = vec![None; n];
    let mut any_hard_failure = false;
    for ((i, _), r) in eligible.iter().zip(results) {
        match r {
            Ok(t) => ann[*i] = Some(t),
            Err(reason) => {
                crate::log_fallback(jev.item(), QSET_VERSION, &reason);
                if reason != Fallback::LowConfidence {
                    any_hard_failure = true;
                }
            }
        }
    }
    if any_hard_failure {
        cache.cool(params.cooldown);
    }
    let keys: Vec<Option<f64>> = ann.iter().map(|a| a.as_ref().map(|t| t.priority)).collect();
    let order = crate::slot_order(&keys);
    let ann_sorted = order.iter().map(|&i| ann[i].clone()).collect();
    (crate::apply_order(items, &order), ann_sorted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::testing::*;
    use crate::client::TransportError;
    use crate::config::Item;
    use crate::wire::mock;

    fn f(id: &str, status: &str) -> ProposalFacts {
        ProposalFacts {
            id: id.into(),
            status: status.into(),
            target_path: Some("knowledge/A.md".into()),
            enrichment_type: Some("definition".into()),
            content: Some(format!("change {id}")),
            rationale: Some("because".into()),
            source_excerpt: None,
        }
    }

    fn ok_body(c: f64, r: f64, u: f64, conf: f64) -> String {
        mock::body(vec![
            ("completeness".into(), mock::score(c, conf)),
            ("relevance".into(), mock::score(r, conf)),
            ("unsupported".into(), mock::noul(u)),
        ])
    }

    #[test]
    fn wire_shape() {
        let r = build_request("jev-latest", &f("1", "pending"));
        r.validate().unwrap();
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["questions"]["completeness"]["criteria"].as_array().unwrap().len(), 4);
        assert_eq!(v["questions"]["relevance"]["criteria"].as_array().unwrap().len(), 3);
        assert_eq!(v["questions"]["unsupported"]["type"], "noul");
        assert_eq!(v["state"]["status"], "pending");
        assert!(v["state"].get("id").is_none(), "the case id is not sent");
    }

    #[test]
    fn long_fields_are_bounded() {
        let mut p = f("1", "pending");
        p.content = Some("x".repeat(10_000));
        let v = state(&p);
        assert!(v["requested_change"].as_str().unwrap().chars().count() <= 1501);
    }

    fn keyed(low_marker: &'static str) -> std::sync::Arc<dyn crate::client::Transport> {
        // Cases whose requested_change contains `low_marker` score low.
        FnTransport::new(move |body| {
            let change = body["state"]["requested_change"].as_str().unwrap_or("");
            let b = if change.contains(low_marker) { ok_body(0.0, 0.0, 0.1, 0.9) } else { ok_body(3.0, 2.0, 0.9, 0.9) };
            Ok(crate::client::RawResponse { status: 200, body: b })
        })
    }

    fn pick(p: &(ProposalFacts, bool)) -> Option<ProposalFacts> {
        p.1.then(|| p.0.clone())
    }

    #[tokio::test]
    async fn accept_path_annotates_and_orders_without_moving_ineligible() {
        let j = jev(Item::ProposalTriage, keyed("change low"));
        let items = vec![
            (f("low", "pending"), true),
            (f("decided", "decided"), false),
            (f("high", "pending"), true),
        ];
        let (out, ann) = triage_queue(Some(&j), &TriageCache::new(), items, pick, &Params::default()).await;
        let ids: Vec<_> = out.iter().map(|(p, _)| p.id.as_str()).collect();
        assert_eq!(ids, vec!["high", "decided", "low"]);
        assert_eq!(ann[0].as_ref().unwrap().priority, 1.0);
        assert!(ann[0].as_ref().unwrap().unsupported_flag);
        assert!(ann[1].is_none(), "ineligible stays unannotated");
        assert_eq!(ann[2].as_ref().unwrap().priority, 0.0);
    }

    #[tokio::test]
    async fn every_fallback_returns_the_queue_unchanged() {
        let original = || vec![(f("a", "pending"), true), (f("b", "decided"), false), (f("c", "pending"), true)];
        let ids = |v: &[(ProposalFacts, bool)]| v.iter().map(|(p, _)| p.id.clone()).collect::<Vec<_>>();
        let want = ids(&original());

        // no client
        let (out, ann) = triage_queue(None, &TriageCache::new(), original(), pick, &Params::default()).await;
        assert_eq!(ids(&out), want);
        assert!(ann.iter().all(Option::is_none));

        let low_conf = ok_body(3.0, 2.0, 0.9, 0.0);
        let missing = mock::body(vec![("completeness".into(), mock::score(1.0, 0.9))]);
        for s in [
            Script::Reply(500, "x".into()),
            Script::Reply(429, "x".into()),
            Script::Reply(200, "garbage".into()),
            Script::Reply(200, missing),
            Script::Reply(200, low_conf),
            Script::Fail(TransportError::Other),
            Script::Fail(TransportError::Timeout),
            Script::Hang,
        ] {
            let j = jev(Item::ProposalTriage, MockTransport::new(s));
            let (out, ann) = triage_queue(Some(&j), &TriageCache::new(), original(), pick, &Params::default()).await;
            assert_eq!(ids(&out), want);
            assert!(ann.iter().all(Option::is_none));
        }
    }

    #[tokio::test]
    async fn results_are_cached_and_failures_cool_down() {
        let t = FnTransport::new(|_| Ok(crate::client::RawResponse { status: 200, body: ok_body(3.0, 2.0, 0.0, 0.9) }));
        let j = jev(Item::ProposalTriage, t.clone());
        let cache = TriageCache::new();
        let items = || vec![(f("a", "pending"), true)];
        triage_queue(Some(&j), &cache, items(), pick, &Params::default()).await;
        triage_queue(Some(&j), &cache, items(), pick, &Params::default()).await;
        assert_eq!(t.calls(), 1, "second poll served from cache");

        let bad = FnTransport::new(|_| Ok(crate::client::RawResponse { status: 503, body: String::new() }));
        let j = jev(Item::ProposalTriage, bad.clone());
        let cache = TriageCache::new();
        triage_queue(Some(&j), &cache, items(), pick, &Params::default()).await;
        triage_queue(Some(&j), &cache, items(), pick, &Params::default()).await;
        assert_eq!(bad.calls(), 1, "outage costs one call, then cools down");
    }
}
