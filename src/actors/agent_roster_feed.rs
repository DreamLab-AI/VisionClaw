//! Real swarm agents on the ADR-2142 agent pathway: roster in, `0x23` frames out.
//!
//! Agents are not graph nodes. The demo director (`agent_demo_director.rs`) and this feed
//! are the two producers of agent `0x23 AGENT_ACTION` frames, and both clients embody
//! whatever those frames name: the headset's registry and choreography (ADR-2109), the
//! desktop's marker beside the target (ADR-2142 D7). This module is the pure state machine
//! behind `AgentBeamActor`, the real-agent producer: time and rosters in, events out.
//!
//! - **Ids.** A real agent's wire id is `AGENT_NODE_FLAG | REAL_AGENT_BASE | h`, with `h`
//!   the low 25 bits of the repo's content address of its upstream id
//!   ([`crate::uri::content_address`]). The band is the upper half of the 26-bit id space:
//!   disjoint from the demo range `0xD001..=0xD0FF` and from graph node ids (compact
//!   `0..N-1`). A collision with another live agent probes upward inside the band; an agent
//!   keeps its id for as long as it is on the roster.
//! - **Producers.** Each roster names its [`RosterSource`] and retires only the agents it
//!   spawned and no longer names, so two producers never retire each other's agents.
//! - **State frames.** Presence and state ride ordinary `0x23` events whose JSON payload has
//!   a `state` key (`working` | `done` | `retired`) plus `intent` and `name`. Clients draw no
//!   beam for them. A frame on spawn and on every state change, a keep-alive every 5 s (under
//!   the clients' 30 s evidence TTL), and one `retired` when a departed agent's done run
//!   ends.
//! - **Finish.** Live agentbox lists only running tasks (`getActiveTasks` in
//!   `management-api/utils/process-manager.js`), so a finished task just leaves the roster.
//!   Leaving is the finish: `done` at once (unless a `Completed`/`Failed` status already
//!   said so), embodied with `done` keep-alives for [`DONE_RUN_MS`] while the clients run it
//!   to the scene edge, then `retired`. An agent named again during its run is working again.
//! - **Join.** A hub action whose metadata names a roster task (`taskId` / `task_id`) is
//!   re-sourced to that agent's wire id ([`RosterFeed::join`]), and its target becomes the
//!   agent's work node. Every frame for one agent, from either producer path, passes one
//!   strictly-increasing timestamp guard ([`RosterFeed::stamp`]).

use std::collections::HashMap;

use visionclaw_domain::models::edge::Edge;
use visionclaw_domain::models::graph::GraphData;
use visionclaw_domain::models::node::Node;

use crate::services::bots_client::Agent;
use crate::utils::binary_protocol::AgentActionEvent;

/// Agent flag on a wire id (bit 31).
pub const AGENT_NODE_FLAG: u32 = 0x8000_0000;
/// Real agents live in the upper half of the 26-bit id space.
pub const REAL_AGENT_BASE: u32 = 0x0200_0000;
/// The 25 bits a real agent's id varies in.
pub const REAL_AGENT_SPAN: u32 = 0x01FF_FFFF;
/// Keep-alive period, under the clients' 30 s evidence TTL (as the demo's).
pub const KEEPALIVE_MS: u64 = 5_000;
/// How long a finished agent stays embodied before its `retired` frame: the clients' done
/// run (F16: 30 s out to the scene edge, then stop; `agentNudge.ts` `AGENT_DONE_RUN_MS`).
pub const DONE_RUN_MS: u64 = 30_000;
/// `state` payload values.
pub const STATE_WORKING: &str = "working";
pub const STATE_DONE: &str = "done";
pub const STATE_RETIRED: &str = "retired";

/// Which producer sent a roster. Each owns the agents it spawned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RosterSource {
    /// `AgentMonitorActor`, polling the Management API task list.
    ManagementApi,
    /// `BotsClient`, polling the MCP agent list.
    Mcp,
    /// The authenticated `POST /api/bots/update` (dev beam driver).
    Rest,
    /// `POST /api/bots/mock-agents`.
    Mock,
}

/// Whether a roster status means the agent has finished.
pub fn is_finished_status(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "completed" | "failed" | "done"
    )
}

/// The un-probed band id of an agent key: `REAL_AGENT_BASE | low 25 bits of its content
/// address`. No flag bit.
pub fn band_id(key: &str) -> u32 {
    let addr = crate::uri::content_address(key);
    let hex = addr.rsplit('-').next().unwrap_or("0");
    let bits = u64::from_str_radix(hex, 16).unwrap_or(0) as u32;
    REAL_AGENT_BASE | (bits & REAL_AGENT_SPAN)
}

/// Whether a (flagged or bare) wire id is in the real-agent band.
pub fn is_real_agent_id(wire: u32) -> bool {
    (wire & 0x03FF_FFFF) & REAL_AGENT_BASE != 0
}

/// The millisecond clock every agent frame uses (the hub's `to_binary_event` truncation).
pub fn wire_clock(epoch_ms: u64) -> u32 {
    (epoch_ms % u32::MAX as u64) as u32
}

#[derive(Debug, Clone)]
struct Live {
    /// Bare band id (no flag).
    id: u32,
    source: RosterSource,
    agent: Agent,
    done: bool,
    /// Wire-clock ms of the `done` frame: the run is timed from it.
    done_at: u32,
    /// No longer named by its producer: finished, embodied until the run ends.
    left: bool,
    /// Last work node from a joined hub action (0 = none).
    target: u32,
}

/// The real-agent roster and the frames it implies.
pub struct RosterFeed {
    agents: HashMap<String, Live>,
    by_id: HashMap<u32, String>,
    since_keepalive: u64,
    last_ts: HashMap<u32, u32>,
    hasher: fn(&str) -> u32,
}

impl Default for RosterFeed {
    fn default() -> Self {
        Self::new()
    }
}

impl RosterFeed {
    pub fn new() -> Self {
        Self::with_hasher(band_id)
    }

    /// A feed with an injected band hash (tests force collisions with it).
    pub fn with_hasher(hasher: fn(&str) -> u32) -> Self {
        Self {
            agents: HashMap::new(),
            by_id: HashMap::new(),
            since_keepalive: 0,
            last_ts: HashMap::new(),
            hasher,
        }
    }

    /// The flagged wire id of a live agent, by upstream key.
    pub fn wire_id(&self, key: &str) -> Option<u32> {
        self.agents.get(key).map(|l| AGENT_NODE_FLAG | l.id)
    }

    pub fn len(&self) -> usize {
        self.agents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    /// Allocate a band id for a new key: its hash, or the next free id above it.
    fn allocate(&self, key: &str) -> u32 {
        let start = (self.hasher)(key) & REAL_AGENT_SPAN;
        let mut off = start;
        loop {
            let id = REAL_AGENT_BASE | off;
            if !self.by_id.contains_key(&id) {
                return id;
            }
            off = (off + 1) & REAL_AGENT_SPAN;
            if off == start {
                // 33M live agents; unreachable in practice.
                return REAL_AGENT_BASE | start;
            }
        }
    }

    /// A strictly increasing timestamp for one agent's frames, shared by the roster frames
    /// and the joined hub actions so the clients' precedence rule never drops either.
    pub fn stamp(&mut self, wire: u32, now_ts: u32) -> u32 {
        let key = wire & 0x03FF_FFFF;
        let ts = match self.last_ts.get(&key) {
            Some(&last) if now_ts <= last => last.wrapping_add(1),
            _ => now_ts,
        };
        self.last_ts.insert(key, ts);
        ts
    }

    fn state_event(&mut self, key: &str, state: &str, now_ts: u32) -> Option<AgentActionEvent> {
        let l = self.agents.get(key)?;
        let wire = AGENT_NODE_FLAG | l.id;
        let target = l.target;
        let intent = if l.agent.name.is_empty() {
            key.to_string()
        } else {
            l.agent.name.clone()
        };
        let payload = serde_json::json!({
            "state": state,
            "name": l.agent.name,
            "intent": intent,
        })
        .to_string()
        .into_bytes();
        let ts = self.stamp(wire, now_ts);
        Some(AgentActionEvent {
            source_agent_id: wire,
            target_node_id: target,
            action_type: 0,
            timestamp: ts,
            duration_ms: 0,
            payload,
        })
    }

    fn current_state(l: &Live) -> &'static str {
        if l.done {
            STATE_DONE
        } else {
            STATE_WORKING
        }
    }

    /// Apply one producer's roster. Returns the frames to broadcast now. `targets` are the
    /// producer's work nodes by agent id (the mock roster picks real knowledge nodes); they are
    /// recorded before any frame is built, so a spawn frame already names its node.
    pub fn apply(
        &mut self,
        source: RosterSource,
        agents: &[Agent],
        targets: &[(String, u32)],
        now_ts: u32,
    ) -> Vec<AgentActionEvent> {
        let mut out = Vec::new();
        for (key, node) in targets {
            if let Some(l) = self.agents.get_mut(key) {
                l.target = *node;
            }
        }
        let mut named: Vec<&Agent> = Vec::with_capacity(agents.len());
        let mut seen = std::collections::HashSet::with_capacity(agents.len());
        for a in agents {
            if !a.id.is_empty() && seen.insert(a.id.as_str()) {
                named.push(a);
            }
        }

        // An agent its producer no longer names has finished (live agentbox lists only
        // running tasks): `done` now if it was working, then embodied for the done run.
        let mut gone: Vec<String> = self
            .agents
            .iter()
            .filter(|(k, l)| l.source == source && !l.left && !seen.contains(k.as_str()))
            .map(|(k, _)| k.clone())
            .collect();
        gone.sort();
        for k in gone {
            let l = self.agents.get_mut(&k).expect("listed");
            l.left = true;
            if !l.done {
                l.done = true;
                l.done_at = now_ts;
                if let Some(ev) = self.state_event(&k, STATE_DONE, now_ts) {
                    out.push(ev);
                }
            }
        }
        out.extend(self.retire_finished(now_ts));

        for a in named {
            let done = is_finished_status(&a.status);
            match self.agents.get_mut(&a.id) {
                Some(l) => {
                    let changed = l.done != done || l.agent.name != a.name;
                    if done && !l.done {
                        l.done_at = now_ts;
                    }
                    l.done = done;
                    l.left = false;
                    l.agent = a.clone();
                    if changed {
                        let st = Self::current_state(self.agents.get(&a.id).expect("present"));
                        if let Some(ev) = self.state_event(&a.id, st, now_ts) {
                            out.push(ev);
                        }
                    }
                }
                None => {
                    let id = self.allocate(&a.id);
                    self.by_id.insert(id, a.id.clone());
                    self.agents.insert(
                        a.id.clone(),
                        Live {
                            id,
                            source,
                            agent: a.clone(),
                            done,
                            done_at: now_ts,
                            left: false,
                            target: targets
                                .iter()
                                .find(|(k, _)| *k == a.id)
                                .map_or(0, |(_, n)| *n),
                        },
                    );
                    let st = if done { STATE_DONE } else { STATE_WORKING };
                    if let Some(ev) = self.state_event(&a.id, st, now_ts) {
                        out.push(ev);
                    }
                }
            }
        }
        out
    }

    /// Advance the keep-alive clock. Every `KEEPALIVE_MS` it returns every live agent's
    /// current state, so a late client learns the roster and a finished agent stays.
    /// Also retires every departed agent whose done run has ended, on every call.
    pub fn tick(&mut self, dt_ms: u64, now_ts: u32) -> Vec<AgentActionEvent> {
        let mut out = self.retire_finished(now_ts);
        self.since_keepalive += dt_ms;
        if self.since_keepalive < KEEPALIVE_MS {
            return out;
        }
        self.since_keepalive = 0;
        let mut keys: Vec<String> = self.agents.keys().cloned().collect();
        keys.sort();
        out.extend(keys.iter().filter_map(|k| {
            let st = Self::current_state(self.agents.get(k)?);
            self.state_event(k, st, now_ts)
        }));
        out
    }

    /// One `retired` frame for each departed agent whose done run has ended; frees its id.
    fn retire_finished(&mut self, now_ts: u32) -> Vec<AgentActionEvent> {
        let mut over: Vec<String> = self
            .agents
            .iter()
            .filter(|(_, l)| {
                l.left && l.done && u64::from(now_ts.wrapping_sub(l.done_at)) >= DONE_RUN_MS
            })
            .map(|(k, _)| k.clone())
            .collect();
        over.sort();
        let mut out = Vec::with_capacity(over.len());
        for k in over {
            if let Some(ev) = self.state_event(&k, STATE_RETIRED, now_ts) {
                out.push(ev);
            }
            if let Some(l) = self.agents.remove(&k) {
                self.by_id.remove(&l.id);
                self.last_ts.remove(&l.id);
            }
        }
        out
    }

    /// Re-source a hub action to the roster agent it belongs to, when its metadata names a
    /// roster task (`taskId` / `task_id`). Records the action's target as the agent's work
    /// node and stamps it on the agent's clock. Returns whether it joined.
    pub fn join(&mut self, event: &mut AgentActionEvent, metadata: &serde_json::Value) -> bool {
        let task = metadata
            .get("taskId")
            .or_else(|| metadata.get("task_id"))
            .and_then(|v| v.as_str());
        let Some(task) = task else { return false };
        let Some(l) = self.agents.get_mut(task) else {
            return false;
        };
        let wire = AGENT_NODE_FLAG | l.id;
        if event.target_node_id != 0 {
            l.target = event.target_node_id;
        }
        event.source_agent_id = wire;
        event.timestamp = self.stamp(wire, event.timestamp);
        true
    }

    /// The `/bots/data` view: one record per live agent keyed by its bare wire id, with the
    /// roster's fields and the collaboration edges between agents. No positions: agents
    /// are drawn from frames, never from this.
    pub fn view(&self) -> GraphData {
        let mut g = GraphData::new();
        let mut live: Vec<(&String, &Live)> = self.agents.iter().collect();
        live.sort_by_key(|(_, l)| l.id);
        for (key, l) in &live {
            let a = &l.agent;
            let mut n = Node::new_with_id((*key).clone(), Some(l.id));
            n.id = l.id;
            n.label = a.name.clone();
            n.node_type = Some("agent".to_string());
            let m = &mut n.metadata;
            m.insert("type".into(), "agent".into());
            m.insert("is_agent".into(), "true".into());
            m.insert("agent_type".into(), a.agent_type.clone());
            m.insert("status".into(), a.status.clone());
            m.insert("cpu_usage".into(), a.cpu_usage.to_string());
            m.insert("memory_usage".into(), a.memory_usage.to_string());
            m.insert("health".into(), a.health.to_string());
            m.insert("workload".into(), a.workload.to_string());
            if let Some(c) = &a.created_at {
                m.insert("created_at".into(), c.clone());
            }
            if let Some(age) = a.age {
                m.insert("age".into(), age.to_string());
            }
            if let Some(did) = a.did_nostr.as_deref().filter(|d| !d.is_empty()) {
                m.insert("did_nostr".into(), did.to_string());
            }
            if l.target != 0 {
                m.insert("target_node_id".into(), l.target.to_string());
            }
            g.nodes.push(n);
        }
        for (_, s) in &live {
            for (_, t) in &live {
                if s.id == t.id {
                    continue;
                }
                let w = if s.agent.agent_type == "coordinator" || t.agent.agent_type == "coordinator" {
                    0.8
                } else if s.agent.status == "active" && t.agent.status == "active" {
                    0.5
                } else {
                    0.2
                };
                let mut e = Edge::new(s.id, t.id, w);
                e.edge_type = Some("agent_collaboration".to_string());
                let md = e.metadata.get_or_insert_with(HashMap::new);
                md.insert(
                    "communication_type".to_string(),
                    "agent_collaboration".to_string(),
                );
                md.insert("intensity".to_string(), w.to_string());
                g.edges.push(e);
            }
        }
        g
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::binary_protocol::{decode_agent_actions, encode_agent_actions};

    /// An agent exactly as `AgentMonitorActor` builds it from a Management API task
    /// (`agent_monitor_actor.rs:674-693`): the task id is a string.
    fn agent(id: &str, status: &str) -> Agent {
        Agent {
            id: id.to_string(),
            name: format!("agent {id}"),
            agent_type: "coder".to_string(),
            status: status.to_string(),
            x: 0.0,
            y: 0.0,
            z: 0.0,
            cpu_usage: 1.0,
            memory_usage: 2.0,
            health: 100.0,
            workload: 0.5,
            created_at: None,
            age: None,
            did_nostr: None,
        }
    }

    fn state_of(ev: &AgentActionEvent) -> String {
        let v: serde_json::Value = serde_json::from_slice(&ev.payload).unwrap();
        v["state"].as_str().unwrap().to_string()
    }

    /// F8: a producer that knows an agent's work node (the mock roster) has it on the spawn
    /// frame itself, so the desktop marker spawns beside its node instead of jumping there on
    /// the first keep-alive five seconds later.
    #[test]
    fn the_spawn_frame_carries_the_producers_target() {
        let mut f = RosterFeed::new();
        let out = f.apply(
            RosterSource::Mock,
            &[agent("m1", "active")],
            &[("m1".to_string(), 77)],
            1_000);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].target_node_id, 77, "the spawn frame already names the work node");
        // A later roster that names a different node updates it on the next frame.
        let out = f.apply(
            RosterSource::Mock,
            &[agent("m1", "Completed")],
            &[("m1".to_string(), 78)],
            2_000);
        assert_eq!(out[0].target_node_id, 78);
    }

    #[test]
    fn a_spawned_agent_gets_a_working_frame_in_the_real_band() {
        let mut f = RosterFeed::new();
        let out = f.apply(RosterSource::ManagementApi, &[agent("task-1", "Running")], &[], 1_000);
        assert_eq!(out.len(), 1);
        let id = out[0].source_agent_id;
        assert_eq!(id, f.wire_id("task-1").unwrap());
        assert_eq!(id & AGENT_NODE_FLAG, AGENT_NODE_FLAG);
        assert!(is_real_agent_id(id));
        let bare = id & 0x03FF_FFFF;
        assert!(!(0xD001..=0xD0FF).contains(&bare), "never the demo range");
        assert!(bare >= REAL_AGENT_BASE, "never a compact graph node id");
        assert_eq!(state_of(&out[0]), STATE_WORKING);
        assert_eq!(out[0].target_node_id, 0);
    }

    #[test]
    fn the_id_is_stable_across_roster_updates_and_derived_from_the_key() {
        let mut f = RosterFeed::new();
        f.apply(RosterSource::ManagementApi, &[agent("task-1", "Running")], &[], 1_000);
        let first = f.wire_id("task-1").unwrap();
        let out = f.apply(
            RosterSource::ManagementApi,
            &[agent("task-0", "Running"), agent("task-1", "Running")], &[],
            2_000);
        assert_eq!(f.wire_id("task-1"), Some(first));
        assert_eq!(out.len(), 1, "only the new agent gets a frame");
        assert_eq!(first & 0x03FF_FFFF, band_id("task-1"));
        // A fresh feed (a server restart) derives the same id.
        let mut g = RosterFeed::new();
        g.apply(RosterSource::ManagementApi, &[agent("task-1", "Running")], &[], 1);
        assert_eq!(g.wire_id("task-1"), Some(first));
    }

    #[test]
    fn a_hash_collision_probes_upward_and_both_ids_stay_stable() {
        fn same(_: &str) -> u32 {
            REAL_AGENT_BASE | 7
        }
        let mut f = RosterFeed::with_hasher(same);
        f.apply(RosterSource::ManagementApi, &[agent("a", "Running"), agent("b", "Running")], &[], 1);
        assert_eq!(f.wire_id("a"), Some(AGENT_NODE_FLAG | REAL_AGENT_BASE | 7));
        assert_eq!(f.wire_id("b"), Some(AGENT_NODE_FLAG | REAL_AGENT_BASE | 8));
        f.apply(RosterSource::ManagementApi, &[agent("b", "Running"), agent("a", "Running")], &[], 2);
        assert_eq!(f.wire_id("a"), Some(AGENT_NODE_FLAG | REAL_AGENT_BASE | 7));
        assert_eq!(f.wire_id("b"), Some(AGENT_NODE_FLAG | REAL_AGENT_BASE | 8));
    }

    #[test]
    fn finishing_sends_done_and_keepalives_keep_a_finished_agent_present() {
        let mut f = RosterFeed::new();
        f.apply(RosterSource::ManagementApi, &[agent("task-1", "Running")], &[], 1_000);
        let out = f.apply(RosterSource::ManagementApi, &[agent("task-1", "Completed")], &[], 2_000);
        assert_eq!(out.len(), 1);
        assert_eq!(state_of(&out[0]), STATE_DONE);
        // Nothing for 4.9 s, then a keep-alive with the done state; repeated past the TTL.
        assert!(f.tick(4_900, 6_900).is_empty());
        for k in 0..8 {
            let ka = f.tick(5_000, 12_000 + k * 5_000);
            assert_eq!(ka.len(), 1);
            assert_eq!(state_of(&ka[0]), STATE_DONE);
        }
        // An unchanged roster sends nothing.
        assert!(f.apply(RosterSource::ManagementApi, &[agent("task-1", "Completed")], &[], 60_000).is_empty());
    }

    /// Live agentbox lists only `running` tasks (`management-api/utils/process-manager.js`
    /// `getActiveTasks`), so a finishing task simply vanishes from the roster. Leaving the
    /// roster is finishing: `done` at once, present for the whole 30 s run, then one
    /// `retired`, and the id is freed.
    #[test]
    fn leaving_the_roster_is_done_for_the_run_then_one_retired_frame_frees_the_id() {
        let mut f = RosterFeed::new();
        f.apply(RosterSource::ManagementApi, &[agent("task-1", "Running")], &[], 1_000);
        let id = f.wire_id("task-1").unwrap();

        let out = f.apply(RosterSource::ManagementApi, &[], &[], 2_000);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].source_agent_id, id);
        assert_eq!(state_of(&out[0]), STATE_DONE);
        assert_eq!(f.wire_id("task-1"), Some(id), "present for the done run");

        // Keep-alives during the run carry `done`, so the clients' TTL never drops it.
        let ka = f.tick(KEEPALIVE_MS, 7_000);
        assert_eq!(ka.len(), 1);
        assert_eq!(state_of(&ka[0]), STATE_DONE);
        // Still-absent rosters during the run send nothing new.
        assert!(f.apply(RosterSource::ManagementApi, &[], &[], 20_000).is_empty());
        assert!(f.tick(1, 31_999).is_empty(), "not before the run ends");

        let end = f.tick(1, 2_000 + DONE_RUN_MS as u32);
        assert_eq!(end.len(), 1);
        assert_eq!(end[0].source_agent_id, id);
        assert_eq!(state_of(&end[0]), STATE_RETIRED);
        assert_eq!(f.wire_id("task-1"), None);
        assert!(f.tick(KEEPALIVE_MS, 40_000).is_empty());
    }

    /// A roster that reports the finish itself (`Completed`) runs the same course once the
    /// agent leaves: the run is timed from the `done`, not from the removal.
    #[test]
    fn a_completed_agent_that_then_leaves_retires_when_its_run_ends() {
        let mut f = RosterFeed::new();
        f.apply(RosterSource::Rest, &[agent("dev-1", "active")], &[], 1_000);
        let out = f.apply(RosterSource::Rest, &[agent("dev-1", "Completed")], &[], 3_000);
        assert_eq!(state_of(&out[0]), STATE_DONE);
        assert!(f.apply(RosterSource::Rest, &[], &[], 4_000).is_empty(), "already done");
        assert!(f.tick(1, 32_999).is_empty());
        let end = f.tick(1, 3_000 + DONE_RUN_MS as u32);
        assert_eq!(state_of(&end[0]), STATE_RETIRED);
    }

    /// A task that reappears during its run is working again and is not retired.
    #[test]
    fn an_agent_back_in_the_roster_during_its_run_is_working_again() {
        let mut f = RosterFeed::new();
        f.apply(RosterSource::ManagementApi, &[agent("task-1", "Running")], &[], 1_000);
        let id = f.wire_id("task-1").unwrap();
        f.apply(RosterSource::ManagementApi, &[], &[], 2_000);
        let back = f.apply(RosterSource::ManagementApi, &[agent("task-1", "Running")], &[], 5_000);
        assert_eq!(back.len(), 1);
        assert_eq!(state_of(&back[0]), STATE_WORKING);
        assert_eq!(f.wire_id("task-1"), Some(id));
        assert!(f.tick(1, 40_000).iter().all(|e| state_of(e) != STATE_RETIRED));
    }

    #[test]
    fn a_producer_never_retires_another_producers_agents() {
        let mut f = RosterFeed::new();
        f.apply(RosterSource::Rest, &[agent("dev-1", "active")], &[], 1);
        let out = f.apply(RosterSource::ManagementApi, &[agent("task-1", "Running")], &[], 2);
        assert_eq!(out.len(), 1);
        assert_eq!(state_of(&out[0]), STATE_WORKING);
        assert_eq!(f.len(), 2);
        let task = f.wire_id("task-1").unwrap();
        let out = f.apply(RosterSource::ManagementApi, &[], &[], 3);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].source_agent_id, task);
        assert_eq!(state_of(&out[0]), STATE_DONE);
        let end = f.tick(1, 3 + DONE_RUN_MS as u32);
        assert_eq!(end.len(), 1, "only the ManagementApi agent retires");
        assert_eq!(state_of(&end[0]), STATE_RETIRED);
        assert!(f.wire_id("dev-1").is_some());
        assert!(f.wire_id("task-1").is_none());
    }

    #[test]
    fn a_hub_action_naming_a_roster_task_joins_that_agent_on_one_clock() {
        let mut f = RosterFeed::new();
        f.apply(RosterSource::ManagementApi, &[agent("task-1", "Running")], &[], 5_000);
        let mut ev = AgentActionEvent {
            source_agent_id: AGENT_NODE_FLAG | 1003,
            target_node_id: 42,
            action_type: 1,
            timestamp: 5_000, // same millisecond as the spawn frame
            duration_ms: 900,
            payload: Vec::new(),
        };
        assert!(f.join(&mut ev, &serde_json::json!({ "taskId": "task-1" })));
        assert_eq!(ev.source_agent_id, f.wire_id("task-1").unwrap());
        assert!(ev.timestamp > 5_000, "strictly after the spawn frame");
        // Its target is the agent's work node in the next keep-alive.
        let ka = f.tick(KEEPALIVE_MS, 9_000);
        assert_eq!(ka[0].target_node_id, 42);
        // No task id, or an unknown one: untouched.
        let mut other = ev.clone();
        other.source_agent_id = AGENT_NODE_FLAG | 1004;
        assert!(!f.join(&mut other, &serde_json::json!({ "agent": "coder" })));
        assert!(!f.join(&mut other, &serde_json::json!({ "task_id": "task-9" })));
        assert_eq!(other.source_agent_id, AGENT_NODE_FLAG | 1004);
    }

    #[test]
    fn the_view_is_keyed_by_wire_id_and_carries_no_position_source() {
        let mut f = RosterFeed::new();
        let mut a = agent("task-1", "Running");
        a.did_nostr = Some(format!("did:nostr:{}", "a".repeat(64)));
        f.apply(RosterSource::ManagementApi, &[a, agent("task-2", "Running")], &[], 1);
        let v = f.view();
        assert_eq!(v.nodes.len(), 2);
        let n = v.nodes.iter().find(|n| n.metadata_id == "task-1").unwrap();
        assert_eq!(n.id | AGENT_NODE_FLAG, f.wire_id("task-1").unwrap());
        assert!(n.metadata.contains_key("did_nostr"));
        assert_eq!(v.edges.len(), 2);
    }

    /// The shared fixture both clients decode (`tests/fixtures/agents/real-agent-frames.v1.json`):
    /// the real encoder's bytes for a `task-1` spawn, keep-alive, done and retirement. Run with
    /// `UPDATE_FIXTURES=1` to rewrite it after an intended wire change.
    #[test]
    fn the_shared_real_agent_frame_fixture_is_current() {
        let mut f = RosterFeed::new();
        // Evaluated in order: the feed's state advances spawn → keep-alive → done → retired.
        let frames: Vec<(&str, Vec<AgentActionEvent>)> = vec![
            ("spawn", f.apply(RosterSource::ManagementApi, &[agent("task-1", "Running")], &[], 1_000)),
            ("keepalive", f.tick(KEEPALIVE_MS, 6_000)),
            // Live agentbox drops a finished task from its list: leaving is the finish.
            ("done", f.apply(RosterSource::ManagementApi, &[], &[], 7_000)),
            ("retired", f.tick(1, 7_000 + DONE_RUN_MS as u32)),
        ];
        let json_frames: Vec<serde_json::Value> = frames
            .iter()
            .map(|(name, evs)| {
                let frame = encode_agent_actions(evs);
                // The batch payload as the clients' decoders take it: the frame minus its
                // leading message-type byte (`0x23`).
                assert_eq!(frame[0], 0x23);
                let bytes = &frame[1..];
                let decoded = decode_agent_actions(bytes).expect("round-trips");
                assert_eq!(decoded.len(), evs.len());
                let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                serde_json::json!({
                    "name": name,
                    "hex": hex,
                    "events": evs.iter().map(|e| serde_json::json!({
                        "sourceAgentId": e.source_agent_id,
                        "targetNodeId": e.target_node_id,
                        "actionType": e.action_type,
                        "timestamp": e.timestamp,
                        "durationMs": e.duration_ms,
                        "payload": serde_json::from_slice::<serde_json::Value>(&e.payload).unwrap(),
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        let doc = serde_json::json!({
            "schema": "visionclaw.real-agent-frames.v1",
            "agentKey": "task-1",
            "wireId": f_wire_id_for("task-1"),
            "frames": json_frames,
        });
        let text = serde_json::to_string_pretty(&doc).unwrap() + "\n";
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/agents/real-agent-frames.v1.json");
        if std::env::var_os("UPDATE_FIXTURES").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &text).unwrap();
        }
        let committed = std::fs::read_to_string(&path).expect("fixture committed");
        assert_eq!(committed, text, "the real-agent frame fixture is stale; rerun with UPDATE_FIXTURES=1");
    }

    fn f_wire_id_for(key: &str) -> u32 {
        AGENT_NODE_FLAG | band_id(key)
    }
}
