//! Shared agents demo (ADR-2142): ONE server-side producer of `0x23` agent-action frames.
//!
//! The demo used to live twice, once per client, each feeding only its own screen. It is now a
//! single director on the server. It does not touch the graph, the physics, the persistent store
//! or the KPI/provenance hub: it only encodes action frames and hands them to the client
//! coordinator, which fans them out to every connected client, the same door real agent actions
//! leave through. Both clients therefore draw the same six agents working the same real nodes,
//! and either client's button starts or stops it.
//!
//! The synthetic agents play as real agents: nothing they emit says "demo". Provenance lives in
//! code alone (reserved agent ids `0x80000000 | 0xD001..=0xD006`); stopping simply stops the
//! frames, so the clients' own evidence expiry retires them.
//!
//! Loop, per agent, independently seeded so nothing moves in lockstep: stagger in, work a real
//! node 9-14 s (a keep-alive frame every 5 s, under the clients' 30 s evidence TTL), 60 % of the
//! time follow a real edge to a neighbour and work 7-11 s, rest 18-26 s, new target.
//!
//! [`DemoDirector`] is a pure state machine (time in, events out) so it is tested without actors;
//! [`AgentDemoDirector`] is the thin actor around it.

use std::collections::HashMap;
use std::time::Duration;

use actix::prelude::*;
use log::{info, warn};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::Serialize;

use crate::actors::messages::{BroadcastAgentActionFrame, GetGraphData};
use crate::actors::{ClientCoordinatorActor, GraphStateActor};
use crate::utils::binary_protocol::{encode_agent_actions, AgentActionEvent};

/// Agent-node flag on a wire id (the client resolves a beam's source as an agent by this bit).
pub const AGENT_NODE_FLAG: u32 = 0x8000_0000;

/// One synthetic role: its reserved id suffix, label, the `AgentActionType` it emits
/// (0 Query, 1 Update, 2 Create, 3 Link, 4 Delete, 5 Transform) and its caption verb.
#[derive(Debug, Clone, Copy)]
pub struct Role {
    pub suffix: u32,
    pub label: &'static str,
    pub action: u8,
    pub verb: &'static str,
}

pub const ROLES: [Role; 6] = [
    Role { suffix: 0xD001, label: "Architect", action: 2, verb: "Mapping" },
    Role { suffix: 0xD002, label: "Analyst", action: 0, verb: "Analysing" },
    Role { suffix: 0xD003, label: "Coder", action: 1, verb: "Refactoring" },
    Role { suffix: 0xD004, label: "Reviewer", action: 3, verb: "Reviewing" },
    Role { suffix: 0xD005, label: "Tester", action: 0, verb: "Testing" },
    Role { suffix: 0xD006, label: "Optimizer", action: 5, verb: "Optimising" },
];

/// Loop timings in milliseconds.
pub mod timing {
    pub const STAGGER_MS: [u64; 6] = [0, 1_300, 2_900, 4_600, 6_800, 9_100];
    pub const KEEPALIVE_MS: u64 = 5_000;
    pub const WORK_MIN_MS: u64 = 9_000;
    pub const WORK_MAX_MS: u64 = 14_000;
    pub const FOLLOW_MIN_MS: u64 = 7_000;
    pub const FOLLOW_MAX_MS: u64 = 11_000;
    pub const REST_MIN_MS: u64 = 18_000;
    pub const REST_MAX_MS: u64 = 26_000;
    pub const FOLLOW_CHANCE: f64 = 0.6;
    pub const CANDIDATE_POOL: usize = 80;
    /// How long each action's beam animates on the clients.
    pub const ACTION_DURATION_MS: u16 = 2_000;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Wait,
    Work,
    Follow,
    Rest,
}

#[derive(Debug, Clone)]
struct Agent {
    role: Role,
    phase: Phase,
    /// Milliseconds spent in the current phase, and how long it lasts.
    elapsed: u64,
    duration: u64,
    keepalive: u64,
    target: u32,
}

impl Agent {
    /// The wire id of this agent (reserved suffix with the agent flag).
    pub fn wire_id(&self) -> u32 {
        AGENT_NODE_FLAG | self.role.suffix
    }
}

/// The graph facts the director needs: real, publicly visible nodes and their edges.
#[derive(Debug, Clone, Default)]
pub struct DemoGraph {
    /// node id -> label, for every node a demo agent may work on.
    pub labels: HashMap<u32, String>,
    pub edges: Vec<(u32, u32)>,
}

/// The pure state machine.
pub struct DemoDirector {
    agents: Vec<Agent>,
    adjacency: HashMap<u32, Vec<u32>>,
    candidates: Vec<u32>,
    labels: HashMap<u32, String>,
    rng: StdRng,
    last_ts: u32,
}

impl DemoDirector {
    /// Build a director over `graph`. Targets are the highest-degree nodes (the graph's mass
    /// centre), and agents start on well-spread distinct nodes. `None` when the graph offers
    /// nothing to work on.
    pub fn new(graph: &DemoGraph, seed: u64) -> Option<Self> {
        let mut adjacency: HashMap<u32, Vec<u32>> = HashMap::new();
        for &(s, t) in &graph.edges {
            if graph.labels.contains_key(&s) && graph.labels.contains_key(&t) && s != t {
                adjacency.entry(s).or_default().push(t);
                adjacency.entry(t).or_default().push(s);
            }
        }
        let mut ranked: Vec<u32> = graph.labels.keys().copied().collect();
        // Highest degree first; ties by id so a seeded run is reproducible.
        ranked.sort_by_key(|id| (std::cmp::Reverse(adjacency.get(id).map_or(0, Vec::len)), *id));
        ranked.truncate(timing::CANDIDATE_POOL);
        if ranked.is_empty() {
            return None;
        }

        let mut rng = StdRng::seed_from_u64(seed);
        let mut order = ranked.clone();
        for i in (1..order.len()).rev() {
            order.swap(i, rng.gen_range(0..=i));
        }
        let agents = ROLES
            .iter()
            .enumerate()
            .map(|(i, role)| Agent {
                role: *role,
                phase: Phase::Wait,
                elapsed: 0,
                duration: timing::STAGGER_MS[i % timing::STAGGER_MS.len()],
                keepalive: 0,
                target: order[i % order.len()],
            })
            .collect();
        Some(Self { agents, adjacency, candidates: ranked, labels: graph.labels.clone(), rng, last_ts: 0 })
    }

    /// The wire ids of the six synthetic agents.
    pub fn wire_ids() -> [u32; 6] {
        ROLES.map(|r| AGENT_NODE_FLAG | r.suffix)
    }

    /// Whether `wire_id` is in the reserved demo range (flag bit and the node bits both ignored
    /// for the flag test, as clients mask the class bits off).
    pub fn is_demo_id(wire_id: u32) -> bool {
        let masked = wire_id & 0x03FF_FFFF;
        (0xD001..=0xD0FF).contains(&masked)
    }

    /// Advance every agent by `dt_ms`. Returns the action events to broadcast now.
    /// `now_ts` is a millisecond clock; timestamps in the events are strictly increasing.
    pub fn tick(&mut self, dt_ms: u64, now_ts: u32) -> Vec<AgentActionEvent> {
        let mut out = Vec::new();
        for i in 0..self.agents.len() {
            self.agents[i].elapsed += dt_ms;
            match self.agents[i].phase {
                Phase::Wait => {
                    if self.agents[i].elapsed >= self.agents[i].duration {
                        self.begin_work(i, Phase::Work, timing::WORK_MIN_MS, timing::WORK_MAX_MS, now_ts, &mut out);
                    }
                }
                Phase::Work | Phase::Follow => {
                    self.agents[i].keepalive += dt_ms;
                    if self.agents[i].keepalive >= timing::KEEPALIVE_MS {
                        self.agents[i].keepalive = 0;
                        out.push(self.action_event(i, now_ts));
                    }
                    if self.agents[i].elapsed >= self.agents[i].duration {
                        let neighbour = self.neighbour_of(self.agents[i].target);
                        let follow = self.agents[i].phase == Phase::Work
                            && neighbour.is_some()
                            && self.rng.gen::<f64>() < timing::FOLLOW_CHANCE;
                        if follow {
                            self.agents[i].target = neighbour.unwrap_or(self.agents[i].target);
                            self.begin_work(i, Phase::Follow, timing::FOLLOW_MIN_MS, timing::FOLLOW_MAX_MS, now_ts, &mut out);
                        } else {
                            self.agents[i].phase = Phase::Rest;
                            self.agents[i].elapsed = 0;
                            self.agents[i].duration = self.rng.gen_range(timing::REST_MIN_MS..=timing::REST_MAX_MS);
                        }
                    }
                }
                Phase::Rest => {
                    if self.agents[i].elapsed >= self.agents[i].duration {
                        self.agents[i].target = self.next_target(self.agents[i].target);
                        self.begin_work(i, Phase::Work, timing::WORK_MIN_MS, timing::WORK_MAX_MS, now_ts, &mut out);
                    }
                }
            }
        }
        out
    }

    /// What each agent is doing, for status and tests: `(label, phase name, target node)`.
    pub fn snapshot(&self) -> Vec<(&'static str, &'static str, u32)> {
        self.agents
            .iter()
            .map(|a| {
                let p = match a.phase {
                    Phase::Wait => "wait",
                    Phase::Work => "working",
                    Phase::Follow => "follow",
                    Phase::Rest => "rest",
                };
                (a.role.label, p, a.target)
            })
            .collect()
    }

    fn begin_work(&mut self, i: usize, phase: Phase, min: u64, max: u64, now_ts: u32, out: &mut Vec<AgentActionEvent>) {
        let duration = self.rng.gen_range(min..=max);
        let a = &mut self.agents[i];
        a.phase = phase;
        a.elapsed = 0;
        a.duration = duration;
        a.keepalive = 0;
        out.push(self.action_event(i, now_ts));
    }

    fn action_event(&mut self, i: usize, now_ts: u32) -> AgentActionEvent {
        let a = &self.agents[i];
        let label = self.labels.get(&a.target).cloned().unwrap_or_else(|| format!("node {}", a.target));
        let payload = serde_json::json!({ "intent": format!("{}: {}", a.role.verb, label) })
            .to_string()
            .into_bytes();
        // Strictly increasing, so evidence orders coherently next to live evidence.
        let ts = now_ts.max(self.last_ts.wrapping_add(1));
        self.last_ts = ts;
        AgentActionEvent {
            source_agent_id: a.wire_id(),
            target_node_id: a.target,
            action_type: a.role.action,
            timestamp: ts,
            duration_ms: timing::ACTION_DURATION_MS,
            payload,
        }
    }

    fn neighbour_of(&mut self, node: u32) -> Option<u32> {
        let nb = self.adjacency.get(&node)?;
        if nb.is_empty() {
            return None;
        }
        // Prefer a neighbour that is itself a candidate (high degree, central).
        for _ in 0..nb.len().min(8) {
            let pick = nb[self.rng.gen_range(0..nb.len())];
            if pick != node && self.candidates.contains(&pick) {
                return Some(pick);
            }
        }
        let fallback = nb[self.rng.gen_range(0..nb.len())];
        (fallback != node).then_some(fallback)
    }

    fn next_target(&mut self, prev: u32) -> u32 {
        if let Some(nb) = self.neighbour_of(prev) {
            if self.rng.gen::<f64>() < 0.5 {
                return nb;
            }
        }
        self.candidates[self.rng.gen_range(0..self.candidates.len())]
    }
}

// ───────────────────────────── actor ─────────────────────────────

/// Start the demo. Fails when the graph offers nothing to work on.
#[derive(Message)]
#[rtype(result = "Result<DemoStatus, String>")]
pub struct StartAgentDemo;

/// Stop the demo: the frames stop, and the clients' evidence expiry retires the agents.
#[derive(Message)]
#[rtype(result = "DemoStatus")]
pub struct StopAgentDemo;

/// Whether the demo is running.
#[derive(Message)]
#[rtype(result = "DemoStatus")]
pub struct GetAgentDemoStatus;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DemoStatus {
    pub running: bool,
    /// The six reserved agent ids while running (empty when stopped).
    pub agent_ids: Vec<u32>,
}

/// The server actor around [`DemoDirector`].
pub struct AgentDemoDirector {
    graph_state: Addr<GraphStateActor>,
    coordinator: Addr<ClientCoordinatorActor>,
    director: Option<DemoDirector>,
    started: Option<std::time::Instant>,
}

/// How often the director advances.
const TICK: Duration = Duration::from_millis(250);

impl AgentDemoDirector {
    pub fn new(graph_state: Addr<GraphStateActor>, coordinator: Addr<ClientCoordinatorActor>) -> Self {
        Self { graph_state, coordinator, director: None, started: None }
    }

    fn status(&self) -> DemoStatus {
        match self.director {
            Some(_) => DemoStatus { running: true, agent_ids: DemoDirector::wire_ids().to_vec() },
            None => DemoStatus { running: false, agent_ids: Vec::new() },
        }
    }
}

impl Actor for AgentDemoDirector {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.run_interval(TICK, |act, _ctx| {
            let Some(director) = act.director.as_mut() else { return };
            let now_ts = act.started.map_or(0, |t| t.elapsed().as_millis() as u32);
            let events = director.tick(TICK.as_millis() as u64, now_ts);
            if !events.is_empty() {
                act.coordinator.do_send(BroadcastAgentActionFrame(encode_agent_actions(&events)));
            }
        });
    }
}

impl Handler<GetAgentDemoStatus> for AgentDemoDirector {
    type Result = MessageResult<GetAgentDemoStatus>;
    fn handle(&mut self, _: GetAgentDemoStatus, _: &mut Self::Context) -> Self::Result {
        MessageResult(self.status())
    }
}

impl Handler<StopAgentDemo> for AgentDemoDirector {
    type Result = MessageResult<StopAgentDemo>;
    fn handle(&mut self, _: StopAgentDemo, _: &mut Self::Context) -> Self::Result {
        if self.director.take().is_some() {
            self.started = None;
            info!("AgentDemoDirector: stopped");
        }
        MessageResult(self.status())
    }
}

impl Handler<StartAgentDemo> for AgentDemoDirector {
    type Result = ResponseActFuture<Self, Result<DemoStatus, String>>;

    fn handle(&mut self, _: StartAgentDemo, _: &mut Self::Context) -> Self::Result {
        if self.director.is_some() {
            let s = self.status();
            return Box::pin(async move { Ok(s) }.into_actor(self));
        }
        let graph_state = self.graph_state.clone();
        Box::pin(
            async move { graph_state.send(GetGraphData).await }
                .into_actor(self)
                .map(|res, act, _| {
                    let data = res.map_err(|e| format!("graph state unavailable: {e}"))??;
                    let graph = demo_graph_from(&data);
                    let seed = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(1, |d| d.as_nanos() as u64);
                    let director = DemoDirector::new(&graph, seed)
                        .ok_or_else(|| "the graph has no nodes the demo agents can work on".to_string())?;
                    info!("AgentDemoDirector: started over {} nodes", graph.labels.len());
                    act.director = Some(director);
                    act.started = Some(std::time::Instant::now());
                    Ok(act.status())
                }),
        )
    }
}

/// Real, publicly visible, non-agent nodes and the edges among them.
///
/// Private nodes are excluded: the `0x23` frames go to every client, so a frame must never name a
/// node some client is not entitled to see (the position stream withholds those per session).
fn demo_graph_from(data: &visionclaw_domain::models::graph::GraphData) -> DemoGraph {
    let is_agent = |n: &visionclaw_domain::models::node::Node| {
        n.metadata.get("nodeType").is_some_and(|t| t == "agent") || n.metadata.get("type").is_some_and(|t| t == "agent")
    };
    let public = |n: &visionclaw_domain::models::node::Node| {
        n.metadata.get("public").is_some_and(|v| v.eq_ignore_ascii_case("true"))
    };
    let labels: HashMap<u32, String> = data
        .nodes
        .iter()
        .filter(|n| !is_agent(n) && public(n))
        .map(|n| (n.id, n.label.clone()))
        .collect();
    if labels.is_empty() {
        warn!("AgentDemoDirector: no public nodes in the graph");
    }
    DemoGraph { labels, edges: data.edges.iter().map(|e| (e.source, e.target)).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A star around node 1 plus a chain 2-3-4-5, so there are central and peripheral nodes.
    fn graph() -> DemoGraph {
        let labels: HashMap<u32, String> = (1..=30).map(|i| (i, format!("Node {i}"))).collect();
        let mut edges: Vec<(u32, u32)> = (2..=20).map(|i| (1, i)).collect();
        edges.extend([(2, 3), (3, 4), (4, 5), (21, 22)]);
        DemoGraph { labels, edges }
    }

    fn run(seed: u64, ms: u64) -> (Vec<AgentActionEvent>, DemoDirector) {
        let mut d = DemoDirector::new(&graph(), seed).unwrap();
        let mut all = Vec::new();
        let mut t = 0u64;
        while t < ms {
            all.extend(d.tick(250, t as u32));
            t += 250;
        }
        (all, d)
    }

    #[test]
    fn the_six_reserved_ids_carry_the_agent_flag_and_are_recognised() {
        let ids = DemoDirector::wire_ids();
        assert_eq!(ids, [0x8000_D001, 0x8000_D002, 0x8000_D003, 0x8000_D004, 0x8000_D005, 0x8000_D006]);
        assert!(ids.iter().all(|&i| DemoDirector::is_demo_id(i)));
        assert!(DemoDirector::is_demo_id(0xD001), "with the flag bits masked off");
        assert!(!DemoDirector::is_demo_id(0x8000_0001) && !DemoDirector::is_demo_id(0xD100));
    }

    #[test]
    fn agents_stagger_in_on_the_stored_schedule() {
        let mut d = DemoDirector::new(&graph(), 7).unwrap();
        let mut first_action_ms: HashMap<u32, u64> = HashMap::new();
        let mut t = 0u64;
        while t <= 10_000 {
            for e in d.tick(250, t as u32) {
                first_action_ms.entry(e.source_agent_id).or_insert(t);
            }
            t += 250;
        }
        assert_eq!(first_action_ms.len(), 6, "all six are working within the stagger window");
        for (i, id) in DemoDirector::wire_ids().iter().enumerate() {
            let want = timing::STAGGER_MS[i];
            let got = first_action_ms[id];
            // A tick labelled t has already counted its own 250 ms, so the first action lands on the
            // tick whose label is within one tick BEFORE the scheduled time.
            assert!(got + 250 >= want && got < want + 250, "agent {i} first acts at {got} ms, scheduled {want}");
        }
    }

    #[test]
    fn every_event_names_a_real_node_and_a_demo_agent_and_carries_the_intent_caption() {
        let (events, _) = run(3, 120_000);
        assert!(events.len() > 30);
        let ids = DemoDirector::wire_ids();
        for e in &events {
            assert!(ids.contains(&e.source_agent_id));
            assert!((1..=30).contains(&e.target_node_id), "target {} is a real node", e.target_node_id);
            assert!(e.action_type <= 5);
            assert_eq!(e.duration_ms, timing::ACTION_DURATION_MS);
            let v: serde_json::Value = serde_json::from_slice(&e.payload).unwrap();
            let intent = v["intent"].as_str().unwrap();
            assert!(intent.contains(": Node "), "{intent}");
            assert!(!intent.to_lowercase().contains("demo"), "nothing says demo: {intent}");
        }
    }

    #[test]
    fn working_agents_send_a_keepalive_every_five_seconds_under_the_clients_ttl() {
        let (events, _) = run(5, 60_000);
        let id = DemoDirector::wire_ids()[0];
        let ts: Vec<u32> = events.iter().filter(|e| e.source_agent_id == id).map(|e| e.timestamp).collect();
        // within any working stretch consecutive actions are <= keepalive apart (plus one tick)
        let mut worst_within_work = 0;
        for w in ts.windows(2) {
            let gap = w[1] - w[0];
            if gap < 8_000 {
                worst_within_work = worst_within_work.max(gap);
            }
        }
        assert!(worst_within_work > 0 && worst_within_work <= timing::KEEPALIVE_MS as u32 + 250);
    }

    #[test]
    fn timestamps_are_strictly_increasing_even_within_one_tick() {
        let (events, _) = run(9, 60_000);
        for w in events.windows(2) {
            assert!(w[1].timestamp > w[0].timestamp, "{} then {}", w[0].timestamp, w[1].timestamp);
        }
    }

    #[test]
    fn agents_rest_between_jobs_and_a_following_agent_works_a_neighbour_of_its_first_target() {
        let mut d = DemoDirector::new(&graph(), 11).unwrap();
        let mut saw_rest = false;
        let mut saw_follow_on_neighbour = false;
        let adj: HashMap<u32, Vec<u32>> = {
            let mut m: HashMap<u32, Vec<u32>> = HashMap::new();
            for (s, t) in graph().edges {
                m.entry(s).or_default().push(t);
                m.entry(t).or_default().push(s);
            }
            m
        };
        let mut prev: HashMap<&'static str, (&'static str, u32)> = HashMap::new();
        let mut t = 0u64;
        while t < 600_000 {
            d.tick(250, t as u32);
            for (label, phase, target) in d.snapshot() {
                saw_rest |= phase == "rest";
                if let Some(&(pp, pt)) = prev.get(label) {
                    if pp == "working" && phase == "follow" {
                        assert!(adj[&pt].contains(&target), "{label} followed {pt} -> {target}, not an edge");
                        saw_follow_on_neighbour = true;
                    }
                }
                prev.insert(label, (phase, target));
            }
            t += 250;
        }
        assert!(saw_rest, "agents rest");
        assert!(saw_follow_on_neighbour, "agents follow real edges");
    }

    #[test]
    fn targets_are_the_high_degree_nodes() {
        let (events, _) = run(13, 300_000);
        let hub = events.iter().filter(|e| e.target_node_id == 1).count();
        assert!(hub as f64 / events.len() as f64 > 0.05, "the hub (degree 19) is worked often: {hub}/{}", events.len());
    }

    #[test]
    fn the_same_seed_reproduces_the_same_run_and_different_seeds_differ() {
        let sig = |seed| {
            let (e, _) = run(seed, 90_000);
            e.iter().map(|x| (x.source_agent_id, x.target_node_id, x.timestamp)).collect::<Vec<_>>()
        };
        assert_eq!(sig(21), sig(21));
        assert_ne!(sig(21), sig(22));
    }

    #[test]
    fn an_empty_graph_cannot_start_and_a_single_node_graph_still_runs() {
        assert!(DemoDirector::new(&DemoGraph::default(), 1).is_none());
        let g = DemoGraph { labels: HashMap::from([(7, "Only".into())]), edges: vec![] };
        let (events, _) = {
            let mut d = DemoDirector::new(&g, 1).unwrap();
            let mut all = Vec::new();
            for t in 0..400u64 {
                all.extend(d.tick(250, (t * 250) as u32));
            }
            (all, d)
        };
        assert!(!events.is_empty() && events.iter().all(|e| e.target_node_id == 7));
    }

    #[test]
    fn edges_to_filtered_out_nodes_are_ignored() {
        // node 99 is not an allowed target (e.g. private): no edge through it may be followed.
        let g = DemoGraph {
            labels: HashMap::from([(1, "A".into()), (2, "B".into())]),
            edges: vec![(1, 99), (99, 2), (1, 2)],
        };
        let (events, _) = {
            let mut d = DemoDirector::new(&g, 4).unwrap();
            let mut all = Vec::new();
            for t in 0..2400u64 {
                all.extend(d.tick(250, (t * 250) as u32));
            }
            (all, ())
        };
        assert!(events.iter().all(|e| e.target_node_id == 1 || e.target_node_id == 2));
    }

    #[test]
    fn the_frames_decode_with_the_shared_wire_format() {
        let (events, _) = run(2, 30_000);
        let frame = encode_agent_actions(&events[..events.len().min(6)]);
        assert_eq!(frame[0], 0x23);
        assert_eq!(u16::from_le_bytes([frame[1], frame[2]]) as usize, events.len().min(6));
    }
}
