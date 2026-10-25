//! Request and answer shapes for `POST /v1/systemone`.
//!
//! The shapes follow the published OpenAPI schema: Choice `criteria` is a
//! **map** (choice name → description), Score `criteria` is an **ordered
//! list** (position = score level, from zero), Noul has only instructions.
//! Question IDs are bookkeeping and are not shown to the model, so every
//! `instructions` string carries the question's complete meaning.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::Fallback;

/// One question.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    /// Select one of `criteria`.
    Choice {
        /// The complete meaning of the question.
        instructions: String,
        /// Choice name → when it applies.
        criteria: BTreeMap<String, String>,
    },
    /// Rate against ordered levels (level = index).
    Score {
        /// The complete meaning of the question.
        instructions: String,
        /// Ordered level descriptions, 1..=10.
        criteria: Vec<String>,
    },
    /// Probability of yes.
    Noul {
        /// The yes/no statement, complete in itself.
        instructions: String,
    },
}

/// `POST /v1/systemone` body.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Request {
    /// Model name or alias.
    pub model: String,
    /// The content every question refers to.
    pub state: Value,
    /// Question ID → question.
    pub questions: BTreeMap<String, Question>,
}

impl Request {
    /// Check the schema constraints the service enforces, so a request that
    /// would be rejected never leaves the process.
    ///
    /// # Errors
    ///
    /// A description of the first violated constraint.
    pub fn validate(&self) -> Result<(), String> {
        if self.model.trim().is_empty() {
            return Err("model is empty".into());
        }
        if self.questions.is_empty() {
            return Err("questions is empty".into());
        }
        for (id, q) in &self.questions {
            match q {
                Question::Choice { instructions, criteria } => {
                    if instructions.trim().is_empty() {
                        return Err(format!("{id}: empty instructions"));
                    }
                    if criteria.is_empty() || criteria.len() > 255 {
                        return Err(format!("{id}: choice criteria must hold 1..=255 entries"));
                    }
                }
                Question::Score { instructions, criteria } => {
                    if instructions.trim().is_empty() {
                        return Err(format!("{id}: empty instructions"));
                    }
                    if criteria.is_empty() || criteria.len() > 10 {
                        return Err(format!("{id}: score criteria must hold 1..=10 levels"));
                    }
                }
                Question::Noul { instructions } => {
                    if instructions.trim().is_empty() {
                        return Err(format!("{id}: empty instructions"));
                    }
                }
            }
        }
        Ok(())
    }
}

/// A validated Choice answer.
#[derive(Debug, Clone, PartialEq)]
pub struct ChoiceAnswer {
    /// The selected key, always one of the offered set.
    pub choice: String,
    /// Documented confidence `(p - 1/n) / (1 - 1/n)`, 0..=1.
    pub confidence: f64,
}

/// A validated Score answer.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreAnswer {
    /// Expected score in `0..=levels-1`.
    pub score: f64,
    /// Confidence, 0..=1.
    pub confidence: f64,
}

/// Parsed `answers` map.
#[derive(Debug, Clone)]
pub struct Answers(serde_json::Map<String, Value>);

fn unit(v: &Value) -> Option<f64> {
    v.as_f64().filter(|x| x.is_finite() && (0.0..=1.0).contains(x))
}

impl Answers {
    /// Parse a response body.
    ///
    /// # Errors
    ///
    /// [`Fallback::Malformed`] unless the body is a JSON object with a
    /// non-empty `answers` object.
    pub fn parse(body: &str) -> Result<Self, Fallback> {
        let v: Value = serde_json::from_str(body).map_err(|_| Fallback::Malformed)?;
        match v.get("answers") {
            Some(Value::Object(m)) if !m.is_empty() => Ok(Answers(m.clone())),
            _ => Err(Fallback::Malformed),
        }
    }

    fn typed(&self, id: &str, ty: &str) -> Result<&Value, Fallback> {
        let a = self.0.get(id).ok_or(Fallback::Malformed)?;
        if a.get("type").and_then(Value::as_str) != Some(ty) {
            return Err(Fallback::Malformed);
        }
        Ok(a)
    }

    /// A Choice answer whose key is in `offered`.
    ///
    /// # Errors
    ///
    /// [`Fallback::Malformed`] if the answer is missing, mistyped, outside the
    /// offered set, or has no usable confidence.
    pub fn choice(&self, id: &str, offered: &[&str]) -> Result<ChoiceAnswer, Fallback> {
        let a = self.typed(id, "choice")?;
        let choice = a.get("choice").and_then(Value::as_str).ok_or(Fallback::Malformed)?;
        if !offered.contains(&choice) {
            return Err(Fallback::Malformed);
        }
        let confidence = a.get("confidence").and_then(unit).ok_or(Fallback::Malformed)?;
        Ok(ChoiceAnswer { choice: choice.to_string(), confidence })
    }

    /// A Score answer for a rubric of `levels` levels.
    ///
    /// # Errors
    ///
    /// [`Fallback::Malformed`] if missing, mistyped, outside `0..=levels-1`, or
    /// without a usable confidence.
    pub fn score(&self, id: &str, levels: usize) -> Result<ScoreAnswer, Fallback> {
        let a = self.typed(id, "score")?;
        let score = a.get("score").and_then(Value::as_f64).ok_or(Fallback::Malformed)?;
        let top = levels.saturating_sub(1) as f64;
        if !score.is_finite() || score < -1e-9 || score > top + 1e-9 {
            return Err(Fallback::Malformed);
        }
        let confidence = a.get("confidence").and_then(unit).ok_or(Fallback::Malformed)?;
        Ok(ScoreAnswer { score: score.clamp(0.0, top), confidence })
    }

    /// A Noul answer: the probability of yes.
    ///
    /// # Errors
    ///
    /// [`Fallback::Malformed`] if missing, mistyped or outside `0..=1`.
    pub fn noul(&self, id: &str) -> Result<f64, Fallback> {
        let a = self.typed(id, "noul")?;
        a.get("noul").and_then(unit).ok_or(Fallback::Malformed)
    }
}

/// Test helper builders for mock responses (used by every item's tests).
#[cfg(test)]
pub mod mock {
    use serde_json::{json, Value};

    /// A Score answer.
    pub fn score(s: f64, c: f64) -> Value {
        json!({"type": "score", "score": s, "confidence": c, "legend": {}, "probabilities": {}})
    }
    /// A Noul answer.
    pub fn noul(p: f64) -> Value {
        json!({"type": "noul", "noul": p})
    }
    /// A Choice answer.
    pub fn choice(k: &str, c: f64) -> Value {
        json!({"type": "choice", "choice": k, "confidence": c, "probabilities": {}})
    }
    /// Wrap answers into a full response body.
    pub fn body(answers: Vec<(String, Value)>) -> String {
        let m: serde_json::Map<String, Value> = answers.into_iter().collect();
        json!({"model": "jev-test", "answers": m, "usage": {"input_tokens": 1, "output_tokens": 1}}).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn question_wire_shape_matches_the_schema() {
        let mut crit = BTreeMap::new();
        crit.insert("a".to_string(), "first".to_string());
        crit.insert("none".to_string(), "neither".to_string());
        let mut qs = BTreeMap::new();
        qs.insert("c".to_string(), Question::Choice { instructions: "pick".into(), criteria: crit });
        qs.insert("s".to_string(), Question::Score { instructions: "rate".into(), criteria: vec!["low".into(), "high".into()] });
        qs.insert("n".to_string(), Question::Noul { instructions: "is it".into() });
        let r = Request { model: "jev-latest".into(), state: json!({"x": 1}), questions: qs };
        r.validate().unwrap();
        let v = serde_json::to_value(&r).unwrap();
        assert!(v["questions"]["c"]["criteria"].is_object(), "choice criteria is a map");
        assert!(v["questions"]["s"]["criteria"].is_array(), "score criteria is an ordered list");
        assert_eq!(v["questions"]["n"], json!({"type": "noul", "instructions": "is it"}));
        assert_eq!(v["questions"]["c"]["type"], "choice");
        assert_eq!(v["model"], "jev-latest");
    }

    #[test]
    fn validate_rejects_what_the_service_would() {
        let mut qs = BTreeMap::new();
        qs.insert("s".to_string(), Question::Score { instructions: "x".into(), criteria: (0..11).map(|i| i.to_string()).collect() });
        let r = Request { model: "m".into(), state: json!("s"), questions: qs };
        assert!(r.validate().is_err());
        let r = Request { model: "m".into(), state: json!("s"), questions: BTreeMap::new() };
        assert!(r.validate().is_err());
    }

    #[test]
    fn answers_parse_and_validate() {
        let b = mock::body(vec![
            ("c".into(), mock::choice("a", 0.7)),
            ("s".into(), mock::score(1.5, 0.6)),
            ("n".into(), mock::noul(0.9)),
        ]);
        let a = Answers::parse(&b).unwrap();
        assert_eq!(a.choice("c", &["a", "none"]).unwrap().choice, "a");
        assert_eq!(a.score("s", 3).unwrap().score, 1.5);
        assert_eq!(a.noul("n").unwrap(), 0.9);
    }

    #[test]
    fn malformed_variants_are_all_rejected() {
        assert_eq!(Answers::parse("not json").unwrap_err(), Fallback::Malformed);
        assert_eq!(Answers::parse("{}").unwrap_err(), Fallback::Malformed);
        assert_eq!(Answers::parse(r#"{"answers":{}}"#).unwrap_err(), Fallback::Malformed);
        let b = mock::body(vec![
            ("c".into(), mock::choice("zzz", 0.7)),
            ("s".into(), mock::score(5.0, 0.6)),
            ("n".into(), mock::noul(1.5)),
            ("t".into(), mock::noul(0.5)),
        ]);
        let a = Answers::parse(&b).unwrap();
        assert_eq!(a.choice("c", &["a"]).unwrap_err(), Fallback::Malformed, "outside offered set");
        assert_eq!(a.score("s", 3).unwrap_err(), Fallback::Malformed, "out of range");
        assert_eq!(a.noul("n").unwrap_err(), Fallback::Malformed, "probability > 1");
        assert_eq!(a.noul("missing").unwrap_err(), Fallback::Malformed, "missing id");
        assert_eq!(a.score("t", 3).unwrap_err(), Fallback::Malformed, "wrong type");
    }
}
