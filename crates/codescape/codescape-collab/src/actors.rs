//! Who is present, what they last did, and where activity is concentrated.
//!
//! The server allocates actor ids from the shared id interner (key
//! `"\u{1}actor:<label>"`), so actors share the node id space, and the binary
//! wire carries them with the agent flag set.

use std::collections::{BTreeMap, HashMap, VecDeque};

use codescape_model::{PresenceEvent, PresenceKind};
use serde::{Deserialize, Serialize};

use crate::heat::{self, HeatField};

/// Default number of events remembered per actor.
pub const DEFAULT_HISTORY_CAP: usize = 256;

/// Records between sweeps that drop cold heat entries.
const PRUNE_EVERY: u32 = 256;

/// Whether an actor is a coding agent or a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    /// An automated coding agent, reported by an activity hook.
    Agent,
    /// A person at a desktop or XR client.
    Human,
}

impl ActorKind {
    /// Parse the `actor_kind` field of a [`PresenceEvent`]. `"human"` (in any
    /// case) is [`Human`](Self::Human); anything else is [`Agent`](Self::Agent).
    pub fn from_wire(s: &str) -> Self {
        if s.eq_ignore_ascii_case("human") {
            Self::Human
        } else {
            Self::Agent
        }
    }

    /// Wire name: `"agent"` or `"human"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Human => "human",
        }
    }
}

/// One present actor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Actor {
    /// Interned actor id.
    pub id: u32,
    /// Display name (`claude:tab0`, `john`…).
    pub label: String,
    /// Agent or human.
    pub kind: ActorKind,
    /// The node this actor last touched or focused, if any.
    pub focus: Option<u32>,
    /// Unix milliseconds of the most recent sighting.
    pub last_seen_ms: u64,
}

/// All known actors, each with a bounded event history, plus the heat field
/// their events feed.
#[derive(Debug, Clone)]
pub struct ActorRegistry {
    actors: BTreeMap<u32, Actor>,
    history: HashMap<u32, VecDeque<PresenceEvent>>,
    heat: HeatField,
    history_cap: usize,
    last_event_ms: Option<u64>,
    since_prune: u32,
}

impl Default for ActorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ActorRegistry {
    /// An empty registry with [`DEFAULT_HISTORY_CAP`] and a 60 s heat half-life.
    pub fn new() -> Self {
        Self::with_history_cap(DEFAULT_HISTORY_CAP)
    }

    /// An empty registry that keeps up to `cap` events per actor (at least one).
    pub fn with_history_cap(cap: usize) -> Self {
        Self {
            actors: BTreeMap::new(),
            history: HashMap::new(),
            heat: HeatField::new(heat::HALF_LIFE_MS),
            history_cap: cap.max(1),
            last_event_ms: None,
            since_prune: 0,
        }
    }

    /// Insert or refresh an actor. Updates the label and kind, and moves
    /// `last_seen_ms` forward but never back. Does not count as an event for
    /// [`stale_after`](Self::stale_after).
    pub fn upsert(&mut self, id: u32, label: &str, kind: ActorKind, now_ms: u64) -> &Actor {
        let a = self.actors.entry(id).or_insert_with(|| Actor {
            id,
            label: label.to_owned(),
            kind,
            focus: None,
            last_seen_ms: now_ms,
        });
        label.clone_into(&mut a.label);
        a.kind = kind;
        a.last_seen_ms = a.last_seen_ms.max(now_ms);
        a
    }

    /// Record one presence event:
    ///
    /// * registers the actor if it is new;
    /// * moves its focus to the first touched node (`0` or an empty `Focus`
    ///   event clears it; `Say` leaves the focus alone);
    /// * adds heat to every touched node (`0` is skipped);
    /// * appends the event to the actor's bounded history.
    pub fn record(&mut self, ev: PresenceEvent) {
        let kind = ActorKind::from_wire(&ev.actor_kind);
        let actor = self.actors.entry(ev.actor).or_insert_with(|| Actor {
            id: ev.actor,
            label: ev.actor_label.clone(),
            kind,
            focus: None,
            last_seen_ms: ev.ts_ms,
        });
        if !ev.actor_label.is_empty() {
            ev.actor_label.clone_into(&mut actor.label);
        }
        actor.last_seen_ms = actor.last_seen_ms.max(ev.ts_ms);
        if ev.kind != PresenceKind::Say {
            match ev.node_ids.first() {
                Some(&0) => actor.focus = None,
                Some(&n) => actor.focus = Some(n),
                None if ev.kind == PresenceKind::Focus => actor.focus = None,
                None => {}
            }
        }

        let amount = heat::intensity(ev.kind);
        for &n in ev.node_ids.iter().filter(|&&n| n != 0) {
            self.heat.add(n, amount, ev.ts_ms);
        }
        self.last_event_ms = Some(self.last_event_ms.map_or(ev.ts_ms, |t| t.max(ev.ts_ms)));

        let log = self.history.entry(ev.actor).or_default();
        log.push_back(ev);
        while log.len() > self.history_cap {
            log.pop_front();
        }

        self.since_prune += 1;
        if self.since_prune >= PRUNE_EVERY {
            self.since_prune = 0;
            if let Some(t) = self.last_event_ms {
                self.heat.prune(t);
            }
        }
    }

    /// Every known actor, in id order.
    pub fn actors(&self) -> impl Iterator<Item = &Actor> {
        self.actors.values()
    }

    /// Number of known actors.
    pub fn len(&self) -> usize {
        self.actors.len()
    }

    /// `true` when no actor is known.
    pub fn is_empty(&self) -> bool {
        self.actors.is_empty()
    }

    /// Forget an actor: its record and its history are dropped and returned.
    ///
    /// Heat is deliberately left alone. The field is shared across actors and
    /// clamped at `1.0`, so one actor's share cannot be recovered after the
    /// fact. Callers evict actors that have been stale for minutes, by which
    /// time a 60 s half-life has already taken their contribution below
    /// [`heat::FLOOR`]; the next [`prune`](heat::HeatField::prune) drops it.
    pub fn remove(&mut self, actor: u32) -> Option<Actor> {
        let removed = self.actors.remove(&actor);
        self.history.remove(&actor);
        removed
    }

    /// One actor by id.
    pub fn get(&self, actor: u32) -> Option<&Actor> {
        self.actors.get(&actor)
    }

    /// Per-node heat at `now_ms`: 60 s half-life, at most `1.0`, only nodes
    /// above [`heat::FLOOR`].
    pub fn heat(&self, now_ms: u64) -> HashMap<u32, f32> {
        self.heat.snapshot(now_ms)
    }

    /// Up to `n` of the actor's most recent events, newest first.
    pub fn recent(&self, actor: u32, n: usize) -> Vec<&PresenceEvent> {
        self.history
            .get(&actor)
            .map(|h| h.iter().rev().take(n).collect())
            .unwrap_or_default()
    }

    /// The node the actor last touched or focused.
    pub fn focus_of(&self, actor: u32) -> Option<u32> {
        self.actors.get(&actor).and_then(|a| a.focus)
    }

    /// `true` when no event from any actor has arrived in the last `ms`
    /// milliseconds, or when none has arrived at all.
    pub fn stale_after(&self, now_ms: u64, ms: u64) -> bool {
        self.last_event_ms
            .is_none_or(|t| now_ms.saturating_sub(t) >= ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(actor: u32, kind: PresenceKind, nodes: &[u32], ts: u64) -> PresenceEvent {
        PresenceEvent {
            actor,
            actor_label: format!("a{actor}"),
            actor_kind: "agent".into(),
            kind,
            node_ids: nodes.to_vec(),
            paths: vec![],
            text: None,
            ts_ms: ts,
        }
    }

    #[test]
    fn heat_decay_numbers() {
        let mut r = ActorRegistry::new();
        r.record(ev(1, PresenceKind::Update, &[10], 0));
        r.record(ev(1, PresenceKind::Query, &[11], 0));
        r.record(ev(2, PresenceKind::Focus, &[12], 0));
        r.record(ev(2, PresenceKind::Say, &[13], 0));
        r.record(ev(2, PresenceKind::Create, &[14], 0));
        r.record(ev(2, PresenceKind::Delete, &[15], 0));
        let h0 = r.heat(0);
        assert_eq!(h0[&10], 1.0);
        assert_eq!(h0[&11], 0.4);
        assert_eq!(h0[&12], 0.2);
        assert_eq!(h0[&13], 0.3);
        assert_eq!(h0[&14], 1.0);
        assert_eq!(h0[&15], 1.0);
        let h60 = r.heat(60_000);
        assert_eq!(h60[&10], 0.5);
        assert_eq!(h60[&11], 0.2);
        assert_eq!(h60[&12], 0.1);
        let h120 = r.heat(120_000);
        assert_eq!(h120[&10], 0.25);
        // Focus 0.2 needs log2(20) ≈ 4.32 half-lives to drop below 0.01.
        assert!(!r.heat(300_000).contains_key(&12));
        assert!(r.heat(300_000).contains_key(&10));
    }

    #[test]
    fn heat_clamps_at_one() {
        let mut r = ActorRegistry::new();
        for t in 0..10 {
            r.record(ev(1, PresenceKind::Update, &[10, 0], t));
        }
        assert_eq!(r.heat(9)[&10], 1.0);
        assert!(!r.heat(9).contains_key(&0), "node 0 is 'none'");
    }

    #[test]
    fn recent_focus_and_staleness() {
        let mut r = ActorRegistry::with_history_cap(3);
        assert!(r.stale_after(0, 1));
        for t in 0..5u32 {
            r.record(ev(1, PresenceKind::Query, &[100 + t], u64::from(t) * 1000));
        }
        let recent: Vec<u64> = r.recent(1, 10).iter().map(|e| e.ts_ms).collect();
        assert_eq!(recent, vec![4000, 3000, 2000]);
        assert_eq!(r.recent(1, 1).len(), 1);
        assert!(r.recent(9, 3).is_empty());
        assert_eq!(r.focus_of(1), Some(104));
        r.record(ev(1, PresenceKind::Say, &[7], 4500));
        assert_eq!(r.focus_of(1), Some(104), "Say does not move focus");
        r.record(ev(1, PresenceKind::Focus, &[], 4600));
        assert_eq!(r.focus_of(1), None);
        assert!(!r.stale_after(5000, 1000));
        assert!(r.stale_after(5600, 1000));
        assert_eq!(r.get(1).unwrap().last_seen_ms, 4600);
    }

    #[test]
    fn upsert_refreshes_without_rewinding() {
        let mut r = ActorRegistry::new();
        r.upsert(5, "john", ActorKind::Human, 100);
        let a = r.upsert(5, "john2", ActorKind::Human, 50);
        assert_eq!(a.label, "john2");
        assert_eq!(a.last_seen_ms, 100);
        assert_eq!(r.actors().count(), 1);
        assert!(r.stale_after(100, 1), "upsert is not an event");
        assert_eq!(ActorKind::from_wire("HUMAN"), ActorKind::Human);
        assert_eq!(ActorKind::from_wire("bot").as_str(), "agent");
    }

    #[test]
    fn prune_keeps_registry_bounded() {
        let mut r = ActorRegistry::new();
        for i in 0..1000u32 {
            r.record(ev(1, PresenceKind::Query, &[i + 1], u64::from(i) * 10_000));
        }
        // Only the last ~6 minutes of nodes can still be warm.
        assert!(r.heat.len() < 300, "len {}", r.heat.len());
    }

    #[test]
    fn remove_drops_actor_and_history_but_not_others() {
        let mut r = ActorRegistry::new();
        r.record(ev(1, PresenceKind::Update, &[10], 0));
        r.record(ev(2, PresenceKind::Update, &[10, 20], 0));
        assert_eq!(r.len(), 2);
        let gone = r.remove(1).expect("actor 1 existed");
        assert_eq!(gone.id, 1);
        assert_eq!(r.len(), 1);
        assert!(r.get(1).is_none());
        assert!(r.recent(1, 5).is_empty());
        assert_eq!(r.recent(2, 5).len(), 1, "other actors keep their history");
        assert_eq!(r.remove(1), None, "second remove is a no-op");
        assert_eq!(r.remove(99), None);
        // Shared heat is untouched by removal (documented semantics).
        assert!(r.heat(0)[&10] >= 1.0 - 1e-6);
        assert!(r.heat(0)[&20] >= 1.0 - 1e-6);
    }

    #[test]
    fn stale_actor_heat_is_below_floor_before_eviction() {
        // Eviction targets actors unseen for 10 minutes; by then the 60 s
        // half-life has decayed their heat by 2^-10, under FLOOR.
        let mut r = ActorRegistry::new();
        r.record(ev(1, PresenceKind::Update, &[10], 0));
        let ten_min = 10 * 60 * 1000;
        assert!(
            !r.heat(ten_min).contains_key(&10),
            "heat must be below FLOOR after 10 min"
        );
        r.remove(1);
        assert!(r.is_empty());
    }
}
