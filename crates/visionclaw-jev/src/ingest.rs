//! VC-21-S1-01: metadata and entity-type suggestions during ingest.
//!
//! State: a bounded page excerpt (only for pages the §V4 inclusion gate has
//! already admitted), the current frontmatter, the vocabulary's type
//! candidates and the page's frontmatter relation claims. Questions: a
//! **Choice** between the vocabulary types and `unknown`, and one **Noul** per
//! relation claim judging its textual support in the excerpt.
//!
//! Suggestions are drafts. They are written to a JSON-lines file (or logged)
//! for a human; they never modify the page, its frontmatter, its node, the
//! inclusion gate, the parsers or generation identity.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::Serialize;
use serde_json::json;

use crate::client::Jev;
use crate::config::Item;
use crate::wire::{Answers, Question, Request};
use crate::{clip, Fallback};

/// Question-set version, logged on every fallback.
pub const QSET_VERSION: &str = "vc-21-s1-01/1";

/// The reserved abstention key in the type Choice.
pub const UNKNOWN: &str = "unknown";

/// One frontmatter relation claim to be judged against the excerpt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelationClaim {
    /// Relation key from the vocabulary (for example `is-subclass-of`).
    pub key: String,
    /// The target the frontmatter names.
    pub target: String,
}

/// What may be sent about one admitted page.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PageFacts {
    /// Source-relative page path (identity only; also used in the draft).
    pub path: String,
    /// Bounded page body excerpt.
    pub excerpt: String,
    /// Current frontmatter as authored (bounded).
    pub frontmatter: String,
    /// Vocabulary type candidates: `(name, owl description)`.
    pub type_candidates: Vec<(String, String)>,
    /// Frontmatter relation claims to judge.
    pub relations: Vec<RelationClaim>,
}

/// Tunables.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    /// Type Choice confidence below this yields no suggestion.
    pub min_confidence: f64,
    /// At most this many relation claims are judged.
    pub max_relations: usize,
    /// At most this many pages are suggested per [`Suggester`].
    pub max_pages: usize,
}

impl Default for Params {
    fn default() -> Self {
        Self { min_confidence: 0.5, max_relations: 8, max_pages: 25 }
    }
}

impl Params {
    /// Read `VISIONCLAW_JEV_INGEST_MIN_CONFIDENCE`, `…_MAX_RELATIONS` and
    /// `…_MAX_PAGES`.
    pub fn from_lookup(lookup: &dyn Fn(&str) -> Option<String>) -> Self {
        let d = Self::default();
        Self {
            min_confidence: crate::config::threshold(lookup, "VISIONCLAW_JEV_INGEST_MIN_CONFIDENCE", d.min_confidence),
            max_relations: crate::config::count(lookup, "VISIONCLAW_JEV_INGEST_MAX_RELATIONS", d.max_relations, 20),
            max_pages: crate::config::count(lookup, "VISIONCLAW_JEV_INGEST_MAX_PAGES", d.max_pages, 10_000),
        }
    }
}

/// A judged relation claim.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RelationSupport {
    /// Relation key.
    pub key: String,
    /// Target named by the frontmatter.
    pub target: String,
    /// Probability that the excerpt textually supports the claim.
    pub support: f64,
}

/// A draft suggestion for a human.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Suggestion {
    /// Page path.
    pub path: String,
    /// Suggested vocabulary type, or `None` when Jev chose `unknown`.
    pub suggested_type: Option<String>,
    /// Documented Choice confidence.
    pub type_confidence: f64,
    /// Per-claim support probabilities.
    pub relations: Vec<RelationSupport>,
    /// Question-set version.
    pub question_set: &'static str,
}

fn relation_id(i: usize) -> String {
    format!("rel_{i}")
}

/// Build the request. The Choice offers every candidate plus [`UNKNOWN`].
pub fn build_request(model: &str, f: &PageFacts, max_relations: usize) -> Request {
    let state = json!({
        "path": clip(&f.path, 300),
        "excerpt": clip(&f.excerpt, 2000),
        "current_frontmatter": clip(&f.frontmatter, 800),
        "relation_claims": f.relations.iter().take(max_relations).enumerate()
            .map(|(i, r)| json!({"id": relation_id(i), "relation": r.key, "target": clip(&r.target, 200)}))
            .collect::<Vec<_>>(),
    });
    let mut criteria: BTreeMap<String, String> = f
        .type_candidates
        .iter()
        .filter(|(n, _)| n != UNKNOWN)
        .map(|(n, d)| (n.clone(), clip(&format!("The page is a {n} ({d})."), 300)))
        .collect();
    criteria.insert(
        UNKNOWN.into(),
        "The excerpt does not give enough evidence to choose any of the other types, or fits none of them.".into(),
    );
    let mut questions = BTreeMap::new();
    questions.insert(
        "type".into(),
        Question::Choice {
            instructions: "Which entity type does the page in `excerpt` describe? Choose only from the offered types, using the \
                           page's own text. Choose unknown when the excerpt does not establish a type."
                .into(),
            criteria,
        },
    );
    for (i, r) in f.relations.iter().take(max_relations).enumerate() {
        questions.insert(
            relation_id(i),
            Question::Noul {
                instructions: format!(
                    "The text in `excerpt` supports the claim in `relation_claims` with id \"{}\": that this page has the relation \"{}\" to \"{}\".",
                    relation_id(i),
                    r.key,
                    clip(&r.target, 200)
                ),
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
/// [`Fallback::LowConfidence`] if the type Choice is under the threshold.
pub fn interpret(answers: &Answers, f: &PageFacts, params: &Params) -> Result<Suggestion, Fallback> {
    let mut offered: Vec<&str> =
        f.type_candidates.iter().map(|(n, _)| n.as_str()).filter(|n| *n != UNKNOWN).collect();
    offered.push(UNKNOWN);
    let t = answers.choice("type", &offered)?;
    let mut relations = Vec::new();
    for (i, r) in f.relations.iter().take(params.max_relations).enumerate() {
        relations.push(RelationSupport { key: r.key.clone(), target: r.target.clone(), support: answers.noul(&relation_id(i))? });
    }
    if t.confidence < params.min_confidence {
        return Err(Fallback::LowConfidence);
    }
    Ok(Suggestion {
        path: f.path.clone(),
        suggested_type: (t.choice != UNKNOWN).then_some(t.choice),
        type_confidence: t.confidence,
        relations,
        question_set: QSET_VERSION,
    })
}

/// Ask Jev about one page.
///
/// # Errors
///
/// Any [`Fallback`].
pub async fn suggest(jev: &Jev, f: &PageFacts, params: &Params) -> Result<Suggestion, Fallback> {
    if f.type_candidates.is_empty() {
        return Err(Fallback::Malformed);
    }
    let answers = jev.ask(&build_request(jev.model(), f, params.max_relations)).await?;
    interpret(&answers, f, params)
}

/// Per-run suggestion driver: a page budget and an optional draft sink.
pub struct Suggester {
    jev: Jev,
    params: Params,
    used: AtomicUsize,
    sink: Option<PathBuf>,
}

impl Suggester {
    /// Build from the environment, or `None` (flag off, no endpoint, …).
    /// `VISIONCLAW_JEV_INGEST_DRAFTS` names the JSON-lines draft file.
    pub fn from_env() -> Option<Self> {
        let jev = Jev::from_env_logged(Item::IngestSuggest, QSET_VERSION)?;
        let lookup = |k: &str| std::env::var(k).ok();
        let sink = lookup("VISIONCLAW_JEV_INGEST_DRAFTS").filter(|s| !s.trim().is_empty()).map(PathBuf::from);
        Some(Self::new(jev, Params::from_lookup(&lookup), sink))
    }

    /// Build from parts (tests, or a host with its own transport).
    pub fn new(jev: Jev, params: Params, sink: Option<PathBuf>) -> Self {
        Self { jev, params, used: AtomicUsize::new(0), sink }
    }

    /// Suggest for one admitted page. Returns the draft (also recorded) or
    /// `None` on budget exhaustion or any fallback. Never fails the caller.
    pub async fn suggest(&self, f: &PageFacts) -> Option<Suggestion> {
        if self.used.fetch_add(1, Ordering::Relaxed) >= self.params.max_pages {
            return None;
        }
        match suggest(&self.jev, f, &self.params).await {
            Ok(s) => {
                self.record(&s).await;
                Some(s)
            }
            Err(reason) => {
                crate::log_fallback(Item::IngestSuggest, QSET_VERSION, &reason);
                None
            }
        }
    }

    async fn record(&self, s: &Suggestion) {
        match &self.sink {
            Some(path) => {
                let path = path.clone();
                let line = serde_json::to_string(s).unwrap_or_default();
                let wrote = tokio::task::spawn_blocking(move || {
                    use std::io::Write;
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                        .and_then(|mut f| writeln!(f, "{line}"))
                })
                .await;
                if !matches!(wrote, Ok(Ok(()))) {
                    log::warn!("jev ingest draft could not be written");
                }
            }
            None => log::info!(
                "jev ingest draft path={} type={} confidence={:.2}",
                s.path,
                s.suggested_type.as_deref().unwrap_or(UNKNOWN),
                s.type_confidence
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::testing::*;
    use crate::client::TransportError;
    use crate::wire::mock;

    fn facts() -> PageFacts {
        PageFacts {
            path: "knowledge/Foo.md".into(),
            excerpt: "Foo is a kind of Bar used for baz.".into(),
            frontmatter: "public: true".into(),
            type_candidates: vec![("Class".into(), "owl:Class".into()), ("Individual".into(), "owl:NamedIndividual".into())],
            relations: vec![RelationClaim { key: "is-subclass-of".into(), target: "Bar".into() }],
        }
    }

    fn body(choice: &str, conf: f64, rel: f64) -> String {
        mock::body(vec![("type".into(), mock::choice(choice, conf)), ("rel_0".into(), mock::noul(rel))])
    }

    #[test]
    fn wire_shape_offers_unknown_and_a_noul_per_relation() {
        let r = build_request("jev-latest", &facts(), 8);
        r.validate().unwrap();
        let v = serde_json::to_value(&r).unwrap();
        let crit = v["questions"]["type"]["criteria"].as_object().unwrap();
        assert!(crit.contains_key("Class") && crit.contains_key("Individual") && crit.contains_key("unknown"));
        assert_eq!(v["questions"]["rel_0"]["type"], "noul");
        assert_eq!(v["state"]["relation_claims"][0]["id"], "rel_0");
    }

    #[tokio::test]
    async fn accept_path_returns_a_draft_and_records_it() {
        let dir = std::env::temp_dir().join(format!("jev-ingest-{}", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let j = jev(Item::IngestSuggest, MockTransport::new(Script::Reply(200, body("Class", 0.9, 0.8))));
        let s = Suggester::new(j, Params::default(), Some(dir.clone()));
        let d = s.suggest(&facts()).await.unwrap();
        assert_eq!(d.suggested_type.as_deref(), Some("Class"));
        assert_eq!(d.relations[0].support, 0.8);
        let written = std::fs::read_to_string(&dir).unwrap();
        assert!(written.contains("\"suggested_type\":\"Class\""));
        let _ = std::fs::remove_file(&dir);
    }

    #[tokio::test]
    async fn unknown_is_an_abstention() {
        let j = jev(Item::IngestSuggest, MockTransport::new(Script::Reply(200, body("unknown", 0.9, 0.1))));
        let d = suggest(&j, &facts(), &Params::default()).await.unwrap();
        assert_eq!(d.suggested_type, None);
    }

    #[tokio::test]
    async fn every_fallback_yields_no_suggestion() {
        let outside = body("Property", 0.9, 0.5);
        let low = body("Class", 0.1, 0.5);
        let missing_rel = mock::body(vec![("type".into(), mock::choice("Class", 0.9))]);
        for s in [
            Script::Reply(500, "x".into()),
            Script::Reply(429, "x".into()),
            Script::Reply(200, "garbage".into()),
            Script::Reply(200, outside),
            Script::Reply(200, low),
            Script::Reply(200, missing_rel),
            Script::Fail(TransportError::Other),
            Script::Fail(TransportError::Timeout),
            Script::Hang,
        ] {
            let sg = Suggester::new(jev(Item::IngestSuggest, MockTransport::new(s)), Params::default(), None);
            assert_eq!(sg.suggest(&facts()).await, None);
        }
    }

    #[tokio::test]
    async fn budget_stops_further_requests() {
        let t = MockTransport::new(Script::Reply(200, body("Class", 0.9, 0.8)));
        let sg = Suggester::new(jev(Item::IngestSuggest, t.clone()), Params { max_pages: 1, ..Params::default() }, None);
        assert!(sg.suggest(&facts()).await.is_some());
        assert!(sg.suggest(&facts()).await.is_none());
        assert_eq!(t.calls(), 1);
    }
}
