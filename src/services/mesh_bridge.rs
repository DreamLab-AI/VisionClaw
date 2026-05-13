//! Mesh Bridge: multi-peer relay subscription with dedup and kind-based routing.
//!
//! PRD-010 F22: replaces the single-relay JSS-only `NostrBridge` with a generic
//! `MeshBridge` that subscribes to ALL mesh peers listed in `MESH_PEER_RELAYS`.
//! Falls back to `JSS_RELAY_URL` for backwards compatibility when the mesh env
//! var is absent or empty.
//!
//! Subscribes to configurable event kinds (`MESH_FEDERATED_KINDS`, default
//! `1059,30001,30050,30910`) on every peer relay. Kind-based routing:
//! - kind 30001 (bead provenance): re-sign as NIP-29 group message (kind 9)
//!   and publish to the forum relay (legacy behaviour).
//! - kind 30050 (IS-Envelope mesh-event): forward as-is to the forum relay
//!   (no re-signing — IS-Envelope events carry their own authentication).
//! - Other federated kinds: forward as-is (same as 30050 path).
//!
//! PRD-010 F21: LRU dedup cache (capacity 4096, TTL 600 s) prevents fan-out
//! storms when the same event arrives from multiple peers.
//!
//! Each peer relay runs in its own tokio task with independent exponential
//! backoff (5 s -> 300 s, reset after 60 s healthy). `BridgeHealth` reports
//! aggregate connectivity: `is_connected()` returns true if ANY peer is up.
//!
//! ```ignore
//! let bridge = MeshBridge::from_env().unwrap();
//! let health = bridge.health();
//! tokio::spawn(bridge.run());
//! assert!(!health.is_connected()); // not yet
//! ```

use futures_util::{SinkExt, StreamExt};
use log::{error, info, warn};
use lru::LruCache;
use nostr_sdk::prelude::*;
use serde_json::{json, Value};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const INITIAL_BACKOFF_SECS: u64 = 5;
const MAX_BACKOFF_SECS: u64 = 300;
const BACKOFF_MULTIPLIER: f64 = 2.0;
/// If a connection lasted longer than this, reset backoff on reconnect.
const HEALTHY_CONNECTION_SECS: u64 = 60;

/// Dedup cache capacity (number of event IDs retained).
const DEDUP_CACHE_CAPACITY: usize = 4096;
/// Dedup TTL: events older than this are evicted on next check.
const DEDUP_TTL_SECS: u64 = 600;

/// Default federated event kinds when `MESH_FEDERATED_KINDS` is not set.
/// 1059 = NIP-59 gift-wrap, 30001 = bead provenance, 30050 = IS-Envelope,
/// 30910 = mesh control.
const DEFAULT_FEDERATED_KINDS: &[u64] = &[1059, 30001, 30050, 30910];

/// Kind 30001: bead provenance — requires re-signing before forum publish.
const KIND_BEAD_PROVENANCE: u64 = 30001;

/// Cheaply cloneable handle for querying bridge health from external code.
#[derive(Clone)]
pub struct BridgeHealth {
    /// True if ANY peer relay is connected.
    connected: Arc<AtomicBool>,
    last_event_at: Arc<Mutex<Option<Instant>>>,
    /// Number of peers currently connected (informational).
    peers_connected: Arc<AtomicUsize>,
    /// Total number of configured peer relays.
    peers_total: usize,
}

impl BridgeHealth {
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub fn last_event_age_secs(&self) -> Option<u64> {
        self.last_event_at
            .lock()
            .ok()
            .and_then(|guard| guard.map(|t| t.elapsed().as_secs()))
    }

    /// Number of peer relays currently connected.
    pub fn peers_connected(&self) -> usize {
        self.peers_connected.load(Ordering::Relaxed)
    }

    /// Total number of configured peer relays.
    pub fn peers_total(&self) -> usize {
        self.peers_total
    }
}

/// Per-peer connection state, shared between the peer task and the bridge.
struct PeerState {
    connected: AtomicBool,
}

/// LRU-based event dedup cache with TTL expiry.
struct DedupCache {
    inner: LruCache<String, Instant>,
}

impl DedupCache {
    fn new(capacity: usize) -> Self {
        Self {
            inner: LruCache::new(
                NonZeroUsize::new(capacity).expect("dedup cache capacity must be > 0"),
            ),
        }
    }

    /// Returns `true` if the event ID was already seen within the TTL window.
    /// Inserts the ID if not present or expired.
    fn is_duplicate(&mut self, event_id: &str) -> bool {
        if let Some(ts) = self.inner.get(event_id) {
            if ts.elapsed().as_secs() < DEDUP_TTL_SECS {
                return true;
            }
            // Expired — fall through to re-insert with fresh timestamp.
        }
        self.inner.put(event_id.to_string(), Instant::now());
        false
    }
}

pub struct MeshBridge {
    keys: Keys,
    peer_relay_urls: Vec<String>,
    forum_relay_url: String,
    federated_kinds: Vec<u64>,
    // Shared state for health monitoring.
    aggregate_connected: Arc<AtomicBool>,
    peers_connected_count: Arc<AtomicUsize>,
    last_event_at: Arc<Mutex<Option<Instant>>>,
    // Shared dedup cache across all peer tasks.
    dedup_cache: Arc<Mutex<DedupCache>>,
}

impl MeshBridge {
    /// Load from environment. Returns `None` if required vars are absent.
    ///
    /// PRD-010 F1: the secret key is resolved through
    /// `crate::services::server_identity::resolve_canonical_nostr_privkey`.
    /// Both `SERVER_NOSTR_PRIVKEY` and `VISIONCLAW_NOSTR_PRIVKEY` are accepted;
    /// if both are set and DIVERGE, the resolver fails closed and this method
    /// returns `None` after logging the divergence error so the bridge does
    /// not silently re-sign forwarded events under a stale identity.
    ///
    /// PRD-010 F22: reads `MESH_PEER_RELAYS` (comma-separated ws:// or wss://
    /// URLs). Falls back to `JSS_RELAY_URL` when empty. Reads
    /// `MESH_FEDERATED_KINDS` (comma-separated u64) for subscription filter.
    pub fn from_env() -> Option<Self> {
        let privkey = match super::server_identity::resolve_canonical_nostr_privkey() {
            Ok(Some(k)) => k,
            Ok(None) => {
                // No key set — bridge cannot operate. Caller treats as opt-out.
                return None;
            }
            Err(e) => {
                error!("[MeshBridge] {e}");
                return None;
            }
        };
        let forum_relay_url = std::env::var("FORUM_RELAY_URL")
            .ok()
            .filter(|s| !s.is_empty())?;

        // Validate forum relay URL scheme.
        if !forum_relay_url.starts_with("ws://") && !forum_relay_url.starts_with("wss://") {
            error!(
                "[MeshBridge] FORUM_RELAY_URL must start with ws:// or wss://: {forum_relay_url}"
            );
            return None;
        }

        // --- Peer relay URLs ---
        // PRD-010 F22: MESH_PEER_RELAYS takes priority; fall back to JSS_RELAY_URL.
        let peer_relay_urls = parse_peer_relay_urls();
        if peer_relay_urls.is_empty() {
            error!("[MeshBridge] No valid peer relay URLs configured (checked MESH_PEER_RELAYS and JSS_RELAY_URL)");
            return None;
        }

        // --- Federated kinds ---
        let federated_kinds = parse_federated_kinds();

        let secret_key = super::server_identity::parse_secret_key(&privkey)
            .map_err(|e| error!("[MeshBridge] Invalid canonical privkey: {e}"))
            .ok()?;

        Some(Self {
            keys: Keys::new(secret_key),
            peer_relay_urls,
            forum_relay_url,
            federated_kinds,
            aggregate_connected: Arc::new(AtomicBool::new(false)),
            peers_connected_count: Arc::new(AtomicUsize::new(0)),
            last_event_at: Arc::new(Mutex::new(None)),
            dedup_cache: Arc::new(Mutex::new(DedupCache::new(DEDUP_CACHE_CAPACITY))),
        })
    }

    /// Return a cloneable health handle. Call *before* `run()` consumes self.
    pub fn health(&self) -> BridgeHealth {
        BridgeHealth {
            connected: Arc::clone(&self.aggregate_connected),
            last_event_at: Arc::clone(&self.last_event_at),
            peers_connected: Arc::clone(&self.peers_connected_count),
            peers_total: self.peer_relay_urls.len(),
        }
    }

    /// Run the bridge: spawn one task per peer relay, all sharing the dedup
    /// cache and forum-publish path. Blocks indefinitely (all peer tasks run
    /// their own reconnect loops).
    pub async fn run(self) {
        let peer_count = self.peer_relay_urls.len();
        info!(
            target: "mesh_bridge",
            "[MeshBridge] Starting with {peer_count} peer(s) -> forum={}, kinds={:?}",
            self.forum_relay_url, self.federated_kinds
        );

        // Build per-peer state.
        let peer_states: Vec<Arc<PeerState>> = (0..peer_count)
            .map(|_| {
                Arc::new(PeerState {
                    connected: AtomicBool::new(false),
                })
            })
            .collect();

        let keys = Arc::new(self.keys);
        let forum_url = Arc::new(self.forum_relay_url);
        let kinds = Arc::new(self.federated_kinds);
        let aggregate_connected = self.aggregate_connected;
        let peers_connected_count = self.peers_connected_count;
        let last_event_at = self.last_event_at;
        let dedup_cache = self.dedup_cache;

        let mut handles = Vec::with_capacity(peer_count);

        for (idx, peer_url) in self.peer_relay_urls.into_iter().enumerate() {
            let keys = Arc::clone(&keys);
            let forum_url = Arc::clone(&forum_url);
            let kinds = Arc::clone(&kinds);
            let peer_state = Arc::clone(&peer_states[idx]);
            let aggregate_connected = Arc::clone(&aggregate_connected);
            let peers_connected_count = Arc::clone(&peers_connected_count);
            let last_event_at = Arc::clone(&last_event_at);
            let dedup_cache = Arc::clone(&dedup_cache);
            let all_peer_states: Vec<Arc<PeerState>> = peer_states.clone();

            let handle = tokio::spawn(async move {
                run_peer_loop(
                    idx,
                    peer_url,
                    keys,
                    forum_url,
                    kinds,
                    peer_state,
                    aggregate_connected,
                    peers_connected_count,
                    last_event_at,
                    dedup_cache,
                    all_peer_states,
                )
                .await;
            });
            handles.push(handle);
        }

        // Await all peer tasks (they loop indefinitely, so this effectively
        // blocks forever unless all tasks panic).
        futures_util::future::join_all(handles).await;
    }
}

/// Run the reconnect loop for a single peer relay.
#[allow(clippy::too_many_arguments)]
async fn run_peer_loop(
    peer_idx: usize,
    peer_url: String,
    keys: Arc<Keys>,
    forum_url: Arc<String>,
    kinds: Arc<Vec<u64>>,
    peer_state: Arc<PeerState>,
    aggregate_connected: Arc<AtomicBool>,
    peers_connected_count: Arc<AtomicUsize>,
    last_event_at: Arc<Mutex<Option<Instant>>>,
    dedup_cache: Arc<Mutex<DedupCache>>,
    all_peer_states: Vec<Arc<PeerState>>,
) {
    let mut backoff_secs = INITIAL_BACKOFF_SECS;
    loop {
        let started = Instant::now();
        match run_peer_once(
            peer_idx,
            &peer_url,
            &keys,
            &forum_url,
            &kinds,
            &last_event_at,
            &dedup_cache,
        )
        .await
        {
            Ok(()) => {
                warn!(
                    target: "mesh_bridge",
                    "[MeshBridge] peer[{peer_idx}] {peer_url}: stream ended, reconnecting in {backoff_secs}s"
                );
            }
            Err(e) => {
                warn!(
                    target: "mesh_bridge",
                    "[MeshBridge] peer[{peer_idx}] {peer_url}: connection lost ({e}), reconnecting in {backoff_secs}s"
                );
            }
        }

        // Mark this peer as disconnected and recompute aggregate.
        peer_state.connected.store(false, Ordering::Relaxed);
        recompute_aggregate(&all_peer_states, &aggregate_connected, &peers_connected_count);

        if started.elapsed().as_secs() > HEALTHY_CONNECTION_SECS {
            backoff_secs = INITIAL_BACKOFF_SECS;
        }
        tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
        backoff_secs = ((backoff_secs as f64 * BACKOFF_MULTIPLIER) as u64).min(MAX_BACKOFF_SECS);
    }
}

/// Single connection attempt to a peer relay. Subscribes to configured kinds
/// and processes events until the stream ends or errors.
async fn run_peer_once(
    peer_idx: usize,
    peer_url: &str,
    keys: &Keys,
    forum_url: &str,
    kinds: &[u64],
    last_event_at: &Arc<Mutex<Option<Instant>>>,
    dedup_cache: &Arc<Mutex<DedupCache>>,
) -> Result<(), String> {
    let (ws_stream, _) = connect_async(peer_url)
        .await
        .map_err(|e| format!("peer[{peer_idx}] connect failed: {e}"))?;

    let (mut write, mut read) = ws_stream.split();

    // Build Nostr REQ filter for the configured kinds.
    let kinds_json: Vec<Value> = kinds.iter().map(|k| json!(k)).collect();
    write
        .send(Message::Text(
            json!(["REQ", format!("mesh-{peer_idx}"), {"kinds": kinds_json}]).to_string(),
        ))
        .await
        .map_err(|e| format!("peer[{peer_idx}] REQ send failed: {e}"))?;

    info!(
        target: "mesh_bridge",
        "[MeshBridge] peer[{peer_idx}] {peer_url}: subscribed to kinds {kinds:?}"
    );

    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(txt)) => {
                if let Ok(parsed) = serde_json::from_str::<Value>(&txt) {
                    // ["EVENT", "<sub_id>", <event_object>]
                    if parsed[0] == "EVENT" {
                        if let Some(event_obj) = parsed.get(2) {
                            handle_event(
                                peer_idx,
                                event_obj,
                                keys,
                                forum_url,
                                last_event_at,
                                dedup_cache,
                            )
                            .await;
                        }
                    }
                }
            }
            Ok(Message::Close(_)) => {
                return Err(format!("peer[{peer_idx}] relay closed connection"));
            }
            Err(e) => return Err(format!("peer[{peer_idx}] WebSocket error: {e}")),
            _ => {}
        }
    }

    Err(format!("peer[{peer_idx}] relay stream ended"))
}

/// Handle a single incoming event: dedup, verify, route by kind.
async fn handle_event(
    peer_idx: usize,
    original: &Value,
    keys: &Keys,
    forum_url: &str,
    last_event_at: &Arc<Mutex<Option<Instant>>>,
    dedup_cache: &Arc<Mutex<DedupCache>>,
) {
    let event_id = match original["id"].as_str() {
        Some(id) => id.to_string(),
        None => {
            warn!(
                target: "mesh_bridge",
                "[MeshBridge] peer[{peer_idx}]: event without id, skipping"
            );
            return;
        }
    };

    // F21: LRU dedup — skip if we've already processed this event ID.
    {
        let mut cache = match dedup_cache.lock() {
            Ok(c) => c,
            Err(_) => {
                warn!(target: "mesh_bridge", "[MeshBridge] dedup cache lock poisoned");
                return;
            }
        };
        if cache.is_duplicate(&event_id) {
            info!(
                target: "mesh_bridge",
                "[MeshBridge] peer[{peer_idx}]: dedup skip event_id={event_id}"
            );
            return;
        }
    }

    // Verify the Nostr event signature before forwarding.
    let event_json = match serde_json::to_string(original) {
        Ok(s) => s,
        Err(e) => {
            warn!(target: "mesh_bridge", "[MeshBridge] peer[{peer_idx}]: serialise failed: {e}");
            return;
        }
    };
    let verified = match Event::from_json(&event_json) {
        Err(e) => {
            warn!(target: "mesh_bridge", "[MeshBridge] peer[{peer_idx}]: unparseable event: {e}");
            return;
        }
        Ok(ev) => ev,
    };
    if let Err(e) = verified.verify() {
        warn!(target: "mesh_bridge", "[MeshBridge] peer[{peer_idx}]: bad signature: {e}");
        return;
    }

    let kind_u64 = original["kind"].as_u64().unwrap_or(0);

    let result = if kind_u64 == KIND_BEAD_PROVENANCE {
        // Kind 30001: re-sign as NIP-29 group message (legacy bead path).
        forward_bead_to_forum(peer_idx, original, &event_id, keys, forum_url).await
    } else {
        // Kind 30050 (IS-Envelope) and all other federated kinds: forward as-is.
        forward_raw_to_forum(peer_idx, &verified, &event_id, kind_u64, forum_url).await
    };

    match result {
        Ok(()) => {
            if let Ok(mut guard) = last_event_at.lock() {
                *guard = Some(Instant::now());
            }
        }
        Err(e) => {
            warn!(
                target: "mesh_bridge",
                "[MeshBridge] peer[{peer_idx}]: forward failed event_id={event_id} kind={kind_u64}: {e}"
            );
        }
    }
}

/// Kind 30001 (bead provenance): re-sign as NIP-29 group message and publish.
async fn forward_bead_to_forum(
    peer_idx: usize,
    original: &Value,
    source_event_id: &str,
    keys: &Keys,
    forum_url: &str,
) -> Result<(), String> {
    let bead_id = tag_value(original, "bead_id").unwrap_or("unknown");
    let brief_id = tag_value(original, "brief_id").unwrap_or("-");
    let debrief_path = tag_value(original, "debrief_path").unwrap_or("-");

    let content = format!("bead:{bead_id} brief:{brief_id} path:{debrief_path}");

    let tags = vec![
        Tag::custom(
            TagKind::Custom("h".into()),
            vec!["visionclaw-activity".to_string()],
        ),
        Tag::custom(TagKind::Custom("bead_id".into()), vec![bead_id.to_string()]),
        Tag::custom(
            TagKind::Custom("source_event".into()),
            vec![source_event_id.to_string()],
        ),
    ];

    let event = EventBuilder::new(Kind::Custom(9), &content)
        .tags(tags)
        .sign_with_keys(keys)
        .map_err(|e| format!("peer[{peer_idx}] sign failed: {e}"))?;

    send_event_to_forum(&event, forum_url).await?;

    info!(
        target: "mesh_bridge",
        "[MeshBridge] peer[{peer_idx}]: forwarded bead event_id={source_event_id} bead_id={bead_id}"
    );
    Ok(())
}

/// Forward a verified event as-is to the forum relay (no re-signing).
/// Used for IS-Envelope (kind 30050) and other federated kinds.
async fn forward_raw_to_forum(
    peer_idx: usize,
    event: &Event,
    event_id: &str,
    kind: u64,
    forum_url: &str,
) -> Result<(), String> {
    send_event_to_forum(event, forum_url).await?;

    info!(
        target: "mesh_bridge",
        "[MeshBridge] peer[{peer_idx}]: forwarded raw kind={kind} event_id={event_id}"
    );
    Ok(())
}

/// Send a signed event to the forum relay and wait for OK acknowledgement.
async fn send_event_to_forum(event: &Event, forum_url: &str) -> Result<(), String> {
    let (ws_stream, _) = connect_async(forum_url)
        .await
        .map_err(|e| format!("forum connect: {e}"))?;

    let (mut write, mut read) = ws_stream.split();

    write
        .send(Message::Text(json!(["EVENT", event]).to_string()))
        .await
        .map_err(|e| format!("send failed: {e}"))?;

    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(Ok(Message::Text(txt))) = read.next().await {
            if let Ok(parsed) = serde_json::from_str::<Value>(&txt) {
                if parsed[0] == "OK" {
                    return if parsed[2].as_bool().unwrap_or(false) {
                        Ok(())
                    } else {
                        Err(format!("forum rejected: {}", parsed[3]))
                    };
                }
            }
        }
        Err("forum relay closed without OK".to_string())
    })
    .await
    .map_err(|_| "forum relay timeout".to_string())?
}

/// Recompute the aggregate connected flag and counter from all peer states.
fn recompute_aggregate(
    peer_states: &[Arc<PeerState>],
    aggregate: &Arc<AtomicBool>,
    count: &Arc<AtomicUsize>,
) {
    let connected = peer_states
        .iter()
        .filter(|p| p.connected.load(Ordering::Relaxed))
        .count();
    count.store(connected, Ordering::Relaxed);
    aggregate.store(connected > 0, Ordering::Relaxed);
}

/// Parse `MESH_PEER_RELAYS` env var. Falls back to `JSS_RELAY_URL`.
/// Validates that all URLs start with ws:// or wss://.
fn parse_peer_relay_urls() -> Vec<String> {
    // Try MESH_PEER_RELAYS first.
    let raw = std::env::var("MESH_PEER_RELAYS")
        .ok()
        .filter(|s| !s.is_empty());

    let candidates: Vec<String> = if let Some(raw) = raw {
        raw.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        // Backwards compat: fall back to JSS_RELAY_URL.
        let jss = std::env::var("JSS_RELAY_URL")
            .unwrap_or_else(|_| "ws://jss:3030/relay".to_string());
        if jss.is_empty() {
            return Vec::new();
        }
        vec![jss]
    };

    // Validate schemes.
    let mut valid = Vec::with_capacity(candidates.len());
    for url in candidates {
        if url.starts_with("ws://") || url.starts_with("wss://") {
            valid.push(url);
        } else {
            error!(
                "[MeshBridge] Rejecting peer relay URL with invalid scheme: {url}"
            );
        }
    }
    valid
}

/// Parse `MESH_FEDERATED_KINDS` env var. Falls back to `DEFAULT_FEDERATED_KINDS`.
fn parse_federated_kinds() -> Vec<u64> {
    let raw = std::env::var("MESH_FEDERATED_KINDS")
        .ok()
        .filter(|s| !s.is_empty());

    if let Some(raw) = raw {
        let mut kinds = Vec::new();
        for part in raw.split(',') {
            match part.trim().parse::<u64>() {
                Ok(k) => kinds.push(k),
                Err(e) => {
                    warn!("[MeshBridge] Ignoring invalid kind in MESH_FEDERATED_KINDS: '{part}': {e}");
                }
            }
        }
        if kinds.is_empty() {
            warn!("[MeshBridge] MESH_FEDERATED_KINDS produced no valid kinds, using defaults");
            DEFAULT_FEDERATED_KINDS.to_vec()
        } else {
            kinds
        }
    } else {
        DEFAULT_FEDERATED_KINDS.to_vec()
    }
}

/// Extract the first value of a named tag from a raw Nostr event JSON object.
fn tag_value<'a>(event: &'a Value, tag_name: &str) -> Option<&'a str> {
    event["tags"].as_array()?.iter().find_map(|t| {
        if t[0].as_str() == Some(tag_name) {
            t[1].as_str()
        } else {
            None
        }
    })
}

/// Compute backoff duration for a given iteration (0-indexed).
/// Public for testing only via `#[cfg(test)]`.
fn compute_backoff(iteration: u32) -> u64 {
    let delay = (INITIAL_BACKOFF_SECS as f64) * BACKOFF_MULTIPLIER.powi(iteration as i32);
    (delay as u64).min(MAX_BACKOFF_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Clear both PRD-010 F1 canonical-key env vars so each test sees
    /// a deterministic resolver state regardless of inherited environment.
    fn clear_canonical_keys() {
        std::env::remove_var("VISIONCLAW_NOSTR_PRIVKEY");
        std::env::remove_var("SERVER_NOSTR_PRIVKEY");
    }

    fn clear_mesh_env() {
        std::env::remove_var("MESH_PEER_RELAYS");
        std::env::remove_var("JSS_RELAY_URL");
        std::env::remove_var("MESH_FEDERATED_KINDS");
    }

    // ── from_env ───────────────────────────────────────────────────────

    #[test]
    fn from_env_returns_none_without_required_vars() {
        clear_canonical_keys();
        clear_mesh_env();
        std::env::remove_var("FORUM_RELAY_URL");

        assert!(MeshBridge::from_env().is_none());
    }

    #[test]
    fn from_env_returns_none_without_forum_relay() {
        clear_canonical_keys();
        clear_mesh_env();
        std::env::set_var(
            "VISIONCLAW_NOSTR_PRIVKEY",
            "0000000000000000000000000000000000000000000000000000000000000001",
        );
        std::env::remove_var("FORUM_RELAY_URL");

        let result = MeshBridge::from_env();
        assert!(result.is_none());

        clear_canonical_keys();
    }

    #[test]
    fn from_env_rejects_non_ws_forum_url() {
        clear_canonical_keys();
        clear_mesh_env();
        std::env::set_var(
            "VISIONCLAW_NOSTR_PRIVKEY",
            "0000000000000000000000000000000000000000000000000000000000000001",
        );
        std::env::set_var("FORUM_RELAY_URL", "http://evil.com/forum");

        let result = MeshBridge::from_env();
        assert!(result.is_none());

        clear_canonical_keys();
        std::env::remove_var("FORUM_RELAY_URL");
    }

    // ── parse_peer_relay_urls ──────────────────────────────────────────

    #[test]
    fn parse_peer_relay_urls_reads_mesh_env() {
        clear_mesh_env();
        std::env::set_var(
            "MESH_PEER_RELAYS",
            "ws://peer1:3030/relay, wss://peer2.example.com",
        );

        let urls = parse_peer_relay_urls();
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "ws://peer1:3030/relay");
        assert_eq!(urls[1], "wss://peer2.example.com");

        clear_mesh_env();
    }

    #[test]
    fn parse_peer_relay_urls_falls_back_to_jss() {
        clear_mesh_env();
        std::env::set_var("JSS_RELAY_URL", "ws://jss:3030/relay");

        let urls = parse_peer_relay_urls();
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0], "ws://jss:3030/relay");

        clear_mesh_env();
    }

    #[test]
    fn parse_peer_relay_urls_defaults_jss_when_both_absent() {
        clear_mesh_env();
        // Neither MESH_PEER_RELAYS nor JSS_RELAY_URL set — uses default.
        let urls = parse_peer_relay_urls();
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0], "ws://jss:3030/relay");
    }

    #[test]
    fn parse_peer_relay_urls_rejects_non_ws_schemes() {
        clear_mesh_env();
        std::env::set_var(
            "MESH_PEER_RELAYS",
            "ws://good:3030, http://bad.com, wss://also-good",
        );

        let urls = parse_peer_relay_urls();
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "ws://good:3030");
        assert_eq!(urls[1], "wss://also-good");

        clear_mesh_env();
    }

    // ── parse_federated_kinds ──────────────────────────────────────────

    #[test]
    fn parse_federated_kinds_reads_env() {
        clear_mesh_env();
        std::env::set_var("MESH_FEDERATED_KINDS", "1059,30001,30050");

        let kinds = parse_federated_kinds();
        assert_eq!(kinds, vec![1059, 30001, 30050]);

        clear_mesh_env();
    }

    #[test]
    fn parse_federated_kinds_uses_defaults_when_absent() {
        clear_mesh_env();
        let kinds = parse_federated_kinds();
        assert_eq!(kinds, DEFAULT_FEDERATED_KINDS);
    }

    #[test]
    fn parse_federated_kinds_skips_invalid_values() {
        clear_mesh_env();
        std::env::set_var("MESH_FEDERATED_KINDS", "30001,notanumber,30050");

        let kinds = parse_federated_kinds();
        assert_eq!(kinds, vec![30001, 30050]);

        clear_mesh_env();
    }

    // ── DedupCache ─────────────────────────────────────────────────────

    #[test]
    fn dedup_cache_detects_duplicates() {
        let mut cache = DedupCache::new(16);

        assert!(!cache.is_duplicate("event-1"));
        assert!(cache.is_duplicate("event-1")); // second time = duplicate
        assert!(!cache.is_duplicate("event-2")); // different ID
    }

    #[test]
    fn dedup_cache_respects_capacity() {
        let mut cache = DedupCache::new(2);

        assert!(!cache.is_duplicate("a"));
        assert!(!cache.is_duplicate("b"));
        assert!(!cache.is_duplicate("c")); // evicts "a"
        assert!(!cache.is_duplicate("a")); // "a" was evicted, so it's new again
    }

    // ── tag_value ──────────────────────────────────────────────────────

    #[test]
    fn tag_value_extracts_correct_tag() {
        let event: Value = serde_json::json!({
            "tags": [
                ["bead_id", "bead-42"],
                ["brief_id", "brief-7"],
                ["debrief_path", "/out/debrief"]
            ]
        });

        assert_eq!(tag_value(&event, "bead_id"), Some("bead-42"));
        assert_eq!(tag_value(&event, "brief_id"), Some("brief-7"));
        assert_eq!(tag_value(&event, "debrief_path"), Some("/out/debrief"));
    }

    #[test]
    fn tag_value_returns_none_for_missing_tag() {
        let event: Value = serde_json::json!({
            "tags": [["bead_id", "bead-1"]]
        });

        assert_eq!(tag_value(&event, "nonexistent"), None);
    }

    #[test]
    fn tag_value_returns_none_for_no_tags() {
        let event: Value = serde_json::json!({});
        assert_eq!(tag_value(&event, "bead_id"), None);
    }

    #[test]
    fn tag_value_returns_none_for_empty_tags_array() {
        let event: Value = serde_json::json!({"tags": []});
        assert_eq!(tag_value(&event, "bead_id"), None);
    }

    // ── BridgeHealth ───────────────────────────────────────────────────

    #[test]
    fn bridge_health_is_connected_initially_false() {
        let health = BridgeHealth {
            connected: Arc::new(AtomicBool::new(false)),
            last_event_at: Arc::new(Mutex::new(None)),
            peers_connected: Arc::new(AtomicUsize::new(0)),
            peers_total: 3,
        };

        assert!(!health.is_connected());
        assert_eq!(health.peers_connected(), 0);
        assert_eq!(health.peers_total(), 3);
    }

    #[test]
    fn bridge_health_reflects_connected_state() {
        let connected = Arc::new(AtomicBool::new(true));
        let health = BridgeHealth {
            connected: connected.clone(),
            last_event_at: Arc::new(Mutex::new(None)),
            peers_connected: Arc::new(AtomicUsize::new(2)),
            peers_total: 3,
        };

        assert!(health.is_connected());
        assert_eq!(health.peers_connected(), 2);

        connected.store(false, Ordering::Relaxed);
        assert!(!health.is_connected());
    }

    #[test]
    fn bridge_health_last_event_age_none_initially() {
        let health = BridgeHealth {
            connected: Arc::new(AtomicBool::new(false)),
            last_event_at: Arc::new(Mutex::new(None)),
            peers_connected: Arc::new(AtomicUsize::new(0)),
            peers_total: 1,
        };

        assert!(health.last_event_age_secs().is_none());
    }

    #[test]
    fn bridge_health_last_event_age_returns_elapsed() {
        let health = BridgeHealth {
            connected: Arc::new(AtomicBool::new(true)),
            last_event_at: Arc::new(Mutex::new(Some(Instant::now()))),
            peers_connected: Arc::new(AtomicUsize::new(1)),
            peers_total: 1,
        };

        let age = health.last_event_age_secs().unwrap();
        assert!(age < 2);
    }

    // ── recompute_aggregate ────────────────────────────────────────────

    #[test]
    fn recompute_aggregate_all_disconnected() {
        let states: Vec<Arc<PeerState>> = (0..3)
            .map(|_| {
                Arc::new(PeerState {
                    connected: AtomicBool::new(false),
                })
            })
            .collect();
        let agg = Arc::new(AtomicBool::new(true));
        let count = Arc::new(AtomicUsize::new(99));

        recompute_aggregate(&states, &agg, &count);

        assert!(!agg.load(Ordering::Relaxed));
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn recompute_aggregate_one_connected() {
        let states: Vec<Arc<PeerState>> = vec![
            Arc::new(PeerState {
                connected: AtomicBool::new(false),
            }),
            Arc::new(PeerState {
                connected: AtomicBool::new(true),
            }),
            Arc::new(PeerState {
                connected: AtomicBool::new(false),
            }),
        ];
        let agg = Arc::new(AtomicBool::new(false));
        let count = Arc::new(AtomicUsize::new(0));

        recompute_aggregate(&states, &agg, &count);

        assert!(agg.load(Ordering::Relaxed));
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    // ── Backoff calculation ────────────────────────────────────────────

    #[test]
    fn backoff_doubles_correctly() {
        assert_eq!(compute_backoff(0), 5);
        assert_eq!(compute_backoff(1), 10);
        assert_eq!(compute_backoff(2), 20);
        assert_eq!(compute_backoff(3), 40);
    }

    #[test]
    fn backoff_caps_at_max_backoff_secs() {
        let capped = compute_backoff(10);
        assert_eq!(capped, MAX_BACKOFF_SECS);
    }

    #[test]
    fn backoff_constants_are_consistent() {
        assert!(INITIAL_BACKOFF_SECS < MAX_BACKOFF_SECS);
        assert!(BACKOFF_MULTIPLIER > 1.0);
        assert!(HEALTHY_CONNECTION_SECS > 0);
    }
}
