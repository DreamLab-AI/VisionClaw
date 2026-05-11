//! Nostr Bead Provenance Publisher
//!
//! Publishes bead lifecycle events (kind 30001) to the JSS Nostr relay for
//! cryptographic provenance tracking. Each completed brief -> debrief cycle
//! emits one signed event carrying the bead_id, brief_id, user pubkey, and
//! debrief path as tags.
//!
//! The event uses kind 30001 (parameterized replaceable, NIP-33) with the
//! bead_id as the `d` tag -- so a re-published bead overwrites the previous
//! entry on the relay rather than creating duplicates.
//!
//! Retry behaviour: transient failures (timeout, connection error) are retried
//! with exponential backoff up to `BeadRetryConfig::max_attempts`. Permanent
//! failures (signing error, relay rejection) fail immediately.
//!
//! Optionally writes provenance to Neo4j when an `Arc<neo4rs::Graph>` is
//! injected via `with_neo4j`. Schema written:
//!   (:NostrEvent {id, pubkey, kind, created_at})-[:PROVENANCE_OF]->(:Bead {bead_id, brief_id, debrief_path})
//!
//! Configure via environment:
//!   VISIONCLAW_NOSTR_PRIVKEY  -- 64-char hex secret key for the bridge bot
//!   JSS_RELAY_URL             -- WebSocket relay URL (default: ws://jss:3030/relay)

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use log::{debug, error, warn};
use neo4rs::{Graph, Query};
use nostr_sdk::prelude::*;
use serde_json::json;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use super::bead_types::{BeadOutcome, BeadRetryConfig};

/// Publishes provenance events to the JSS Nostr relay with retry support.
#[derive(Clone)]
pub struct NostrBeadPublisher {
    keys: Keys,
    relay_url: String,
    neo4j: Option<Arc<Graph>>,
    retry_config: BeadRetryConfig,
}

impl NostrBeadPublisher {
    /// Load from environment. Returns `None` if no canonical privkey is set.
    ///
    /// PRD-010 F1: the secret key is resolved through
    /// `crate::services::server_identity::resolve_canonical_nostr_privkey`.
    /// Both `SERVER_NOSTR_PRIVKEY` and `VISIONCLAW_NOSTR_PRIVKEY` are accepted;
    /// if both are set and DIVERGE, the resolver fails closed and this method
    /// returns `None` after logging the divergence error so bead provenance
    /// is not silently signed under a stale identity.
    pub fn from_env() -> Option<Self> {
        let privkey = match super::server_identity::resolve_canonical_nostr_privkey() {
            Ok(Some(k)) => k,
            Ok(None) => return None,
            Err(e) => {
                error!("[NostrBeadPublisher] {e}");
                return None;
            }
        };
        let relay_url = std::env::var("JSS_RELAY_URL")
            .unwrap_or_else(|_| "ws://jss:3030/relay".to_string());

        // Validate relay URL scheme to prevent SSRF via env var injection.
        if !relay_url.starts_with("ws://") && !relay_url.starts_with("wss://") {
            error!("[NostrBeadPublisher] JSS_RELAY_URL must start with ws:// or wss://: {relay_url}");
            return None;
        }

        let secret_key = super::server_identity::parse_secret_key(&privkey)
            .map_err(|e| error!("[NostrBeadPublisher] Invalid canonical privkey: {e}"))
            .ok()?;

        Some(Self {
            keys: Keys::new(secret_key),
            relay_url,
            neo4j: None,
            retry_config: BeadRetryConfig::default(),
        })
    }

    /// Inject a Neo4j graph handle for provenance writes. Call before first publish.
    pub fn with_neo4j(mut self, graph: Arc<Graph>) -> Self {
        self.neo4j = Some(graph);
        self
    }

    /// Override the default retry configuration.
    pub fn with_retry_config(mut self, config: BeadRetryConfig) -> Self {
        self.retry_config = config;
        self
    }

    /// Publish a bead-complete provenance event (kind 30001, parameterized replaceable).
    ///
    /// Returns a typed `BeadOutcome` classifying every terminal state.
    /// Callers may still `tokio::spawn` this so it does not block the debrief
    /// response path -- the outcome is available for logging or metrics.
    pub async fn publish_bead_complete(
        &self,
        bead_id: &str,
        brief_id: &str,
        user_pubkey: Option<&str>,
        debrief_path: &str,
    ) -> BeadOutcome {
        let mut tags = vec![
            Tag::custom(TagKind::Custom("h".into()), vec!["visionclaw-activity".to_string()]),
            Tag::custom(TagKind::Custom("d".into()), vec![bead_id.to_string()]),
            Tag::custom(TagKind::Custom("bead_id".into()), vec![bead_id.to_string()]),
            Tag::custom(TagKind::Custom("brief_id".into()), vec![brief_id.to_string()]),
            Tag::custom(TagKind::Custom("debrief_path".into()), vec![debrief_path.to_string()]),
        ];

        if let Some(pk) = user_pubkey {
            tags.push(Tag::custom(
                TagKind::Custom("user_pubkey".into()),
                vec![pk.to_string()],
            ));
        }

        let event = match EventBuilder::new(Kind::Custom(30001), "")
            .tags(tags)
            .sign_with_keys(&self.keys)
        {
            Ok(e) => e,
            Err(e) => {
                let msg = format!("{e}");
                error!("[NostrBeadPublisher] Failed to sign bead event: {msg}");
                return BeadOutcome::SigningFailed { error: msg };
            }
        };

        let outcome = self.send_with_retry(&event).await;

        if outcome.is_success() {
            debug!("[NostrBeadPublisher] Published bead {bead_id} (event {})", event.id);
            if let Some(ref graph) = self.neo4j {
                if let Err(e) = self.write_provenance(graph, &event, bead_id, brief_id, debrief_path).await {
                    warn!("[NostrBeadPublisher] Neo4j provenance write failed for bead {bead_id}: {e}");
                    return BeadOutcome::Neo4jWriteFailed { error: e };
                }
            }
        }

        outcome
    }

    /// Send an event to the relay with exponential-backoff retry on transient failures.
    async fn send_with_retry(&self, event: &Event) -> BeadOutcome {
        for attempt in 0..self.retry_config.max_attempts {
            match self.send_to_relay(event).await {
                Ok(()) => return BeadOutcome::Success,
                Err(e) if is_transient(&e) => {
                    if attempt + 1 < self.retry_config.max_attempts {
                        let delay = self.retry_config.delay_for_attempt(attempt);
                        warn!(
                            "[NostrBeadPublisher] Attempt {}/{} failed ({}), retrying in {}ms",
                            attempt + 1,
                            self.retry_config.max_attempts,
                            e,
                            delay
                        );
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                    } else {
                        warn!(
                            "[NostrBeadPublisher] All {} attempts exhausted: {}",
                            self.retry_config.max_attempts, e
                        );
                        return BeadOutcome::RelayTimeout {
                            attempts: self.retry_config.max_attempts,
                        };
                    }
                }
                Err(e) => return classify_error(e),
            }
        }
        unreachable!("all code paths return above")
    }

    /// Open a WebSocket, send the EVENT message, and wait for the relay's OK response.
    async fn send_to_relay(&self, event: &Event) -> Result<(), String> {
        let (ws_stream, _) = connect_async(&self.relay_url)
            .await
            .map_err(|e| format!("connect {}: {e}", self.relay_url))?;

        let (mut write, mut read) = ws_stream.split();

        write
            .send(Message::Text(json!(["EVENT", event]).to_string()))
            .await
            .map_err(|e| format!("send failed: {e}"))?;

        // Wait up to 5 s for the relay's OK response.
        tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(Ok(Message::Text(txt))) = read.next().await {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&txt) {
                    if parsed[0] == "OK" {
                        return if parsed[2].as_bool().unwrap_or(false) {
                            Ok(())
                        } else {
                            Err(format!("relay rejected: {}", parsed[3]))
                        };
                    }
                }
            }
            Err("relay closed without OK".to_string())
        })
        .await
        .map_err(|_| "relay response timeout".to_string())?
    }

    /// Write a (:NostrEvent)-[:PROVENANCE_OF]->(:Bead) pair to Neo4j.
    async fn write_provenance(
        &self,
        graph: &Arc<Graph>,
        event: &Event,
        bead_id: &str,
        brief_id: &str,
        debrief_path: &str,
    ) -> Result<(), String> {
        let query = Query::new(
            "MERGE (e:NostrEvent {id: $event_id}) \
             SET e.pubkey = $pubkey, e.kind = $kind, e.created_at = $created_at \
             WITH e \
             MERGE (b:Bead {bead_id: $bead_id}) \
             ON CREATE SET b.brief_id = $brief_id, b.debrief_path = $debrief_path \
             MERGE (e)-[:PROVENANCE_OF]->(b)"
            .to_string(),
        )
        .param("event_id", event.id.to_string())
        .param("pubkey", event.pubkey.to_string())
        .param("kind", event.kind.as_u16() as i64)
        .param("created_at", event.created_at.as_secs() as i64)
        .param("bead_id", bead_id.to_string())
        .param("brief_id", brief_id.to_string())
        .param("debrief_path", debrief_path.to_string());

        match graph.run(query).await {
            Ok(()) => {
                debug!(
                    "[NostrBeadPublisher] Provenance written to Neo4j: bead={bead_id} event={}",
                    event.id
                );
                Ok(())
            }
            Err(e) => Err(format!("{e}")),
        }
    }
}

/// Classify a `send_to_relay` error string as transient (retryable).
fn is_transient(error: &str) -> bool {
    error.contains("timeout")
        || error.contains("connect")
        || error.contains("closed without OK")
        || error.contains("send failed")
}

/// Map a permanent error string to the appropriate `BeadOutcome` variant.
fn classify_error(error: String) -> BeadOutcome {
    if error.contains("relay rejected") {
        BeadOutcome::RelayRejected { reason: error }
    } else if error.contains("connect") {
        BeadOutcome::RelayUnreachable { error }
    } else {
        // Fallback for unexpected permanent errors.
        BeadOutcome::RelayRejected { reason: error }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Clear both PRD-010 F1 canonical-key env vars so the resolver behaves
    /// identically for every test, regardless of inherited environment.
    fn clear_canonical_keys() {
        std::env::remove_var("VISIONCLAW_NOSTR_PRIVKEY");
        std::env::remove_var("SERVER_NOSTR_PRIVKEY");
    }

    // ── from_env ───────────────────────────────────────────────────────

    #[test]
    fn from_env_returns_none_without_privkey() {
        // GIVEN: no canonical privkey set on either env var
        clear_canonical_keys();

        // WHEN: calling from_env
        let result = NostrBeadPublisher::from_env();

        // THEN: None -- publisher cannot operate without a signing key
        assert!(result.is_none());
    }

    #[test]
    fn from_env_rejects_non_ws_relay_url() {
        // GIVEN: a valid privkey but an HTTP relay URL (SSRF vector)
        // Use a known-valid 64-char hex key for the nostr-sdk SecretKey parse
        clear_canonical_keys();
        std::env::set_var(
            "VISIONCLAW_NOSTR_PRIVKEY",
            "0000000000000000000000000000000000000000000000000000000000000001",
        );
        std::env::set_var("JSS_RELAY_URL", "http://evil.com/relay");

        // WHEN: calling from_env
        let result = NostrBeadPublisher::from_env();

        // THEN: None -- scheme validation rejects http://
        assert!(result.is_none());

        // Cleanup
        clear_canonical_keys();
        std::env::remove_var("JSS_RELAY_URL");
    }

    #[test]
    fn from_env_accepts_valid_ws_url() {
        // GIVEN: valid privkey and ws:// relay
        clear_canonical_keys();
        std::env::set_var(
            "VISIONCLAW_NOSTR_PRIVKEY",
            "0000000000000000000000000000000000000000000000000000000000000001",
        );
        std::env::set_var("JSS_RELAY_URL", "ws://localhost:3030/relay");

        // WHEN: calling from_env
        let result = NostrBeadPublisher::from_env();

        // THEN: publisher is created
        assert!(result.is_some());

        // Cleanup
        clear_canonical_keys();
        std::env::remove_var("JSS_RELAY_URL");
    }

    #[test]
    fn from_env_returns_none_for_empty_privkey() {
        // GIVEN: empty VISIONCLAW_NOSTR_PRIVKEY
        clear_canonical_keys();
        std::env::set_var("VISIONCLAW_NOSTR_PRIVKEY", "");

        // WHEN: calling from_env
        let result = NostrBeadPublisher::from_env();

        // THEN: None -- filter(|s| !s.is_empty()) rejects it
        assert!(result.is_none());

        // Cleanup
        std::env::remove_var("VISIONCLAW_NOSTR_PRIVKEY");
    }

    #[test]
    #[ignore = "PRD-010 F1: env-var divergence test — runs in single-threaded test harness only \
                because std::env::set_var is process-global and races with other from_env tests"]
    fn from_env_fails_closed_on_divergent_keys() {
        // GIVEN: two DIFFERENT canonical keys set on the two env vars
        clear_canonical_keys();
        std::env::set_var(
            "SERVER_NOSTR_PRIVKEY",
            "0000000000000000000000000000000000000000000000000000000000000001",
        );
        std::env::set_var(
            "VISIONCLAW_NOSTR_PRIVKEY",
            "0000000000000000000000000000000000000000000000000000000000000002",
        );
        std::env::set_var("JSS_RELAY_URL", "ws://localhost:3030/relay");

        // WHEN: calling from_env
        let result = NostrBeadPublisher::from_env();

        // THEN: None -- divergence is fail-closed per PRD-010 F1
        assert!(
            result.is_none(),
            "Divergent SERVER_NOSTR_PRIVKEY and VISIONCLAW_NOSTR_PRIVKEY \
             must NOT silently pick one — refuse to start"
        );

        clear_canonical_keys();
        std::env::remove_var("JSS_RELAY_URL");
    }

    // ── is_transient classification ────────────────────────────────────

    #[test]
    fn is_transient_true_for_timeout() {
        assert!(is_transient("relay response timeout"));
    }

    #[test]
    fn is_transient_true_for_connect_failure() {
        assert!(is_transient("connect ws://relay:3030: connection refused"));
    }

    #[test]
    fn is_transient_true_for_closed_without_ok() {
        assert!(is_transient("relay closed without OK"));
    }

    #[test]
    fn is_transient_true_for_send_failed() {
        assert!(is_transient("send failed: broken pipe"));
    }

    #[test]
    fn is_transient_false_for_rejection() {
        assert!(!is_transient("relay rejected: spam"));
    }

    #[test]
    fn is_transient_false_for_unknown_permanent() {
        assert!(!is_transient("unknown permanent error"));
    }

    // ── classify_error ─────────────────────────────────────────────────

    #[test]
    fn classify_error_relay_rejected() {
        let outcome = classify_error("relay rejected: spam filter".to_string());
        assert!(matches!(outcome, BeadOutcome::RelayRejected { .. }));
    }

    #[test]
    fn classify_error_connect_failure_maps_to_unreachable() {
        let outcome = classify_error("connect ws://relay:3030: refused".to_string());
        assert!(matches!(outcome, BeadOutcome::RelayUnreachable { .. }));
    }

    #[test]
    fn classify_error_unknown_falls_back_to_rejected() {
        let outcome = classify_error("something unexpected".to_string());
        assert!(matches!(outcome, BeadOutcome::RelayRejected { .. }));
    }

    // ── BeadRetryConfig ──────────────────────────────────────────────
    // NOTE: env var tests replaced with direct struct tests to avoid
    // thread-safety issues with std::env::set_var in parallel test runs.

    #[test]
    fn retry_config_custom_values() {
        // GIVEN: a custom retry config
        let cfg = BeadRetryConfig {
            max_attempts: 5,
            base_delay_ms: 500,
            max_delay_ms: 20_000,
            backoff_multiplier: 3.0,
        };

        // THEN: values match
        assert_eq!(cfg.max_attempts, 5);
        assert_eq!(cfg.base_delay_ms, 500);
        assert_eq!(cfg.max_delay_ms, 20_000);
        assert!((cfg.backoff_multiplier - 3.0).abs() < f64::EPSILON);
        // Verify backoff calculation: attempt 0 = 500, attempt 1 = 1500, attempt 2 = 4500
        assert_eq!(cfg.delay_for_attempt(0), 500);
        assert_eq!(cfg.delay_for_attempt(1), 1500);
        assert_eq!(cfg.delay_for_attempt(2), 4500);
    }

    #[test]
    fn retry_config_defaults() {
        // WHEN: using defaults
        let cfg = BeadRetryConfig::default();

        // THEN: defaults apply
        assert_eq!(cfg.max_attempts, 3);
        assert_eq!(cfg.base_delay_ms, 1000);
        assert_eq!(cfg.max_delay_ms, 10_000);
        assert!((cfg.backoff_multiplier - 2.0).abs() < f64::EPSILON);
    }
}
