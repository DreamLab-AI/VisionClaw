//! Jev option for bounded natural-language graph queries (VC-25-S1-02).
//!
//! A fast path *in front of* the generative translator in
//! [`crate::services::natural_language_query_service`]: Jev picks one of a
//! small enumerated set of read-only SPARQL templates and, where the template
//! has a slot, one entity candidate that **code** extracted from the question.
//! Code instantiates the template, validates the result with the same
//! read-only validator the NLQ service enforces, and only then offers it.
//! Anything else (no template, no grounded entity, low confidence, any error)
//! returns `Err`, and the existing generative route runs untouched.
//!
//! Instantiating a template changes the outcome (a query is produced without
//! the LLM), so it uses the outcome-changing threshold
//! (`VISIONCLAW_JEV_AUTO_MIN_CONFIDENCE`; unset ⇒ accept nothing).

use serde_json::json;

use crate::services::natural_language_query_service::QueryTranslation;
use crate::services::system_one::{FallbackReason, JevClient, JevItem, Question};

/// Question-set version (logged on fallback).
pub const NLQ_QUESTION_SET: &str = "vc-nlq-plan-v1";
/// Choice key for "no template fits — use the generative route".
pub const GENERATE: &str = "generate";
/// Entity choice key for "none of the candidates".
pub const NO_ENTITY: &str = "none";
const MAX_QUESTION_CHARS: usize = 400;
const MAX_ENTITIES: usize = 8;
const PREFIX: &str = "PREFIX vc: <https://narrativegoldmine.com/ns/v1#>\nPREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>\n";

/// What fills a template's slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    None,
    ClassIri,
    Domain,
}

struct Template {
    id: &'static str,
    description: &'static str,
    slot: Slot,
    build: fn(&str) -> String,
}

fn t_nodes(_: &str) -> String {
    format!("{PREFIX}SELECT ?node ?label WHERE {{ GRAPH <urn:ngm:graph:knowledge> {{ ?node a vc:KnowledgeNode ; rdfs:label ?label }} }} LIMIT 50")
}
fn t_edges(_: &str) -> String {
    format!("{PREFIX}SELECT ?src ?tgt ?weight WHERE {{ GRAPH <urn:ngm:graph:knowledge> {{ ?edge a vc:KGEdge ; vc:source ?src ; vc:target ?tgt ; vc:weight ?weight }} }} LIMIT 50")
}
fn t_domain(d: &str) -> String {
    format!("{PREFIX}SELECT ?class ?term WHERE {{ GRAPH <urn:ngm:graph:ontology:assert> {{ ?class a vc:OntologyClass ; vc:preferredTerm ?term ; vc:sourceDomain \"{d}\" }} }} LIMIT 50")
}
fn t_subclasses(iri: &str) -> String {
    format!("{PREFIX}SELECT ?child WHERE {{ GRAPH <urn:ngm:graph:ontology:inferred> {{ ?child rdfs:subClassOf <{iri}> }} }} LIMIT 50")
}
fn t_describe(iri: &str) -> String {
    format!("DESCRIBE <{iri}>")
}

const TEMPLATES: &[Template] = &[
    Template { id: "list_knowledge_nodes", description: "List the knowledge-graph nodes with their labels. No specific entity is named.", slot: Slot::None, build: t_nodes },
    Template { id: "list_knowledge_edges", description: "List knowledge-graph edges with their source, target and weight. No specific entity is named.", slot: Slot::None, build: t_edges },
    Template { id: "classes_in_domain", description: "List the ontology classes that belong to one named source domain (for example physics). Needs one domain name.", slot: Slot::Domain, build: t_domain },
    Template { id: "subclasses_of_class", description: "List the subclasses of one named ontology class. Needs one class.", slot: Slot::ClassIri, build: t_subclasses },
    Template { id: "describe_class", description: "Describe one named ontology class. Needs one class.", slot: Slot::ClassIri, build: t_describe },
];

/// Entity candidates extracted deterministically from the question. IDs are
/// `e0…`; the slot value is only ever one of these strings.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityCandidate {
    /// Stable reference shown to the model.
    pub id: String,
    /// Raw value (a class IRI or a lower-case domain word).
    pub value: String,
    /// `class_iri` or `domain`.
    pub kind: &'static str,
}

fn safe_iri(s: &str) -> bool {
    s.strip_prefix("urn:ngm:class:")
        .map(|r| !r.is_empty() && r.len() <= 80 && r.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')))
        .unwrap_or(false)
}

fn safe_domain(s: &str) -> bool {
    !s.is_empty() && s.len() <= 40 && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, ' ' | '-' | '_'))
}

/// Extract grounded entity candidates: `urn:ngm:class:…` IRIs and quoted
/// strings (as domains). Nothing here is model-generated.
pub fn extract_entities(question: &str) -> Vec<EntityCandidate> {
    let mut out: Vec<(String, &'static str)> = Vec::new();
    for tok in question.split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | ',' | ';' | '(' | ')')) {
        let tok = tok.trim_matches(|c: char| matches!(c, '"' | '\'' | '.' | '?'));
        if safe_iri(tok) && !out.iter().any(|(v, _)| v == tok) {
            out.push((tok.to_string(), "class_iri"));
        }
    }
    for q in ['"', '\''] {
        let parts: Vec<&str> = question.split(q).collect();
        // Odd-indexed segments are inside quotes.
        for seg in parts.iter().skip(1).step_by(2) {
            let d = seg.trim().to_ascii_lowercase();
            if safe_domain(&d) && !out.iter().any(|(v, _)| *v == d) {
                out.push((d, "domain"));
            }
        }
    }
    out.truncate(MAX_ENTITIES);
    out.into_iter().enumerate().map(|(i, (value, kind))| EntityCandidate { id: format!("e{i}"), value, kind }).collect()
}

fn template_question() -> Question {
    let mut criteria: Vec<(String, String)> =
        TEMPLATES.iter().map(|t| (t.id.to_string(), t.description.to_string())).collect();
    criteria.push((GENERATE.into(), "No listed template answers the question exactly: it asks for something else, combines several things, or needs filtering the templates cannot express.".into()));
    Question::Choice {
        instructions: "Which read-only graph query template answers `question` exactly? Choose a template only if running it returns what the question asks for. Choose `generate` otherwise.".into(),
        criteria,
    }
}

fn entity_question(cands: &[EntityCandidate]) -> Question {
    let mut criteria: Vec<(String, String)> = cands
        .iter()
        .map(|c| (c.id.clone(), format!("The {} `{}` named in the question.", c.kind.replace('_', " "), c.value)))
        .collect();
    criteria.push((NO_ENTITY.into(), "None of the listed entities is the one the question is about.".into()));
    Question::Choice {
        instructions: "Which entity listed in `entities` is the subject the question in `question` asks about? Choose `none` if the question is not about exactly one of them.".into(),
        criteria,
    }
}

fn answerable_question() -> Question {
    Question::Noul {
        instructions: "The question in `question` can be answered completely by one of the listed templates, with no extra filtering, aggregation or joins.".into(),
    }
}

/// Plan a template query. `Err` ⇒ run the existing generative route.
pub async fn plan_query(client: &JevClient, question: &str) -> Result<QueryTranslation, FallbackReason> {
    if !client.enabled(JevItem::NlqPlan) {
        return Err(FallbackReason::FlagOff);
    }
    let q: String = question.trim().chars().take(MAX_QUESTION_CHARS).collect();
    if q.is_empty() {
        return Err(FallbackReason::Malformed);
    }
    let entities = extract_entities(&q);
    let state = json!({
        "question": q,
        "entities": entities.iter().map(|e| json!({"ref": e.id, "kind": e.kind, "value": e.value})).collect::<Vec<_>>(),
    });
    let mut qs = vec![
        ("template".to_string(), template_question()),
        ("answerable".to_string(), answerable_question()),
    ];
    if !entities.is_empty() {
        qs.push(("entity".to_string(), entity_question(&entities)));
    }
    let answers = client.ask(JevItem::NlqPlan, NLQ_QUESTION_SET, false, state, &qs).await?;
    let mut offered: Vec<&str> = TEMPLATES.iter().map(|t| t.id).collect();
    offered.push(GENERATE);
    let pick = answers.choice("template", &offered)?;
    let answerable = answers.noul("answerable")?;
    let fail = |r: FallbackReason| {
        r.log(JevItem::NlqPlan, NLQ_QUESTION_SET);
        Err(r)
    };
    if pick.choice == GENERATE {
        return fail(FallbackReason::LowConfidence);
    }
    if let Err(r) = client.accept_outcome(pick.confidence) {
        return fail(r);
    }
    if client.accept_outcome(answerable).is_err() {
        return fail(FallbackReason::LowConfidence);
    }
    let tpl = TEMPLATES.iter().find(|t| t.id == pick.choice).ok_or(FallbackReason::Malformed)?;
    let slot_value = match tpl.slot {
        Slot::None => String::new(),
        slot => {
            if entities.is_empty() {
                return fail(FallbackReason::LowConfidence);
            }
            let mut ids: Vec<&str> = entities.iter().map(|e| e.id.as_str()).collect();
            ids.push(NO_ENTITY);
            let e = answers.choice("entity", &ids)?;
            if e.choice == NO_ENTITY || client.accept_outcome(e.confidence).is_err() {
                return fail(FallbackReason::LowConfidence);
            }
            let ent = entities.iter().find(|c| c.id == e.choice).ok_or(FallbackReason::Malformed)?;
            let want = if slot == Slot::ClassIri { "class_iri" } else { "domain" };
            if ent.kind != want {
                return fail(FallbackReason::LowConfidence);
            }
            ent.value.clone()
        }
    };
    let sparql = (tpl.build)(&slot_value);
    // Same read-only validator the NLQ service enforces.
    if crate::handlers::ontology_handler::validate_read_only_sparql(&sparql).is_err() {
        return fail(FallbackReason::Malformed);
    }
    Ok(QueryTranslation {
        original_query: question.to_string(),
        sparql_query: sparql,
        explanation: format!("Template `{}` selected by the Jev option", tpl.id),
        confidence: pick.confidence as f32,
        warnings: vec!["jev-template".to_string()],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::system_one::testing::*;
    use crate::services::system_one::{JevSettings, TransportError};

    fn body(tpl: &str, tc: f64, ans: f64, ent: Option<(&str, f64)>) -> String {
        let mut a = json!({
            "template": {"type":"choice","choice":tpl,"confidence":tc,"probabilities":{}},
            "answerable": {"type":"noul","noul":ans},
        });
        if let Some((e, c)) = ent {
            a["entity"] = json!({"type":"choice","choice":e,"confidence":c,"probabilities":{}});
        }
        json!({"model":"m","answers":a,"usage":{"input_tokens":1,"output_tokens":1}}).to_string()
    }
    fn client(script: Script, auto: Option<&str>) -> (JevClient, std::sync::Arc<MockTransport>) {
        let t = MockTransport::new(script);
        let extra: Vec<(&str, &str)> = auto.map(|a| vec![("VISIONCLAW_JEV_AUTO_MIN_CONFIDENCE", a)]).unwrap_or_default();
        (JevClient::with_transport(enabled_settings(&extra), t.clone()), t)
    }

    #[test]
    fn templates_are_read_only_and_valid() {
        for t in TEMPLATES {
            let q = (t.build)(match t.slot { Slot::ClassIri => "urn:ngm:class:x", Slot::Domain => "physics", Slot::None => "" });
            assert!(crate::handlers::ontology_handler::validate_read_only_sparql(&q).is_ok(), "{}", t.id);
        }
    }

    #[test]
    fn entity_extraction_is_injection_safe() {
        let e = extract_entities("subclasses of <urn:ngm:class:Physics> and \"physics\" and urn:ngm:class:x> } DROP");
        assert!(e.iter().any(|c| c.value == "urn:ngm:class:Physics" && c.kind == "class_iri"));
        assert!(e.iter().any(|c| c.value == "physics" && c.kind == "domain"));
        assert!(e.iter().all(|c| !c.value.contains('}') && !c.value.contains(' ') || c.kind == "domain"));
        assert!(extract_entities("\"a\"; DROP GRAPH <x>\" ").iter().all(|c| safe_domain(&c.value) || safe_iri(&c.value)));
        assert!(extract_entities("show \"Bad\\\" } value\"").is_empty());
    }

    #[tokio::test]
    async fn accept_path_instantiates_and_validates() {
        let (c, t) = client(Script::Reply(200, body("subclasses_of_class", 0.95, 0.96, Some(("e0", 0.97)))), Some("0.9"));
        let r = plan_query(&c, "What are the subclasses of urn:ngm:class:Physics?").await.unwrap();
        assert!(r.sparql_query.contains("<urn:ngm:class:Physics>"));
        assert!(r.sparql_query.contains("rdfs:subClassOf"));
        let b = t.last_body().unwrap();
        assert!(b["questions"]["template"]["criteria"].is_object());
        assert!(b["questions"]["template"]["criteria"].get(GENERATE).is_some());
        assert!(b["questions"]["entity"]["criteria"].get("none").is_some());
        assert_eq!(b["state"]["entities"][0]["ref"], "e0");
        let (c, _) = client(Script::Reply(200, body("list_knowledge_nodes", 0.95, 0.96, None)), Some("0.9"));
        let r = plan_query(&c, "Show me all nodes").await.unwrap();
        assert!(r.sparql_query.contains("vc:KnowledgeNode"));
    }

    #[tokio::test]
    async fn fallbacks_return_err_so_generative_route_runs() {
        let q = "What are the subclasses of urn:ngm:class:Physics?";
        let t = MockTransport::new(Script::Reply(200, "{}".into()));
        let off = JevClient::with_transport(JevSettings::disabled(), t.clone());
        assert_eq!(plan_query(&off, q).await.unwrap_err(), FallbackReason::FlagOff);
        assert_eq!(t.calls(), 0);
        let (c, _) = client(Script::Reply(200, body("describe_class", 1.0, 1.0, Some(("e0", 1.0)))), None);
        assert_eq!(plan_query(&c, q).await.unwrap_err(), FallbackReason::LowConfidence, "default accepts nothing");
        let cases: Vec<(Script, FallbackReason)> = vec![
            (Script::Fail(TransportError::Timeout), FallbackReason::Timeout),
            (Script::Reply(503, "".into()), FallbackReason::Http(503)),
            (Script::Reply(200, "junk".into()), FallbackReason::Malformed),
            (Script::Reply(200, body("drop_everything", 0.99, 0.99, None)), FallbackReason::Malformed),
            (Script::Reply(200, body("describe_class", 0.99, 0.99, Some(("e9", 0.99)))), FallbackReason::Malformed),
            (Script::Reply(200, body(GENERATE, 0.99, 0.99, None)), FallbackReason::LowConfidence),
            (Script::Reply(200, body("describe_class", 0.5, 0.99, Some(("e0", 0.99)))), FallbackReason::LowConfidence),
            (Script::Reply(200, body("describe_class", 0.99, 0.4, Some(("e0", 0.99)))), FallbackReason::LowConfidence),
            (Script::Reply(200, body("describe_class", 0.99, 0.99, Some(("none", 0.99)))), FallbackReason::LowConfidence),
        ];
        for (s, want) in cases {
            let (c, _) = client(s, Some("0.9"));
            assert_eq!(plan_query(&c, q).await.unwrap_err(), want);
        }
        // Slot template with no grounded entity falls back.
        let (c, _) = client(Script::Reply(200, body("describe_class", 0.99, 0.99, None)), Some("0.9"));
        assert_eq!(plan_query(&c, "describe the thing").await.unwrap_err(), FallbackReason::LowConfidence);
        // A domain entity cannot fill a class-IRI slot.
        let (c, _) = client(Script::Reply(200, body("describe_class", 0.99, 0.99, Some(("e0", 0.99)))), Some("0.9"));
        assert_eq!(plan_query(&c, "describe \"physics\"").await.unwrap_err(), FallbackReason::LowConfidence);
    }
}
