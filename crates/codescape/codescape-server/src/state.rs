//! Application state shared by every route.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use codescape_collab::{FloorArbiter, PathMap};
use codescape_model::events::{FloorState, ServerEvent};
use codescape_model::Tier;
use codescape_server::auth::{AllocLimiter, Verifier};
use codescape_server::config::Config;
use codescape_server::egress::EgressAllowlist;
use codescape_server::origin::OriginPolicy;
use codescape_server::persist::{atomic_write, is_ephemeral_label, ActorIds};
use codescape_server::voice::loom::LoomClient;
use codescape_server::voice::tts::TtsClient;
use parking_lot::Mutex;

use crate::engine::RepoEngine;

/// What one client wants on its binary channel. Shared between a client's
/// `/ws/events` and `/ws/positions` sockets when both carry the same `?session=`.
#[derive(Debug)]
pub struct SharedView {
    inner: Mutex<ViewState>,
    version: AtomicU64,
    /// Frames awaiting ack on the position socket (0 = keeping up).
    pub lag: AtomicU32,
}

/// The view itself.
#[derive(Debug, Clone)]
pub struct ViewState {
    /// LOD tier.
    pub tier: Tier,
    /// Files whose symbols are included.
    pub focus_files: Vec<u32>,
    /// Focused node (SSSP source).
    pub focus: Option<u32>,
}

impl SharedView {
    /// A view at `tier` with no focus.
    pub fn new(tier: Tier) -> Self {
        Self {
            inner: Mutex::new(ViewState {
                tier,
                focus_files: vec![],
                focus: None,
            }),
            version: AtomicU64::new(1),
            lag: AtomicU32::new(0),
        }
    }
    /// Current state and its version.
    pub fn get(&self) -> (ViewState, u64) {
        let g = self.inner.lock();
        (g.clone(), self.version.load(Ordering::Acquire))
    }
    /// Set tier and focus files.
    pub fn set_view(&self, tier: Tier, focus_files: Vec<u32>) {
        let mut g = self.inner.lock();
        g.tier = tier;
        g.focus_files = focus_files;
        self.version.fetch_add(1, Ordering::AcqRel);
    }
    /// Set (or clear with 0) the focus. A focused symbol's file joins the focus files.
    pub fn set_focus(&self, focus: Option<u32>, file_of_focus: Option<u32>) {
        let mut g = self.inner.lock();
        g.focus = focus.filter(|&f| f != 0);
        if let Some(f) = file_of_focus {
            if !g.focus_files.contains(&f) {
                g.focus_files.push(f);
                if g.focus_files.len() > 32 {
                    g.focus_files.remove(0);
                }
            }
        }
        self.version.fetch_add(1, Ordering::AcqRel);
    }
}

/// Who is asking the floor gate to act.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloorCaller<'a> {
    /// The `/ws/voice` connection with this id.
    Voice(u64),
    /// A `/ws/events` connection carrying this `?session=` (if any).
    Events(Option<&'a str>),
}

/// Why the floor gate refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FloorDenied {
    /// The caller does not own this actor's voice connection.
    #[error("the floor belongs to your open voice connection; open /ws/voice with the same session first")]
    NotOwner,
}

/// The voice connection that may move an actor's floor.
#[derive(Debug, Clone, PartialEq, Eq)]
struct VoiceClaim {
    conn: u64,
    session: Option<String>,
}

/// Shortest `?session=` accepted as a link between `/ws/events` and `/ws/voice`:
/// the session key is the capability, so it must not be guessable.
pub const MIN_SESSION_KEY: usize = 16;

/// The single ASR floor, bound to connections rather than labels.
///
/// The arbiter still reports actor ids (that is what clients display), but only
/// the `/ws/voice` connection that claimed an actor may request or release for
/// it. A `/ws/events` socket may act only when it carries the same `?session=`
/// as that open voice connection. Taking another actor's label therefore gets a
/// connection nothing: the label is already claimed (connect refused), or the
/// impersonator holds a claim of its own, which never touches anyone else's hold.
/// Disconnect drops only the connection's own claim and hold.
#[derive(Debug)]
pub struct FloorGate {
    arbiter: FloorArbiter,
    claims: HashMap<u32, VoiceClaim>,
}

impl FloorGate {
    /// A free floor with the given hold cap.
    pub fn new(hold_cap_ms: u64) -> Self {
        Self {
            arbiter: FloorArbiter::new(hold_cap_ms),
            claims: HashMap::new(),
        }
    }

    /// Bind `actor`'s floor to voice connection `conn`. `false` when another connection holds it.
    pub fn claim(&mut self, actor: u32, conn: u64, session: Option<&str>) -> bool {
        match self.claims.get(&actor) {
            Some(c) if c.conn != conn => false,
            _ => {
                let session = session
                    .filter(|s| s.len() >= MIN_SESSION_KEY && s.len() <= 128)
                    .map(str::to_string);
                self.claims.insert(actor, VoiceClaim { conn, session });
                true
            }
        }
    }

    /// `true` when some connection has claimed `actor`.
    pub fn is_claimed(&self, actor: u32) -> bool {
        self.claims.contains_key(&actor)
    }

    /// Voice connection `conn` closed: drop its claim and its hold or queue place.
    /// Returns the new state when anything changed. A different connection's claim is untouched.
    pub fn unclaim(&mut self, actor: u32, conn: u64, now_ms: u64) -> Option<FloorState> {
        if self.claims.get(&actor).is_none_or(|c| c.conn != conn) {
            return None;
        }
        self.claims.remove(&actor);
        let before = self.arbiter.state();
        let after = self.arbiter.release(actor, now_ms);
        (after != before).then_some(after)
    }

    fn authorised(&self, actor: u32, caller: FloorCaller<'_>) -> bool {
        let Some(c) = self.claims.get(&actor) else {
            return false;
        };
        match caller {
            FloorCaller::Voice(conn) => c.conn == conn,
            FloorCaller::Events(session) => {
                session.is_some_and(|s| c.session.as_deref() == Some(s))
            }
        }
    }

    /// Request the floor for `actor`.
    pub fn request(
        &mut self,
        actor: u32,
        caller: FloorCaller<'_>,
        now_ms: u64,
    ) -> Result<FloorState, FloorDenied> {
        if !self.authorised(actor, caller) {
            return Err(FloorDenied::NotOwner);
        }
        Ok(self.arbiter.request(actor, now_ms))
    }

    /// Release (or leave the queue) for `actor`.
    pub fn release(
        &mut self,
        actor: u32,
        caller: FloorCaller<'_>,
        now_ms: u64,
    ) -> Result<FloorState, FloorDenied> {
        if !self.authorised(actor, caller) {
            return Err(FloorDenied::NotOwner);
        }
        Ok(self.arbiter.release(actor, now_ms))
    }

    /// Current state.
    pub fn state(&self) -> FloorState {
        self.arbiter.state()
    }

    /// Current holder.
    pub fn holder(&self) -> Option<u32> {
        self.arbiter.holder()
    }

    /// Enforce the hold cap.
    pub fn tick(&mut self, now_ms: u64) -> Option<FloorState> {
        self.arbiter.tick(now_ms)
    }

    /// Mark the shared ASR busy (another consumer holds nemotron).
    pub fn mark_busy(&mut self, busy: bool) -> FloorState {
        self.arbiter.mark_busy(busy)
    }
}

/// Most open WebSocket connections across the server; beyond, handshakes get 503.
pub const MAX_CONNECTIONS: usize = 256;

/// One counted WebSocket connection; dropping it frees the slot.
#[derive(Debug)]
pub struct ConnGuard(Arc<std::sync::atomic::AtomicUsize>);

impl Drop for ConnGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Take a slot from `counter` unless `max` are in use.
pub fn try_take_slot(
    counter: &Arc<std::sync::atomic::AtomicUsize>,
    max: usize,
) -> Option<ConnGuard> {
    // Plain CAS loop: fetch_update is deprecated on current toolchains and its
    // replacement is above the workspace MSRV.
    let mut n = counter.load(Ordering::Acquire);
    loop {
        if n >= max {
            return None;
        }
        match counter.compare_exchange_weak(n, n + 1, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return Some(ConnGuard(counter.clone())),
            Err(actual) => n = actual,
        }
    }
}

/// Why an actor id could not be handed out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ActorError {
    /// The id range is full.
    #[error("actor id space exhausted")]
    Exhausted,
    /// This address allocated too many new actors in the last hour.
    #[error("too many new actor names from this address; try again later")]
    RateLimited,
}

/// Parse a `ClientEvent::View` tier string.
pub fn parse_tier(s: &str) -> Option<Tier> {
    match s.to_ascii_lowercase().as_str() {
        "l0" | "0" => Some(Tier::L0),
        "l1" | "1" => Some(Tier::L1),
        "l2" | "2" => Some(Tier::L2),
        _ => None,
    }
}

/// Everything a handler can reach.
pub struct AppState {
    /// Configuration.
    pub cfg: Arc<Config>,
    /// Served repositories, in `--repo` order.
    pub repos: Vec<Arc<RepoEngine>>,
    /// Outbound allowlist.
    pub egress: EgressAllowlist,
    /// Request verifier.
    pub verifier: Arc<dyn Verifier>,
    /// Hook path rewriting.
    pub path_map: PathMap,
    /// The single ASR floor (shared by every repo: there is one ASR).
    pub floor: Mutex<FloorGate>,
    /// Persisted actor ids (named actors), shared across repos so the floor sees one id space.
    pub actor_ids: Mutex<ActorIds>,
    /// Per-boot ids for anonymous actors (`guest-*`, `voice-*`), never persisted.
    pub ephemeral_ids: Mutex<ActorIds>,
    /// New-actor allocations per source address.
    pub alloc_limiter: AllocLimiter,
    /// WebSocket handshake origin policy.
    pub origins: OriginPolicy,
    next_conn: AtomicU64,
    open_connections: Arc<std::sync::atomic::AtomicUsize>,
    /// TTS client.
    pub tts: TtsClient,
    /// Loom fallback, when configured.
    pub loom: Option<LoomClient>,
    sessions: Mutex<HashMap<String, Weak<SharedView>>>,
    /// Set on SIGTERM.
    pub shutdown: Arc<AtomicBool>,
    anon: AtomicU64,
}

impl AppState {
    /// Assemble the state.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cfg: Arc<Config>,
        repos: Vec<Arc<RepoEngine>>,
        egress: EgressAllowlist,
        verifier: Arc<dyn Verifier>,
        path_map: PathMap,
        actor_ids: ActorIds,
        origins: OriginPolicy,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        let tts = TtsClient::new(&egress, &cfg.tts_url, &cfg.tts_voice);
        let loom = cfg
            .loom_url
            .as_deref()
            .map(|u| LoomClient::new(&egress, u, &cfg.loom_model));
        Self {
            cfg,
            repos,
            egress,
            verifier,
            path_map,
            floor: Mutex::new(FloorGate::new(codescape_collab::DEFAULT_HOLD_CAP_MS)),
            actor_ids: Mutex::new(actor_ids),
            ephemeral_ids: Mutex::new(ActorIds::ephemeral()),
            alloc_limiter: AllocLimiter::hourly(),
            origins,
            next_conn: AtomicU64::new(1),
            open_connections: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            tts,
            loom,
            sessions: Mutex::new(HashMap::new()),
            shutdown,
            anon: AtomicU64::new(1),
        }
    }

    /// The repo named `label`, or the first one.
    pub fn repo(&self, label: Option<&str>) -> Option<Arc<RepoEngine>> {
        match label {
            Some(l) if !l.is_empty() => self.repos.iter().find(|r| r.label == l).cloned(),
            _ => self.repos.first().cloned(),
        }
    }

    /// The shared view for `session`, or a private one when absent.
    pub fn view_for(&self, session: Option<&str>, default_tier: Tier) -> Arc<SharedView> {
        let Some(s) = session.filter(|s| !s.is_empty() && s.len() <= 128) else {
            return Arc::new(SharedView::new(default_tier));
        };
        let mut map = self.sessions.lock();
        map.retain(|_, w| w.strong_count() > 0);
        if let Some(v) = map.get(s).and_then(Weak::upgrade) {
            return v;
        }
        let v = Arc::new(SharedView::new(default_tier));
        map.insert(s.to_string(), Arc::downgrade(&v));
        v
    }

    /// A slot for a new WebSocket connection, or `None` at [`MAX_CONNECTIONS`].
    pub fn try_connect(&self) -> Option<ConnGuard> {
        try_take_slot(&self.open_connections, MAX_CONNECTIONS)
    }

    /// Open WebSocket connections.
    pub fn connections(&self) -> usize {
        self.open_connections.load(Ordering::Acquire)
    }

    /// A process-unique connection id.
    pub fn conn_id(&self) -> u64 {
        self.next_conn.fetch_add(1, Ordering::Relaxed)
    }

    /// The id already assigned to `label`, without allocating.
    pub fn lookup_actor(&self, label: &str) -> Option<u32> {
        if is_ephemeral_label(label) {
            self.ephemeral_ids.lock().get(label)
        } else {
            self.actor_ids.lock().get(label)
        }
    }

    /// Actor id for a label, allocated on first use. Named labels are persisted to
    /// `actors.bin`; `guest-*`/`voice-*` labels get per-boot ids from a separate
    /// range. A new allocation from a network client (`source`) counts against
    /// that address's hourly cap; known labels and authenticated ingest
    /// (`source = None`) never do.
    pub fn actor_id(
        &self,
        label: &str,
        source: Option<std::net::IpAddr>,
    ) -> Result<u32, ActorError> {
        if let Some(id) = self.lookup_actor(label) {
            return Ok(id);
        }
        if let Some(ip) = source {
            if !self.alloc_limiter.try_take(ip) {
                return Err(ActorError::RateLimited);
            }
        }
        let max_len = self
            .repos
            .iter()
            .map(|r| r.interner_len())
            .max()
            .unwrap_or(0);
        if is_ephemeral_label(label) {
            return self
                .ephemeral_ids
                .lock()
                .get_or_alloc(label, max_len)
                .ok_or(ActorError::Exhausted);
        }
        let mut ids = self.actor_ids.lock();
        let id = ids
            .get_or_alloc(label, max_len)
            .ok_or(ActorError::Exhausted)?;
        let bytes = ids.to_bytes();
        drop(ids);
        if let Err(e) = atomic_write(&self.cfg.data_dir.join("actors.bin"), &bytes) {
            tracing::warn!(error = %e, "persisting actors.bin failed");
        }
        Ok(id)
    }

    /// A fresh anonymous label (`guest-N`).
    pub fn anon_label(&self, prefix: &str) -> String {
        format!("{prefix}-{}", self.anon.fetch_add(1, Ordering::Relaxed))
    }

    /// Broadcast a floor change to every repo.
    pub fn broadcast_floor(&self, state: FloorState) {
        for r in &self.repos {
            r.emit(ServerEvent::Floor(state.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &str = "0123456789abcdef";

    #[test]
    fn only_the_claiming_connection_moves_the_floor() {
        let mut g = FloorGate::new(30_000);
        assert!(g.claim(7, 1, Some(SESSION)));
        assert!(
            !g.claim(7, 2, None),
            "a second connection cannot take the label's floor"
        );
        assert!(matches!(
            g.request(7, FloorCaller::Voice(1), 0),
            Ok(FloorState::Held { holder: 7, .. })
        ));
        assert_eq!(
            g.release(7, FloorCaller::Voice(2), 10),
            Err(FloorDenied::NotOwner),
            "impersonator cannot release"
        );
        assert_eq!(
            g.release(7, FloorCaller::Events(None), 10),
            Err(FloorDenied::NotOwner)
        );
        assert_eq!(
            g.release(7, FloorCaller::Events(Some("guessed-wrong-key")), 10),
            Err(FloorDenied::NotOwner)
        );
        assert_eq!(g.holder(), Some(7));
        assert_eq!(
            g.unclaim(7, 2, 20),
            None,
            "another connection's disconnect leaves the hold alone"
        );
        assert_eq!(g.holder(), Some(7));
        assert!(
            matches!(
                g.release(7, FloorCaller::Events(Some(SESSION)), 30),
                Ok(FloorState::Free)
            ),
            "the linked events socket may release"
        );
    }

    #[test]
    fn events_need_a_linked_open_voice_connection() {
        let mut g = FloorGate::new(30_000);
        assert_eq!(
            g.request(3, FloorCaller::Events(Some(SESSION)), 0),
            Err(FloorDenied::NotOwner),
            "no voice socket yet"
        );
        assert!(g.claim(3, 9, Some("short")));
        assert_eq!(
            g.request(3, FloorCaller::Events(Some("short")), 0),
            Err(FloorDenied::NotOwner),
            "short keys never link"
        );
        assert!(g.claim(3, 9, Some(SESSION)));
        assert!(g.request(3, FloorCaller::Events(Some(SESSION)), 0).is_ok());
    }

    #[test]
    fn disconnect_releases_only_its_own_hold() {
        let mut g = FloorGate::new(30_000);
        g.claim(1, 10, None);
        g.claim(2, 20, None);
        g.request(1, FloorCaller::Voice(10), 0).unwrap();
        g.request(2, FloorCaller::Voice(20), 1).unwrap(); // queued
        assert!(
            matches!(g.unclaim(2, 20, 2), Some(FloorState::Held { holder: 1, ref queue }) if queue.is_empty())
        );
        assert!(!g.is_claimed(2));
        assert!(matches!(g.unclaim(1, 10, 3), Some(FloorState::Free)));
        assert_eq!(
            g.request(1, FloorCaller::Voice(10), 4),
            Err(FloorDenied::NotOwner),
            "a closed connection's claim is gone"
        );
    }

    #[test]
    fn connection_slots_are_capped_and_freed_on_drop() {
        let c = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let a = try_take_slot(&c, 2).unwrap();
        let _b = try_take_slot(&c, 2).unwrap();
        assert!(try_take_slot(&c, 2).is_none(), "full: 503");
        drop(a);
        assert!(
            try_take_slot(&c, 2).is_some(),
            "a closed connection frees its slot"
        );
    }

    #[test]
    fn tiers_parse() {
        assert_eq!(parse_tier("l2"), Some(Tier::L2));
        assert_eq!(parse_tier("L0"), Some(Tier::L0));
        assert_eq!(parse_tier("x"), None);
    }

    #[test]
    fn shared_view_versions_move() {
        let v = SharedView::new(Tier::L1);
        let (_, a) = v.get();
        v.set_focus(Some(5), Some(2));
        let (s, b) = v.get();
        assert!(b > a);
        assert_eq!(s.focus, Some(5));
        assert_eq!(s.focus_files, vec![2]);
        v.set_focus(Some(0), None);
        assert_eq!(v.get().0.focus, None);
    }
}
