//! [`JevHub`]: one shared, cheap-to-clone entry point for hosts that drive
//! several Jev items from one process (the VisionClaw server).
//!
//! The hub owns a flag/endpoint lookup and a [`Transport`]. Each call resolves
//! the endpoint for the item being asked about ([`crate::config::resolve`]),
//! builds the request, sends it with the item's hard deadline and zero
//! retries, and turns every failure into a [`Fallback`]. A non-`flag-off`
//! reason is logged once (item, question-set version, reason; never request
//! content). Callers keep their pre-change result on `Err`.
//!
//! The acceptance thresholds live here too: [`JevHub::accept_suggestion`] for
//! annotate / suggest decisions and [`JevHub::accept_outcome`] for decisions
//! that change an outcome, which accepts nothing unless
//! `VISIONCLAW_JEV_AUTO_MIN_CONFIDENCE` is set (contract §3).

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;

use crate::client::{HttpTransport, Jev, Transport};
use crate::config::{self, Item};
use crate::wire::{Answers, Question, Request};
use crate::{log_fallback, Fallback};

type Lookup = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Shared Jev entry point. Cheap to clone.
#[derive(Clone)]
pub struct JevHub {
    lookup: Lookup,
    transport: Option<Arc<dyn Transport>>,
    http: Arc<std::sync::OnceLock<Arc<dyn Transport>>>,
}

impl std::fmt::Debug for JevHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JevHub").finish_non_exhaustive()
    }
}

impl JevHub {
    /// A hub reading the live process environment, over the real transport
    /// (built on first use so a missing HTTP stack can never stop a boot).
    pub fn from_env() -> Self {
        Self { lookup: Arc::new(|k| std::env::var(k).ok()), transport: None, http: Arc::default() }
    }

    /// A hub over a supplied lookup and transport (tests, and hosts that own
    /// their HTTP stack).
    pub fn with_transport(
        lookup: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
        transport: Arc<dyn Transport>,
    ) -> Self {
        Self { lookup: Arc::new(lookup), transport: Some(transport), http: Arc::default() }
    }

    /// A hub with everything off.
    pub fn disabled() -> Self {
        Self { lookup: Arc::new(|_| None), transport: None, http: Arc::default() }
    }

    /// Whether `item` is on (flag model in [`crate::config`]). Says nothing
    /// about whether an endpoint resolves.
    pub fn enabled(&self, item: Item) -> bool {
        config::enabled(item, &*self.lookup)
    }

    /// Whether an endpoint resolves for `item` (flag on and a usable
    /// destination). Carries no URL, model or key.
    pub fn available(&self, item: Item) -> bool {
        config::resolve(item, &*self.lookup).is_ok()
    }

    /// The acceptance threshold for annotate / suggest decisions.
    pub fn min_confidence(&self) -> f64 {
        config::min_confidence(&*self.lookup)
    }

    /// Accept an annotate / suggest decision at `confidence`.
    ///
    /// # Errors
    ///
    /// [`Fallback::LowConfidence`] below the threshold.
    pub fn accept_suggestion(&self, confidence: f64) -> Result<(), Fallback> {
        if confidence >= self.min_confidence() {
            Ok(())
        } else {
            Err(Fallback::LowConfidence)
        }
    }

    /// Accept an outcome-changing decision at `confidence`. With no
    /// `VISIONCLAW_JEV_AUTO_MIN_CONFIDENCE` nothing is accepted automatically.
    ///
    /// # Errors
    ///
    /// [`Fallback::LowConfidence`] below the threshold or with none set.
    pub fn accept_outcome(&self, confidence: f64) -> Result<(), Fallback> {
        match config::auto_min_confidence(&*self.lookup) {
            Some(t) if confidence >= t => Ok(()),
            _ => Err(Fallback::LowConfidence),
        }
    }

    fn bind(&self, item: Item) -> Result<Jev, Fallback> {
        // Flag first: with the item off nothing is constructed.
        if !self.enabled(item) {
            return Err(Fallback::FlagOff);
        }
        let transport: Arc<dyn Transport> = match &self.transport {
            Some(t) => t.clone(),
            None => {
                if let Some(t) = self.http.get() {
                    t.clone()
                } else {
                    let t: Arc<dyn Transport> = Arc::new(HttpTransport::new()?);
                    self.http.get_or_init(|| t.clone()).clone()
                }
            }
        };
        Jev::from_lookup(item, &*self.lookup, transport)
    }

    /// Ask `questions` about `state` for `item`. `question_set` is the stable
    /// version string of the question set, logged on fallback.
    ///
    /// # Errors
    ///
    /// Any [`Fallback`]; the reason has already been logged (no content).
    /// With the flag off nothing is built and nothing is sent.
    pub async fn ask(
        &self,
        item: Item,
        question_set: &str,
        state: Value,
        questions: Vec<(String, Question)>,
    ) -> Result<Answers, Fallback> {
        let r = self.ask_inner(item, state, questions).await;
        if let Err(reason) = &r {
            log_fallback(item, question_set, reason);
        }
        r
    }

    async fn ask_inner(
        &self,
        item: Item,
        state: Value,
        questions: Vec<(String, Question)>,
    ) -> Result<Answers, Fallback> {
        let jev = self.bind(item)?;
        let mut map = BTreeMap::new();
        for (id, q) in questions {
            // A repeated question ID would silently drop a question.
            if map.insert(id, q).is_some() {
                return Err(Fallback::Malformed);
            }
        }
        jev.ask(&Request { model: jev.model().to_string(), state, questions: map }).await
    }
}

/// The process-wide hub, built once from the environment. Off unless a flag
/// is set. Handlers and the voice path share it.
pub fn global() -> &'static JevHub {
    static HUB: std::sync::OnceLock<JevHub> = std::sync::OnceLock::new();
    HUB.get_or_init(JevHub::from_env)
}

/// Test support: a hub that is on for every item against a fake endpoint.
#[cfg(any(test, feature = "test-util"))]
pub mod testing {
    use super::*;
    use std::collections::HashMap;

    /// A hub with the master flag on, `http://jev.test` as the endpoint and a
    /// short deadline, plus optional extra variables, over `transport`.
    pub fn enabled_hub(extra: &[(&str, &str)], transport: Arc<dyn Transport>) -> JevHub {
        let mut m: HashMap<String, String> = HashMap::new();
        m.insert(config::MASTER_FLAG.into(), "1".into());
        m.insert("VISIONCLAW_JEV_ENDPOINT".into(), "http://jev.test".into());
        m.insert("VISIONCLAW_JEV_TIMEOUT_MS".into(), "500".into());
        for (k, v) in extra {
            m.insert((*k).into(), (*v).into());
        }
        JevHub::with_transport(move |k| m.get(k).cloned(), transport)
    }

    /// A hub built from exactly these variables (nothing added).
    pub fn hub_with(vars: &[(&str, &str)], transport: Arc<dyn Transport>) -> JevHub {
        let m: HashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        JevHub::with_transport(move |k| m.get(k).cloned(), transport)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use crate::client::testing::{MockTransport, Script};
    use crate::client::TransportError;
    use crate::wire::mock;
    use serde_json::json;

    fn q() -> Vec<(String, Question)> {
        vec![("q".into(), Question::Noul { instructions: "x".into() })]
    }

    fn noul_body(p: f64) -> String {
        mock::body(vec![("q".into(), mock::noul(p))])
    }

    #[tokio::test]
    async fn flag_off_builds_no_request() {
        let t = MockTransport::new(Script::Reply(200, "{}".into()));
        let hub = hub_with(&[("VISIONCLAW_JEV_ENDPOINT", "http://x")], t.clone());
        assert_eq!(hub.ask(Item::CommandIntent, "v1", json!({}), q()).await.unwrap_err(), Fallback::FlagOff);
        assert_eq!(t.calls(), 0, "nothing may leave the process with the flag off");
        assert_eq!(
            JevHub::disabled().ask(Item::CommandIntent, "v1", json!({}), q()).await.unwrap_err(),
            Fallback::FlagOff
        );
    }

    #[tokio::test]
    async fn no_endpoint_builds_no_request() {
        let t = MockTransport::new(Script::Reply(200, "{}".into()));
        let hub = hub_with(&[(config::MASTER_FLAG, "1")], t.clone());
        assert_eq!(hub.ask(Item::CommandIntent, "v1", json!({}), q()).await.unwrap_err(), Fallback::NoEndpoint);
        assert_eq!(t.calls(), 0);
    }

    #[tokio::test]
    async fn failure_modes_map_to_reasons_with_zero_retries() {
        let cases: Vec<(Script, Fallback)> = vec![
            (Script::Fail(TransportError::Timeout), Fallback::Timeout),
            (Script::Fail(TransportError::Other), Fallback::Transport),
            (Script::Reply(429, "{}".into()), Fallback::Http(429)),
            (Script::Reply(529, "{}".into()), Fallback::Http(529)),
            (Script::Reply(500, "{}".into()), Fallback::Http(500)),
            (Script::Reply(200, "garbage".into()), Fallback::Malformed),
            (Script::Hang, Fallback::Timeout),
        ];
        for (script, want) in cases {
            let t = MockTransport::new(script);
            let hub = enabled_hub(&[], t.clone());
            assert_eq!(hub.ask(Item::CommandIntent, "v1", json!({}), q()).await.unwrap_err(), want);
            assert_eq!(t.calls(), 1);
        }
    }

    #[tokio::test]
    async fn success_path_sends_the_wire_shape_and_no_authorization_for_the_proxy() {
        let t = MockTransport::new(Script::Reply(200, noul_body(0.9)));
        let hub = hub_with(
            &[
                (config::MASTER_FLAG, "1"),
                ("AGENTBOX_SYSTEM_ONE_PROXY", "1"),
                ("AGENTBOX_SYSTEM_ONE_ENDPOINT", "http://127.0.0.1:9/v1/systemone"),
                ("AGENTBOX_SYSTEM_ONE_MODEL", "jev-test"),
                ("TYPESAFE_API_KEY", "must-not-be-sent"),
            ],
            t.clone(),
        );
        let a = hub.ask(Item::CommandIntent, "v1", json!({"command": "x"}), q()).await.unwrap();
        assert!((a.noul("q").unwrap() - 0.9).abs() < 1e-9);
        assert_eq!(t.bearers.lock().unwrap()[0], None);
        let body = t.seen.lock().unwrap()[0].clone();
        assert_eq!(body["model"], "jev-test");
        assert_eq!(body["state"]["command"], "x");
        assert_eq!(body["questions"]["q"]["type"], "noul");
    }

    #[tokio::test]
    async fn choice_criteria_is_a_map_and_score_criteria_an_ordered_list() {
        let t = MockTransport::new(Script::Reply(200, "{}".into()));
        let hub = enabled_hub(&[], t.clone());
        let qs = vec![
            (
                "pick".to_string(),
                Question::Choice {
                    instructions: "Which?".into(),
                    criteria: [("a".to_string(), "is a".to_string()), ("none".into(), "neither".into())].into(),
                },
            ),
            (
                "rate".to_string(),
                Question::Score { instructions: "How good?".into(), criteria: vec!["bad".into(), "good".into()] },
            ),
        ];
        let _ = hub.ask(Item::PaletteSearch, "v1", json!({}), qs).await;
        let body = t.seen.lock().unwrap()[0].clone();
        assert!(body["questions"]["pick"]["criteria"].is_object());
        assert_eq!(body["questions"]["pick"]["criteria"]["a"], "is a");
        assert!(body["questions"]["rate"]["criteria"].is_array());
        assert_eq!(body["questions"]["rate"]["criteria"][1], "good");
    }

    #[tokio::test]
    async fn invalid_or_duplicate_questions_never_leave_the_process() {
        let t = MockTransport::new(Script::Reply(200, "{}".into()));
        let hub = enabled_hub(&[], t.clone());
        assert_eq!(hub.ask(Item::CommandIntent, "v1", json!({}), vec![]).await.unwrap_err(), Fallback::Malformed);
        let dup = vec![q().remove(0), q().remove(0)];
        assert_eq!(hub.ask(Item::CommandIntent, "v1", json!({}), dup).await.unwrap_err(), Fallback::Malformed);
        let too_many = vec![(
            "q".to_string(),
            Question::Score { instructions: "x".into(), criteria: (0..11).map(|i| i.to_string()).collect() },
        )];
        assert_eq!(hub.ask(Item::CommandIntent, "v1", json!({}), too_many).await.unwrap_err(), Fallback::Malformed);
        assert_eq!(t.calls(), 0);
    }

    #[tokio::test]
    async fn a_foreign_choice_key_is_malformed_not_an_answer() {
        let body = mock::body(vec![("q".into(), mock::choice("zzz", 0.9))]);
        let t = MockTransport::new(Script::Reply(200, body));
        let a = enabled_hub(&[], t)
            .ask(Item::CommandIntent, "v1", json!({}), q())
            .await
            .unwrap();
        assert_eq!(a.choice("q", &["a", "none"]).unwrap_err(), Fallback::Malformed);
        assert!(a.choice("q", &["zzz"]).is_ok());
    }

    #[test]
    fn availability_reports_resolution_without_leaking_it() {
        let t = MockTransport::new(Script::Reply(200, "{}".into()));
        assert!(enabled_hub(&[], t.clone()).available(Item::NlqPlan));
        assert!(!JevHub::disabled().available(Item::NlqPlan));
        let no_endpoint = hub_with(&[(config::MASTER_FLAG, "1")], t);
        assert!(no_endpoint.enabled(Item::NlqPlan));
        assert!(!no_endpoint.available(Item::NlqPlan));
        assert!(!format!("{:?}", enabled_hub(&[], MockTransport::new(Script::Reply(200, "{}".into())))).contains("jev.test"));
    }

    #[test]
    fn thresholds() {
        let t = MockTransport::new(Script::Reply(200, "{}".into()));
        let hub = enabled_hub(&[], t.clone());
        assert!(hub.accept_suggestion(0.5).is_ok());
        assert_eq!(hub.accept_suggestion(0.49), Err(Fallback::LowConfidence));
        assert_eq!(hub.accept_outcome(1.0), Err(Fallback::LowConfidence), "no threshold: accept nothing");
        let hub = enabled_hub(&[("VISIONCLAW_JEV_AUTO_MIN_CONFIDENCE", "0.9")], t);
        assert!(hub.accept_outcome(0.95).is_ok());
        assert_eq!(hub.accept_outcome(0.89), Err(Fallback::LowConfidence));
    }
}
