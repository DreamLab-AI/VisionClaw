//! VC-38-S1-01: rerank memory-cloud hits before the answer route is drawn.
//!
//! State: the user query and already-authorised candidate snippets with stable
//! IDs (`m0`, `m1`, …). Per candidate: a **Score** for relevance and a **Noul**
//! for direct answer support; one extra **Noul** asks whether *no* candidate
//! suffices. The result is a permutation of the candidates plus the
//! none-suffices probability as an annotation.
//!
//! Jev only reorders. Retrieval, the privacy exclusion, sampling indices,
//! freshness and generation gates, and route geometry are unchanged. A
//! candidate whose relevance confidence is below the threshold is *unjudged*
//! and keeps its place; only the slots held by judged candidates are
//! reassigned. If nothing is judged the result is a low-confidence fallback.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::json;

use crate::client::Jev;
use crate::wire::{Answers, Question, Request};
use crate::Fallback;

/// Question-set version, logged on every fallback.
pub const QSET_VERSION: &str = "vc-38-s1-01/1";

/// Relevance rubric (index = score).
pub const RELEVANCE_LEVELS: [&str; 3] = [
    "Unrelated: the candidate has nothing to do with the query.",
    "Related but insufficient: the candidate is on the same topic but does not state what the query asks.",
    "Directly answers: the candidate itself states the answer to the query.",
];

/// One candidate sent to Jev.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Namespace (context only).
    pub namespace: String,
    /// Bounded snippet text of an already-authorised memory.
    pub snippet: String,
}

/// Tunables, all with safe defaults.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    /// At most this many leading candidates are judged.
    pub max_candidates: usize,
    /// Relevance confidence below this leaves a candidate unjudged.
    pub min_confidence: f64,
}

impl Default for Params {
    fn default() -> Self {
        Self { max_candidates: 10, min_confidence: 0.2 }
    }
}

impl Params {
    /// Read `VISIONCLAW_JEV_RERANK_MAX` (1..=20) and
    /// `VISIONCLAW_JEV_RERANK_MIN_CONFIDENCE` (0..=1).
    pub fn from_lookup(lookup: &dyn Fn(&str) -> Option<String>) -> Self {
        let d = Self::default();
        Self {
            max_candidates: crate::config::count(lookup, "VISIONCLAW_JEV_RERANK_MAX", d.max_candidates, 20),
            min_confidence: crate::config::threshold(lookup, "VISIONCLAW_JEV_RERANK_MIN_CONFIDENCE", d.min_confidence),
        }
    }
}

/// The result of an accepted rerank.
#[derive(Debug, Clone, PartialEq)]
pub struct Reranked {
    /// A permutation of `0..n`: `order[pos]` is the original index now at `pos`.
    pub order: Vec<usize>,
    /// Probability that no candidate suffices to answer the query.
    pub no_candidate_suffices: f64,
    /// How many candidates were judged confidently.
    pub judged: usize,
}

/// Optional annotation a host may attach to its response.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    /// Probability that no returned hit suffices to answer the query.
    pub no_candidate_suffices: f64,
    /// Hits judged confidently.
    pub judged: usize,
    /// Question-set version.
    pub question_set: &'static str,
}

/// Build the request: `2n + 1` questions over one shared state.
pub fn build_request(model: &str, query: &str, cands: &[Candidate]) -> Request {
    let state_cands: Vec<_> = cands
        .iter()
        .enumerate()
        .map(|(i, c)| json!({"id": format!("m{i}"), "namespace": c.namespace, "snippet": c.snippet}))
        .collect();
    let mut questions = BTreeMap::new();
    for i in 0..cands.len() {
        questions.insert(
            format!("rel_{i}"),
            Question::Score {
                instructions: format!(
                    "How useful is the candidate whose `id` is \"m{i}\" in `candidates` for answering `query`? \
                     Judge only the snippet text of that candidate."
                ),
                criteria: RELEVANCE_LEVELS.iter().map(|s| s.to_string()).collect(),
            },
        );
        questions.insert(
            format!("sup_{i}"),
            Question::Noul {
                instructions: format!(
                    "The snippet of the candidate whose `id` is \"m{i}\" in `candidates` explicitly states the answer to `query`."
                ),
            },
        );
    }
    questions.insert(
        "none".into(),
        Question::Noul {
            instructions: "None of the candidates in `candidates` contains enough information to answer `query`.".into(),
        },
    );
    Request { model: model.into(), state: json!({"query": query, "candidates": state_cands}), questions }
}

/// Interpret answers for `n` candidates.
///
/// # Errors
///
/// [`Fallback::Malformed`] if any expected answer is missing or out of range;
/// [`Fallback::LowConfidence`] if no candidate was judged confidently.
pub fn interpret(answers: &Answers, n: usize, params: &Params) -> Result<Reranked, Fallback> {
    let mut composite: Vec<Option<f64>> = Vec::with_capacity(n);
    for i in 0..n {
        let rel = answers.score(&format!("rel_{i}"), RELEVANCE_LEVELS.len())?;
        let sup = answers.noul(&format!("sup_{i}"))?;
        composite.push(if rel.confidence >= params.min_confidence {
            Some(0.5 * (rel.score / (RELEVANCE_LEVELS.len() - 1) as f64) + 0.5 * sup)
        } else {
            None
        });
    }
    let none = answers.noul("none")?;
    let mut slots: Vec<usize> = (0..n).filter(|&i| composite[i].is_some()).collect();
    if slots.is_empty() {
        return Err(Fallback::LowConfidence);
    }
    let mut ranked = slots.clone();
    ranked.sort_by(|&a, &b| {
        composite[b]
            .partial_cmp(&composite[a])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });
    let judged = slots.len();
    let mut order: Vec<usize> = (0..n).collect();
    slots.sort_unstable();
    for (slot, cand) in slots.into_iter().zip(ranked) {
        order[slot] = cand;
    }
    Ok(Reranked { order, no_candidate_suffices: none, judged })
}

/// Ask Jev and interpret.
///
/// # Errors
///
/// Any [`Fallback`].
pub async fn rerank(jev: &Jev, query: &str, cands: &[Candidate], params: &Params) -> Result<Reranked, Fallback> {
    if cands.is_empty() || query.trim().is_empty() {
        return Err(Fallback::Malformed);
    }
    let req = build_request(jev.model(), query, cands);
    let answers = jev.ask(&req).await?;
    interpret(&answers, cands.len(), params)
}

/// Reorder `items` by Jev, or return them unchanged.
///
/// `candidate(item)` returns `None` for an item that must not leave the
/// process (for example one in an excluded namespace); such items are never
/// sent and keep their place. At most `params.max_candidates` leading eligible
/// items are judged. With `jev == None`, or on any [`Fallback`], the returned
/// vector is **identical** to `items`.
pub async fn rerank_or_original<T>(
    jev: Option<&Jev>,
    query: &str,
    items: Vec<T>,
    candidate: impl Fn(&T) -> Option<Candidate>,
    params: &Params,
) -> (Vec<T>, Option<Note>) {
    let Some(jev) = jev else { return (items, None) };
    let eligible: Vec<(usize, Candidate)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, it)| candidate(it).map(|c| (i, c)))
        .take(params.max_candidates)
        .collect();
    if eligible.is_empty() {
        return (items, None);
    }
    let cands: Vec<Candidate> = eligible.iter().map(|(_, c)| c.clone()).collect();
    match rerank(jev, query, &cands, params).await {
        Ok(r) => {
            // Map the local permutation onto the global positions.
            let mut perm: Vec<usize> = (0..items.len()).collect();
            for (local_pos, &local_src) in r.order.iter().enumerate() {
                perm[eligible[local_pos].0] = eligible[local_src].0;
            }
            let mut slots: Vec<Option<T>> = items.into_iter().map(Some).collect();
            let out = perm.into_iter().map(|src| slots[src].take().expect("permutation")).collect();
            (out, Some(Note { no_candidate_suffices: r.no_candidate_suffices, judged: r.judged, question_set: QSET_VERSION }))
        }
        Err(reason) => {
            crate::log_fallback(jev.item(), QSET_VERSION, &reason);
            (items, None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::testing::*;
    use crate::client::TransportError;
    use crate::config::Item;
    use crate::wire::mock;

    fn cands(n: usize) -> Vec<Candidate> {
        (0..n).map(|i| Candidate { namespace: "ns".into(), snippet: format!("snippet {i}") }).collect()
    }

    fn body_for(scores: &[(f64, f64, f64)], none: f64) -> String {
        let mut a = vec![];
        for (i, (s, c, p)) in scores.iter().enumerate() {
            a.push((format!("rel_{i}"), mock::score(*s, *c)));
            a.push((format!("sup_{i}"), mock::noul(*p)));
        }
        a.push(("none".into(), mock::noul(none)));
        mock::body(a)
    }

    #[test]
    fn wire_shape_has_map_free_score_lists_and_noul_per_candidate() {
        let r = build_request("jev-latest", "what is X?", &cands(2));
        r.validate().unwrap();
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["questions"].as_object().unwrap().len(), 5);
        assert!(v["questions"]["rel_0"]["criteria"].is_array());
        assert_eq!(v["questions"]["rel_1"]["criteria"].as_array().unwrap().len(), 3);
        assert_eq!(v["questions"]["none"]["type"], "noul");
        assert_eq!(v["state"]["candidates"][1]["id"], "m1");
        assert!(v["questions"]["rel_1"]["instructions"].as_str().unwrap().contains("\"m1\""));
    }

    #[tokio::test]
    async fn accept_path_moves_the_best_candidate_to_the_front() {
        // candidate 2 is directly answering, 0 unrelated, 1 related
        let t = MockTransport::new(Script::Reply(200, body_for(&[(0.1, 0.9, 0.05), (1.0, 0.9, 0.3), (2.0, 0.9, 0.95)], 0.1)));
        let j = jev(Item::MemoryRerank, t);
        let items = vec!["a", "b", "c"];
        let (out, note) = rerank_or_original(Some(&j), "q", items, |s| Some(Candidate { namespace: "n".into(), snippet: s.to_string() }), &Params::default()).await;
        assert_eq!(out, vec!["c", "b", "a"]);
        let note = note.unwrap();
        assert_eq!(note.judged, 3);
        assert!((note.no_candidate_suffices - 0.1).abs() < 1e-9);
    }

    #[tokio::test]
    async fn unjudged_candidates_keep_their_slot() {
        // b has low confidence: stays at index 1; a and c swap.
        let t = MockTransport::new(Script::Reply(200, body_for(&[(0.0, 0.9, 0.0), (2.0, 0.01, 1.0), (2.0, 0.9, 1.0)], 0.0)));
        let j = jev(Item::MemoryRerank, t);
        let (out, _) = rerank_or_original(Some(&j), "q", vec!["a", "b", "c"], |s| Some(Candidate { namespace: "n".into(), snippet: s.to_string() }), &Params::default()).await;
        assert_eq!(out, vec!["c", "b", "a"]);
    }

    #[tokio::test]
    async fn ineligible_items_are_never_sent_and_keep_their_place() {
        let t = MockTransport::new(Script::Reply(200, body_for(&[(0.0, 0.9, 0.0), (2.0, 0.9, 1.0)], 0.0)));
        let j = jev(Item::MemoryRerank, t.clone());
        let items = vec![("secret", "personal-context"), ("x", "ok"), ("y", "ok")];
        let (out, _) = rerank_or_original(
            Some(&j),
            "q",
            items,
            |(s, ns)| (*ns != "personal-context").then(|| Candidate { namespace: ns.to_string(), snippet: s.to_string() }),
            &Params::default(),
        )
        .await;
        assert_eq!(out.iter().map(|x| x.0).collect::<Vec<_>>(), vec!["secret", "y", "x"]);
        let sent = serde_json::to_string(&t.seen.lock().unwrap()[0]).unwrap();
        assert!(!sent.contains("secret"), "excluded content must not be sent");
    }

    #[tokio::test]
    async fn every_fallback_returns_the_original_order() {
        let original = vec!["a", "b", "c", "d"];
        let cand = |s: &&str| Some(Candidate { namespace: "n".into(), snippet: s.to_string() });
        // no client at all (flag off / no endpoint)
        let (out, note) = rerank_or_original(None, "q", original.clone(), cand, &Params::default()).await;
        assert_eq!((out, note), (original.clone(), None));

        let all_low = body_for(&[(2.0, 0.0, 1.0), (1.0, 0.0, 1.0), (0.0, 0.0, 0.0), (2.0, 0.0, 1.0)], 0.5);
        let missing_none = {
            let mut a = vec![];
            for i in 0..4 {
                a.push((format!("rel_{i}"), mock::score(1.0, 0.9)));
                a.push((format!("sup_{i}"), mock::noul(0.5)));
            }
            mock::body(a)
        };
        let scripts = vec![
            Script::Reply(500, "boom".into()),
            Script::Reply(429, "slow down".into()),
            Script::Reply(200, "garbage".into()),
            Script::Reply(200, missing_none),
            Script::Reply(200, all_low),
            Script::Fail(TransportError::Other),
            Script::Fail(TransportError::Timeout),
            Script::Hang,
        ];
        for s in scripts {
            let j = jev(Item::MemoryRerank, MockTransport::new(s));
            let (out, note) = rerank_or_original(Some(&j), "q", original.clone(), cand, &Params::default()).await;
            assert_eq!(out, original);
            assert!(note.is_none());
        }
    }

    #[tokio::test]
    async fn only_the_leading_max_candidates_are_judged() {
        let t = MockTransport::new(Script::Reply(200, body_for(&[(0.0, 0.9, 0.0), (2.0, 0.9, 1.0)], 0.0)));
        let j = jev(Item::MemoryRerank, t.clone());
        let p = Params { max_candidates: 2, ..Params::default() };
        let (out, _) = rerank_or_original(Some(&j), "q", vec!["a", "b", "c", "d"], |s| Some(Candidate { namespace: "n".into(), snippet: s.to_string() }), &p).await;
        assert_eq!(out, vec!["b", "a", "c", "d"]);
        assert_eq!(t.seen.lock().unwrap()[0]["state"]["candidates"].as_array().unwrap().len(), 2);
    }
}
