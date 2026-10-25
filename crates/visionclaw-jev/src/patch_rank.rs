//! VC-20-S1-01: rank ontology patch proposals by evidence sufficiency.
//!
//! State per proposal (from `vault propose`): the kind, the hypothesis, the
//! diff and a summary of the formal consequence delta. Questions: a **Score**
//! for how well the supplied evidence supports the hypothesis, a **Score** for
//! clarity, and a **Noul** flagging a contradiction between the hypothesis and
//! the supplied evidence.
//!
//! The same implementation serves VG-01-S1-01 (curator queue for evidence
//! support, novelty and review need), relocated from visionGraph: `vault
//! propose` drafts are that queue's only code consumer. It adds three
//! annotations: a Noul flagging claims unsupported by the supplied evidence, a
//! Score for novelty (judged from the consequence delta) and a Score for
//! review urgency. Source provenance, confidence and machine-confirmed status
//! are never changed by any of them.
//!
//! This orders a reviewer's reading list. Formal OWL reasoning, derivation
//! certificates and human acceptance stay exact: a semantic score cannot
//! certify a theorem, post a proposal or make a write. Blocked proposals are
//! ranked by the same rule; their blockers are part of the evidence.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::json;

use crate::client::Jev;
use crate::wire::{Answers, Question, Request};
use crate::{clip, Fallback};

/// Question-set version, logged on every fallback.
pub const QSET_VERSION: &str = "vc-20-s1-01+vg-01-s1-01/2";

/// Support rubric (index = score).
pub const SUPPORT_LEVELS: [&str; 4] = [
    "None: the diff and consequence delta give no support for the hypothesis.",
    "Weak: the evidence is related to the hypothesis but does not establish it.",
    "Adequate: the evidence establishes the hypothesis for the pages it changes.",
    "Strong: the evidence establishes the hypothesis and its stated consequences.",
];

/// Clarity rubric (index = score).
pub const CLARITY_LEVELS: [&str; 3] = [
    "Unclear: a reviewer could not tell what is being asserted.",
    "Understandable: the assertion is clear after careful reading.",
    "Clear: the assertion and its scope are stated plainly.",
];

/// Novelty rubric (index = score).
pub const NOVELTY_LEVELS: [&str; 3] = [
    "Redundant: the change adds nothing the corpus does not already entail.",
    "Incremental: the change adds a few conclusions or refines existing ones.",
    "New: the change adds substantive conclusions not already entailed.",
];

/// Review-urgency rubric (index = score).
pub const URGENCY_LEVELS: [&str; 3] = [
    "Can wait: low impact, no time pressure.",
    "Soon: moderate impact or a blocker that needs a human look.",
    "Urgent: broad impact, or a possible contradiction a curator should see first.",
];

/// The facts about one proposal that may be sent.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PatchFacts {
    /// Proposal kind (for example `edit` or `create`).
    pub kind: String,
    /// The proposer's hypothesis.
    pub hypothesis: String,
    /// The unified diff (the exact supporting spans).
    pub diff: String,
    /// A compact rendering of the consequence delta, if one was computed.
    pub consequences: Option<String>,
    /// Blockers already found by code.
    pub blockers: Vec<String>,
}

/// Tunables.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    /// Score confidence below this leaves a proposal unassessed.
    pub min_confidence: f64,
    /// Contradiction probability at or above this sets the flag.
    pub contradiction_flag_at: f64,
}

impl Default for Params {
    fn default() -> Self {
        Self { min_confidence: 0.2, contradiction_flag_at: 0.7 }
    }
}

impl Params {
    /// Read `VISIONCLAW_JEV_PATCH_MIN_CONFIDENCE` and
    /// `VISIONCLAW_JEV_PATCH_CONTRADICTION_AT`.
    pub fn from_lookup(lookup: &dyn Fn(&str) -> Option<String>) -> Self {
        let d = Self::default();
        Self {
            min_confidence: crate::config::threshold(lookup, "VISIONCLAW_JEV_PATCH_MIN_CONFIDENCE", d.min_confidence),
            contradiction_flag_at: crate::config::threshold(lookup, "VISIONCLAW_JEV_PATCH_CONTRADICTION_AT", d.contradiction_flag_at),
        }
    }
}

/// A reviewer-facing assessment. Advisory only.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Assessment {
    /// Evidence support, 0..=1.
    pub support: f64,
    /// Clarity, 0..=1.
    pub clarity: f64,
    /// Probability that the evidence contradicts the hypothesis.
    pub contradiction_probability: f64,
    /// `contradiction_probability` crossed the flag threshold.
    pub contradiction_flag: bool,
    /// Probability that the hypothesis asserts claims the supplied evidence
    /// does not support (VG-01-S1-01).
    pub unsupported_probability: f64,
    /// `unsupported_probability` crossed the flag threshold.
    pub unsupported_flag: bool,
    /// Novelty of the change, 0..=1 (VG-01-S1-01).
    pub novelty: f64,
    /// Review urgency, 0..=1 (VG-01-S1-01).
    pub urgency: f64,
    /// Ordering key: `0.5 * support + 0.15 * clarity + 0.35 * urgency`.
    pub priority: f64,
    /// Question-set version.
    pub question_set: &'static str,
}

/// Build the request for one proposal.
pub fn build_request(model: &str, f: &PatchFacts) -> Request {
    let state = json!({
        "kind": clip(&f.kind, 40),
        "hypothesis": clip(&f.hypothesis, 1000),
        "diff": clip(&f.diff, 3000),
        "consequence_delta": f.consequences.as_deref().map(|s| clip(s, 1500)),
        "blockers": f.blockers.iter().take(10).map(|b| clip(b, 200)).collect::<Vec<_>>(),
    });
    let mut questions = BTreeMap::new();
    questions.insert(
        "support".into(),
        Question::Score {
            instructions: "How well do `diff` and `consequence_delta` support the `hypothesis`? Judge only the evidence supplied; \
                           do not assume anything about pages that are not shown."
                .into(),
            criteria: SUPPORT_LEVELS.iter().map(|s| s.to_string()).collect(),
        },
    );
    questions.insert(
        "clarity".into(),
        Question::Score {
            instructions: "How clearly do `hypothesis` and `diff` state what this ontology change asserts?".into(),
            criteria: CLARITY_LEVELS.iter().map(|s| s.to_string()).collect(),
        },
    );
    questions.insert(
        "unsupported".into(),
        Question::Noul {
            instructions: "The `hypothesis` asserts at least one claim that neither `diff` nor `consequence_delta` supports.".into(),
        },
    );
    questions.insert(
        "novelty".into(),
        Question::Score {
            instructions: "How much new knowledge does this change add, judged from `consequence_delta` and `diff` only?".into(),
            criteria: NOVELTY_LEVELS.iter().map(|s| s.to_string()).collect(),
        },
    );
    questions.insert(
        "urgency".into(),
        Question::Score {
            instructions: "How urgently does a curator need to review this proposal, given its `diff`, `consequence_delta` and `blockers`?".into(),
            criteria: URGENCY_LEVELS.iter().map(|s| s.to_string()).collect(),
        },
    );
    questions.insert(
        "contradiction".into(),
        Question::Noul {
            instructions: "The `diff` or `consequence_delta` contradicts the `hypothesis`: what is asserted in one is denied or reversed in the other."
                .into(),
        },
    );
    Request { model: model.into(), state, questions }
}

/// Interpret one proposal's answers.
///
/// # Errors
///
/// [`Fallback::Malformed`] for missing/out-of-range answers;
/// [`Fallback::LowConfidence`] if either Score is under the threshold.
pub fn interpret(answers: &Answers, params: &Params) -> Result<Assessment, Fallback> {
    let s = answers.score("support", SUPPORT_LEVELS.len())?;
    let c = answers.score("clarity", CLARITY_LEVELS.len())?;
    let x = answers.noul("contradiction")?;
    let un = answers.noul("unsupported")?;
    let nv = answers.score("novelty", NOVELTY_LEVELS.len())?;
    let ur = answers.score("urgency", URGENCY_LEVELS.len())?;
    if [&s, &c, &nv, &ur].iter().any(|a| a.confidence < params.min_confidence) {
        return Err(Fallback::LowConfidence);
    }
    let support = s.score / (SUPPORT_LEVELS.len() - 1) as f64;
    let clarity = c.score / (CLARITY_LEVELS.len() - 1) as f64;
    let novelty = nv.score / (NOVELTY_LEVELS.len() - 1) as f64;
    let urgency = ur.score / (URGENCY_LEVELS.len() - 1) as f64;
    Ok(Assessment {
        support,
        clarity,
        unsupported_probability: un,
        unsupported_flag: un >= params.contradiction_flag_at,
        novelty,
        urgency,
        contradiction_probability: x,
        contradiction_flag: x >= params.contradiction_flag_at,
        priority: 0.5 * support + 0.15 * clarity + 0.35 * urgency,
        question_set: QSET_VERSION,
    })
}

/// Assess one proposal.
///
/// # Errors
///
/// Any [`Fallback`].
pub async fn assess(jev: &Jev, f: &PatchFacts, params: &Params) -> Result<Assessment, Fallback> {
    let answers = jev.ask(&build_request(jev.model(), f)).await?;
    interpret(&answers, params)
}

/// Order proposals best-supported first, or return them unchanged.
///
/// Returns the items and, aligned with them, each one's [`Assessment`]. With
/// `jev == None`, or when a proposal's assessment falls back, that proposal is
/// unassessed and keeps its place; if nothing is assessed the returned items
/// are **identical** to the input.
pub async fn rank_proposals<T>(
    jev: Option<&Jev>,
    items: Vec<T>,
    facts: impl Fn(&T) -> PatchFacts,
    params: &Params,
) -> (Vec<T>, Vec<Option<Assessment>>) {
    let n = items.len();
    let Some(jev) = jev else { return (items, vec![None; n]) };
    let fs: Vec<PatchFacts> = items.iter().map(&facts).collect();
    let results = futures_util::future::join_all(fs.iter().map(|f| assess(jev, f, params))).await;
    let mut ann: Vec<Option<Assessment>> = Vec::with_capacity(n);
    for r in results {
        match r {
            Ok(a) => ann.push(Some(a)),
            Err(reason) => {
                crate::log_fallback(jev.item(), QSET_VERSION, &reason);
                ann.push(None);
            }
        }
    }
    let keys: Vec<Option<f64>> = ann.iter().map(|a| a.as_ref().map(|a| a.priority)).collect();
    let order = crate::slot_order(&keys);
    let ann_sorted = order.iter().map(|&i| ann[i].clone()).collect();
    (crate::apply_order(items, &order), ann_sorted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::testing::*;
    use crate::client::{RawResponse, TransportError};
    use crate::config::Item;
    use crate::wire::mock;

    fn facts(h: &str) -> PatchFacts {
        PatchFacts { kind: "edit".into(), hypothesis: h.into(), diff: format!("+ {h}"), consequences: Some("{\"added\":1}".into()), blockers: vec![] }
    }

    fn body(support: f64, clarity: f64, contra: f64, conf: f64) -> String {
        mock::body(vec![
            ("support".into(), mock::score(support, conf)),
            ("clarity".into(), mock::score(clarity, conf)),
            ("contradiction".into(), mock::noul(contra)),
            ("unsupported".into(), mock::noul(1.0 - support / 3.0)),
            ("novelty".into(), mock::score(1.0, conf)),
            ("urgency".into(), mock::score(support * 2.0 / 3.0, conf)),
        ])
    }

    #[test]
    fn wire_shape() {
        let r = build_request("jev-latest", &facts("A subclass of B"));
        r.validate().unwrap();
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["questions"]["support"]["criteria"].as_array().unwrap().len(), 4);
        assert_eq!(v["questions"]["clarity"]["criteria"].as_array().unwrap().len(), 3);
        assert_eq!(v["questions"]["contradiction"]["type"], "noul");
        assert_eq!(v["questions"]["unsupported"]["type"], "noul");
        assert_eq!(v["questions"]["novelty"]["criteria"].as_array().unwrap().len(), 3);
        assert_eq!(v["questions"]["urgency"]["criteria"].as_array().unwrap().len(), 3);
        assert_eq!(v["state"]["hypothesis"], "A subclass of B");
    }

    #[tokio::test]
    async fn accept_path_puts_best_supported_first_and_flags_contradiction() {
        let t = FnTransport::new(|b| {
            let h = b["state"]["hypothesis"].as_str().unwrap();
            let body = match h {
                "weak" => body(0.0, 2.0, 0.9, 0.9),
                "strong" => body(3.0, 2.0, 0.1, 0.9),
                _ => body(1.0, 1.0, 0.1, 0.9),
            };
            Ok(RawResponse { status: 200, body })
        });
        let j = jev(Item::PatchRank, t);
        let (out, ann) = rank_proposals(Some(&j), vec!["weak", "mid", "strong"], |h| facts(h), &Params::default()).await;
        assert_eq!(out, vec!["strong", "mid", "weak"]);
        assert!(!ann[0].as_ref().unwrap().contradiction_flag);
        assert!(ann[2].as_ref().unwrap().contradiction_flag);
        assert!(ann[2].as_ref().unwrap().unsupported_flag, "weak evidence is flagged unsupported");
        assert!(!ann[0].as_ref().unwrap().unsupported_flag);
    }

    #[tokio::test]
    async fn every_fallback_returns_the_input_order() {
        let input = vec!["a", "b", "c"];
        let (out, ann) = rank_proposals(None, input.clone(), |h| facts(h), &Params::default()).await;
        assert_eq!((out, ann.iter().all(Option::is_none)), (input.clone(), true));
        let missing = mock::body(vec![("support".into(), mock::score(1.0, 0.9))]);
        for s in [
            Script::Reply(500, "x".into()),
            Script::Reply(429, "x".into()),
            Script::Reply(200, "nope".into()),
            Script::Reply(200, missing),
            Script::Reply(200, body(3.0, 2.0, 0.0, 0.0)),
            Script::Fail(TransportError::Other),
            Script::Fail(TransportError::Timeout),
            Script::Hang,
        ] {
            let j = jev(Item::PatchRank, MockTransport::new(s));
            let (out, ann) = rank_proposals(Some(&j), input.clone(), |h| facts(h), &Params::default()).await;
            assert_eq!(out, input);
            assert!(ann.iter().all(Option::is_none));
        }
    }
}
