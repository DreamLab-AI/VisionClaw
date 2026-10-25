//! System One ("Jev") typed-decision client — the one shared adapter for the
//! VisionClaw **Jev option**.
//!
//! The Jev option is an opt-in step (`Choice` / `Score` / `Noul` over
//! `POST /v1/systemone`) that sits *beside* existing behaviour. With the flag
//! off, with no endpoint, or on any failure, every consumer takes its
//! pre-change code path and produces its pre-change output.
//!
//! ## Contract (see `docs/reference/configuration.md` § "Jev option")
//!
//! * **Off by default.** [`JevSettings::enabled`] is false unless
//!   `VISIONCLAW_JEV=1`. When off, no request is built and nothing leaves the
//!   process.
//! * **Endpoint resolution, first match wins** ([`resolve_endpoint`]):
//!   1. explicit `VISIONCLAW_JEV_ENDPOINT` (+ `VISIONCLAW_JEV_MODEL`,
//!      `VISIONCLAW_JEV_KEY_ENV` naming the env var that holds the key);
//!   2. `AGENTBOX_SYSTEM_ONE_ENDPOINT` + `AGENTBOX_SYSTEM_ONE_MODEL` when
//!      `AGENTBOX_SYSTEM_ONE_PROXY=1` (loopback proxy holds the vendor key, so
//!      **no** `Authorization` header is sent);
//!   3. `https://api.typesafe.ai/v1/systemone`, model `jev-latest`, bearer
//!      `$TYPESAFE_API_KEY` — only when that key is set;
//!   4. otherwise Jev is unavailable and the caller falls back.
//!
//!   A *local-only* consumer refuses steps 2 and 3 unless the endpoint is
//!   declared local (`AGENTBOX_SYSTEM_ONE_BACKEND_LOCAL=1` or
//!   `VISIONCLAW_JEV_LOCAL=1`).
//! * **Fail-open.** [`JevClient::ask`] never panics and never blocks past the
//!   hard deadline; every failure is a [`FallbackReason`] and one log line with
//!   the reason and **no request content**.
//! * **Authority stays in code.** Jev proposes; it never writes, signs or
//!   approves. Callers run their existing validation after any suggestion.
//!
//! The browser never holds a vendor key: client features reach Jev only via
//! the authenticated `/api/jev/*` routes (`handlers::jev_handler`), which use
//! this module.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Map, Value};

/// Master flag. Truthy (`1`/`true`/`yes`/`on`) enables the Jev option.
pub const ENV_JEV: &str = "VISIONCLAW_JEV";
/// Explicit endpoint URL (contract step 1).
pub const ENV_ENDPOINT: &str = "VISIONCLAW_JEV_ENDPOINT";
/// Model for the explicit endpoint (default `jev-latest`).
pub const ENV_MODEL: &str = "VISIONCLAW_JEV_MODEL";
/// *Name* of the env var holding the key for the explicit endpoint.
pub const ENV_KEY_ENV: &str = "VISIONCLAW_JEV_KEY_ENV";
/// Declares the explicit endpoint local (satisfies local-only consumers).
pub const ENV_LOCAL: &str = "VISIONCLAW_JEV_LOCAL";
/// Acceptance threshold for *annotate / suggest* decisions (default 0.5).
pub const ENV_MIN_CONFIDENCE: &str = "VISIONCLAW_JEV_MIN_CONFIDENCE";
/// Acceptance threshold for decisions that *change an outcome*. Unset ⇒
/// accept nothing automatically (contract §3).
pub const ENV_AUTO_MIN_CONFIDENCE: &str = "VISIONCLAW_JEV_AUTO_MIN_CONFIDENCE";
/// Hard deadline in milliseconds (default 4000, capped at 4000).
pub const ENV_TIMEOUT_MS: &str = "VISIONCLAW_JEV_TIMEOUT_MS";

/// Agentbox loopback proxy (ADR-2139).
pub const ENV_PROXY: &str = "AGENTBOX_SYSTEM_ONE_PROXY";
/// Agentbox loopback proxy endpoint.
pub const ENV_PROXY_ENDPOINT: &str = "AGENTBOX_SYSTEM_ONE_ENDPOINT";
/// Agentbox loopback proxy model.
pub const ENV_PROXY_MODEL: &str = "AGENTBOX_SYSTEM_ONE_MODEL";
/// Declares the proxy's backend local.
pub const ENV_PROXY_BACKEND_LOCAL: &str = "AGENTBOX_SYSTEM_ONE_BACKEND_LOCAL";
/// Vendor key env var for contract step 3.
pub const ENV_TYPESAFE_KEY: &str = "TYPESAFE_API_KEY";

/// Native endpoint (contract step 3).
pub const DEFAULT_ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
/// Default model alias.
pub const DEFAULT_MODEL: &str = "jev-latest";
/// Interactive deadline ceiling (contract §3: ≤ 4 s, 0 retries).
pub const MAX_TIMEOUT_MS: u64 = 4000;
/// Default acceptance threshold for annotate-only decisions.
pub const DEFAULT_MIN_CONFIDENCE: f64 = 0.5;

/// Every VisionClaw Jev integration point. Each has a stable slug used in the
/// status route and log lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum JevItem {
    /// VC-06-S1-01 — settings text box → reviewed patch.
    SettingsPatch,
    /// VC-06-S1-02 — saved preference vs one-off action.
    ActionClass,
    /// VC-34-S1-01 — CommandInput semantic intent.
    CommandIntent,
    /// VC-34-S1-02 — command palette semantic search.
    PaletteSearch,
    /// VC-25-S1-01 — edge meaning from source evidence.
    EdgeMeaning,
    /// VC-25-S1-02 — bounded NL graph query planning.
    NlqPlan,
    /// VC-35-S1-01 — voice intent disambiguation.
    VoiceIntent,
}

impl JevItem {
    /// All items, in a stable order.
    pub const ALL: [JevItem; 7] = [
        JevItem::SettingsPatch,
        JevItem::ActionClass,
        JevItem::CommandIntent,
        JevItem::PaletteSearch,
        JevItem::EdgeMeaning,
        JevItem::NlqPlan,
        JevItem::VoiceIntent,
    ];

    /// Stable kebab-case slug.
    pub fn slug(self) -> &'static str {
        match self {
            JevItem::SettingsPatch => "settings-patch",
            JevItem::ActionClass => "action-class",
            JevItem::CommandIntent => "command-intent",
            JevItem::PaletteSearch => "palette-search",
            JevItem::EdgeMeaning => "edge-meaning",
            JevItem::NlqPlan => "nlq-plan",
            JevItem::VoiceIntent => "voice-intent",
        }
    }

    /// Per-item opt-out variable (`VISIONCLAW_JEV_<ITEM>=0` disables one item
    /// while the master flag is on).
    pub fn env_name(self) -> &'static str {
        match self {
            JevItem::SettingsPatch => "VISIONCLAW_JEV_SETTINGS_PATCH",
            JevItem::ActionClass => "VISIONCLAW_JEV_ACTION_CLASS",
            JevItem::CommandIntent => "VISIONCLAW_JEV_COMMAND_INTENT",
            JevItem::PaletteSearch => "VISIONCLAW_JEV_PALETTE_SEARCH",
            JevItem::EdgeMeaning => "VISIONCLAW_JEV_EDGE_MEANING",
            JevItem::NlqPlan => "VISIONCLAW_JEV_NLQ_PLAN",
            JevItem::VoiceIntent => "VISIONCLAW_JEV_VOICE_INTENT",
        }
    }
}

/// Why a Jev call produced no usable decision. Every variant means "take the
/// pre-change path".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FallbackReason {
    /// Master flag (or this item's flag) is off.
    FlagOff,
    /// No endpoint resolved (or a local-only consumer refused a non-local one).
    NoEndpoint,
    /// Transport failure or deadline exceeded.
    Timeout,
    /// Non-2xx status (carries the code).
    Http(u16),
    /// Body failed to parse, answers missing, or a Choice key was outside the
    /// offered set.
    Malformed,
    /// Confidence/probability below the acceptance threshold (or no threshold
    /// configured for an outcome-changing decision).
    LowConfidence,
}

impl FallbackReason {
    /// Log token: `flag-off`, `no-endpoint`, `timeout`, `http-<code>`,
    /// `malformed`, `low-confidence`.
    pub fn as_token(&self) -> String {
        match self {
            FallbackReason::FlagOff => "flag-off".into(),
            FallbackReason::NoEndpoint => "no-endpoint".into(),
            FallbackReason::Timeout => "timeout".into(),
            FallbackReason::Http(c) => format!("http-{c}"),
            FallbackReason::Malformed => "malformed".into(),
            FallbackReason::LowConfidence => "low-confidence".into(),
        }
    }

    /// Emit the single permitted log line: item, question-set version, reason.
    /// Never includes request content. `flag-off` is silent (the common case).
    pub fn log(&self, item: JevItem, question_set: &str) {
        if *self != FallbackReason::FlagOff {
            log::info!(
                "jev fallback item={} question_set={} reason={}",
                item.slug(),
                question_set,
                self.as_token()
            );
        }
    }
}

/// A resolved destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// Full URL of the System One route.
    pub url: String,
    /// Model name sent in the request body.
    pub model: String,
    /// Bearer token, `None` for the keyless loopback proxy.
    pub bearer: Option<String>,
    /// Whether the endpoint is declared local.
    pub local: bool,
}

fn truthy(v: Option<String>) -> bool {
    matches!(
        v.as_deref().map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

fn falsy_explicit(v: Option<String>) -> bool {
    matches!(
        v.as_deref().map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        Some("0" | "false" | "no" | "off")
    )
}

fn nonempty(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Resolve the endpoint per the contract. `env` is injected so tests do not
/// touch process state. `local_only` consumers refuse non-local endpoints.
pub fn resolve_endpoint(
    env: &dyn Fn(&str) -> Option<String>,
    local_only: bool,
) -> Result<Endpoint, FallbackReason> {
    // 1. Explicit repo config.
    if let Some(url) = nonempty(env(ENV_ENDPOINT)) {
        let model = nonempty(env(ENV_MODEL)).unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let bearer = match nonempty(env(ENV_KEY_ENV)) {
            Some(key_var) => match nonempty(env(&key_var)) {
                Some(k) => Some(k),
                // A named key variable that is unset means the explicit
                // endpoint cannot authenticate: unavailable, not "send none".
                None => return Err(FallbackReason::NoEndpoint),
            },
            None => None,
        };
        let local = truthy(env(ENV_LOCAL));
        if local_only && !local {
            return Err(FallbackReason::NoEndpoint);
        }
        return Ok(Endpoint { url, model, bearer, local });
    }
    // 2. Agentbox loopback proxy — holds the vendor key; no Authorization.
    if truthy(env(ENV_PROXY)) {
        if let Some(url) = nonempty(env(ENV_PROXY_ENDPOINT)) {
            let model = nonempty(env(ENV_PROXY_MODEL)).unwrap_or_else(|| DEFAULT_MODEL.to_string());
            let local = truthy(env(ENV_PROXY_BACKEND_LOCAL));
            if local_only && !local {
                return Err(FallbackReason::NoEndpoint);
            }
            return Ok(Endpoint { url, model, bearer: None, local });
        }
    }
    // 3. Vendor endpoint, only with a key. Never local.
    if let Some(key) = nonempty(env(ENV_TYPESAFE_KEY)) {
        if local_only {
            return Err(FallbackReason::NoEndpoint);
        }
        return Ok(Endpoint {
            url: DEFAULT_ENDPOINT.to_string(),
            model: DEFAULT_MODEL.to_string(),
            bearer: Some(key),
            local: false,
        });
    }
    // 4. Unavailable.
    Err(FallbackReason::NoEndpoint)
}

/// Process-level Jev settings: flags, thresholds and deadline. Endpoint
/// resolution is deferred to call time so a misconfigured endpoint can never
/// stop the server booting.
#[derive(Debug, Clone)]
pub struct JevSettings {
    master: bool,
    item_off: Vec<JevItem>,
    /// Acceptance threshold for annotate/suggest decisions.
    pub min_confidence: f64,
    /// Acceptance threshold for outcome-changing decisions; `None` ⇒ accept
    /// nothing automatically.
    pub auto_min_confidence: Option<f64>,
    /// Hard deadline for one call.
    pub timeout: Duration,
    env: Arc<HashMap<String, String>>,
}

impl JevSettings {
    /// Everything off. The default for tests and for the unset-environment case.
    pub fn disabled() -> Self {
        Self::from_lookup(&|_| None)
    }

    /// Read settings from the process environment.
    pub fn from_env() -> Self {
        Self::from_lookup(&|k| std::env::var(k).ok())
    }

    /// Read settings from an arbitrary lookup (tests). The lookup is also
    /// snapshotted so endpoint resolution at call time sees the same values.
    pub fn from_lookup(lookup: &dyn Fn(&str) -> Option<String>) -> Self {
        let mut snapshot: HashMap<String, String> = HashMap::new();
        let mut names: Vec<&str> = vec![
            ENV_JEV,
            ENV_ENDPOINT,
            ENV_MODEL,
            ENV_KEY_ENV,
            ENV_LOCAL,
            ENV_MIN_CONFIDENCE,
            ENV_AUTO_MIN_CONFIDENCE,
            ENV_TIMEOUT_MS,
            ENV_PROXY,
            ENV_PROXY_ENDPOINT,
            ENV_PROXY_MODEL,
            ENV_PROXY_BACKEND_LOCAL,
            ENV_TYPESAFE_KEY,
        ];
        names.extend(JevItem::ALL.iter().map(|i| i.env_name()));
        for n in names {
            if let Some(v) = lookup(n) {
                snapshot.insert(n.to_string(), v);
            }
        }
        // The explicit key variable is itself named by config.
        if let Some(key_var) = nonempty(lookup(ENV_KEY_ENV)) {
            if let Some(v) = lookup(&key_var) {
                snapshot.insert(key_var, v);
            }
        }
        let parse = |name: &str| -> Option<f64> {
            nonempty(lookup(name))
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|v| v.is_finite())
                .map(|v| v.clamp(0.0, 1.0))
        };
        let timeout_ms = nonempty(lookup(ENV_TIMEOUT_MS))
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(MAX_TIMEOUT_MS)
            .min(MAX_TIMEOUT_MS);
        Self {
            master: truthy(lookup(ENV_JEV)),
            item_off: JevItem::ALL
                .iter()
                .copied()
                .filter(|i| falsy_explicit(lookup(i.env_name())))
                .collect(),
            min_confidence: parse(ENV_MIN_CONFIDENCE).unwrap_or(DEFAULT_MIN_CONFIDENCE),
            auto_min_confidence: parse(ENV_AUTO_MIN_CONFIDENCE),
            timeout: Duration::from_millis(timeout_ms),
            env: Arc::new(snapshot),
        }
    }

    /// Whether the Jev option is on for `item`. False unless `VISIONCLAW_JEV`
    /// is truthy and the item has not been opted out.
    pub fn enabled(&self, item: JevItem) -> bool {
        self.master && !self.item_off.contains(&item)
    }

    /// Resolve the endpoint from the snapshotted environment.
    pub fn endpoint(&self, local_only: bool) -> Result<Endpoint, FallbackReason> {
        let env = self.env.clone();
        resolve_endpoint(&move |k| env.get(k).cloned(), local_only)
    }
}

/// One typed question.
#[derive(Debug, Clone, PartialEq)]
pub enum Question {
    /// Pick one key. `criteria` is a **map** key → when it applies.
    Choice {
        /// Full meaning of the question (IDs are not shown to the model).
        instructions: String,
        /// Ordered (key, description) pairs, serialised as a JSON map.
        criteria: Vec<(String, String)>,
    },
    /// Rate on an **ordered list** of level descriptions (position = score).
    Score {
        /// Full meaning of the question.
        instructions: String,
        /// Ordered level descriptions (1..=10).
        criteria: Vec<String>,
    },
    /// Probability of yes.
    Noul {
        /// Full meaning of the question.
        instructions: String,
    },
}

impl Question {
    fn to_json(&self) -> Value {
        match self {
            Question::Choice { instructions, criteria } => {
                let mut m = Map::new();
                for (k, v) in criteria {
                    m.insert(k.clone(), Value::String(v.clone()));
                }
                json!({"type": "choice", "instructions": instructions, "criteria": Value::Object(m)})
            }
            Question::Score { instructions, criteria } => {
                json!({"type": "score", "instructions": instructions, "criteria": criteria})
            }
            Question::Noul { instructions } => json!({"type": "noul", "instructions": instructions}),
        }
    }

    fn validate(&self) -> bool {
        match self {
            Question::Choice { instructions, criteria } => {
                !instructions.is_empty()
                    && !criteria.is_empty()
                    && criteria.len() <= 255
                    && criteria.iter().all(|(k, _)| !k.is_empty())
                    && {
                        let mut keys: Vec<&String> = criteria.iter().map(|(k, _)| k).collect();
                        keys.sort();
                        keys.dedup();
                        keys.len() == criteria.len()
                    }
            }
            Question::Score { instructions, criteria } => {
                !instructions.is_empty() && !criteria.is_empty() && criteria.len() <= 10
            }
            Question::Noul { instructions } => !instructions.is_empty(),
        }
    }
}

/// Build the `POST /v1/systemone` body. Returns `None` when a question is
/// structurally invalid (caller falls back as `malformed` rather than sending).
pub fn build_request(
    model: &str,
    state: Value,
    questions: &[(String, Question)],
) -> Option<Value> {
    if questions.is_empty() || questions.iter().any(|(id, q)| id.is_empty() || !q.validate()) {
        return None;
    }
    let mut qs = Map::new();
    for (id, q) in questions {
        qs.insert(id.clone(), q.to_json());
    }
    Some(json!({"model": model, "state": state, "questions": Value::Object(qs)}))
}

/// A parsed Choice answer, already checked against the offered key set.
#[derive(Debug, Clone, PartialEq)]
pub struct ChoiceAnswer {
    /// Selected key (guaranteed ∈ offered set).
    pub choice: String,
    /// Documented confidence `(p − 1/n) / (1 − 1/n)` in `[0,1]`.
    pub confidence: f64,
    /// Full probability map when supplied.
    pub probabilities: BTreeMap<String, f64>,
}

/// A parsed Score answer.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreAnswer {
    /// Expected score in `[0, levels-1]`.
    pub score: f64,
    /// Confidence in `[0,1]`.
    pub confidence: f64,
}

/// The `answers` map of a response.
#[derive(Debug, Clone)]
pub struct Answers {
    raw: Map<String, Value>,
}

impl Answers {
    /// Parse a response body. Anything other than an object with a non-empty
    /// `answers` object is `malformed`.
    pub fn parse(body: &str) -> Result<Self, FallbackReason> {
        let v: Value = serde_json::from_str(body).map_err(|_| FallbackReason::Malformed)?;
        let answers = v
            .get("answers")
            .and_then(Value::as_object)
            .filter(|m| !m.is_empty())
            .ok_or(FallbackReason::Malformed)?;
        Ok(Self { raw: answers.clone() })
    }

    fn get(&self, id: &str, ty: &str) -> Result<&Value, FallbackReason> {
        let a = self.raw.get(id).ok_or(FallbackReason::Malformed)?;
        if a.get("type").and_then(Value::as_str) != Some(ty) {
            return Err(FallbackReason::Malformed);
        }
        Ok(a)
    }

    /// Read a Choice answer; the key must be one of `offered`.
    pub fn choice(&self, id: &str, offered: &[&str]) -> Result<ChoiceAnswer, FallbackReason> {
        let a = self.get(id, "choice")?;
        let choice = a.get("choice").and_then(Value::as_str).ok_or(FallbackReason::Malformed)?;
        if !offered.contains(&choice) {
            return Err(FallbackReason::Malformed);
        }
        let confidence = unit(a.get("confidence"))?;
        let mut probabilities = BTreeMap::new();
        if let Some(p) = a.get("probabilities").and_then(Value::as_object) {
            for (k, v) in p {
                if let Some(f) = v.as_f64() {
                    probabilities.insert(k.clone(), f);
                }
            }
        }
        Ok(ChoiceAnswer { choice: choice.to_string(), confidence, probabilities })
    }

    /// Read a Noul probability-of-yes in `[0,1]`.
    pub fn noul(&self, id: &str) -> Result<f64, FallbackReason> {
        unit(self.get(id, "noul")?.get("noul"))
    }

    /// Read a Score answer for a rubric of `levels` levels.
    pub fn score(&self, id: &str, levels: usize) -> Result<ScoreAnswer, FallbackReason> {
        let a = self.get(id, "score")?;
        let score = a.get("score").and_then(Value::as_f64).ok_or(FallbackReason::Malformed)?;
        if !score.is_finite() || score < 0.0 || score > (levels.max(1) - 1) as f64 + 1e-9 {
            return Err(FallbackReason::Malformed);
        }
        Ok(ScoreAnswer { score, confidence: unit(a.get("confidence"))? })
    }
}

fn unit(v: Option<&Value>) -> Result<f64, FallbackReason> {
    let f = v.and_then(Value::as_f64).ok_or(FallbackReason::Malformed)?;
    if f.is_finite() && (0.0..=1.0).contains(&f) {
        Ok(f)
    } else {
        Err(FallbackReason::Malformed)
    }
}

/// Transport failure: the caller maps both to a fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// Deadline exceeded.
    Timeout,
    /// Any other transport failure (DNS, connect, TLS, body read).
    Failed,
}

/// The seam tests mock. Returns `(status, body)`.
#[async_trait]
pub trait JevTransport: Send + Sync {
    /// POST `body` as JSON to `endpoint`, honouring `timeout`.
    async fn post(
        &self,
        endpoint: &Endpoint,
        body: &Value,
        timeout: Duration,
    ) -> Result<(u16, String), TransportError>;
}

/// `reqwest`-backed transport (the HTTP stack the repo already depends on).
#[derive(Debug, Clone, Default)]
pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    /// Build a transport with a fresh client.
    pub fn new() -> Self {
        Self { client: reqwest::Client::new() }
    }
}

#[async_trait]
impl JevTransport for ReqwestTransport {
    async fn post(
        &self,
        endpoint: &Endpoint,
        body: &Value,
        timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        let mut req = self
            .client
            .post(&endpoint.url)
            .timeout(timeout)
            .header("Content-Type", "application/json")
            .json(body);
        if let Some(b) = &endpoint.bearer {
            req = req.bearer_auth(b);
        }
        let fut = async {
            let resp = req.send().await.map_err(|e| {
                if e.is_timeout() {
                    TransportError::Timeout
                } else {
                    TransportError::Failed
                }
            })?;
            let status = resp.status().as_u16();
            let text = resp.text().await.map_err(|e| {
                if e.is_timeout() {
                    TransportError::Timeout
                } else {
                    TransportError::Failed
                }
            })?;
            Ok((status, text))
        };
        // Hard deadline independent of reqwest's own timer.
        match tokio::time::timeout(timeout, fut).await {
            Ok(r) => r,
            Err(_) => Err(TransportError::Timeout),
        }
    }
}

/// The shared Jev client. Cheap to clone.
#[derive(Clone)]
pub struct JevClient {
    settings: Arc<JevSettings>,
    transport: Arc<dyn JevTransport>,
}

impl std::fmt::Debug for JevClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JevClient").field("settings", &self.settings).finish()
    }
}

impl JevClient {
    /// Production client over `reqwest`.
    pub fn new(settings: JevSettings) -> Self {
        Self::with_transport(settings, Arc::new(ReqwestTransport::new()))
    }

    /// Client over a supplied transport (tests mock here).
    pub fn with_transport(settings: JevSettings, transport: Arc<dyn JevTransport>) -> Self {
        Self { settings: Arc::new(settings), transport }
    }

    /// A client that is off. Used when nothing is wired.
    pub fn disabled() -> Self {
        Self::new(JevSettings::disabled())
    }

    /// The settings this client was built with.
    pub fn settings(&self) -> &JevSettings {
        &self.settings
    }

    /// Whether `item` is on.
    pub fn enabled(&self, item: JevItem) -> bool {
        self.settings.enabled(item)
    }

    /// Ask the questions. `Err` ⇒ the caller takes its pre-change path; the
    /// reason has already been logged (no content). `question_set` is the
    /// stable version string of the question set, logged on fallback.
    pub async fn ask(
        &self,
        item: JevItem,
        question_set: &str,
        local_only: bool,
        state: Value,
        questions: &[(String, Question)],
    ) -> Result<Answers, FallbackReason> {
        let r = self.ask_inner(item, local_only, state, questions).await;
        if let Err(reason) = &r {
            reason.log(item, question_set);
        }
        r
    }

    async fn ask_inner(
        &self,
        item: JevItem,
        local_only: bool,
        state: Value,
        questions: &[(String, Question)],
    ) -> Result<Answers, FallbackReason> {
        if !self.settings.enabled(item) {
            return Err(FallbackReason::FlagOff);
        }
        let endpoint = self.settings.endpoint(local_only)?;
        let body = build_request(&endpoint.model, state, questions).ok_or(FallbackReason::Malformed)?;
        let (status, text) = self
            .transport
            .post(&endpoint, &body, self.settings.timeout)
            .await
            .map_err(|_| FallbackReason::Timeout)?;
        if !(200..300).contains(&status) {
            return Err(FallbackReason::Http(status));
        }
        Answers::parse(&text)
    }

    /// Apply the annotate/suggest threshold.
    pub fn accept_suggestion(&self, confidence: f64) -> Result<(), FallbackReason> {
        if confidence >= self.settings.min_confidence {
            Ok(())
        } else {
            Err(FallbackReason::LowConfidence)
        }
    }

    /// Apply the outcome-changing threshold. With none configured, nothing is
    /// accepted automatically (contract §3).
    pub fn accept_outcome(&self, confidence: f64) -> Result<(), FallbackReason> {
        match self.settings.auto_min_confidence {
            Some(t) if confidence >= t => Ok(()),
            _ => Err(FallbackReason::LowConfidence),
        }
    }
}

/// The process-wide client, built once from the environment. Off unless
/// `VISIONCLAW_JEV=1`. Handlers and the voice path share it.
pub fn global() -> &'static JevClient {
    static CLIENT: once_cell::sync::OnceCell<JevClient> = once_cell::sync::OnceCell::new();
    CLIENT.get_or_init(|| JevClient::new(JevSettings::from_env()))
}

/// Test support: a scripted transport. Public so integration tests in other
/// modules can use it; carries no production behaviour.
pub mod testing {
    use super::*;
    use std::sync::Mutex;

    /// What the mocked transport does on the next call.
    #[derive(Debug, Clone)]
    pub enum Script {
        /// Respond with status + body.
        Reply(u16, String),
        /// Fail with a transport error.
        Fail(TransportError),
    }

    /// Records requests and replays a scripted outcome.
    #[derive(Debug)]
    pub struct MockTransport {
        script: Script,
        /// Requests seen: `(endpoint, body)`.
        pub seen: Mutex<Vec<(Endpoint, Value)>>,
    }

    impl MockTransport {
        /// Build a transport that always plays `script`.
        pub fn new(script: Script) -> Arc<Self> {
            Arc::new(Self { script, seen: Mutex::new(Vec::new()) })
        }
        /// Number of requests that reached the transport.
        pub fn calls(&self) -> usize {
            self.seen.lock().unwrap().len()
        }
        /// The last request body.
        pub fn last_body(&self) -> Option<Value> {
            self.seen.lock().unwrap().last().map(|(_, b)| b.clone())
        }
    }

    #[async_trait]
    impl JevTransport for MockTransport {
        async fn post(
            &self,
            endpoint: &Endpoint,
            body: &Value,
            _timeout: Duration,
        ) -> Result<(u16, String), TransportError> {
            self.seen.lock().unwrap().push((endpoint.clone(), body.clone()));
            match &self.script {
                Script::Reply(s, b) => Ok((*s, b.clone())),
                Script::Fail(e) => Err(e.clone()),
            }
        }
    }

    /// Settings with the master flag on and a loopback-proxy endpoint, plus
    /// optional extra variables.
    pub fn enabled_settings(extra: &[(&str, &str)]) -> JevSettings {
        let mut m: HashMap<String, String> = HashMap::new();
        m.insert(ENV_JEV.into(), "1".into());
        m.insert(ENV_PROXY.into(), "1".into());
        m.insert(ENV_PROXY_ENDPOINT.into(), "http://127.0.0.1:9/v1/systemone".into());
        m.insert(ENV_PROXY_MODEL.into(), "jev-test".into());
        for (k, v) in extra {
            m.insert((*k).into(), (*v).into());
        }
        JevSettings::from_lookup(&move |k| m.get(k).cloned())
    }

    /// A one-answer choice response body.
    pub fn choice_body(id: &str, choice: &str, confidence: f64) -> String {
        json!({"model":"jev-test","answers":{id:{"type":"choice","choice":choice,"confidence":confidence,"probabilities":{choice:1.0}}},"usage":{"input_tokens":1,"output_tokens":1}}).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    fn lookup(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
    }

    #[test]
    fn off_by_default() {
        let s = JevSettings::from_lookup(&lookup(&[]));
        for i in JevItem::ALL {
            assert!(!s.enabled(i));
        }
    }

    #[test]
    fn master_on_and_item_opt_out() {
        let s = JevSettings::from_lookup(&lookup(&[(ENV_JEV, "1"), ("VISIONCLAW_JEV_NLQ_PLAN", "0")]));
        assert!(s.enabled(JevItem::SettingsPatch));
        assert!(!s.enabled(JevItem::NlqPlan));
    }

    #[test]
    fn endpoint_order_explicit_wins_then_proxy_then_vendor() {
        let all: &'static [(&str, &str)] = &[
            (ENV_ENDPOINT, "http://explicit/x"),
            (ENV_PROXY, "1"),
            (ENV_PROXY_ENDPOINT, "http://proxy/x"),
            (ENV_TYPESAFE_KEY, "k"),
        ];
        let e = resolve_endpoint(&lookup(all), false).unwrap();
        assert_eq!(e.url, "http://explicit/x");
        assert_eq!(e.bearer, None);

        let e = resolve_endpoint(&lookup(&all[1..]), false).unwrap();
        assert_eq!(e.url, "http://proxy/x");
        assert_eq!(e.bearer, None, "proxy must never carry an Authorization header");

        let e = resolve_endpoint(&lookup(&all[3..]), false).unwrap();
        assert_eq!(e.url, DEFAULT_ENDPOINT);
        assert_eq!(e.model, DEFAULT_MODEL);
        assert_eq!(e.bearer.as_deref(), Some("k"));

        assert_eq!(resolve_endpoint(&lookup(&[]), false), Err(FallbackReason::NoEndpoint));
    }

    #[test]
    fn proxy_needs_flag_and_endpoint() {
        let only_endpoint: &'static [(&str, &str)] = &[(ENV_PROXY_ENDPOINT, "http://proxy/x")];
        assert_eq!(resolve_endpoint(&lookup(only_endpoint), false), Err(FallbackReason::NoEndpoint));
    }

    #[test]
    fn explicit_key_env_must_resolve() {
        let missing: &'static [(&str, &str)] = &[(ENV_ENDPOINT, "http://e"), (ENV_KEY_ENV, "MY_KEY")];
        assert_eq!(resolve_endpoint(&lookup(missing), false), Err(FallbackReason::NoEndpoint));
        let present: &'static [(&str, &str)] =
            &[(ENV_ENDPOINT, "http://e"), (ENV_KEY_ENV, "MY_KEY"), ("MY_KEY", "s3")];
        assert_eq!(resolve_endpoint(&lookup(present), false).unwrap().bearer.as_deref(), Some("s3"));
    }

    #[test]
    fn local_only_refuses_non_local() {
        let proxy: &'static [(&str, &str)] = &[(ENV_PROXY, "1"), (ENV_PROXY_ENDPOINT, "http://p")];
        assert_eq!(resolve_endpoint(&lookup(proxy), true), Err(FallbackReason::NoEndpoint));
        let local: &'static [(&str, &str)] =
            &[(ENV_PROXY, "1"), (ENV_PROXY_ENDPOINT, "http://p"), (ENV_PROXY_BACKEND_LOCAL, "1")];
        assert!(resolve_endpoint(&lookup(local), true).is_ok());
        let vendor: &'static [(&str, &str)] = &[(ENV_TYPESAFE_KEY, "k")];
        assert_eq!(resolve_endpoint(&lookup(vendor), true), Err(FallbackReason::NoEndpoint));
        let explicit_local: &'static [(&str, &str)] = &[(ENV_ENDPOINT, "http://e"), (ENV_LOCAL, "1")];
        assert!(resolve_endpoint(&lookup(explicit_local), true).is_ok());
    }

    #[test]
    fn timeout_is_capped() {
        let s = JevSettings::from_lookup(&lookup(&[(ENV_TIMEOUT_MS, "60000")]));
        assert_eq!(s.timeout, Duration::from_millis(4000));
    }

    #[test]
    fn request_wire_shape() {
        let qs = vec![
            (
                "pick".to_string(),
                Question::Choice {
                    instructions: "Which?".into(),
                    criteria: vec![("a".into(), "is a".into()), ("none".into(), "neither".into())],
                },
            ),
            (
                "rate".to_string(),
                Question::Score { instructions: "How good?".into(), criteria: vec!["bad".into(), "good".into()] },
            ),
            ("yes".to_string(), Question::Noul { instructions: "Is it?".into() }),
        ];
        let body = build_request("jev-latest", json!({"x": 1}), &qs).unwrap();
        assert_eq!(body["model"], "jev-latest");
        assert_eq!(body["state"]["x"], 1);
        // Choice criteria is a MAP, Score criteria an ORDERED LIST.
        assert!(body["questions"]["pick"]["criteria"].is_object());
        assert_eq!(body["questions"]["pick"]["criteria"]["a"], "is a");
        assert!(body["questions"]["rate"]["criteria"].is_array());
        assert_eq!(body["questions"]["rate"]["criteria"][1], "good");
        assert_eq!(body["questions"]["yes"]["type"], "noul");
        assert!(body["questions"]["yes"].get("criteria").is_none());
    }

    #[test]
    fn invalid_questions_do_not_build() {
        assert!(build_request("m", json!({}), &[]).is_none());
        let dup = vec![(
            "q".to_string(),
            Question::Choice {
                instructions: "x".into(),
                criteria: vec![("a".into(), "1".into()), ("a".into(), "2".into())],
            },
        )];
        assert!(build_request("m", json!({}), &dup).is_none());
        let too_many = vec![(
            "q".to_string(),
            Question::Score { instructions: "x".into(), criteria: (0..11).map(|i| i.to_string()).collect() },
        )];
        assert!(build_request("m", json!({}), &too_many).is_none());
    }

    #[test]
    fn answer_parsing_rejects_foreign_keys_and_bad_types() {
        let body = choice_body("q", "zzz", 0.9);
        let a = Answers::parse(&body).unwrap();
        assert_eq!(a.choice("q", &["a", "none"]), Err(FallbackReason::Malformed));
        assert_eq!(a.noul("q"), Err(FallbackReason::Malformed));
        assert_eq!(a.choice("missing", &["zzz"]), Err(FallbackReason::Malformed));
        assert!(a.choice("q", &["zzz"]).is_ok());
        assert!(Answers::parse("not json").is_err());
        assert!(Answers::parse("{\"answers\":{}}").is_err());
    }

    fn q() -> Vec<(String, Question)> {
        vec![("q".into(), Question::Noul { instructions: "x".into() })]
    }

    #[tokio::test]
    async fn flag_off_builds_no_request() {
        let t = MockTransport::new(Script::Reply(200, "{}".into()));
        let c = JevClient::with_transport(JevSettings::disabled(), t.clone());
        let r = c.ask(JevItem::CommandIntent, "v1", false, json!({}), &q()).await;
        assert_eq!(r.unwrap_err(), FallbackReason::FlagOff);
        assert_eq!(t.calls(), 0, "nothing may leave the process with the flag off");
    }

    #[tokio::test]
    async fn no_endpoint_builds_no_request() {
        let t = MockTransport::new(Script::Reply(200, "{}".into()));
        let s = JevSettings::from_lookup(&lookup(&[(ENV_JEV, "1")]));
        let c = JevClient::with_transport(s, t.clone());
        let r = c.ask(JevItem::CommandIntent, "v1", false, json!({}), &q()).await;
        assert_eq!(r.unwrap_err(), FallbackReason::NoEndpoint);
        assert_eq!(t.calls(), 0);
    }

    #[tokio::test]
    async fn failure_modes_map_to_reasons() {
        let cases: Vec<(Script, FallbackReason)> = vec![
            (Script::Fail(TransportError::Timeout), FallbackReason::Timeout),
            (Script::Fail(TransportError::Failed), FallbackReason::Timeout),
            (Script::Reply(429, "{}".into()), FallbackReason::Http(429)),
            (Script::Reply(529, "{}".into()), FallbackReason::Http(529)),
            (Script::Reply(500, "{}".into()), FallbackReason::Http(500)),
            (Script::Reply(200, "garbage".into()), FallbackReason::Malformed),
        ];
        for (script, want) in cases {
            let t = MockTransport::new(script);
            let c = JevClient::with_transport(enabled_settings(&[]), t.clone());
            let r = c.ask(JevItem::CommandIntent, "v1", false, json!({}), &q()).await;
            assert_eq!(r.unwrap_err(), want);
            assert_eq!(t.calls(), 1, "0 retries");
        }
    }

    #[tokio::test]
    async fn success_path_sends_no_authorization_for_proxy() {
        let body = json!({"model":"m","answers":{"q":{"type":"noul","noul":0.9}},"usage":{"input_tokens":1,"output_tokens":1}}).to_string();
        let t = MockTransport::new(Script::Reply(200, body));
        let c = JevClient::with_transport(enabled_settings(&[]), t.clone());
        let a = c.ask(JevItem::CommandIntent, "v1", false, json!({}), &q()).await.unwrap();
        assert!((a.noul("q").unwrap() - 0.9).abs() < 1e-9);
        let seen = t.seen.lock().unwrap();
        assert_eq!(seen[0].0.bearer, None);
        assert_eq!(seen[0].1["model"], "jev-test");
    }

    #[test]
    fn thresholds() {
        let c = JevClient::with_transport(enabled_settings(&[]), MockTransport::new(Script::Reply(200, "{}".into())));
        assert!(c.accept_suggestion(0.5).is_ok());
        assert_eq!(c.accept_suggestion(0.49), Err(FallbackReason::LowConfidence));
        assert_eq!(c.accept_outcome(1.0), Err(FallbackReason::LowConfidence), "no threshold ⇒ accept nothing");
        let c = JevClient::with_transport(
            enabled_settings(&[(ENV_AUTO_MIN_CONFIDENCE, "0.9")]),
            MockTransport::new(Script::Reply(200, "{}".into())),
        );
        assert!(c.accept_outcome(0.95).is_ok());
        assert_eq!(c.accept_outcome(0.89), Err(FallbackReason::LowConfidence));
    }

    #[test]
    fn reason_tokens() {
        assert_eq!(FallbackReason::Http(503).as_token(), "http-503");
        assert_eq!(FallbackReason::LowConfidence.as_token(), "low-confidence");
    }
}
