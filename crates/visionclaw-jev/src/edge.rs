//! Jev option for edge meaning (VC-25-S1-01).
//!
//! `EdgeClassifier` (the server's keyword classifier) guesses a predicate
//! from keywords and class pairs. This module adds an opt-in second opinion
//! that fills only the *gaps* (edges the deterministic classifier leaves
//! unclassified): a Choice over the admissible vocabulary predicates (with an
//! `unsupported` option) and a separate Noul asking whether the evidence span
//! actually asserts the relation.
//!
//! Jev produces a **proposal only**. The proposal must clear the
//! outcome-changing threshold (`VISIONCLAW_JEV_AUTO_MIN_CONFIDENCE`; unset ⇒
//! accept nothing), must pass the deterministic domain/range gate below, and
//! then flows through the same enrichment write and downstream governed
//! acceptance as a keyword-classified edge. A deterministic classification is
//! never overridden.

use std::collections::BTreeMap;

use serde_json::json;

use crate::config::Item;
use crate::hub::JevHub;
use crate::wire::Question;
use crate::Fallback;

/// Question-set version (logged on fallback).
pub const EDGE_QUESTION_SET: &str = "vc-edge-meaning-v1";
/// Choice key meaning "no admissible predicate".
pub const UNSUPPORTED: &str = "unsupported";
const MAX_EVIDENCE_CHARS: usize = 1200;

struct Predicate {
    iri: &'static str,
    definition: &'static str,
    /// Substring the source class IRI must contain when known ("" = any).
    domain: &'static str,
    /// Substring the target class IRI must contain when known ("" = any).
    range: &'static str,
}

/// Admissible predicates: exactly the IRIs the keyword classifier can emit,
/// each with a definition and a domain/range.
const VOCAB: &[Predicate] = &[
    Predicate { iri: "mv:worksAt", definition: "The source person is currently employed by, or works for, the target organisation.", domain: "Person", range: "Company" },
    Predicate { iri: "mv:hasCEO", definition: "The target person is the chief executive of the source organisation.", domain: "Company", range: "Person" },
    Predicate { iri: "mv:hasCTO", definition: "The target person is the chief technology officer of the source organisation.", domain: "Company", range: "Person" },
    Predicate { iri: "mv:hasFounder", definition: "The target person founded the source organisation.", domain: "Company", range: "Person" },
    Predicate { iri: "mv:contributesTo", definition: "The source person contributes to the target project.", domain: "Person", range: "Project" },
    Predicate { iri: "mv:sponsors", definition: "The source organisation funds or sponsors the target project.", domain: "Company", range: "Project" },
    Predicate { iri: "mv:usesProject", definition: "The source uses the target project.", domain: "", range: "Project" },
    Predicate { iri: "mv:usesTechnology", definition: "The source project uses the target technology.", domain: "Project", range: "Technology" },
    Predicate { iri: "mv:subConceptOf", definition: "The source concept is a narrower kind of the target concept.", domain: "Concept", range: "Concept" },
    Predicate { iri: "mv:relatedTo", definition: "The source and target concepts are associated but no more specific predicate is stated.", domain: "Concept", range: "Concept" },
];

/// Source evidence for one edge.
#[derive(Debug, Clone)]
pub struct EdgeEvidence<'a> {
    /// Source node label.
    pub source_label: &'a str,
    /// Target node label.
    pub target_label: &'a str,
    /// Source class IRI when known.
    pub source_class: Option<&'a str>,
    /// Target class IRI when known.
    pub target_class: Option<&'a str>,
    /// The span of text that carries the link.
    pub evidence: &'a str,
}

/// A proposed predicate. Not an accepted edge.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeProposal {
    /// Vocabulary predicate IRI.
    pub predicate: String,
    /// Choice confidence.
    pub confidence: f64,
    /// Probability that the span asserts the relation.
    pub asserted: f64,
}

/// Deterministic domain/range gate. A known class that contradicts the
/// predicate's domain or range rejects it; unknown classes are not rejected
/// (the pre-change classifier does not check them either).
pub fn domain_range_ok(iri: &str, source_class: Option<&str>, target_class: Option<&str>) -> bool {
    let Some(p) = VOCAB.iter().find(|p| p.iri == iri) else { return false };
    let ok = |class: Option<&str>, want: &str| match class {
        Some(c) if !want.is_empty() => c.contains(want),
        _ => true,
    };
    ok(source_class, p.domain) && ok(target_class, p.range)
}

fn questions(ev: &EdgeEvidence<'_>) -> Vec<(String, Question)> {
    let mut criteria: BTreeMap<String, String> =
        VOCAB.iter().map(|p| (p.iri.to_string(), p.definition.to_string())).collect();
    criteria.insert(
        UNSUPPORTED.into(),
        "None of the listed predicates is stated by `evidence`: the relation is negated, only historical, hypothetical, or absent, or the span names a different relation.".into(),
    );
    let _ = ev;
    vec![
        (
            "predicate".into(),
            Question::Choice {
                instructions: "Which predicate does `evidence` assert between `source` and `target`? Choose the single most specific predicate the text states. Choose `unsupported` when the text negates the relation, describes it only as past or hypothetical, or states none of the listed predicates.".into(),
                criteria,
            },
        ),
        (
            "asserted".into(),
            Question::Noul {
                instructions: "The text in `evidence` explicitly states a present, non-negated relationship between `source` and `target`.".into(),
            },
        ),
    ]
}

/// Ask Jev for a predicate proposal. `Err` ⇒ leave the edge unclassified,
/// exactly as the deterministic classifier did.
pub async fn propose_edge(client: &JevHub, ev: &EdgeEvidence<'_>) -> Result<EdgeProposal, Fallback> {
    if !client.enabled(Item::EdgeMeaning) {
        return Err(Fallback::FlagOff);
    }
    let evidence: String = ev.evidence.chars().take(MAX_EVIDENCE_CHARS).collect();
    if evidence.trim().is_empty() {
        return Err(Fallback::Malformed);
    }
    let state = json!({
        "source": {"label": ev.source_label, "class": ev.source_class},
        "target": {"label": ev.target_label, "class": ev.target_class},
        "evidence": evidence,
    });
    let answers = client.ask(Item::EdgeMeaning, EDGE_QUESTION_SET, state, questions(ev)).await?;
    let mut offered: Vec<&str> = VOCAB.iter().map(|p| p.iri).collect();
    offered.push(UNSUPPORTED);
    let c = answers.choice("predicate", &offered)?;
    let asserted = answers.noul("asserted")?;
    let reject = |r: Fallback| {
        r.log(Item::EdgeMeaning, EDGE_QUESTION_SET);
        Err(r)
    };
    if c.choice == UNSUPPORTED {
        return reject(Fallback::LowConfidence);
    }
    if let Err(r) = client.accept_outcome(c.confidence) {
        return reject(r);
    }
    if client.accept_outcome(asserted).is_err() {
        return reject(Fallback::LowConfidence);
    }
    if !domain_range_ok(&c.choice, ev.source_class, ev.target_class) {
        return reject(Fallback::LowConfidence);
    }
    Ok(EdgeProposal { predicate: c.choice, confidence: c.confidence, asserted })
}

/// The predicate IRIs the Jev edge vocabulary offers, so the host can check
/// they stay in lockstep with its deterministic classifier.
pub fn vocabulary_iris() -> Vec<&'static str> {
    VOCAB.iter().map(|p| p.iri).collect()
}

/// Gap-fill helper used by the enrichment service: the proposed predicate IRI,
/// or `None` for every fallback (flag off, no client, any failure).
pub async fn jev_gap_fill(client: Option<&JevHub>, ev: &EdgeEvidence<'_>) -> Option<String> {
    let client = client?;
    propose_edge(client, ev).await.ok().map(|p| p.predicate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::testing::{MockTransport, Script};
    use crate::client::TransportError;
    use crate::hub::testing::enabled_hub;

    fn ev() -> EdgeEvidence<'static> {
        EdgeEvidence {
            source_label: "Ada",
            target_label: "Acme",
            source_class: Some("mv:Person"),
            target_class: Some("mv:Company"),
            evidence: "Ada is employed by Acme.",
        }
    }
    fn body(pred: &str, conf: f64, noul: f64) -> String {
        json!({"model":"m","answers":{
            "predicate":{"type":"choice","choice":pred,"confidence":conf,"probabilities":{}},
            "asserted":{"type":"noul","noul":noul}},"usage":{"input_tokens":1,"output_tokens":1}}).to_string()
    }
    fn client(script: Script, auto: Option<&str>) -> (JevHub, std::sync::Arc<MockTransport>) {
        let t = MockTransport::new(script);
        let extra: Vec<(&str, &str)> = auto.map(|a| vec![("VISIONCLAW_JEV_AUTO_MIN_CONFIDENCE", a)]).unwrap_or_default();
        (enabled_hub(&extra, t.clone()), t)
    }

    #[test]
    fn domain_range_gate() {
        assert!(domain_range_ok("mv:worksAt", Some("mv:Person"), Some("mv:Company")));
        assert!(!domain_range_ok("mv:worksAt", Some("mv:Company"), Some("mv:Person")));
        assert!(domain_range_ok("mv:worksAt", None, None));
        assert!(!domain_range_ok("mv:invented", None, None));
    }

    #[tokio::test]
    async fn wire_shape_and_accept_path() {
        let (c, t) = client(Script::Reply(200, body("mv:worksAt", 0.95, 0.97)), Some("0.9"));
        let p = propose_edge(&c, &ev()).await.unwrap();
        assert_eq!(p.predicate, "mv:worksAt");
        let b = t.last_body().unwrap();
        assert!(b["questions"]["predicate"]["criteria"].is_object());
        assert!(b["questions"]["predicate"]["criteria"].get(UNSUPPORTED).is_some());
        assert_eq!(b["questions"]["asserted"]["type"], "noul");
        assert_eq!(b["state"]["evidence"], "Ada is employed by Acme.");
        // Existing validation still runs: domain/range rejects a swapped pair.
        let swapped = EdgeEvidence { source_class: Some("mv:Company"), target_class: Some("mv:Person"), ..ev() };
        let (c, _) = client(Script::Reply(200, body("mv:worksAt", 0.95, 0.97)), Some("0.9"));
        assert_eq!(propose_edge(&c, &swapped).await.unwrap_err(), Fallback::LowConfidence);
    }

    #[tokio::test]
    async fn fallbacks_leave_edge_unclassified() {
        // flag off
        let t = MockTransport::new(Script::Reply(200, body("mv:worksAt", 1.0, 1.0)));
        let off = JevHub::with_transport(|_| None, t.clone());
        assert_eq!(propose_edge(&off, &ev()).await.unwrap_err(), Fallback::FlagOff);
        assert_eq!(t.calls(), 0);
        // default threshold: accept nothing automatically
        let (c, _) = client(Script::Reply(200, body("mv:worksAt", 1.0, 1.0)), None);
        assert_eq!(propose_edge(&c, &ev()).await.unwrap_err(), Fallback::LowConfidence);
        let cases: Vec<(Script, Fallback)> = vec![
            (Script::Fail(TransportError::Timeout), Fallback::Timeout),
            (Script::Reply(500, "".into()), Fallback::Http(500)),
            (Script::Reply(200, "junk".into()), Fallback::Malformed),
            (Script::Reply(200, body("mv:invented", 0.99, 0.99)), Fallback::Malformed),
            (Script::Reply(200, body("mv:worksAt", 0.3, 0.99)), Fallback::LowConfidence),
            (Script::Reply(200, body("mv:worksAt", 0.99, 0.3)), Fallback::LowConfidence),
            (Script::Reply(200, body(UNSUPPORTED, 0.99, 0.99)), Fallback::LowConfidence),
        ];
        for (s, want) in cases {
            let (c, _) = client(s, Some("0.9"));
            assert_eq!(propose_edge(&c, &ev()).await.unwrap_err(), want);
        }
    }
}
