//! VC-08-S1-01: triage degraded-state and canary failures.
//!
//! State: sanitised canary results (id, description, kind, armed/fired,
//! counts, registration SHA), the `kg_backend_up` gauge and the current build
//! SHA. Questions: a **Choice** over a bounded fault catalogue
//! ([`FAULT_CLASSES`]) and one **Score** per runbook step in [`RUNBOOK`].
//!
//! Probe verdicts stay exact and unchanged. The triage is a hypothesis for a
//! human: it supplies no readiness verdict, no restart permission and no
//! evidence of repair. A deploy *diff* is not available to the process; only
//! the build SHA and each canary's registration SHA are supplied, and the
//! question set says so.
//!
//! Jev is only asked when [`is_degraded`] holds: the common healthy path makes
//! no request.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::json;

use crate::client::Jev;
use crate::wire::{Answers, Question, Request};
use crate::{clip, Fallback};

/// Question-set version, logged on every fallback.
pub const QSET_VERSION: &str = "vc-08-s1-01/1";

/// The bounded fault catalogue: `(class, when it applies)`.
pub const FAULT_CLASSES: [(&str, &str); 6] = [
    ("kg-backend-down", "The knowledge-graph backend gauge reports down while other checks are green."),
    ("canary-stale-after-deploy", "Canaries registered at an earlier build SHA have not fired since the current build SHA was deployed."),
    ("canary-never-fired", "Armed canaries have no observations at all, so the user journey they cover may never have run."),
    ("upstream-dependency", "Failures follow one external dependency (relay, pod, model or embedder) rather than this service."),
    ("data-pipeline-stalled", "Ingest, sync or reasoning canaries are silent while request-path canaries still fire."),
    ("no-fault-evident", "The supplied results show no fault pattern; this is a monitoring gap, not a diagnosis."),
];

/// A runbook step Jev may rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunbookStep {
    /// Stable id.
    pub id: &'static str,
    /// What the step is.
    pub text: &'static str,
}

/// The fixed runbook. Code owns the steps; Jev only ranks their relevance.
pub const RUNBOOK: [RunbookStep; 6] = [
    RunbookStep { id: "check-kg-backend", text: "Check the knowledge-graph backend and its watchdog self-poll." },
    RunbookStep { id: "compare-build-sha", text: "Compare the running build SHA with the canaries' registration SHAs." },
    RunbookStep { id: "replay-journey", text: "Replay the user journey that the silent canary covers and observe it." },
    RunbookStep { id: "check-upstreams", text: "Check relay, pod, model and embedder reachability." },
    RunbookStep { id: "check-sync-logs", text: "Read the corpus sync and reasoning logs for the last run." },
    RunbookStep { id: "check-canary-registry", text: "Verify the canary registrations and arming are correct." },
];

/// One canary's sanitised facts.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CanaryFact {
    /// Canary id.
    pub id: String,
    /// Description.
    pub description: String,
    /// Kind.
    pub kind: String,
    /// Armed.
    pub armed: bool,
    /// Fired for the current SHA within the window.
    pub fired: bool,
    /// Total observations.
    pub observation_count: i64,
    /// SHA at registration.
    pub sha_at_registration: String,
}

/// Everything sent for one triage.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HealthFacts {
    /// Current build SHA.
    pub sha: String,
    /// The KG backend gauge.
    pub kg_backend_up: Option<bool>,
    /// Canary results.
    pub canaries: Vec<CanaryFact>,
}

/// Whether the facts show anything worth triaging: the KG gauge is down, or an
/// armed canary has not fired.
pub fn is_degraded(f: &HealthFacts) -> bool {
    f.kg_backend_up == Some(false) || f.canaries.iter().any(|c| c.armed && !c.fired)
}

/// Tunables.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    /// Class Choice confidence below this yields no triage.
    pub min_class_confidence: f64,
    /// Step Score confidence below this drops that step from the ranking.
    pub min_step_confidence: f64,
}

impl Default for Params {
    fn default() -> Self {
        Self { min_class_confidence: 0.3, min_step_confidence: 0.2 }
    }
}

impl Params {
    /// Read `VISIONCLAW_JEV_HEALTH_MIN_CONFIDENCE` (class) and
    /// `VISIONCLAW_JEV_HEALTH_MIN_STEP_CONFIDENCE`.
    pub fn from_lookup(lookup: &dyn Fn(&str) -> Option<String>) -> Self {
        let d = Self::default();
        Self {
            min_class_confidence: crate::config::threshold(lookup, "VISIONCLAW_JEV_HEALTH_MIN_CONFIDENCE", d.min_class_confidence),
            min_step_confidence: crate::config::threshold(lookup, "VISIONCLAW_JEV_HEALTH_MIN_STEP_CONFIDENCE", d.min_step_confidence),
        }
    }
}

/// A ranked runbook step.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RankedStep {
    /// Step id from [`RUNBOOK`].
    pub id: &'static str,
    /// Step text.
    pub text: &'static str,
    /// Relevance, 0..=1.
    pub relevance: f64,
}

/// A hypothesis for a human. Not a verdict.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Triage {
    /// Likely investigation class from [`FAULT_CLASSES`].
    pub class: String,
    /// Documented Choice confidence.
    pub class_confidence: f64,
    /// Runbook steps, most relevant first.
    pub steps: Vec<RankedStep>,
    /// Always `"hypothesis"`: this is not a readiness verdict.
    pub kind: &'static str,
    /// Question-set version.
    pub question_set: &'static str,
}

/// Build the request.
pub fn build_request(model: &str, f: &HealthFacts) -> Request {
    let state = json!({
        "build_sha": clip(&f.sha, 64),
        "kg_backend_up": f.kg_backend_up,
        "deploy_diff": "not available; compare build_sha with each canary's sha_at_registration",
        "canaries": f.canaries.iter().take(40).map(|c| json!({
            "id": clip(&c.id, 80),
            "description": clip(&c.description, 200),
            "kind": clip(&c.kind, 40),
            "armed": c.armed,
            "fired_for_current_sha": c.fired,
            "observation_count": c.observation_count,
            "sha_at_registration": clip(&c.sha_at_registration, 64),
        })).collect::<Vec<_>>(),
    });
    let mut questions = BTreeMap::new();
    questions.insert(
        "class".into(),
        Question::Choice {
            instructions: "Which class of fault should an operator investigate first, given `kg_backend_up`, `build_sha` and the \
                           `canaries` results? Several liveness checks may be green while a user journey is broken. Choose \
                           no-fault-evident when the results show no pattern."
                .into(),
            criteria: FAULT_CLASSES.iter().map(|(k, d)| (k.to_string(), d.to_string())).collect(),
        },
    );
    for step in RUNBOOK {
        questions.insert(
            format!("step_{}", step.id),
            Question::Score {
                instructions: format!(
                    "How relevant is this operator action to the degraded state shown in `canaries` and `kg_backend_up`: \"{}\"",
                    step.text
                ),
                criteria: vec![
                    "Irrelevant: the results give no reason to do this.".into(),
                    "Possibly useful: it could rule something out.".into(),
                    "Directly relevant: the results point at what this step checks.".into(),
                ],
            },
        );
    }
    Request { model: model.into(), state, questions }
}

/// Interpret the answers.
///
/// # Errors
///
/// [`Fallback::Malformed`] for missing/out-of-set answers;
/// [`Fallback::LowConfidence`] if the class Choice is under the threshold.
pub fn interpret(answers: &Answers, params: &Params) -> Result<Triage, Fallback> {
    let offered: Vec<&str> = FAULT_CLASSES.iter().map(|(k, _)| *k).collect();
    let class = answers.choice("class", &offered)?;
    let mut steps = Vec::new();
    for step in RUNBOOK {
        let s = answers.score(&format!("step_{}", step.id), 3)?;
        if s.confidence >= params.min_step_confidence {
            steps.push(RankedStep { id: step.id, text: step.text, relevance: s.score / 2.0 });
        }
    }
    if class.confidence < params.min_class_confidence {
        return Err(Fallback::LowConfidence);
    }
    // Stable: ties keep runbook order.
    steps.sort_by(|a, b| b.relevance.partial_cmp(&a.relevance).unwrap_or(std::cmp::Ordering::Equal));
    Ok(Triage { class: class.choice, class_confidence: class.confidence, steps, kind: "hypothesis", question_set: QSET_VERSION })
}

/// Triage a degraded state, or return `None`.
///
/// `None` when `jev` is absent, when the facts are not [`is_degraded`] (no
/// request is made), or on any [`Fallback`] (logged, never raised).
pub async fn triage(jev: Option<&Jev>, f: &HealthFacts, params: &Params) -> Option<Triage> {
    let jev = jev?;
    if !is_degraded(f) {
        return None;
    }
    let result = match jev.ask(&build_request(jev.model(), f)).await {
        Ok(a) => interpret(&a, params),
        Err(e) => Err(e),
    };
    match result {
        Ok(t) => Some(t),
        Err(reason) => {
            crate::log_fallback(jev.item(), QSET_VERSION, &reason);
            None
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

    fn degraded() -> HealthFacts {
        HealthFacts {
            sha: "abc123".into(),
            kg_backend_up: Some(false),
            canaries: vec![CanaryFact { id: "c1".into(), description: "journey".into(), kind: "http".into(), armed: true, fired: false, observation_count: 0, sha_at_registration: "old".into() }],
        }
    }

    fn healthy() -> HealthFacts {
        HealthFacts { kg_backend_up: Some(true), canaries: vec![CanaryFact { armed: true, fired: true, ..Default::default() }], ..Default::default() }
    }

    fn body(class: &str, conf: f64, step_conf: f64) -> String {
        let mut a = vec![("class".to_string(), mock::choice(class, conf))];
        for (i, s) in RUNBOOK.iter().enumerate() {
            a.push((format!("step_{}", s.id), mock::score(if i == 2 { 2.0 } else { 0.0 }, step_conf)));
        }
        mock::body(a)
    }

    #[test]
    fn wire_shape() {
        let r = build_request("jev-latest", &degraded());
        r.validate().unwrap();
        let v = serde_json::to_value(&r).unwrap();
        assert!(v["questions"]["class"]["criteria"].is_object());
        assert_eq!(v["questions"]["class"]["criteria"].as_object().unwrap().len(), FAULT_CLASSES.len());
        assert_eq!(v["questions"].as_object().unwrap().len(), 1 + RUNBOOK.len());
        assert!(v["questions"]["step_replay-journey"]["criteria"].is_array());
        assert_eq!(v["state"]["kg_backend_up"], false);
    }

    #[test]
    fn degraded_detection_is_exact() {
        assert!(is_degraded(&degraded()));
        assert!(!is_degraded(&healthy()));
        assert!(!is_degraded(&HealthFacts::default()));
        assert!(is_degraded(&HealthFacts { kg_backend_up: Some(false), ..Default::default() }));
    }

    #[tokio::test]
    async fn accept_path_ranks_steps_and_labels_a_hypothesis() {
        let j = jev(Item::HealthTriage, MockTransport::new(Script::Reply(200, body("canary-never-fired", 0.9, 0.9))));
        let t = triage(Some(&j), &degraded(), &Params::default()).await.unwrap();
        assert_eq!(t.class, "canary-never-fired");
        assert_eq!(t.steps[0].id, "replay-journey");
        assert_eq!(t.kind, "hypothesis");
    }

    #[tokio::test]
    async fn healthy_state_makes_no_request() {
        let t = MockTransport::new(Script::Reply(200, body("no-fault-evident", 0.9, 0.9)));
        let j = jev(Item::HealthTriage, t.clone());
        assert_eq!(triage(Some(&j), &healthy(), &Params::default()).await, None);
        assert_eq!(t.calls(), 0);
    }

    #[tokio::test]
    async fn every_fallback_yields_none() {
        assert_eq!(triage(None, &degraded(), &Params::default()).await, None);
        let outside = body("made-up-class", 0.9, 0.9);
        let low = body("kg-backend-down", 0.05, 0.9);
        let missing = mock::body(vec![("class".into(), mock::choice("kg-backend-down", 0.9))]);
        for s in [
            Script::Reply(500, "x".into()),
            Script::Reply(429, "x".into()),
            Script::Reply(529, "x".into()),
            Script::Reply(200, "garbage".into()),
            Script::Reply(200, outside),
            Script::Reply(200, low),
            Script::Reply(200, missing),
            Script::Fail(TransportError::Other),
            Script::Fail(TransportError::Timeout),
            Script::Hang,
        ] {
            let j = jev(Item::HealthTriage, MockTransport::new(s));
            assert_eq!(triage(Some(&j), &degraded(), &Params::default()).await, None);
        }
    }

    #[tokio::test]
    async fn low_confidence_steps_are_dropped_not_fabricated() {
        let j = jev(Item::HealthTriage, MockTransport::new(Script::Reply(200, body("kg-backend-down", 0.9, 0.0))));
        let t = triage(Some(&j), &degraded(), &Params::default()).await.unwrap();
        assert!(t.steps.is_empty());
    }
}
