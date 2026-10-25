//! The System One client: a mockable [`Transport`], the reqwest-backed
//! [`HttpTransport`], and [`Jev`], which binds a resolved endpoint to a
//! transport and turns every failure into a [`Fallback`].
//!
//! Zero retries: every caller is interactive or a hook path, so a 429/529 is
//! an immediate fallback, not a wait.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;

use crate::config::{self, Item, Resolved};
use crate::wire::{Answers, Request};
use crate::Fallback;

/// Why a send failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// The deadline elapsed.
    Timeout,
    /// Connect, TLS, body-read or any other failure.
    Other,
}

/// A raw HTTP answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawResponse {
    /// HTTP status.
    pub status: u16,
    /// Response body text.
    pub body: String,
}

/// One JSON POST. Implemented by [`HttpTransport`] and by test mocks.
#[async_trait]
pub trait Transport: Send + Sync {
    /// POST `body` as JSON to `url`. `bearer` is the only credential sent.
    ///
    /// # Errors
    ///
    /// [`TransportError::Timeout`] when `timeout` elapses, otherwise
    /// [`TransportError::Other`].
    async fn post(
        &self,
        url: &str,
        bearer: Option<&str>,
        body: &Value,
        timeout: Duration,
    ) -> Result<RawResponse, TransportError>;
}

/// The real transport (reqwest, no redirects).
#[derive(Clone)]
pub struct HttpTransport {
    client: reqwest::Client,
}

impl HttpTransport {
    /// Build the transport.
    ///
    /// # Errors
    ///
    /// [`Fallback::Transport`] if the HTTP client cannot be built.
    pub fn new() -> Result<Self, Fallback> {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("visionclaw-jev/0.1")
            .build()
            .map(|client| Self { client })
            .map_err(|_| Fallback::Transport)
    }
}

#[async_trait]
impl Transport for HttpTransport {
    async fn post(
        &self,
        url: &str,
        bearer: Option<&str>,
        body: &Value,
        timeout: Duration,
    ) -> Result<RawResponse, TransportError> {
        let mut rb = self.client.post(url).timeout(timeout).json(body);
        if let Some(token) = bearer {
            rb = rb.bearer_auth(token);
        }
        let map = |e: reqwest::Error| {
            if e.is_timeout() {
                TransportError::Timeout
            } else {
                TransportError::Other
            }
        };
        let resp = rb.send().await.map_err(map)?;
        let status = resp.status().as_u16();
        let body = resp.text().await.map_err(map)?;
        Ok(RawResponse { status, body })
    }
}

/// A resolved, ready-to-call Jev endpoint for one [`Item`].
#[derive(Clone)]
pub struct Jev {
    item: Item,
    resolved: Resolved,
    transport: Arc<dyn Transport>,
}

impl std::fmt::Debug for Jev {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Jev").field("item", &self.item).field("resolved", &self.resolved).finish()
    }
}

impl Jev {
    /// Resolve from the process environment with the real transport.
    ///
    /// # Errors
    ///
    /// The [`Fallback`] reason Jev is unavailable ([`Fallback::FlagOff`] is the
    /// normal case).
    pub fn from_env(item: Item) -> Result<Self, Fallback> {
        let lookup = |k: &str| std::env::var(k).ok();
        // Resolve first: with the flag off nothing is even constructed.
        let resolved = config::resolve(item, &lookup)?;
        Ok(Self { item, resolved, transport: Arc::new(HttpTransport::new()?) })
    }

    /// Like [`Jev::from_env`] but logs a non-`flag-off` reason once and
    /// returns `None`: the shape consumers hold at construction time.
    pub fn from_env_logged(item: Item, version: &str) -> Option<Self> {
        match Self::from_env(item) {
            Ok(j) => Some(j),
            Err(reason) => {
                crate::log_fallback(item, version, &reason);
                None
            }
        }
    }

    /// Resolve from a lookup closure with a supplied transport (tests, and
    /// hosts that own their own HTTP stack).
    ///
    /// # Errors
    ///
    /// The [`Fallback`] reason Jev is unavailable.
    pub fn from_lookup(
        item: Item,
        lookup: &dyn Fn(&str) -> Option<String>,
        transport: Arc<dyn Transport>,
    ) -> Result<Self, Fallback> {
        Ok(Self { item, resolved: config::resolve(item, lookup)?, transport })
    }

    /// The item this client serves.
    pub fn item(&self) -> Item {
        self.item
    }

    /// The model name sent in every request.
    pub fn model(&self) -> &str {
        &self.resolved.model
    }

    /// The resolved endpoint (the bearer is redacted in `Debug`).
    pub fn resolved(&self) -> &Resolved {
        &self.resolved
    }

    /// Send one request and return validated-shape answers.
    ///
    /// # Errors
    ///
    /// Any [`Fallback`]: [`Fallback::Malformed`] for a request that fails local
    /// validation or a body that does not parse, [`Fallback::Timeout`],
    /// [`Fallback::Http`] for any non-2xx status, [`Fallback::Transport`].
    pub async fn ask(&self, req: &Request) -> Result<Answers, Fallback> {
        req.validate().map_err(|_| Fallback::Malformed)?;
        let body = serde_json::to_value(req).map_err(|_| Fallback::Malformed)?;
        let deadline = self.resolved.timeout;
        let call = self.transport.post(
            &self.resolved.url,
            self.resolved.bearer.as_deref(),
            &body,
            deadline,
        );
        // Hard deadline independent of the transport's own.
        let raw = match tokio::time::timeout(deadline, call).await {
            Err(_) => return Err(Fallback::Timeout),
            Ok(Err(TransportError::Timeout)) => return Err(Fallback::Timeout),
            Ok(Err(TransportError::Other)) => return Err(Fallback::Transport),
            Ok(Ok(raw)) => raw,
        };
        if !(200..300).contains(&raw.status) {
            return Err(Fallback::Http(raw.status));
        }
        Answers::parse(&raw.body)
    }
}

/// A scripted transport for tests in this crate and in hosts.
#[cfg(any(test, feature = "test-util"))]
pub mod testing {
    use super::*;
    use std::sync::Mutex;

    /// What the mock does on each call.
    #[derive(Clone)]
    pub enum Script {
        /// Return this status and body.
        Reply(u16, String),
        /// Fail with this transport error.
        Fail(TransportError),
        /// Sleep past the deadline.
        Hang,
    }

    /// Records every request body it receives and replays a [`Script`].
    pub struct MockTransport {
        script: Script,
        /// Every request body sent.
        pub seen: Mutex<Vec<Value>>,
        /// Every bearer token presented (None = no Authorization header).
        pub bearers: Mutex<Vec<Option<String>>>,
    }

    impl MockTransport {
        /// A mock replaying `script`.
        pub fn new(script: Script) -> Arc<Self> {
            Arc::new(Self { script, seen: Mutex::new(vec![]), bearers: Mutex::new(vec![]) })
        }
        /// How many requests were sent.
        pub fn calls(&self) -> usize {
            self.seen.lock().unwrap().len()
        }
        /// The last request body sent.
        pub fn last_body(&self) -> Option<Value> {
            self.seen.lock().unwrap().last().cloned()
        }
    }

    #[async_trait]
    impl Transport for MockTransport {
        async fn post(
            &self,
            _url: &str,
            bearer: Option<&str>,
            body: &Value,
            _timeout: Duration,
        ) -> Result<RawResponse, TransportError> {
            self.seen.lock().unwrap().push(body.clone());
            self.bearers.lock().unwrap().push(bearer.map(str::to_string));
            match &self.script {
                Script::Reply(status, body) => Ok(RawResponse { status: *status, body: body.clone() }),
                Script::Fail(e) => Err(e.clone()),
                Script::Hang => {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    Err(TransportError::Timeout)
                }
            }
        }
    }

    /// A transport whose reply is computed from the request body, so tests can
    /// answer differently per question set.
    pub struct FnTransport<F>(pub F, pub Mutex<usize>);

    impl<F> FnTransport<F>
    where
        F: Fn(&Value) -> Result<RawResponse, TransportError> + Send + Sync + 'static,
    {
        /// Wrap a responder.
        pub fn new(f: F) -> Arc<Self> {
            Arc::new(Self(f, Mutex::new(0)))
        }
        /// How many requests were sent.
        pub fn calls(&self) -> usize {
            *self.1.lock().unwrap()
        }
    }

    #[async_trait]
    impl<F> Transport for FnTransport<F>
    where
        F: Fn(&Value) -> Result<RawResponse, TransportError> + Send + Sync + 'static,
    {
        async fn post(&self, _u: &str, _b: Option<&str>, body: &Value, _t: Duration) -> Result<RawResponse, TransportError> {
            *self.1.lock().unwrap() += 1;
            (self.0)(body)
        }
    }

    /// A lookup that enables `item` against a fake local endpoint.
    pub fn enabled_lookup(item: Item) -> impl Fn(&str) -> Option<String> {
        let flag = item.flag_var();
        move |k| match k {
            k if k == flag => Some("1".into()),
            "VISIONCLAW_JEV_ENDPOINT" => Some("http://jev.test".into()),
            "VISIONCLAW_JEV_TIMEOUT_MS" => Some("150".into()),
            _ => None,
        }
    }

    /// A [`Jev`] for `item` over `transport`, short deadline.
    pub fn jev(item: Item, transport: Arc<dyn Transport>) -> Jev {
        Jev::from_lookup(item, &enabled_lookup(item), transport).expect("test endpoint resolves")
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use crate::wire::{mock, Question};
    use std::collections::BTreeMap;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn req() -> Request {
        let mut qs = BTreeMap::new();
        qs.insert("n".to_string(), Question::Noul { instructions: "is it?".into() });
        Request { model: "jev-latest".into(), state: serde_json::json!("s"), questions: qs }
    }

    #[tokio::test]
    async fn happy_path_and_wire_body() {
        let t = MockTransport::new(Script::Reply(200, mock::body(vec![("n".into(), mock::noul(0.8))])));
        let j = jev(Item::ProposalTriage, t.clone());
        let a = j.ask(&req()).await.unwrap();
        assert_eq!(a.noul("n").unwrap(), 0.8);
        let sent = t.seen.lock().unwrap()[0].clone();
        assert_eq!(sent["model"], "jev-latest");
        assert_eq!(sent["questions"]["n"]["type"], "noul");
    }

    #[tokio::test]
    async fn every_failure_maps_to_a_fallback_reason() {
        let cases: Vec<(Script, Fallback)> = vec![
            (Script::Reply(500, "x".into()), Fallback::Http(500)),
            (Script::Reply(429, "x".into()), Fallback::Http(429)),
            (Script::Reply(529, "x".into()), Fallback::Http(529)),
            (Script::Reply(401, "x".into()), Fallback::Http(401)),
            (Script::Reply(200, "not json".into()), Fallback::Malformed),
            (Script::Reply(200, "{}".into()), Fallback::Malformed),
            (Script::Fail(TransportError::Timeout), Fallback::Timeout),
            (Script::Fail(TransportError::Other), Fallback::Transport),
            (Script::Hang, Fallback::Timeout),
        ];
        for (script, want) in cases {
            let j = jev(Item::ProposalTriage, MockTransport::new(script));
            assert_eq!(j.ask(&req()).await.unwrap_err(), want);
        }
    }

    #[tokio::test]
    async fn invalid_request_never_leaves_the_process() {
        let t = MockTransport::new(Script::Reply(200, "{}".into()));
        let j = jev(Item::ProposalTriage, t.clone());
        let bad = Request { model: "m".into(), state: serde_json::json!("s"), questions: BTreeMap::new() };
        assert_eq!(j.ask(&bad).await.unwrap_err(), Fallback::Malformed);
        assert_eq!(t.calls(), 0);
    }

    #[test]
    fn flag_off_builds_nothing() {
        let t = MockTransport::new(Script::Reply(200, "{}".into()));
        let none = |_: &str| None;
        assert_eq!(Jev::from_lookup(Item::MemoryRerank, &none, t.clone()).unwrap_err(), Fallback::FlagOff);
        assert_eq!(t.calls(), 0);
    }

    #[test]
    fn proxy_presents_no_bearer() {
        let lookup = |k: &str| match k {
            "VISIONCLAW_JEV_HEALTH_TRIAGE" => Some("1".to_string()),
            "AGENTBOX_SYSTEM_ONE_PROXY" => Some("1".to_string()),
            "AGENTBOX_SYSTEM_ONE_ENDPOINT" => Some("http://127.0.0.1:1".to_string()),
            "AGENTBOX_SYSTEM_ONE_MODEL" => Some("jev-latest".to_string()),
            "TYPESAFE_API_KEY" => Some("must-not-be-sent".to_string()),
            _ => None,
        };
        let t = MockTransport::new(Script::Reply(200, mock::body(vec![("n".into(), mock::noul(0.5))])));
        let j = Jev::from_lookup(Item::HealthTriage, &lookup, t.clone()).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(j.ask(&req())).unwrap();
        assert_eq!(t.bearers.lock().unwrap()[0], None);
    }

    /// One-shot local HTTP server so the real transport is exercised too.
    fn serve_once(status_line: &'static str, body: String, delay_ms: u64) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut buf = [0u8; 8192];
                let _ = s.read(&mut buf);
                std::thread::sleep(Duration::from_millis(delay_ms));
                let resp = format!(
                    "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = s.write_all(resp.as_bytes());
            }
        });
        format!("http://{addr}")
    }

    fn real(item: Item, base: String) -> Jev {
        let flag = item.flag_var();
        let lookup = move |k: &str| match k {
            k if k == flag => Some("1".to_string()),
            "VISIONCLAW_JEV_ENDPOINT" => Some(base.clone()),
            "VISIONCLAW_JEV_TIMEOUT_MS" => Some("300".to_string()),
            _ => None,
        };
        Jev::from_lookup(item, &lookup, Arc::new(HttpTransport::new().unwrap())).unwrap()
    }

    #[tokio::test]
    async fn real_http_transport_ok_status_timeout_and_malformed() {
        let ok = serve_once("200 OK", mock::body(vec![("n".into(), mock::noul(0.7))]), 0);
        let a = real(Item::ProposalTriage, ok).ask(&req()).await.unwrap();
        assert_eq!(a.noul("n").unwrap(), 0.7);

        let e = serve_once("503 Service Unavailable", "{}".into(), 0);
        assert_eq!(real(Item::ProposalTriage, e).ask(&req()).await.unwrap_err(), Fallback::Http(503));

        let m = serve_once("200 OK", "<html>".into(), 0);
        assert_eq!(real(Item::ProposalTriage, m).ask(&req()).await.unwrap_err(), Fallback::Malformed);

        let slow = serve_once("200 OK", "{}".into(), 1_500);
        assert_eq!(real(Item::ProposalTriage, slow).ask(&req()).await.unwrap_err(), Fallback::Timeout);

        // nothing listening
        assert!(real(Item::ProposalTriage, "http://127.0.0.1:1".into()).ask(&req()).await.is_err());
    }
}
