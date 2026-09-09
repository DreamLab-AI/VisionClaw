//! Hot-path graph render store (PRD-008 perf endgame).
//!
//! At full density (13,164 nodes / 145,692 edges) the previous GDScript per-frame
//! path — `_hunt_positions` over a 13k Dictionary plus `_update_multimesh` /
//! `_update_edge_multimesh` issuing ~100k `set_instance_*` calls — cost ~90 ms and
//! held the client at 11 fps. This module owns the position store in Rust, runs the
//! per-poll hunt (lerp render→target) in tight `Vec` loops, and packs the two
//! `MultiMesh` instance buffers as flat `PackedFloat32Array`s so GDScript does a
//! single `.buffer =` assignment per phase instead of one API call per instance.
//!
//! Pure (no Godot deps) so the buffer layout, hunt convergence, grabbed-node
//! override and AABB maths are unit-tested directly. `BinaryProtocolClient` wraps
//! it with thin `#[func]` adapters that convert to/from Godot packed arrays.
//!
//! ## MultiMesh buffer layout (must match GraphScene.tscn format flags)
//! * Nodes: `transform_format = TRANSFORM_3D` + `use_colors` + `use_custom_data`
//!   → **20 floats/instance**: 12 transform (row-major 3×4) + 4 colour (RGBA) +
//!   4 custom (INSTANCE_CUSTOM: cen_norm, fold_badge, query_var_flag, 1).
//! * Edges: `TRANSFORM_3D` + `use_custom_data` → **16 floats/instance** (12
//!   transform + 4 custom). The edge shader is uniform-tinted for geometry but
//!   reads INSTANCE_CUSTOM for the relation-type grammar (Wave 2, OntoAir
//!   clean-room). Custom channel map: `.r/.g/.b` reserved (0); **`.a` = relation
//!   style code** — `0.0` untyped (faint), `1.0` typed (solid), `2.0` subclass
//!   (dashed + dimmer). See [`edge_style_code`] for the wire-predicate → code map.
//!
//! The 12-float transform block is Godot's row-major 3×4: for basis columns
//! `c0,c1,c2` and origin `o` it is `[c0.x,c1.x,c2.x,o.x, c0.y,c1.y,c2.y,o.y,
//! c0.z,c1.z,c2.z,o.z]`.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

/// One `search_labels` match. `Ord` is defined so that a GREATER value is a WORSE
/// result (higher rank tier, then lower centrality, then larger id) — a max-heap
/// of these therefore has the worst-so-far on top, which is exactly what a bounded
/// top-k keep-the-best-`max` selection pops. `into_sorted_vec()` then yields the
/// survivors best-first.
#[derive(Clone, Copy)]
struct LabelHit {
    /// 0 = prefix match, 1 = substring match (lower is better).
    rank: u8,
    centrality: f32,
    id: u32,
}

impl LabelHit {
    /// Worse-than ordering: bigger rank is worse; for equal rank, lower centrality
    /// is worse; for equal centrality, larger id is worse (stable tie-break).
    fn worseness(&self, other: &Self) -> Ordering {
        self.rank
            .cmp(&other.rank)
            .then_with(|| {
                // lower centrality = worse = "greater" in this ordering.
                other
                    .centrality
                    .partial_cmp(&self.centrality)
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| self.id.cmp(&other.id))
    }
}

impl Ord for LabelHit {
    fn cmp(&self, other: &Self) -> Ordering {
        self.worseness(other)
    }
}
impl PartialOrd for LabelHit {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for LabelHit {
    fn eq(&self, other: &Self) -> bool {
        self.worseness(other) == Ordering::Equal
    }
}
impl Eq for LabelHit {}

/// Proximity-label metadata for one node (kept small — no full metadata map).
#[derive(Default, Clone)]
struct NodeMeta {
    /// Stable id used as the narrativegoldmine page slug source (falls back to the
    /// slugified label when empty).
    meta_id: String,
    label: String,
    /// Lowercased `label`, precomputed at `set_meta` time so keystroke-driven
    /// `search_labels` never re-lowercases every label per call.
    label_lower: String,
    node_type: String,
    detail: String,
    /// Byte count from `metadata.file_size` (page / ontology_node carry it; 0 when
    /// absent). Feeds the desktop-parity metadata size formula's log-volume term.
    file_size: u64,
}

/// Floats per node instance in the MultiMesh buffer (12 transform + 4 colour + 4 custom).
pub const NODE_STRIDE: usize = 20;

/// Alpha a node settles to while its proximity/grab label overlay is showing.
/// The label sits co-located with the node, so the sphere must read as glass for
/// the text inside it to be legible; 30 % keeps the node's colour and silhouette.
pub const LABEL_FADE_ALPHA: f32 = 0.3;
/// Per-build alpha step of the label fade. `build_node_buffer` runs at ~45 Hz on
/// the desktop-Vive path, so 0.1/build is a ~0.15 s ramp each way — fast enough to
/// track the 4 Hz label cadence, slow enough not to pop.
pub const LABEL_FADE_STEP: f32 = 0.1;
/// Floats per **semantic-plane** edge instance (12 transform only — plane edges
/// are uniform-tinted, no per-instance channel).
pub const EDGE_STRIDE: usize = 12;
/// Floats per **main** edge instance (12 transform + 4 INSTANCE_CUSTOM). The main
/// edge MultiMesh carries the relation-type style in custom `.a` (Wave 2, Feature
/// 4). See [`edge_style_code`].
pub const EDGE_STRIDE_TYPED: usize = 16;

/// Relation-type style code for the edge shader's INSTANCE_CUSTOM.a channel
/// (Wave 2, Feature 4 — OntoAir clean-room relation grammar). Maps a wire
/// predicate to a visual class: `2` = subclass/taxonomy (rendered dashed + dimmer),
/// `1` = typed (any named predicate — solid), `0` = untyped (empty predicate —
/// faint). Case-insensitive; the subclass family is matched on the local name
/// (after the last `/`, `#` or `:`) so both bare and IRI-qualified spellings hit.
pub fn edge_style_code(edge_type: &str) -> u8 {
    let t = edge_type.trim().to_lowercase();
    if t.is_empty() {
        return 0;
    }
    let local = t.rsplit(['/', '#', ':']).next().unwrap_or(t.as_str());
    match local {
        "subclassof" | "subclass_of" | "sub_class_of" | "subclass" | "is_a" | "isa" => 2,
        _ => 1,
    }
}

/// INSTANCE_CUSTOM.a style code for a reasoner-ENTAILED (inferred) edge (Wave 3,
/// asserted/inferred channel). Rendered amber + dashed, distinct from asserted
/// subclass (code 2, lilac-dashed). Extends the Wave-2 code map: 0 untyped, 1
/// typed, 2 subclass, **3 inferred**.
pub const STYLE_INFERRED: u8 = 3;

/// Edge style code with epistemic status folded in. An inferred edge is
/// [`STYLE_INFERRED`] regardless of its predicate (epistemic status is the
/// dominant visual channel); an asserted edge keeps its predicate-derived
/// [`edge_style_code`]. This is the single place the asserted/inferred decision
/// is made, so the wire only needs to carry a per-edge `inferred` bool.
pub fn edge_style_code_prov(edge_type: &str, inferred: bool) -> u8 {
    if inferred {
        STYLE_INFERRED
    } else {
        edge_style_code(edge_type)
    }
}

// Per-node metadata sizing (desktop parity — client/src/features/graph/utils/
// nodeScaling.ts, ontology branch). The desktop composes an ADDITIVE size
// `base + sqrt(degree)*connInfl + log(fileSize+1)*sizeInfl`; in VR we apply the
// same two terms (same 0.8:0.9 ratio and constants) but as a NORMALISED MULTIPLIER
// over the existing centrality band, so a zero-metadata node renders exactly as
// before (multiplier == 1.0) and hubs/large files grow up to a hard cap. The cap
// avoids giant hubs occluding the headset view.
/// Desktop `connectionInfluence` — weight on the sqrt(degree) term.
pub const META_CONN_INFLUENCE: f32 = 0.8;
/// Desktop `sizeInfluence` — weight on the log(file_size+1) volume term.
pub const META_SIZE_INFLUENCE: f32 = 0.9;
/// VR normalisation of the additive desktop term into a multiplier. Keeps the
/// degree:file ratio; tuned so a busy hub roughly doubles and stays under the cap.
pub const META_SIZE_NORM: f32 = 0.15;
/// Hard cap on the metadata multiplier's effect: final size ≤ `size_hi * this`.
pub const META_SIZE_CAP_FACTOR: f32 = 2.0;

/// Wire node id occupies bits 0-25; bits 26-31 are node-type flag bits (mirrors
/// `binary_protocol::NODE_ID_MASK`). Used to strip the `AGENT_NODE_FLAG` high bit
/// off agent/target ids so the registry keys in plain node-id space.
const NODE_ID_MASK: u32 = 0x03FF_FFFF;

/// Node class code for the type show/hide filter (Wave 2, Feature 3).
pub const KIND_KNOWLEDGE: u8 = 0;
pub const KIND_ONTOLOGY: u8 = 1;
pub const KIND_AGENT: u8 = 2;
pub const KIND_OTHER: u8 = 3;

/// Additively merge expansion edges into the flat topology (Feature 1 — the
/// GraphDBViewerWeb additive-merge principle: no rebuild, no re-fit). `flat` is the
/// `[s0,t0,s1,t1,…]` directed pair list; `weights`/`types` are parallel per-edge.
/// A new directed pair is appended only when neither `(s,t)` nor `(t,s)` already
/// exists, so re-expanding a node never duplicates edges. Returns the number of
/// index-pairs (edges) appended — the tail `flat[old_len..]` is exactly the new
/// edges, for the caller to register styles. Self-loops (`s == t`) are skipped.
pub fn append_new_edges(
    flat: &mut Vec<i32>,
    weights: &mut Vec<f32>,
    types: &mut Vec<String>,
    new_pairs: &[i32],
    new_weights: &[f32],
    new_types: &[String],
) -> usize {
    let mut seen: HashSet<(i32, i32)> = HashSet::with_capacity(flat.len() / 2);
    let existing = flat.len() / 2;
    for i in 0..existing {
        let (s, t) = (flat[i * 2], flat[i * 2 + 1]);
        seen.insert(if s <= t { (s, t) } else { (t, s) });
    }
    let mut added = 0usize;
    let n = new_pairs.len() / 2;
    for i in 0..n {
        let s = new_pairs[i * 2];
        let t = new_pairs[i * 2 + 1];
        if s == t {
            continue;
        }
        let key = if s <= t { (s, t) } else { (t, s) };
        if !seen.insert(key) {
            continue;
        }
        flat.push(s);
        flat.push(t);
        weights.push(new_weights.get(i).copied().unwrap_or(1.0));
        types.push(new_types.get(i).cloned().unwrap_or_default());
        added += 1;
    }
    added
}

/// Squared server-space distance under which a fold-transition member is treated
/// as having reached its representative (fold-in) or its target (fold-out) and is
/// dropped from the animation set. Node render radius is ~0.5, so 0.25 (=0.5²)
/// snaps them home just as they visually coincide.
const FOLD_ARRIVE_EPS2: f32 = 0.25;

/// HSV→RGB (all components in `[0,1]`), matching Godot `Color.from_hsv`.
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h6 = (h - h.floor()) * 6.0;
    let i = h6.floor() as i32;
    let f = h6 - i as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    match i.rem_euclid(6) {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// Deterministic per-node colour — a direct port of `graph_scene.gd::_community_color`.
/// Golden-ratio hue walk keyed by community (or node id when community is 0), warm
/// red blend for anomalies. The hue product is done in f64 to match GDScript's
/// double-precision `float` so colours are bit-for-bit consistent with the old path.
pub fn community_color(community_id: u32, anomaly: f32, node_id: u32) -> [f32; 4] {
    let key = if community_id != 0 { community_id } else { node_id };
    let hue = ((key as f64) * 0.618_033_988_75).fract() as f32;
    let mut rgb = hsv_to_rgb(hue, 0.6, 0.95);
    if anomaly > 0.5 {
        let t = ((anomaly - 0.5) * 2.0).clamp(0.0, 0.85);
        let warn = [1.0, 0.15, 0.1];
        for c in 0..3 {
            rgb[c] = rgb[c] + (warn[c] - rgb[c]) * t;
        }
    }
    [rgb[0], rgb[1], rgb[2], 1.0]
}

/// Squared Euclidean distance between two server-space points.
#[inline]
fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    dx * dx + dy * dy + dz * dz
}

/// Row-major 3×4 transform for a uniform-scaled, axis-aligned node at `pos`.
fn node_transform12(size: f32, pos: [f32; 3]) -> [f32; 12] {
    [
        size, 0.0, 0.0, pos[0],
        0.0, size, 0.0, pos[1],
        0.0, 0.0, size, pos[2],
    ]
}

/// Row-major 3×4 transform for a unit Y-cylinder rotated onto the edge `a→b`,
/// scaled `radius` across and to the span along Y, centred at the midpoint. Returns
/// `None` for a degenerate (near-zero length) edge. Ports the `scaled_local` basis
/// and the (anti)parallel-to-UP degenerate handling from `_update_edge_multimesh`.
fn edge_transform12(a: [f32; 3], b: [f32; 3], radius: f32) -> Option<[f32; 12]> {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if len < 0.001 {
        return None;
    }
    let dir = [d[0] / len, d[1] / len, d[2] / len];
    let dp = dir[1]; // UP·dir, UP = (0,1,0)
    // Rotation basis columns c0,c1,c2 mapping local Y onto `dir`.
    let (mut c0, mut c1, mut c2): ([f32; 3], [f32; 3], [f32; 3]);
    if dp > 0.9999 {
        c0 = [1.0, 0.0, 0.0];
        c1 = [0.0, 1.0, 0.0];
        c2 = [0.0, 0.0, 1.0];
    } else if dp < -0.9999 {
        // 180° about X: Y→-Y, Z→-Z.
        c0 = [1.0, 0.0, 0.0];
        c1 = [0.0, -1.0, 0.0];
        c2 = [0.0, 0.0, -1.0];
    } else {
        // axis = normalize(UP × dir) = normalize((dir.z, 0, -dir.x)); angle = acos(dp).
        let ax = [dir[2], 0.0, -dir[0]];
        let al = (ax[0] * ax[0] + ax[1] * ax[1] + ax[2] * ax[2]).sqrt();
        let x = ax[0] / al;
        let y = ax[1] / al;
        let z = ax[2] / al;
        let c = dp.clamp(-1.0, 1.0);
        let s = (1.0 - c * c).max(0.0).sqrt();
        let omc = 1.0 - c;
        // Rodrigues rotation matrix, columns.
        c0 = [c + x * x * omc, y * x * omc + z * s, z * x * omc - y * s];
        c1 = [x * y * omc - z * s, c + y * y * omc, z * y * omc + x * s];
        c2 = [x * z * omc + y * s, y * z * omc - x * s, c + z * z * omc];
    }
    // scaled_local: scale each basis column along the cylinder's own axes.
    for k in 0..3 {
        c0[k] *= radius;
        c1[k] *= len;
        c2[k] *= radius;
    }
    let o = [a[0] + d[0] * 0.5, a[1] + d[1] * 0.5, a[2] + d[2] * 0.5];
    Some([
        c0[0], c1[0], c2[0], o[0],
        c0[1], c1[1], c2[1], o[1],
        c0[2], c1[2], c2[2], o[2],
    ])
}

/// Linear-interpolated percentile of an unsorted slice (q in `[0,1]`); mirrors
/// `graph_scene.gd::_percentile`. Copies before sorting.
fn percentile(values: &[f32], q: f32) -> f32 {
    let n = values.len();
    if n == 0 {
        return 0.0;
    }
    if n == 1 {
        return values[0];
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let rank = q.clamp(0.0, 1.0) * (n as f32 - 1.0);
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    if lo == hi {
        return sorted[lo];
    }
    let frac = rank - lo as f32;
    sorted[lo] + (sorted[hi] - sorted[lo]) * frac
}

/// Dense, compact-indexed node store. Ids are assigned a slot on first sight; all
/// per-node arrays are parallel-indexed by slot for cache-friendly frame loops.
/// Derived agent status channel (Pillar 3). The wire has no `blocked`/`done`,
/// so these are DERIVED: an inbound `0x23` action ⇒ `WORKING`; the JSON `state`
/// channel (`AgentStateUpdate.status` string + `current_task`) refines it via
/// [`RenderStore::set_agent_status`]. Codes are stable for the halo colour LUT
/// and the Swarm-tab roster dot.
pub const AGENT_IDLE: u8 = 0;
pub const AGENT_WORKING: u8 = 1;
pub const AGENT_BLOCKED: u8 = 2;
pub const AGENT_DONE: u8 = 3;

/// Map a server status STRING (free-form `String` on the wire — see
/// `src/services/agent_visualization_protocol.rs`) onto the derived 4-channel
/// status code. The backend emits `idle|busy|active|error|initializing|
/// terminating|offline`; P1 additionally documents `blocked`/`done` as valid
/// values (the field is already an unconstrained `String`, so this needs no
/// server schema change — only that producers may now set those two literals).
pub fn agent_status_code(status: &str) -> u8 {
    match status {
        "busy" | "active" | "working" | "running" => AGENT_WORKING,
        "blocked" | "error" => AGENT_BLOCKED,
        "done" | "terminating" | "offline" => AGENT_DONE,
        // idle | initializing | anything unknown ⇒ idle (fail-quiet)
        _ => AGENT_IDLE,
    }
}

/// Status halo colour (Pillar 3) for an agent node, keyed by the derived status
/// code. Applied in `build_node_buffer` (overriding the community colour) and used
/// by the Swarm roster dot (P5): idle = calm slate, working = the beam's green,
/// blocked = the beam's amber-red, done = cool cyan-white.
pub fn agent_status_color(status: u8) -> [f32; 4] {
    match status {
        AGENT_WORKING => [0.30, 0.90, 0.72, 1.0],
        AGENT_BLOCKED => [1.0, 0.35, 0.20, 1.0],
        AGENT_DONE => [0.60, 0.85, 1.0, 1.0],
        _ => [0.50, 0.58, 0.68, 1.0], // idle / unknown
    }
}

/// Embodiment (Pillar 1): how far, in server space, an active agent node hovers
/// from the node it is working on (node render radius ≈ 0.5, so ~1.5 keeps the
/// agent just outside the target) and how much it lifts above it.
const HOVER_RADIUS: f32 = 1.5;
const HOVER_LIFT: f32 = 0.5;

/// Minimum halo (INSTANCE_CUSTOM.r rim-glow tell) forced on an agent node so its
/// status colour always reads as a glowing inhabitant, regardless of centrality.
const AGENT_HALO_MIN: f32 = 0.7;

/// Deterministic hover point around a target node for an agent, so multiple agents
/// on one node fan out around it instead of stacking. Golden-angle walk keyed by
/// the agent id spreads them evenly on a ring, lifted slightly above the node.
fn agent_hover_offset(target: [f32; 3], agent_id: u32, radius: f32) -> [f32; 3] {
    let a = (agent_id as f32) * 2.399_963_2; // golden angle (rad)
    [
        target[0] + radius * a.cos(),
        target[1] + radius * HOVER_LIFT,
        target[2] + radius * a.sin(),
    ]
}

/// One live agent's work state (Pillar 1-3 data plane). Populated from the
/// binary `0x23 AGENT_ACTION` beam frame (`target_node_id` + `action_type`) and
/// refined by the JSON `state` channel (`status`/`current_task`). Pure data; the
/// hover glide (P2), work beam (P3) and Swarm roster (P5) all read from here.
#[derive(Default, Clone)]
pub struct AgentRec {
    /// KG-space id of the node this agent is currently acting on (plain node id,
    /// no flag bits — matches `position_of`). 0 ⇒ no current target (idle).
    pub target_node_id: u32,
    /// Last `AgentActionType` (0 Query..5 Transform); tints the beam later.
    pub action_type: u8,
    /// Derived status channel (`AGENT_IDLE|WORKING|BLOCKED|DONE`).
    pub status: u8,
    /// Wire timestamp of the last action (server ms, `% u32::MAX` — may wrap).
    pub last_action_ts: u32,
    /// Server-clock timestamp at which the JSON `state` channel last spoke about
    /// this agent (ADR-2034). Zero until a state update arrives. Held separately
    /// from [`last_action_ts`](Self::last_action_ts) so the two channels can be
    /// ordered against each other rather than blindly overwriting.
    pub last_state_ts: u32,
    /// Whether the record has been demoted by [`RenderStore::expire_stale_agents`]
    /// because no evidence arrived within the TTL. Purely informational — the
    /// demotion itself already shows in `status`/`target_node_id`.
    pub expired: bool,
    /// Current task line from the JSON `state` channel (empty until it arrives).
    pub task: String,
}

impl AgentRec {
    /// Timestamp of the newest evidence applied to this record, from either
    /// channel. This is the value an incoming update must beat to be applied.
    pub fn evidence_ts(&self) -> u32 {
        if ts_is_newer(self.last_state_ts, self.last_action_ts) {
            self.last_state_ts
        } else {
            self.last_action_ts
        }
    }
}

/// Wrap-safe "is `candidate` strictly newer than `reference`?" on the server's
/// millisecond clock (ADR-2034).
///
/// Action timestamps are `u32` server milliseconds taken `% u32::MAX`, so they
/// **wrap** roughly every 49.7 days. A naive `a > b` would, at the wrap point,
/// treat every fresh timestamp as ancient and freeze every agent's status. This
/// is RFC 1982 serial-number arithmetic: the difference is interpreted as a
/// signed offset, so `5 > u32::MAX - 5` correctly reads as "newer" while an
/// genuinely old timestamp still reads as older.
pub fn ts_is_newer(candidate: u32, reference: u32) -> bool {
    (candidate.wrapping_sub(reference) as i32) > 0
}

/// Default time-to-live for live agent evidence (ADR-2034). An agent whose newest
/// action or state update is older than this is no longer credible evidence of
/// current work, so [`RenderStore::expire_stale_agents`] demotes it to idle and
/// drops its beam. Thirty seconds is well beyond the action cadence but short
/// enough that a disconnected agent stops claiming to be working.
pub const AGENT_EVIDENCE_TTL_MS: u32 = 30_000;

#[derive(Default)]
pub struct RenderStore {
    id_index: HashMap<u32, usize>,
    ids: Vec<u32>,
    targets: Vec<[f32; 3]>,
    positions: Vec<[f32; 3]>,
    centrality: Vec<f32>,
    color: Vec<[f32; 4]>,
    centrality_max: f32,
    // Drawn subset from the most recent build_node_buffer — the edge builder filters
    // against this so an edge only renders when BOTH endpoints are on screen.
    drawn: HashSet<u32>,
    render_ids: Vec<u32>,
    render_positions: Vec<[f32; 3]>,
    // Node label metadata (from initialGraphLoad), keyed by id — independent of the
    // position store so it survives whichever arrives first.
    meta: HashMap<u32, NodeMeta>,
    // Fold-level ladder (Wave 3, Phase 2). A server-computed fold plan applied as
    // an id→representative remap. `fold_hidden`: L1 low-signal ids suppressed from
    // the draw. `fold_remap`: memberId→representativeId (members hide, their edges
    // re-route to the representative). `fold_badge`: representativeId→collapsed
    // count, surfaced via the INSTANCE_CUSTOM.g channel. Empty maps ⇒ ∅ (no fold).
    // `fold_hidden`/`fold_remap`/`fold_badge` are the EFFECTIVE state actually
    // rendered; they are derived from the raw plan below minus the current
    // query-var lift-outs, and recomputed whenever either input changes.
    fold_hidden: HashSet<u32>,
    fold_remap: HashMap<u32, u32>,
    fold_badge: HashMap<u32, u32>,
    // The RAW server fold plan, retained verbatim so the effective state can be
    // re-derived non-destructively when the query-var set changes (clearing a
    // query var must re-fold the node it had lifted out, with no server refetch).
    raw_fold_hidden: Vec<u32>,
    raw_fold_members: Vec<u32>,
    raw_fold_reps: Vec<u32>,
    // Fold-transition animation (Phase 3). `folding`: memberId→representativeId for
    // members currently lerping INTO their representative (still drawn, in transit);
    // pruned to fully-folded (hidden) once they reach the rep. `unfolding`: members
    // currently lerping OUT of a representative (seeded at the rep, hunting to their
    // real target); pruned once they arrive. Both empty ⇒ steady state (snap). The
    // hunt drives the lerp; the two builders keep in-transit members visible.
    folding: HashMap<u32, u32>,
    unfolding: HashSet<u32>,
    // Visual-query-builder overlay (flagship): node id → variable palette index.
    // Marked nodes are recoloured to a saturated query-palette colour in
    // `build_node_buffer` and flagged in INSTANCE_CUSTOM.b so the node shader can
    // rim-glow them. Kept separate from `color` (community colour) so unmarking a
    // node restores its original colour with no reload.
    query_vars: HashMap<u32, u8>,
    // Co-proximate label fade. `labelled` = ids whose label overlay is showing
    // right now (GDScript replaces it at the label cadence); `label_alpha` = the
    // per-node current alpha, eased toward LABEL_FADE_ALPHA while labelled and
    // back to 1.0 after, pruned once fully opaque again. Any node with an entry is
    // packed into `faded_buf` (the transparent NodesFadedMulti pass) instead of the
    // opaque main buffer, so the opaque pass never needs a transparent material.
    labelled: HashSet<u32>,
    label_alpha: HashMap<u32, f32>,
    faded_buf: Vec<f32>,
    // Wave 2 relation-type grammar (Feature 4). Per-edge style code keyed by
    // directed (source,target); 0 untyped / 1 typed / 2 subclass. Populated from
    // initialGraphLoad and additively extended by radial expansion. Looked up by
    // ORIGINAL (pre-fold-remap) endpoints so a folded rep→rep edge keeps the style
    // of the member edge it stands in for.
    edge_styles: HashMap<(u32, u32), u8>,
    // Wave 2 type show/hide filter (Feature 3). node id → class code (0 knowledge,
    // 1 ontology, 2 agent, 3 other). `type_hidden[c]` hides class c from the draw;
    // an unknown id is always visible. All-false ⇒ everything visible.
    node_kind: HashMap<u32, u8>,
    type_hidden: [bool; 4],
    // Per-node degree = incident edge count (desktop `connectionCountMap`). Computed
    // in one O(E) pass at topology and incremented additively on expansion merge.
    // Drives the sqrt(degree) term of the metadata size formula.
    degree: HashMap<u32, u32>,
    // Agent-swarm data plane (Pillar 1-3). Live agents keyed by masked agent id
    // (node-id space, flag bits stripped). Fed by the binary `0x23 AGENT_ACTION`
    // beam frame and refined by the JSON `state` channel. Read by the hover glide
    // (P2), the work beam (P3) and the Swarm roster (P5). Empty ⇒ no swarm.
    agent_registry: HashMap<u32, AgentRec>,
    // Embodiment anchors (server space): where an agent's *avatar* currently is,
    // published by the scene each frame for agents it embodies. A work beam
    // starts at the anchor when one exists, else at the agent's streamed node
    // position. Lets synthetic (demo) agents and embodied live agents beam from
    // the body the user sees, without ever faking a position frame.
    agent_anchors: HashMap<u32, [f32; 3]>,
    // Monotonic count of agent actions ever ingested — a liveness counter for the
    // P1 diagnostics surface (verifiable from the HP log before any visuals exist).
    agent_actions_total: u64,
    // ADR-2034 precedence/expiry counters: actions and state updates dropped for
    // arriving out of order, and records demoted once their evidence went stale.
    agent_actions_stale: u64,
    agent_states_stale: u64,
    agent_expiries_total: u64,
}

/// Distinct query-variable palette colours before they cycle — matches the client
/// marker's `?v1`,`?v2`,… palette length.
pub const QUERY_PALETTE_LEN: u8 = 8;

/// Saturated overlay colour for a query variable, keyed by palette index. A
/// golden-ratio hue walk (like [`community_color`]) at full saturation/value so a
/// marked `?vN` node reads as deliberately highlighted, not organically coloured.
pub fn query_var_color(palette_idx: u8) -> [f32; 4] {
    let slot = (palette_idx % QUERY_PALETTE_LEN) as f32;
    let hue = ((slot as f64) * 0.618_033_988_75).fract() as f32;
    let rgb = hsv_to_rgb(hue, 0.85, 1.0);
    [rgb[0], rgb[1], rgb[2], 1.0]
}

impl RenderStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.id_index.clear();
        self.ids.clear();
        self.targets.clear();
        self.positions.clear();
        self.centrality.clear();
        self.color.clear();
        self.centrality_max = 0.0;
        self.drawn.clear();
        self.render_ids.clear();
        self.render_positions.clear();
        self.meta.clear();
        self.fold_hidden.clear();
        self.fold_remap.clear();
        self.fold_badge.clear();
        self.raw_fold_hidden.clear();
        self.raw_fold_members.clear();
        self.raw_fold_reps.clear();
        self.folding.clear();
        self.unfolding.clear();
        self.query_vars.clear();
        self.labelled.clear();
        self.label_alpha.clear();
        self.faded_buf.clear();
        self.edge_styles.clear();
        self.node_kind.clear();
        self.type_hidden = [false; 4];
        self.degree.clear();
        self.agent_registry.clear();
        self.agent_anchors.clear();
        self.agent_actions_total = 0;
        self.agent_actions_stale = 0;
        self.agent_states_stale = 0;
        self.agent_expiries_total = 0;
    }

    /// Record a node's `file_size` (bytes) for the metadata size formula. Merges
    /// into the existing meta entry (creating an empty one if the label metadata
    /// has not arrived yet), so ordering of the two feeds does not matter.
    pub fn set_file_size(&mut self, node_id: u32, file_size: u64) {
        self.meta.entry(node_id).or_default().file_size = file_size;
    }

    /// Recompute every node's degree (incident edge count) from the full directed
    /// pair list in one O(E) pass. Both endpoints of each pair gain one. Replaces
    /// any prior degrees — call on a fresh topology.
    pub fn compute_degrees(&mut self, pairs: &[i32]) {
        self.degree.clear();
        self.add_degrees(pairs);
    }

    /// Additively add degree from a pair list (used after an expansion merge so a
    /// newly-attached edge bumps both endpoints without a full recount).
    pub fn add_degrees(&mut self, pairs: &[i32]) {
        let n = pairs.len() / 2;
        for i in 0..n {
            let s = pairs[i * 2] as u32;
            let t = pairs[i * 2 + 1] as u32;
            *self.degree.entry(s).or_insert(0) += 1;
            *self.degree.entry(t).or_insert(0) += 1;
        }
    }

    /// Degree (incident edge count) for a node — 0 when unknown.
    pub fn degree_of(&self, node_id: u32) -> u32 {
        self.degree.get(&node_id).copied().unwrap_or(0)
    }

    /// `file_size` (bytes) for a node — 0 when unknown.
    fn file_size_of(&self, node_id: u32) -> u64 {
        self.meta.get(&node_id).map(|m| m.file_size).unwrap_or(0)
    }

    /// Desktop-parity node size (client `nodeScaling.ts`). `base` is the desktop
    /// `metadata.size` default (1.0 — the XR wire carries no generic size field).
    /// The desktop's additive `sqrt(degree)*connInfl + log(fileSize+1)*sizeInfl`
    /// becomes a VR-normalised MULTIPLIER over the retained centrality band, so a
    /// zero-metadata node (degree 0, no file) yields multiplier 1.0 and renders at
    /// exactly the previous size, while hubs/large files grow up to the cap. All
    /// per-node inputs are the node's OWN (a fold representative sizes by its own
    /// degree/file_size, never the collapsed group's). Result is clamped to
    /// `[scale_comp*size_lo*0.5, scale_comp*size_hi*META_SIZE_CAP_FACTOR]`.
    fn node_size(&self, id: u32, cen_norm: f32, scale_comp: f32, size_lo: f32, size_hi: f32) -> f32 {
        let base = 1.0_f32;
        let degree = self.degree_of(id) as f32;
        let file_size = self.file_size_of(id) as f32;
        let meta_term =
            degree.sqrt() * META_CONN_INFLUENCE + (file_size + 1.0).ln() * META_SIZE_INFLUENCE;
        let meta_factor = base + meta_term * META_SIZE_NORM; // 1.0 for a zero-meta node
        let cen_lerp = size_lo + (size_hi - size_lo) * cen_norm.sqrt();
        let size = scale_comp * cen_lerp * meta_factor;
        let cap = scale_comp * size_hi * META_SIZE_CAP_FACTOR;
        let floor = scale_comp * size_lo * 0.5; // keep tiny nodes ray-pickable
        size.clamp(floor, cap)
    }

    /// Record a node's class code (0 knowledge / 1 ontology / 2 agent / 3 other)
    /// for the type show/hide filter. Set once from the decoded frame's node kind.
    pub fn set_node_kind(&mut self, node_id: u32, class_code: u8) {
        self.node_kind.insert(node_id, class_code);
    }

    // --- Agent-swarm data plane (Pillar 1-3, P1) --------------------------------

    /// Ingest one decoded `0x23 AGENT_ACTION` beam event. `source_agent_id` may
    /// carry the `AGENT_NODE_FLAG` high bit (the server stamps it); it is masked
    /// to node-id space for the registry key. An action is live evidence the agent
    /// is WORKING, so status is set accordingly; the target and action type are
    /// recorded for the hover glide (P2) and work beam (P3). `task` is the optional
    /// intent string extracted from the action payload (empty when absent) — it
    /// only overwrites a prior task line when non-empty, so a bare action never
    /// blanks a task set by the richer JSON `state` channel.
    /// # Precedence and expiry (ADR-2034)
    ///
    /// The two channels — binary `0x23` actions and the JSON `state` channel —
    /// are independent producers writing to one record, so "last writer wins by
    /// arrival order" is wrong: a delayed action could resurrect WORKING after a
    /// completion had already been reported. The contract is instead:
    ///
    /// * Both channels carry a position on the same wrapping server-millisecond
    ///   clock, compared with [`ts_is_newer`].
    /// * An update is applied only if it is **strictly newer than the newest
    ///   evidence already applied from either channel**
    ///   ([`AgentRec::evidence_ts`]). An out-of-order or replayed update is
    ///   dropped whole — it cannot change status, target, action type or task.
    /// * Evidence goes stale: [`expire_stale_agents`](Self::expire_stale_agents)
    ///   demotes a live status whose newest evidence is older than the TTL.
    ///
    /// Returns `true` when the action was applied, `false` when it was dropped
    /// as out of order.
    ///
    /// `source_agent_id` may carry the `AGENT_NODE_FLAG` high bit (the server
    /// stamps it); it is masked to node-id space for the registry key. An applied
    /// action is live evidence the agent is WORKING, so status is set accordingly;
    /// the target and action type are recorded for the hover glide (P2) and work
    /// beam (P3). `task` is the optional intent string extracted from the action
    /// payload (empty when absent) — it only overwrites a prior task line when
    /// non-empty, so a bare action never blanks a task set by the richer JSON
    /// `state` channel.
    pub fn record_agent_action(
        &mut self,
        source_agent_id: u32,
        target_node_id: u32,
        action_type: u8,
        timestamp: u32,
        task: &str,
    ) -> bool {
        let key = source_agent_id & NODE_ID_MASK;
        let rec = self.agent_registry.entry(key).or_default();

        // Reordered completion / replayed action: an action older than the newest
        // evidence must not flip a reported completion back to WORKING.
        let known = rec.target_node_id != 0 || rec.last_action_ts != 0 || rec.last_state_ts != 0;
        if known && !ts_is_newer(timestamp, rec.evidence_ts()) {
            self.agent_actions_stale = self.agent_actions_stale.saturating_add(1);
            return false;
        }

        rec.target_node_id = target_node_id & NODE_ID_MASK;
        rec.action_type = action_type;
        rec.status = AGENT_WORKING;
        rec.last_action_ts = timestamp;
        rec.expired = false;
        if !task.is_empty() {
            rec.task = task.to_owned();
        }
        self.agent_actions_total = self.agent_actions_total.saturating_add(1);
        true
    }

    /// Refine an agent's status + task line from the JSON `state` channel (or a
    /// GDScript caller that parsed it), treating the update as **current** —
    /// i.e. newer than every action already seen. Creates the record if the beam
    /// frame has not been seen yet. An empty `task` leaves the existing task line
    /// untouched.
    ///
    /// The JSON channel carries no timestamp of its own, so "current" is the only
    /// honest reading of an untimestamped update: it is the latest word as of its
    /// arrival. It therefore supersedes evidence already applied, while a *later*
    /// action still supersedes it. Use [`set_agent_state_at`](Self::set_agent_state_at)
    /// when the caller does have a server timestamp and needs strict ordering.
    pub fn set_agent_state(&mut self, agent_id: u32, status: &str, task: &str) -> bool {
        let key = agent_id & NODE_ID_MASK;
        let current = self
            .agent_registry
            .get(&key)
            .map(|r| r.evidence_ts())
            .unwrap_or(0);
        self.set_agent_state_at(agent_id, status, task, current)
    }

    /// Apply a JSON `state` update stamped at a known server time, subject to the
    /// same precedence rule as actions: it is dropped when it is older than the
    /// newest evidence already applied. Returns `true` when applied.
    ///
    /// Note the deliberate asymmetry with [`set_agent_state`](Self::set_agent_state):
    /// there, an equal timestamp still applies (an untimestamped update means
    /// "now"); here, an update must be at least as new as the existing evidence,
    /// so a genuinely older completion report loses to a newer action.
    pub fn set_agent_state_at(
        &mut self,
        agent_id: u32,
        status: &str,
        task: &str,
        timestamp: u32,
    ) -> bool {
        let key = agent_id & NODE_ID_MASK;
        let rec = self.agent_registry.entry(key).or_default();
        let known = rec.target_node_id != 0 || rec.last_action_ts != 0 || rec.last_state_ts != 0;
        if known && timestamp != rec.evidence_ts() && !ts_is_newer(timestamp, rec.evidence_ts()) {
            self.agent_states_stale = self.agent_states_stale.saturating_add(1);
            return false;
        }
        rec.status = agent_status_code(status);
        rec.last_state_ts = timestamp;
        rec.expired = false;
        if !task.is_empty() {
            rec.task = task.to_owned();
        }
        true
    }

    /// Demote agents whose newest evidence is older than `ttl_ms` at `now_ms`
    /// (ADR-2034). A live status (`WORKING`/`BLOCKED`) is only ever *derived*
    /// from evidence that has since gone stale, so holding it indefinitely
    /// renders a disconnected agent as permanently busy with a beam into a node
    /// it may have long since left. Demoted agents become `AGENT_IDLE` with no
    /// target, which removes the beam and stops the hover glide.
    ///
    /// Terminal `AGENT_DONE` is left alone: it is a reported outcome, not a live
    /// claim, so it does not decay. Returns the number of records demoted.
    pub fn expire_stale_agents(&mut self, now_ms: u32, ttl_ms: u32) -> usize {
        let mut demoted = 0usize;
        for rec in self.agent_registry.values_mut() {
            if rec.status != AGENT_WORKING && rec.status != AGENT_BLOCKED {
                continue;
            }
            let age = now_ms.wrapping_sub(rec.evidence_ts());
            // Guard against a clock that has not reached the evidence yet (a
            // future-stamped action): `age` would wrap to a huge value.
            if (age as i32) > 0 && age > ttl_ms {
                rec.status = AGENT_IDLE;
                rec.target_node_id = 0;
                rec.expired = true;
                demoted += 1;
            }
        }
        self.agent_expiries_total = self.agent_expiries_total.saturating_add(demoted as u64);
        demoted
    }

    /// Total actions dropped as out of order since construction (ADR-2034 probe).
    pub fn agent_actions_stale(&self) -> u64 {
        self.agent_actions_stale
    }

    /// Total state updates dropped as out of order since construction.
    pub fn agent_states_stale(&self) -> u64 {
        self.agent_states_stale
    }

    /// Total agent records demoted by [`expire_stale_agents`](Self::expire_stale_agents).
    pub fn agent_expiries_total(&self) -> u64 {
        self.agent_expiries_total
    }

    /// Number of live agents in the registry (Swarm-tab roster size / diagnostics).
    pub fn agent_count(&self) -> usize {
        self.agent_registry.len()
    }

    /// Monotonic total of agent actions ever ingested — data-plane liveness counter.
    pub fn agent_actions_total(&self) -> u64 {
        self.agent_actions_total
    }

    /// Sorted list of live agent ids (stable roster order for the Swarm tab / tests).
    pub fn agent_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.agent_registry.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// Read-only view of one agent's record (None if unknown).
    pub fn agent_rec(&self, agent_id: u32) -> Option<&AgentRec> {
        self.agent_registry.get(&(agent_id & NODE_ID_MASK))
    }

    /// Publish embodiment anchors (server space) for the given agents. Each
    /// `ids[i]` pairs with `positions[i]`; extra entries on either side are
    /// ignored. Ids may carry the agent flag bit. An anchor only matters for the
    /// beam origin — it never touches node positions or physics.
    pub fn set_agent_anchors(&mut self, ids: &[u32], positions: &[[f32; 3]]) {
        for (&id, &pos) in ids.iter().zip(positions.iter()) {
            if pos.iter().all(|c| c.is_finite()) {
                self.agent_anchors.insert(id & NODE_ID_MASK, pos);
            }
        }
    }

    /// Drop the embodiment anchor for one agent (its beam falls back to the
    /// streamed node position, or disappears if the agent has none).
    pub fn clear_agent_anchor(&mut self, agent_id: u32) {
        self.agent_anchors.remove(&(agent_id & NODE_ID_MASK));
    }

    /// Anchor for an agent, if the scene has published one.
    pub fn agent_anchor(&self, agent_id: u32) -> Option<[f32; 3]> {
        self.agent_anchors.get(&(agent_id & NODE_ID_MASK)).copied()
    }

    /// Remove agents from the registry outright (record + anchor). This is the
    /// explicit lifecycle end for agents that will not come back — the demo
    /// director retires its synthetic ids on Stop — as opposed to
    /// [`expire_stale_agents`](Self::expire_stale_agents), which only demotes a
    /// live status. Returns how many records were actually removed.
    pub fn retire_agents(&mut self, ids: &[u32]) -> usize {
        let mut removed = 0usize;
        for &id in ids {
            let key = id & NODE_ID_MASK;
            if self.agent_registry.remove(&key).is_some() {
                removed += 1;
            }
            self.agent_anchors.remove(&key);
        }
        removed
    }

    /// Show or hide a whole node class (Wave 2, Feature 3). Out-of-range codes are
    /// ignored. Hidden-class nodes drop from `build_node_buffer`; their edges then
    /// fail the both-endpoints-drawn test in `build_edge_buffer` and disappear too.
    pub fn set_type_visible(&mut self, class_code: u8, visible: bool) {
        if let Some(slot) = self.type_hidden.get_mut(class_code as usize) {
            *slot = !visible;
        }
    }

    /// Whether a node class is currently visible (unknown/out-of-range ⇒ visible).
    pub fn is_type_visible(&self, class_code: u8) -> bool {
        match self.type_hidden.get(class_code as usize) {
            Some(&hidden) => !hidden,
            None => true,
        }
    }

    /// Whether a specific node is visible under the current type filter. A node
    /// with no recorded kind is treated as visible (fail-open).
    fn node_visible(&self, id: u32) -> bool {
        match self.node_kind.get(&id) {
            Some(&c) => self.is_type_visible(c),
            None => true,
        }
    }

    /// Replace the per-edge style map from parallel `pairs`/`codes` (Feature 4).
    pub fn set_edge_styles(&mut self, pairs: &[i32], codes: &[u8]) {
        self.edge_styles.clear();
        self.merge_edge_styles(pairs, codes);
    }

    /// Additively register per-edge styles (used after an expansion merge) without
    /// dropping the existing map. `codes[i]` styles pair `(pairs[2i],pairs[2i+1])`.
    pub fn merge_edge_styles(&mut self, pairs: &[i32], codes: &[u8]) {
        let n = (pairs.len() / 2).min(codes.len());
        for i in 0..n {
            let s = pairs[i * 2] as u32;
            let t = pairs[i * 2 + 1] as u32;
            self.edge_styles.insert((s, t), codes[i]);
        }
    }

    /// Style code for the edge between `s` and `t` (direction-insensitive lookup;
    /// 0 = untyped when unknown).
    fn edge_style_of(&self, s: u32, t: u32) -> u8 {
        self.edge_styles
            .get(&(s, t))
            .or_else(|| self.edge_styles.get(&(t, s)))
            .copied()
            .unwrap_or(0)
    }

    /// Mark a node as query variable `palette_idx` (recoloured + rim-flagged on the
    /// next `build_node_buffer`). Re-marking updates the palette slot.
    pub fn set_query_var(&mut self, node_id: u32, palette_idx: u8) {
        self.query_vars.insert(node_id, palette_idx);
        self.recompute_effective_fold(); // marking lifts the node out of any fold
    }

    /// Unmark a node (restores its community colour on the next build). No-op when
    /// the node was not marked.
    pub fn clear_query_var(&mut self, node_id: u32) {
        self.query_vars.remove(&node_id);
        self.recompute_effective_fold(); // unmarking re-folds it if the raw plan had it
    }

    /// Clear every query-variable mark (Clear Query).
    pub fn clear_query_vars(&mut self) {
        self.query_vars.clear();
        self.recompute_effective_fold(); // all previously-lifted nodes re-fold
    }

    /// Whether a node is currently marked as a query variable.
    pub fn is_query_var(&self, node_id: u32) -> bool {
        self.query_vars.contains_key(&node_id)
    }

    /// Replace the set of nodes whose label overlay is showing (proximity or
    /// grab). A newly labelled node starts fading to [`LABEL_FADE_ALPHA`] on the
    /// next build; a node dropped from the set fades back and rejoins the opaque
    /// buffer once it reaches 1.0. Pass empty to fade everything back.
    pub fn set_labelled(&mut self, ids: &[u32]) {
        self.labelled.clear();
        self.labelled.extend(ids.iter().copied());
        for &id in ids {
            self.label_alpha.entry(id).or_insert(1.0);
        }
    }

    /// Current label-fade alpha of a node: 1.0 unless it is labelled or still
    /// fading back.
    pub fn label_alpha_of(&self, node_id: u32) -> f32 {
        self.label_alpha.get(&node_id).copied().unwrap_or(1.0)
    }

    /// The transparent-pass node buffer packed by the last `build_node_buffer`:
    /// every labelled / fading node, same [`NODE_STRIDE`] layout, COLOR.a = alpha.
    pub fn faded_node_buffer(&self) -> &[f32] {
        &self.faded_buf
    }

    /// Advance every label fade one step (once per `build_node_buffer`).
    fn step_label_fades(&mut self) {
        let labelled = &self.labelled;
        self.label_alpha.retain(|id, a| {
            if labelled.contains(id) {
                *a = (*a - LABEL_FADE_STEP).max(LABEL_FADE_ALPHA);
                true
            } else {
                *a = (*a + LABEL_FADE_STEP).min(1.0);
                *a < 1.0
            }
        });
    }

    /// Apply a server fold plan. `hidden` are ids to suppress (L1); `members[i]`
    /// folds into `reps[i]` (L2/L3). The per-representative badge count is derived
    /// from the remap. Mismatched-length inputs are zipped to the shorter. Passing
    /// all-empty is equivalent to [`clear_fold_plan`](Self::clear_fold_plan).
    pub fn set_fold_plan(&mut self, hidden: &[u32], members: &[u32], reps: &[u32]) {
        // Retain the raw plan verbatim, then derive the effective state. Keeping the
        // raw plan lets a later query-var change re-derive without a refetch.
        self.raw_fold_hidden = hidden.to_vec();
        self.raw_fold_members = members.to_vec();
        self.raw_fold_reps = reps.to_vec();
        self.recompute_effective_fold();
    }

    /// Rebuild the effective fold state (`fold_hidden`/`fold_remap`/`fold_badge`)
    /// from the raw plan, applying the query-var lift-out: a query-marked node must
    /// never be folded away. The server's `?pinned=` promotion can't see the
    /// client-only `query_vars` marking, so reconcile it here. Idempotent and cheap
    /// (O(plan)); called on every fold-plan set AND every query-var change so
    /// clearing a mark re-folds its node with no server round-trip.
    fn recompute_effective_fold(&mut self) {
        // Snapshot the previous effective remap to diff for animation transitions.
        let old_remap = std::mem::take(&mut self.fold_remap);

        self.fold_hidden.clear();
        for i in 0..self.raw_fold_hidden.len() {
            let id = self.raw_fold_hidden[i];
            if !self.query_vars.contains_key(&id) {
                self.fold_hidden.insert(id);
            }
        }
        self.fold_badge.clear();
        let n = self.raw_fold_members.len().min(self.raw_fold_reps.len());
        for i in 0..n {
            let m = self.raw_fold_members[i];
            let r = self.raw_fold_reps[i];
            // A representative must never be its own member (defensive: skip); a
            // query-marked member is lifted out of the fold, not collapsed.
            if m == r || self.query_vars.contains_key(&m) {
                continue;
            }
            self.fold_remap.insert(m, r);
            *self.fold_badge.entry(r).or_insert(0) += 1;
        }

        // --- Phase 3 animation transitions ---
        // Newly folded members (present now, absent before) lerp INTO their rep.
        for (&m, &r) in self.fold_remap.iter() {
            if !old_remap.contains_key(&m) {
                self.folding.insert(m, r);
                self.unfolding.remove(&m);
            }
        }
        // Newly unfolded members (present before, absent now) grow OUT of their old
        // representative: seed them at the rep's current position and let the hunt
        // ease them to their real target. Collect first (position mutation below
        // can't borrow `self` while iterating `old_remap`).
        let mut to_unfold: Vec<(u32, u32)> = Vec::new();
        for (&m, &old_r) in old_remap.iter() {
            if !self.fold_remap.contains_key(&m) {
                to_unfold.push((m, old_r));
            }
        }
        for (m, old_r) in to_unfold {
            let rp = self.position_of(old_r);
            if let Some(&slot) = self.id_index.get(&m) {
                self.positions[slot] = rp;
            }
            self.folding.remove(&m);
            self.unfolding.insert(m);
        }
    }

    /// Fold badge count for a node — the number of members collapsed into it as a
    /// representative (0 for a plain node or when no fold is active). Drives the
    /// "(+N)" proximity-label suffix.
    pub fn badge_of(&self, node_id: u32) -> u32 {
        self.fold_badge.get(&node_id).copied().unwrap_or(0)
    }

    /// Clear any active fold plan (return to full density ∅), raw plan included.
    /// Routed through `recompute_effective_fold` so the members currently folded
    /// animate OUT (grow from their representative) rather than snapping back.
    pub fn clear_fold_plan(&mut self) {
        self.raw_fold_hidden.clear();
        self.raw_fold_members.clear();
        self.raw_fold_reps.clear();
        self.recompute_effective_fold();
    }

    /// Representative a node id renders as under the active fold plan — itself when
    /// not a folded member.
    #[inline]
    fn fold_target(&self, id: u32) -> u32 {
        self.fold_remap.get(&id).copied().unwrap_or(id)
    }

    /// Edge-endpoint resolution under the fold plan: a member animating IN or OUT
    /// is drawn as itself, so its edges attach to the live member (they visibly
    /// shrink/grow through the transition); a fully-folded member reroutes to its
    /// representative. Steady-state ⇒ identical to [`fold_target`](Self::fold_target).
    #[inline]
    fn edge_endpoint(&self, id: u32) -> u32 {
        if self.folding.contains_key(&id) || self.unfolding.contains(&id) {
            id
        } else {
            self.fold_target(id)
        }
    }

    /// Map a ranked id list to its visible representatives under the active fold
    /// plan: drop L1-hidden ids, replace a folded member with its representative,
    /// and dedup while preserving order. Identity (and allocation-free clone of
    /// order) when no fold is active. Used by `search_labels` / `nodes_near` so a
    /// folded member can never be a search-teleport or proximity-label target at an
    /// invisible position — the caller lands on the visible representative instead.
    fn canonical_visible(&self, ids: Vec<u32>) -> Vec<u32> {
        if self.fold_hidden.is_empty() && self.fold_remap.is_empty() {
            return ids;
        }
        let mut out = Vec::with_capacity(ids.len());
        let mut seen: HashSet<u32> = HashSet::new();
        for id in ids {
            if self.fold_hidden.contains(&id) {
                continue;
            }
            let rep = self.fold_target(id);
            if self.fold_hidden.contains(&rep) {
                continue;
            }
            if seen.insert(rep) {
                out.push(rep);
            }
        }
        out
    }

    /// Top `max` nodes by centrality (highest first), resolved to visible
    /// representatives under the active fold plan and deduped. Only nodes that
    /// carry a non-empty label are eligible — this drives the wand search-teleport
    /// "top labels" radial (Wave 2, Feature 2), which needs something to show.
    /// Stable id order breaks centrality ties.
    pub fn top_by_centrality(&self, max: usize) -> Vec<u32> {
        if max == 0 {
            return Vec::new();
        }
        let mut ranked: Vec<(f32, u32)> = Vec::with_capacity(self.ids.len());
        for slot in 0..self.ids.len() {
            let id = self.ids[slot];
            let has_label = self
                .meta
                .get(&id)
                .map(|m| !m.label.is_empty())
                .unwrap_or(false);
            if !has_label {
                continue;
            }
            ranked.push((self.centrality[slot], id));
        }
        // Highest centrality first; larger id first on a tie (stable, deterministic).
        ranked.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(Ordering::Equal)
                .then_with(|| b.1.cmp(&a.1))
        });
        let ids: Vec<u32> = ranked.into_iter().map(|(_, id)| id).collect();
        let mut vis = self.canonical_visible(ids);
        vis.truncate(max);
        vis
    }

    /// Store a node's label metadata (from initialGraphLoad).
    pub fn set_meta(&mut self, node_id: u32, meta_id: String, label: String, node_type: String, detail: String) {
        let label_lower = label.to_lowercase();
        // Preserve any file_size already recorded (set_file_size can land before or
        // after the label metadata) so re-setting the label doesn't zero it.
        let file_size = self.meta.get(&node_id).map(|m| m.file_size).unwrap_or(0);
        self.meta.insert(
            node_id,
            NodeMeta {
                meta_id,
                label,
                label_lower,
                node_type,
                detail,
                file_size,
            },
        );
    }

    /// Primary label for a node (empty string if unknown).
    pub fn label_of(&self, node_id: u32) -> String {
        self.meta.get(&node_id).map(|m| m.label.clone()).unwrap_or_default()
    }

    /// Slug source (metadata_id) for a node (empty if unknown); the double-click
    /// document view slugifies this, falling back to the label.
    pub fn meta_id_of(&self, node_id: u32) -> String {
        self.meta.get(&node_id).map(|m| m.meta_id.clone()).unwrap_or_default()
    }

    /// Secondary detail line: node type and one metadata value, joined by " · ".
    /// Empty when neither is known.
    pub fn detail_of(&self, node_id: u32) -> String {
        match self.meta.get(&node_id) {
            None => String::new(),
            Some(m) => {
                let mut parts: Vec<&str> = Vec::new();
                if !m.node_type.is_empty() {
                    parts.push(&m.node_type);
                }
                if !m.detail.is_empty() {
                    parts.push(&m.detail);
                }
                parts.join(" · ")
            }
        }
    }

    /// Centrality for a node id (0.0 if unknown) — used to rank label search hits.
    fn centrality_of(&self, node_id: u32) -> f32 {
        self.id_index
            .get(&node_id)
            .map(|&s| self.centrality[s])
            .unwrap_or(0.0)
    }

    /// Case-insensitive label search over the node metadata. Ranking:
    /// prefix matches (label starts with `query`) rank before substring matches;
    /// within each tier, higher centrality ranks first (stable id order breaks
    /// remaining ties). Empty `query` or `max == 0` returns an empty vec.
    /// Returns at most `max` node ids.
    pub fn search_labels(&self, query: &str, max: usize) -> Vec<u32> {
        if max == 0 {
            return Vec::new();
        }
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        // Canonicalise + dedup DURING selection so `max` bounds unique VISIBLE
        // results, not raw member matches: several folded members of one group must
        // not eat the quota and starve other reps. Each matching id is resolved to
        // its visible representative (hidden dropped); the best-ranked hit per
        // representative is kept in a map, then a bounded top-k selects `max` of the
        // deduped reps. When no fold is active the canonical id is the id itself, so
        // this degrades to the previous per-node behaviour.
        let mut best: HashMap<u32, LabelHit> = HashMap::new();
        for (&id, meta) in &self.meta {
            let hay = &meta.label_lower;
            if hay.is_empty() {
                continue;
            }
            let rank = if hay.starts_with(&needle) {
                0u8
            } else if hay.contains(&needle) {
                1u8
            } else {
                continue;
            };
            // Resolve to the visible representative; drop a hidden match (or a match
            // whose representative is hidden).
            if self.fold_hidden.contains(&id) {
                continue;
            }
            let canon = self.fold_target(id);
            if self.fold_hidden.contains(&canon) {
                continue;
            }
            // Rank/centrality reflect the matching node; the returned id is `canon`
            // so tie-breaks stay stable on the representative id.
            let hit = LabelHit {
                rank,
                centrality: self.centrality_of(id),
                id: canon,
            };
            best.entry(canon)
                .and_modify(|cur| {
                    if hit.worseness(cur) == Ordering::Less {
                        *cur = hit;
                    }
                })
                .or_insert(hit);
        }
        // Bounded top-k over the deduped representatives (worst on top).
        let mut heap: BinaryHeap<LabelHit> = BinaryHeap::with_capacity(max + 1);
        for hit in best.into_values() {
            if heap.len() < max {
                heap.push(hit);
            } else if let Some(worst) = heap.peek() {
                if hit.worseness(worst) == Ordering::Less {
                    heap.pop();
                    heap.push(hit);
                }
            }
        }
        // into_sorted_vec yields ascending by Ord (= best-first here).
        heap.into_sorted_vec().into_iter().map(|h| h.id).collect()
    }

    /// Ids of nodes within `radius` (server space) of `center`, nearest first,
    /// capped at `max`. A plain O(N) scan — fine at 13k for a few-Hz query.
    pub fn nodes_near(&self, center: [f32; 3], radius: f32, max: usize) -> Vec<u32> {
        let r2 = radius * radius;
        let mut hits: Vec<(f32, u32)> = Vec::new();
        for slot in 0..self.ids.len() {
            let p = self.positions[slot];
            let dx = p[0] - center[0];
            let dy = p[1] - center[1];
            let dz = p[2] - center[2];
            let d2 = dx * dx + dy * dy + dz * dz;
            if d2 <= r2 {
                hits.push((d2, self.ids[slot]));
            }
        }
        hits.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        // Canonicalise through the fold plan (member → visible representative,
        // hidden dropped, deduped) BEFORE the cap, so a folded cluster contributes
        // its representative once and the cap isn't spent on invisible members.
        let ranked: Vec<u32> = hits.into_iter().map(|(_, id)| id).collect();
        let mut vis = self.canonical_visible(ranked);
        vis.truncate(max);
        vis
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Insert or update a node from a decoded frame. New ids seed their render
    /// position at the target (no ease-in from origin); existing ids only move
    /// their target — the hunt eases the render position toward it.
    pub fn upsert(&mut self, node_id: u32, position: [f32; 3], community_id: u32, anomaly: f32, centrality: f32) {
        let color = community_color(community_id, anomaly, node_id);
        match self.id_index.get(&node_id).copied() {
            Some(slot) => {
                self.targets[slot] = position;
                self.centrality[slot] = centrality;
                self.color[slot] = color;
            }
            None => {
                let slot = self.ids.len();
                self.id_index.insert(node_id, slot);
                self.ids.push(node_id);
                self.targets.push(position);
                self.positions.push(position);
                self.centrality.push(centrality);
                self.color.push(color);
            }
        }
        if centrality > self.centrality_max {
            self.centrality_max = centrality;
        }
    }

    /// Ease every render position toward its target. The grabbed node (if any) is
    /// pinned to `grab_pos` (server space) so it tracks the hand exactly.
    pub fn hunt(&mut self, ease: f32, grab_id: Option<u32>, grab_pos: [f32; 3]) {
        // Fold-in members ease toward their representative's CURRENT position, not
        // their own server target. Snapshot rep positions first so the per-slot
        // mutation below doesn't clash with reading another slot.
        let rep_pos: HashMap<u32, [f32; 3]> = if self.folding.is_empty() {
            HashMap::new()
        } else {
            let reps: HashSet<u32> = self.folding.values().copied().collect();
            reps.into_iter().map(|r| (r, self.position_of(r))).collect()
        };
        // Embodiment (Pillar 1): an active agent NODE eases toward a HOVER point near
        // the node it is working on, instead of its server layout target — so the
        // agent visibly glides to inhabit its work (and the beam origin follows it
        // home). Idle/done agents, and agents whose target position is unknown, keep
        // their server target (the proxemics/layout fallback). Snapshot the hover
        // points first (reads other slots) to avoid clashing with the per-slot write.
        let hover: HashMap<u32, [f32; 3]> = if self.agent_registry.is_empty() {
            HashMap::new()
        } else {
            let mut m = HashMap::new();
            for (&aid, rec) in &self.agent_registry {
                if !Self::beam_active(rec.status) || rec.target_node_id == 0 {
                    continue;
                }
                if !self.id_index.contains_key(&aid) {
                    continue; // agent has no node in the store — nothing to move
                }
                let target = self.edge_endpoint(rec.target_node_id);
                if let Some(&ts) = self.id_index.get(&target) {
                    m.insert(aid, agent_hover_offset(self.positions[ts], aid, HOVER_RADIUS));
                }
            }
            m
        };
        for slot in 0..self.ids.len() {
            let id = self.ids[slot];
            if Some(id) == grab_id {
                self.positions[slot] = grab_pos;
                self.targets[slot] = grab_pos;
                continue;
            }
            let p = self.positions[slot];
            // Priority: a grabbed node is pinned (above); an active agent hovers at
            // its target; a member folding IN chases its representative; everything
            // else (incl. members folding OUT, seeded at the rep) eases to its target.
            let t = if let Some(&hp) = hover.get(&id) {
                hp
            } else {
                match self.folding.get(&id) {
                    Some(rep) => rep_pos.get(rep).copied().unwrap_or(p),
                    None => self.targets[slot],
                }
            };
            self.positions[slot] = [
                p[0] + (t[0] - p[0]) * ease,
                p[1] + (t[1] - p[1]) * ease,
                p[2] + (t[2] - p[2]) * ease,
            ];
        }
        // Prune arrived fold-in members (reached the rep → fully folded/hidden).
        if !self.folding.is_empty() {
            let arrived: Vec<u32> = self
                .folding
                .iter()
                .filter(|(&m, &r)| dist2(self.position_of(m), self.position_of(r)) < FOLD_ARRIVE_EPS2)
                .map(|(&m, _)| m)
                .collect();
            for m in arrived {
                self.folding.remove(&m);
            }
        }
        // Prune arrived fold-out members (reached their real target → plain node).
        if !self.unfolding.is_empty() {
            let done: Vec<u32> = self
                .unfolding
                .iter()
                .copied()
                .filter(|&m| match self.id_index.get(&m) {
                    Some(&slot) => dist2(self.positions[slot], self.targets[slot]) < FOLD_ARRIVE_EPS2,
                    None => true, // unknown node — stop tracking
                })
                .collect();
            for m in done {
                self.unfolding.remove(&m);
            }
        }
    }

    /// Pack the node MultiMesh buffer for the drawn `ids` (in order). Records the
    /// drawn set + render positions for the edge builder and the interaction ray.
    /// Ids not present in the store are skipped (buffer shrinks accordingly).
    pub fn build_node_buffer(&mut self, ids: &[i32], scale_comp: f32, size_lo: f32, size_hi: f32) -> Vec<f32> {
        self.drawn.clear();
        self.render_ids.clear();
        self.render_positions.clear();
        self.step_label_fades();
        let mut faded = std::mem::take(&mut self.faded_buf);
        faded.clear();
        let mut buf = Vec::with_capacity(ids.len() * NODE_STRIDE);
        for &raw in ids {
            let id0 = raw as u32;
            // L1-hidden ids never draw. Otherwise resolve the fold state:
            //  * a member folding IN draws IN TRANSIT (as itself) alongside its
            //    representative destination;
            //  * a fully-folded member is replaced by its representative (promotion
            //    — this injects the rep even when the budget picked only members);
            //  * everything else (plain nodes, and members folding OUT which are no
            //    longer remapped) draws as itself.
            // `emit_node` dedups via the drawn set and applies the type filter.
            if self.fold_hidden.contains(&id0) {
                continue;
            }
            if let Some(&rep) = self.folding.get(&id0) {
                self.emit_node(rep, &mut buf, &mut faded, scale_comp, size_lo, size_hi);
                self.emit_node(id0, &mut buf, &mut faded, scale_comp, size_lo, size_hi);
            } else if let Some(&rep) = self.fold_remap.get(&id0) {
                self.emit_node(rep, &mut buf, &mut faded, scale_comp, size_lo, size_hi);
            } else {
                self.emit_node(id0, &mut buf, &mut faded, scale_comp, size_lo, size_hi);
            }
        }
        // Guarantee in-transit fold animations stay visible even if the LOD budget
        // dropped the member and/or its representative from `ids` this frame.
        if !self.folding.is_empty() || !self.unfolding.is_empty() {
            let folding: Vec<(u32, u32)> = self.folding.iter().map(|(&m, &r)| (m, r)).collect();
            for (m, r) in folding {
                self.emit_node(r, &mut buf, &mut faded, scale_comp, size_lo, size_hi);
                self.emit_node(m, &mut buf, &mut faded, scale_comp, size_lo, size_hi);
            }
            let unfolding: Vec<u32> = self.unfolding.iter().copied().collect();
            for m in unfolding {
                self.emit_node(m, &mut buf, &mut faded, scale_comp, size_lo, size_hi);
            }
        }
        self.faded_buf = faded;
        buf
    }

    /// Append one node instance for `id` to `buf`, updating the drawn set + render
    /// arrays. No-op if the id is L1-hidden, type-filtered out, already drawn this
    /// build, or absent from the store. Shared by the fold-aware draw path so a
    /// representative and its in-transit members are packed uniformly. A node with
    /// a live label fade is packed into `faded` (transparent pass, COLOR.a = its
    /// alpha) instead of `buf`; it still enters the drawn set and render arrays so
    /// edges attach and the interaction ray can hit it.
    fn emit_node(
        &mut self,
        id: u32,
        buf: &mut Vec<f32>,
        faded: &mut Vec<f32>,
        scale_comp: f32,
        size_lo: f32,
        size_hi: f32,
    ) {
        if self.fold_hidden.contains(&id) || !self.node_visible(id) || self.drawn.contains(&id) {
            return;
        }
        let Some(&slot) = self.id_index.get(&id) else {
            return;
        };
        let pos = self.positions[slot];
        let cen_norm = if self.centrality_max > 0.0 {
            (self.centrality[slot] / self.centrality_max).clamp(0.0, 1.0)
        } else {
            0.0
        };
        // Desktop-parity size from this node's OWN degree + file_size, with the
        // centrality band retained as a multiplier (centrality also rides the halo
        // custom.r channel below). `id` is already the fold representative here, so
        // a rep sizes by its own metadata, not the collapsed group's.
        let size = self.node_size(id, cen_norm, scale_comp, size_lo, size_hi);
        // INSTANCE_CUSTOM: r = centrality halo tell, g = fold badge count
        // ("N collapsed" on a representative; 0 for a plain node), b = query flag.
        let badge = self.fold_badge.get(&id).copied().unwrap_or(0) as f32;
        let (mut col, query_flag) = match self.query_vars.get(&id) {
            Some(&palette_idx) => (query_var_color(palette_idx), 1.0),
            None => (self.color[slot], 0.0),
        };
        // Status halo (Pillar 3): an agent node is coloured by its derived status
        // (working/blocked/done/idle) with a floored halo so it always reads as a
        // deliberate, glowing inhabitant — unless it is query-marked, which wins
        // (an explicit user selection outranks the ambient status tint).
        let mut halo = cen_norm;
        if query_flag == 0.0 {
            if let Some(rec) = self.agent_registry.get(&id) {
                col = agent_status_color(rec.status);
                halo = halo.max(AGENT_HALO_MIN);
            }
        }
        let target: &mut Vec<f32> = match self.label_alpha.get(&id) {
            Some(&a) => {
                col[3] = a;
                faded
            }
            None => buf,
        };
        target.extend_from_slice(&node_transform12(size, pos));
        target.extend_from_slice(&col);
        target.extend_from_slice(&[halo, badge, query_flag, 1.0]);
        self.drawn.insert(id);
        self.render_ids.push(id);
        self.render_positions.push(pos);
    }

    /// Pack the edge MultiMesh buffer for the ranked `pairs`. An edge is emitted
    /// only when both endpoints are in the drawn set and non-degenerate.
    pub fn build_edge_buffer(&self, pairs: &[i32], radius_comp: f32) -> Vec<f32> {
        let mut buf = Vec::new();
        let n = pairs.len() / 2;
        // Fold plan: many member→member edges collapse onto the same
        // representative→representative pair. Dedup so a folded group draws one
        // cylinder per distinct representative pair, not one per hidden member edge.
        let has_fold =
            !self.fold_remap.is_empty() || !self.folding.is_empty() || !self.unfolding.is_empty();
        let mut seen: HashSet<(u32, u32)> = HashSet::new();
        for i in 0..n {
            // Style is keyed by the ORIGINAL (pre-remap) endpoints so a folded
            // rep→rep edge inherits the style of the member edge it stands in for.
            let os = pairs[i * 2] as u32;
            let ot = pairs[i * 2 + 1] as u32;
            // Re-route each endpoint through the fold remap BEFORE the drawn test,
            // so edges of hidden members attach to their representative instead of
            // vanishing. A member currently animating IN/OUT is drawn as itself, so
            // its edges follow the live member (they visibly shrink/grow) rather
            // than snapping to the representative.
            let s = self.edge_endpoint(os);
            let t = self.edge_endpoint(ot);
            // Intra-group edge (both endpoints fold to the same representative)
            // has no length — drop it.
            if s == t {
                continue;
            }
            if has_fold {
                let key = if s < t { (s, t) } else { (t, s) };
                if !seen.insert(key) {
                    continue;
                }
            }
            if !self.drawn.contains(&s) || !self.drawn.contains(&t) {
                continue;
            }
            let (Some(&ss), Some(&ts)) = (self.id_index.get(&s), self.id_index.get(&t)) else {
                continue;
            };
            if let Some(tf) = edge_transform12(self.positions[ss], self.positions[ts], radius_comp) {
                // 12 transform + 4 INSTANCE_CUSTOM: r/g/b reserved (0), a = relation
                // style code (0 untyped / 1 typed / 2 subclass) for the edge shader.
                let style = self.edge_style_of(os, ot) as f32;
                buf.extend_from_slice(&tf);
                buf.extend_from_slice(&[0.0, 0.0, 0.0, style]);
            }
        }
        buf
    }

    /// Whether an agent's status warrants a live work beam. A beam is the visible
    /// "this agent is acting on that node" affordance (Pillar 2), so it renders
    /// while the agent is `WORKING` or `BLOCKED` (stalled but still owning a
    /// target); `IDLE`/`DONE` agents draw no beam.
    fn beam_active(status: u8) -> bool {
        status == AGENT_WORKING || status == AGENT_BLOCKED
    }

    /// Pack the **work-beam** MultiMesh buffer (Pillar 2, P3): one cylinder per
    /// active agent→target-node link, ready for the restyled `edge_flow`
    /// (`agent_beam`) material on the reserved `AgentMulti` MultiMesh. Stride 16
    /// (12 transform + 4 INSTANCE_CUSTOM: r/g/b reserved, **a = agent status code**
    /// so the beam shader tints working/blocked and animates the flowing stream).
    ///
    /// The beam's source is the agent's embodiment anchor when the scene has
    /// published one ([`set_agent_anchors`](Self::set_agent_anchors)), else the
    /// agent's own streamed node position (agent nodes ride the binary wire with
    /// `AGENT_NODE_FLAG`, upserted like any node). `target_node_id` is a plain
    /// graph node, so `id_index` resolves it — no DID mapping needed. The
    /// cylinder's local Y runs agent→target, so the shader's pulse flows from the
    /// agent toward the node it is working on. Agents whose source or target is
    /// not yet known, or whose endpoints coincide, are skipped. Pure/per-frame —
    /// GDScript does one buffer assignment.
    pub fn build_beam_buffer(&self, radius_comp: f32) -> Vec<f32> {
        let mut buf = Vec::new();
        for (&agent_id, rec) in &self.agent_registry {
            if !Self::beam_active(rec.status) || rec.target_node_id == 0 {
                continue;
            }
            // Resolve the target through the fold plan (a folded member's beam must
            // land on its representative, not a hidden member) and require the
            // resolved target to be actually DRAWN — `drawn` already encodes the
            // fold plan, the type-visibility filter, and the LOD budget, exactly as
            // build_edge_buffer's gate. This stops a hidden/folded/culled target
            // from receiving a beam into empty space.
            let target = self.edge_endpoint(rec.target_node_id);
            if !self.drawn.contains(&target) {
                continue;
            }
            let Some(&ts) = self.id_index.get(&target) else {
                continue;
            };
            let source = match self.agent_anchors.get(&agent_id) {
                Some(&anchor) => anchor,
                None => match self.id_index.get(&agent_id) {
                    Some(&as_) => self.positions[as_],
                    None => continue,
                },
            };
            if let Some(tf) = edge_transform12(source, self.positions[ts], radius_comp) {
                buf.extend_from_slice(&tf);
                buf.extend_from_slice(&[0.0, 0.0, 0.0, rec.status as f32]);
            }
        }
        buf
    }

    /// Pack a **semantic-plane** node buffer: the given `ids` at their stored
    /// positions lifted by `y_offset` (server space). Unlike `build_node_buffer`
    /// this is a clean, ephemeral copy for a query result subgraph — it applies
    /// NEITHER the fold plan NOR the query-variable overlay, does not touch the
    /// drawn set, and colours by community only. Ids absent from the store are
    /// skipped. INSTANCE_CUSTOM is `[cen_norm, 0, 0, 1]`.
    pub fn build_plane_node_buffer(
        &self,
        ids: &[i32],
        y_offset: f32,
        scale_comp: f32,
        size_lo: f32,
        size_hi: f32,
    ) -> Vec<f32> {
        let mut buf = Vec::with_capacity(ids.len() * NODE_STRIDE);
        for &raw in ids {
            let id = raw as u32;
            let Some(&slot) = self.id_index.get(&id) else {
                continue;
            };
            let mut pos = self.positions[slot];
            pos[1] += y_offset;
            let cen_norm = if self.centrality_max > 0.0 {
                (self.centrality[slot] / self.centrality_max).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let size = scale_comp * (size_lo + (size_hi - size_lo) * cen_norm.sqrt());
            buf.extend_from_slice(&node_transform12(size, pos));
            buf.extend_from_slice(&self.color[slot]);
            buf.extend_from_slice(&[cen_norm, 0.0, 0.0, 1.0]);
        }
        buf
    }

    /// Pack a **semantic-plane** edge buffer: the given directed `pairs` at their
    /// stored endpoint positions lifted by `y_offset`. No fold remap, no drawn
    /// filter (a result subgraph draws its own edges regardless of the main LOD);
    /// degenerate/unknown edges are skipped.
    pub fn build_plane_edge_buffer(&self, pairs: &[i32], y_offset: f32, radius_comp: f32) -> Vec<f32> {
        let mut buf = Vec::new();
        let n = pairs.len() / 2;
        for i in 0..n {
            let s = pairs[i * 2] as u32;
            let t = pairs[i * 2 + 1] as u32;
            let (Some(&ss), Some(&ts)) = (self.id_index.get(&s), self.id_index.get(&t)) else {
                continue;
            };
            let mut a = self.positions[ss];
            let mut b = self.positions[ts];
            a[1] += y_offset;
            b[1] += y_offset;
            if let Some(tf) = edge_transform12(a, b, radius_comp) {
                buf.extend_from_slice(&tf);
            }
        }
        buf
    }

    /// Ids drawn by the last `build_node_buffer`, for the interaction ray.
    pub fn render_ids(&self) -> &[u32] {
        &self.render_ids
    }

    /// Render (server-space) positions parallel to [`render_ids`](Self::render_ids).
    pub fn render_positions(&self) -> &[[f32; 3]] {
        &self.render_positions
    }

    /// All node ids currently in the store (slot order), for the LOD selection.
    pub fn all_ids(&self) -> &[u32] {
        &self.ids
    }

    /// Current render position of a node (zeros if unknown) — used at grab start.
    pub fn position_of(&self, node_id: u32) -> [f32; 3] {
        self.id_index
            .get(&node_id)
            .map(|&s| self.positions[s])
            .unwrap_or([0.0, 0.0, 0.0])
    }

    /// Per-axis `[lo,hi]` percentile AABB over render positions, excluding the
    /// grabbed node so a dragged outlier can't inflate the adaptive fit. Returns
    /// `[minx,miny,minz,maxx,maxy,maxz]`, or `None` when empty.
    pub fn aabb_percentile(&self, lo_q: f32, hi_q: f32, exclude_id: Option<u32>) -> Option<[f32; 6]> {
        let mut xs = Vec::with_capacity(self.ids.len());
        let mut ys = Vec::with_capacity(self.ids.len());
        let mut zs = Vec::with_capacity(self.ids.len());
        for slot in 0..self.ids.len() {
            if Some(self.ids[slot]) == exclude_id {
                continue;
            }
            let p = self.positions[slot];
            xs.push(p[0]);
            ys.push(p[1]);
            zs.push(p[2]);
        }
        if xs.is_empty() {
            return None;
        }
        Some([
            percentile(&xs, lo_q),
            percentile(&ys, lo_q),
            percentile(&zs, lo_q),
            percentile(&xs, hi_q),
            percentile(&ys, hi_q),
            percentile(&zs, hi_q),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn beam_buffer_emits_active_agents_only_with_status_in_custom_a() {
        let mut s = RenderStore::new();
        // Agent node 5 and its target node 20 both have positions on the wire.
        s.upsert(5, [0.0, 0.0, 0.0], 0, 0.0, 0.0);
        s.upsert(20, [0.0, 4.0, 0.0], 0, 0.0, 0.0);
        // Agent 6 works on a target (30) whose position has NOT arrived yet.
        s.upsert(6, [1.0, 0.0, 0.0], 0, 0.0, 0.0);
        // A 0x23 action makes agent 5 WORKING on node 20, agent 6 on the unknown 30.
        s.record_agent_action(5, 20, 0, 100, "reading");
        s.record_agent_action(6, 30, 0, 100, "");
        // Beams gate on the target being DRAWN; draw agent+target (30 is unknown so
        // it never enters the drawn set even if listed).
        s.build_node_buffer(&[5, 6, 20, 30], 1.0, 0.7, 1.9);

        let buf = s.build_beam_buffer(1.0);
        // Only agent 5 draws: agent 6's target (30) has no position / is not drawn.
        assert_eq!(buf.len(), EDGE_STRIDE_TYPED, "one beam, stride 16");
        // INSTANCE_CUSTOM.a (index 15) carries the status code = WORKING.
        assert!(approx(buf[15], AGENT_WORKING as f32));

        // DONE / IDLE agents draw no beam; BLOCKED still does (stalled but owning).
        s.set_agent_state(5, "done", "");
        assert!(s.build_beam_buffer(1.0).is_empty(), "done agent: no beam");
        s.set_agent_state(5, "blocked", "");
        let blocked = s.build_beam_buffer(1.0);
        assert_eq!(blocked.len(), EDGE_STRIDE_TYPED, "blocked agent still beams");
        assert!(approx(blocked[15], AGENT_BLOCKED as f32));
    }

    #[test]
    fn beam_starts_at_the_embodiment_anchor_when_one_is_published() {
        let mut s = RenderStore::new();
        // Target node 20 is on the wire and drawn; agent 0xD001 is a synthetic
        // (demo) agent with NO streamed node position.
        s.upsert(20, [0.0, 4.0, 0.0], 0, 0.0, 0.0);
        s.record_agent_action(0x8000_D001, 20, 0, 100, "Reviewing: X");
        s.build_node_buffer(&[20], 1.0, 0.7, 1.9);
        assert!(s.build_beam_buffer(1.0).is_empty(), "no anchor, no node ⇒ no beam");

        // Publishing an anchor (flag bit tolerated) gives the beam its origin.
        s.set_agent_anchors(&[0x8000_D001], &[[2.0, 4.0, 0.0]]);
        let buf = s.build_beam_buffer(1.0);
        assert_eq!(buf.len(), EDGE_STRIDE_TYPED, "anchored synthetic agent beams");
        // Translation column of the 3x4 transform (indices 3,7,11) is the segment
        // midpoint: anchor (2,4,0) → target (0,4,0) ⇒ (1,4,0).
        assert!(approx(buf[3], 1.0) && approx(buf[7], 4.0) && approx(buf[11], 0.0));

        // An anchor wins over a streamed node position for an embodied live agent.
        s.upsert(5, [0.0, 0.0, 0.0], 0, 0.0, 0.0);
        s.record_agent_action(5, 20, 0, 100, "");
        s.build_node_buffer(&[5, 20], 1.0, 0.7, 1.9);
        s.set_agent_anchors(&[5], &[[0.0, 2.0, 0.0]]);
        let buf = s.build_beam_buffer(1.0);
        let n = buf.len() / EDGE_STRIDE_TYPED;
        assert_eq!(n, 2, "both agents beam");
        // Non-finite anchors are ignored; clearing falls back to the node position.
        s.set_agent_anchors(&[5], &[[f32::NAN, 0.0, 0.0]]);
        assert_eq!(s.agent_anchor(5), Some([0.0, 2.0, 0.0]));
        s.clear_agent_anchor(5);
        assert_eq!(s.agent_anchor(5), None);
        assert_eq!(s.build_beam_buffer(1.0).len() / EDGE_STRIDE_TYPED, 2);
    }

    #[test]
    fn retire_agents_removes_records_and_anchors_outright() {
        let mut s = RenderStore::new();
        s.record_agent_action(0x8000_D001, 20, 0, 100, "");
        s.record_agent_action(0x8000_D002, 21, 0, 100, "");
        s.record_agent_action(7, 22, 0, 100, "");
        s.set_agent_anchors(&[0x8000_D001, 7], &[[1.0, 1.0, 1.0], [2.0, 2.0, 2.0]]);
        assert_eq!(s.agent_count(), 3);
        // Retire the two synthetic ids (flag bit on one, masked on the other).
        assert_eq!(s.retire_agents(&[0x8000_D001, 0xD002, 0xD0FF]), 2);
        assert_eq!(s.agent_ids(), vec![7], "only the live agent remains");
        assert_eq!(s.agent_anchor(0xD001), None, "anchor goes with the record");
        assert_eq!(s.agent_anchor(7), Some([2.0, 2.0, 2.0]), "live anchor untouched");
        // Retiring is idempotent and a later action can re-create the record.
        assert_eq!(s.retire_agents(&[0xD001]), 0);
        assert!(s.record_agent_action(0x8000_D001, 20, 0, 200, ""));
        assert_eq!(s.agent_count(), 2);
    }

    #[test]
    fn active_agent_hovers_toward_its_target_not_its_server_position() {
        let mut s = RenderStore::new();
        // Agent 5 sits at the origin (server target = origin); target node 20 far away.
        s.upsert(5, [0.0, 0.0, 0.0], 0, 0.0, 0.0);
        s.upsert(20, [10.0, 0.0, 0.0], 0, 0.0, 0.0);
        // Idle agent: hunt keeps it at its server target (origin).
        s.record_agent_action(5, 20, 0, 100, "");
        s.set_agent_state(5, "idle", "");
        for _ in 0..32 {
            s.hunt(0.5, None, [0.0, 0.0, 0.0]);
        }
        let idle_pos = s.position_of(5);
        assert!(approx(idle_pos[0], 0.0), "idle agent stays at its server position");

        // Now WORKING: it must glide toward the hover ring around node 20 (x≫0).
        s.set_agent_state(5, "busy", "");
        for _ in 0..64 {
            s.hunt(0.5, None, [0.0, 0.0, 0.0]);
        }
        let work_pos = s.position_of(5);
        let expected = agent_hover_offset([10.0, 0.0, 0.0], 5, HOVER_RADIUS);
        assert!(work_pos[0] > 5.0, "working agent glided toward its target node");
        assert!(approx(work_pos[0], expected[0]), "settled on the hover ring x");
        assert!(approx(work_pos[1], expected[1]), "hover lifts above the node");
    }

    #[test]
    fn agent_node_is_coloured_and_haloed_by_status() {
        let mut s = RenderStore::new();
        s.upsert(5, [0.0, 0.0, 0.0], 7, 0.0, 0.0); // community 7 → some community colour
        s.record_agent_action(5, 20, 0, 100, "");
        s.set_agent_state(5, "blocked", "");
        let buf = s.build_node_buffer(&[5], 1.0, 0.7, 1.9);
        assert_eq!(buf.len(), NODE_STRIDE);
        // Colour (floats 12..16) is the BLOCKED status colour, not the community hue.
        let blocked = agent_status_color(AGENT_BLOCKED);
        assert!(approx(buf[12], blocked[0]) && approx(buf[13], blocked[1]));
        // INSTANCE_CUSTOM.r (float 16) is floored to the agent halo minimum.
        assert!(buf[16] >= AGENT_HALO_MIN - 1e-6, "agent node forced to glow its status");
    }

    #[test]
    fn beam_requires_drawn_target_and_follows_fold() {
        let mut s = RenderStore::new();
        s.upsert(5, [0.0, 0.0, 0.0], 0, 0.0, 0.0); // agent
        s.upsert(20, [0.0, 4.0, 0.0], 0, 0.0, 0.0); // target member
        s.upsert(21, [3.0, 4.0, 0.0], 0, 0.0, 0.0); // fold representative
        s.record_agent_action(5, 20, 0, 100, "working");

        // (a) Target known but NOT drawn yet ⇒ no beam (no beam into an unrendered node).
        assert!(
            s.build_beam_buffer(1.0).is_empty(),
            "target not drawn ⇒ no beam"
        );

        // (b) Draw agent + target ⇒ beam appears, landing on node 20.
        s.build_node_buffer(&[5, 20], 1.0, 0.7, 1.9);
        assert_eq!(s.build_beam_buffer(1.0).len(), EDGE_STRIDE_TYPED);

        // (c) Fold node 20 (member) into representative 21, then SETTLE the fold-in
        // animation (mid-transition the member is still drawn in transit, so the beam
        // correctly follows the live member; only once settled does 20 hide and its
        // edges/beam re-route to the representative). After settling: 20 hidden, 21
        // draws, and the beam must land on the representative.
        s.set_fold_plan(&[], &[20], &[21]);
        settle(&mut s);
        let buf = s.build_node_buffer(&[5, 21], 1.0, 0.7, 1.9); // 20 hidden, 21 the rep
        assert!(!buf.is_empty());
        let beam = s.build_beam_buffer(1.0);
        assert_eq!(beam.len(), EDGE_STRIDE_TYPED, "beam re-routes to the fold rep");
        // Reconstruct the beam's TARGET endpoint (independent of where the P2 hover
        // has moved the agent end): row-major 3x4 ⇒ translation o = midpoint at
        // (buf[3],buf[7],buf[11]); column c1 = dir*len at (buf[1],buf[5],buf[9]); so
        // target b = o + 0.5*c1. It must land on the representative (21) at (3,4,0),
        // NOT the hidden member (20) at (0,4,0).
        let bx = beam[3] + 0.5 * beam[1];
        let by = beam[7] + 0.5 * beam[5];
        let rep = s.position_of(21);
        assert!(approx(bx, rep[0]) && approx(by, rep[1]), "beam target end is the fold rep");
        assert!(approx(rep[0], 3.0), "sanity: rep sits at x=3, member was at x=0");

        // (d) Re-fold 20 onto a representative that is NOT drawn ⇒ no beam (20 stays
        // hidden and resolves to the undrawn rep 99).
        s.set_fold_plan(&[], &[20], &[99]); // 99 has no position and isn't drawn
        settle(&mut s);
        s.build_node_buffer(&[5], 1.0, 0.7, 1.9);
        assert!(
            s.build_beam_buffer(1.0).is_empty(),
            "folded onto an undrawn rep ⇒ no beam into empty space"
        );
    }

    /// Drive the hunt to convergence so fold-in/out transitions settle to their end
    /// state (in-transit members reach their representative / target and leave the
    /// animation sets). Ease 0.5 over 64 ticks converges well within FOLD_ARRIVE_EPS2.
    fn settle(s: &mut RenderStore) {
        for _ in 0..64 {
            s.hunt(0.5, None, [0.0, 0.0, 0.0]);
        }
    }

    #[test]
    fn node_transform_places_scale_and_origin() {
        let t = node_transform12(2.0, [5.0, 6.0, 7.0]);
        // row-major 3x4: diag scale, origin in the 4th column of each row.
        assert_eq!(t, [2.0, 0.0, 0.0, 5.0, 0.0, 2.0, 0.0, 6.0, 0.0, 0.0, 2.0, 7.0]);
    }

    #[test]
    fn edge_transform_vertical_is_identity_rotation() {
        // a→b straight up: dir == UP, rotation identity, Y scaled to length 4.
        let tf = edge_transform12([0.0, 0.0, 0.0], [0.0, 4.0, 0.0], 0.1).unwrap();
        // origin = midpoint (0,2,0)
        assert!(approx(tf[3], 0.0) && approx(tf[7], 2.0) && approx(tf[11], 0.0));
        // c1 (local Y column) = (0, length, 0) → row1col1 == 4
        assert!(approx(tf[5], 4.0));
        // c0,c2 scaled by radius on the diagonal
        assert!(approx(tf[0], 0.1) && approx(tf[10], 0.1));
    }

    #[test]
    fn edge_transform_maps_local_y_onto_direction() {
        // For any direction, column 1 (local Y) must equal dir*length.
        let a = [1.0, 2.0, 3.0];
        let b = [4.0, 2.0, 3.0]; // +X direction, length 3
        let tf = edge_transform12(a, b, 0.5).unwrap();
        // c1 = (row0col1,row1col1,row2col1) = tf[1],tf[5],tf[9]
        let c1 = [tf[1], tf[5], tf[9]];
        assert!(approx(c1[0], 3.0) && approx(c1[1], 0.0) && approx(c1[2], 0.0));
        // origin = midpoint
        assert!(approx(tf[3], 2.5) && approx(tf[7], 2.0) && approx(tf[11], 3.0));
    }

    #[test]
    fn edge_transform_antiparallel_flips_y() {
        let tf = edge_transform12([0.0, 0.0, 0.0], [0.0, -2.0, 0.0], 0.1).unwrap();
        // c1 = (0,-length,0) → tf[5] == -2
        assert!(approx(tf[5], -2.0));
    }

    #[test]
    fn degenerate_edge_is_none() {
        assert!(edge_transform12([1.0, 1.0, 1.0], [1.0, 1.0, 1.0], 0.1).is_none());
    }

    #[test]
    fn hunt_converges_toward_target() {
        let mut s = RenderStore::new();
        s.upsert(7, [0.0, 0.0, 0.0], 0, 0.0, 0.0);
        // Move the target; render position should ease toward it, not jump.
        s.upsert(7, [10.0, 0.0, 0.0], 0, 0.0, 0.0);
        s.hunt(0.5, None, [0.0, 0.0, 0.0]);
        assert!(approx(s.position_of(7)[0], 5.0));
        for _ in 0..40 {
            s.hunt(0.5, None, [0.0, 0.0, 0.0]);
        }
        assert!(approx(s.position_of(7)[0], 10.0));
    }

    #[test]
    fn hunt_pins_grabbed_node() {
        let mut s = RenderStore::new();
        s.upsert(3, [0.0, 0.0, 0.0], 0, 0.0, 0.0);
        s.upsert(3, [100.0, 0.0, 0.0], 0, 0.0, 0.0); // target far away
        s.hunt(0.1, Some(3), [1.0, 2.0, 3.0]);
        // Grabbed → pinned exactly at grab position, ignoring the target.
        assert_eq!(s.position_of(3), [1.0, 2.0, 3.0]);
    }

    #[test]
    fn node_buffer_layout_and_size_from_centrality() {
        let mut s = RenderStore::new();
        s.upsert(1, [1.0, 2.0, 3.0], 0, 0.0, 1.0); // max centrality
        s.upsert(2, [4.0, 5.0, 6.0], 0, 0.0, 0.0); // zero centrality
        let buf = s.build_node_buffer(&[1, 2], 2.0, 0.7, 1.9);
        assert_eq!(buf.len(), 2 * NODE_STRIDE);
        // Node 1: cen_norm=1 → size = 2.0*lerp(0.7,1.9,1)=3.8; origin (1,2,3).
        assert!(approx(buf[0], 3.8) && approx(buf[3], 1.0) && approx(buf[7], 2.0) && approx(buf[11], 3.0));
        // custom.r == cen_norm == 1 (index 16..20 block, r at 16).
        assert!(approx(buf[16], 1.0));
        // Node 2: cen_norm=0 → size = 2.0*0.7 = 1.4; custom.r == 0.
        assert!(approx(buf[NODE_STRIDE], 1.4));
        assert!(approx(buf[NODE_STRIDE + 16], 0.0));
    }

    #[test]
    fn edge_buffer_filters_undrawn_endpoints() {
        let mut s = RenderStore::new();
        s.upsert(1, [0.0, 0.0, 0.0], 0, 0.0, 0.0);
        s.upsert(2, [0.0, 2.0, 0.0], 0, 0.0, 0.0);
        s.upsert(3, [0.0, 4.0, 0.0], 0, 0.0, 0.0);
        // Draw only 1 and 2.
        let _ = s.build_node_buffer(&[1, 2], 1.0, 0.7, 1.9);
        // Edge (1,2) both drawn → kept; edge (2,3) has undrawn endpoint → dropped.
        let buf = s.build_edge_buffer(&[1, 2, 2, 3], 0.1);
        assert_eq!(buf.len(), EDGE_STRIDE_TYPED, "only the fully-drawn edge is emitted");
    }

    #[test]
    fn aabb_excludes_grabbed_and_uses_percentiles() {
        let mut s = RenderStore::new();
        for i in 0..100u32 {
            s.upsert(i, [i as f32, 0.0, 0.0], 0, 0.0, 0.0);
        }
        // An outlier that would blow the AABB if included.
        s.upsert(999, [100000.0, 0.0, 0.0], 0, 0.0, 0.0);
        let bb = s.aabb_percentile(0.05, 0.95, Some(999)).unwrap();
        // 5th–95th percentile of 0..99 stays well within range; outlier excluded.
        assert!(bb[3] < 100.0, "outlier excluded, x-max stays bounded");
        assert!(bb[0] >= 0.0);
    }

    #[test]
    fn nodes_near_orders_by_distance_and_caps() {
        let mut s = RenderStore::new();
        // Nodes strung along +X at 1,2,3,4,5 m.
        for i in 1..=5u32 {
            s.upsert(i, [i as f32, 0.0, 0.0], 0, 0.0, 0.0);
        }
        // From the origin, radius 3.5 covers ids 1,2,3; cap to 2 → nearest two.
        let near = s.nodes_near([0.0, 0.0, 0.0], 3.5, 2);
        assert_eq!(near, vec![1, 2], "nearest-first, capped at max");
        // Radius 3.5, no cap pressure → 1,2,3 in order.
        let near_all = s.nodes_near([0.0, 0.0, 0.0], 3.5, 10);
        assert_eq!(near_all, vec![1, 2, 3]);
        // Empty when nothing is in range.
        assert!(s.nodes_near([100.0, 0.0, 0.0], 0.5, 10).is_empty());
    }

    #[test]
    fn meta_label_and_detail() {
        let mut s = RenderStore::new();
        s.set_meta(1, "alpha-page".into(), "Alpha".into(), "page".into(), "knowledge".into());
        assert_eq!(s.meta_id_of(1), "alpha-page");
        assert_eq!(s.label_of(1), "Alpha");
        assert_eq!(s.detail_of(1), "page · knowledge");
        // Unknown node → empty, not a panic.
        assert_eq!(s.meta_id_of(99), "");
        assert_eq!(s.label_of(99), "");
        assert_eq!(s.detail_of(99), "");
        // Only node_type present.
        s.set_meta(2, "".into(), "Beta".into(), "agent".into(), "".into());
        assert_eq!(s.detail_of(2), "agent");
    }

    #[test]
    fn search_labels_ranks_prefix_over_substring() {
        let mut s = RenderStore::new();
        // "Graph" is a prefix match; "Knowledge Graph" only a substring match.
        s.set_meta(1, "".into(), "Knowledge Graph".into(), "page".into(), "".into());
        s.set_meta(2, "".into(), "Graph Theory".into(), "page".into(), "".into());
        let hits = s.search_labels("graph", 10);
        assert_eq!(hits, vec![2, 1], "prefix match ranks before substring match");
    }

    #[test]
    fn search_labels_is_case_insensitive() {
        let mut s = RenderStore::new();
        s.set_meta(1, "".into(), "OntoLogy".into(), "page".into(), "".into());
        assert_eq!(s.search_labels("ONTOLOGY", 10), vec![1]);
        assert_eq!(s.search_labels("onto", 10), vec![1]);
        assert_eq!(s.search_labels("LOG", 10), vec![1]); // substring, mixed case
    }

    #[test]
    fn search_labels_ties_break_by_centrality_desc() {
        let mut s = RenderStore::new();
        // Both prefix matches; centrality (from position upserts) orders them.
        s.set_meta(1, "".into(), "Node Alpha".into(), "page".into(), "".into());
        s.set_meta(2, "".into(), "Node Beta".into(), "page".into(), "".into());
        s.set_meta(3, "".into(), "Node Gamma".into(), "page".into(), "".into());
        s.upsert(1, [0.0; 3], 0, 0.0, 0.2);
        s.upsert(2, [0.0; 3], 0, 0.0, 0.9);
        s.upsert(3, [0.0; 3], 0, 0.0, 0.5);
        assert_eq!(s.search_labels("node", 10), vec![2, 3, 1]);
    }

    #[test]
    fn search_labels_respects_cap_and_empty_query() {
        let mut s = RenderStore::new();
        for i in 1..=5u32 {
            s.set_meta(i, "".into(), format!("Item {i}"), "page".into(), "".into());
        }
        assert_eq!(s.search_labels("item", 2).len(), 2, "capped at max");
        assert!(s.search_labels("", 10).is_empty(), "empty query → no hits");
        assert!(s.search_labels("   ", 10).is_empty(), "whitespace query → no hits");
        assert!(s.search_labels("item", 0).is_empty(), "max 0 → no hits");
        assert!(s.search_labels("nonexistent", 10).is_empty());
    }

    #[test]
    fn search_labels_bounded_topk_keeps_the_best_not_arbitrary() {
        // Many matches, small cap: the bounded heap must return the globally best
        // `max`, independent of HashMap iteration order. Two prefix matches (best
        // tier) plus several substring matches; max=2 must return exactly the two
        // prefix hits, ranked by centrality desc.
        let mut s = RenderStore::new();
        s.set_meta(1, "".into(), "Graph Alpha".into(), "page".into(), "".into()); // prefix
        s.set_meta(2, "".into(), "Graph Beta".into(), "page".into(), "".into());  // prefix
        for i in 3..=12u32 {
            s.set_meta(i, "".into(), format!("A Knowledge Graph {i}"), "page".into(), "".into()); // substring
        }
        s.upsert(1, [0.0; 3], 0, 0.0, 0.3);
        s.upsert(2, [0.0; 3], 0, 0.0, 0.9);
        // give a substring match high centrality — must STILL lose to prefix tier.
        s.upsert(5, [0.0; 3], 0, 0.0, 1.0);
        let hits = s.search_labels("graph", 2);
        assert_eq!(hits, vec![2, 1], "top-2 = the two prefix matches, centrality desc");
    }

    #[test]
    fn search_labels_uses_cached_lowercase() {
        // set_meta caches label_lower; a match on mixed case proves the cache is
        // populated and used (not the raw label).
        let mut s = RenderStore::new();
        s.set_meta(1, "".into(), "MixedCaseLabel".into(), "page".into(), "".into());
        assert_eq!(s.search_labels("mixedcase", 5), vec![1]);
        assert_eq!(s.search_labels("CASELABEL", 5), vec![1]);
    }

    #[test]
    fn fold_plan_hides_members_and_badges_representative() {
        let mut s = RenderStore::new();
        for i in 1..=4u32 {
            s.upsert(i, [i as f32, 0.0, 0.0], 0, 0.0, 0.0);
        }
        // Fold 2,3 into representative 1; hide node 4 (L1 low-signal).
        s.set_fold_plan(&[4], &[2, 3], &[1, 1]);
        settle(&mut s); // let the fold-in animation reach its end state
        let buf = s.build_node_buffer(&[1, 2, 3, 4], 1.0, 0.7, 1.9);
        // Only the representative (1) draws: members 2,3 folded, 4 hidden.
        assert_eq!(buf.len(), NODE_STRIDE, "only the representative renders");
        // Its custom.g (index 17) carries the badge count = 2 folded members.
        assert!(approx(buf[17], 2.0), "badge count in custom.g");
        // Drawn set reflects the fold — the interaction ray only sees the rep.
        assert_eq!(s.render_ids(), &[1]);
    }

    #[test]
    fn fold_in_keeps_members_visible_until_they_reach_the_representative() {
        // Phase 3: members lerp INTO the rep and stay drawn in transit, then leave
        // the draw set once they arrive — not an instant snap.
        let mut s = RenderStore::new();
        for id in [1u32, 2, 3] {
            s.upsert(id, [id as f32 * 10.0, 0.0, 0.0], 0, 0.0, 0.0); // spread apart
        }
        s.set_fold_plan(&[], &[2, 3], &[1, 1]); // 2,3 → rep 1
        // Immediately after the plan (no hunt yet) the members are IN TRANSIT and
        // still drawn alongside their representative.
        let mid = s.build_node_buffer(&[1, 2, 3], 1.0, 0.7, 1.9);
        assert_eq!(mid.len(), 3 * NODE_STRIDE, "members visible mid-fold-in");
        // One hunt tick eases them toward the rep but they haven't arrived yet.
        s.hunt(0.06, None, [0.0, 0.0, 0.0]);
        assert_eq!(
            s.build_node_buffer(&[1, 2, 3], 1.0, 0.7, 1.9).len(),
            3 * NODE_STRIDE,
            "still in transit after one tick"
        );
        // After settling they collapse into the representative.
        settle(&mut s);
        assert_eq!(
            s.build_node_buffer(&[1, 2, 3], 1.0, 0.7, 1.9).len(),
            NODE_STRIDE,
            "folded away once arrived"
        );
    }

    #[test]
    fn unfold_seeds_members_at_representative_then_grows_them_out() {
        let mut s = RenderStore::new();
        s.upsert(1, [0.0, 0.0, 0.0], 0, 0.0, 0.0); // rep
        s.upsert(2, [50.0, 0.0, 0.0], 0, 0.0, 0.0); // member's real home, far away
        s.set_fold_plan(&[], &[2], &[1]);
        settle(&mut s); // fully folded: member 2 hidden at rep
        // Unfold: member 2 is seeded at the representative's position (near origin),
        // NOT snapped to its far home — it grows out from there.
        s.clear_fold_plan();
        let seeded = s.position_of(2);
        assert!(seeded[0].abs() < 1.0, "member seeded at the representative on unfold");
        // It is drawn immediately (in transit) even though the budget list omits it.
        assert_eq!(
            s.build_node_buffer(&[1], 1.0, 0.7, 1.9).len(),
            2 * NODE_STRIDE,
            "unfolding member injected into the draw"
        );
        // After settling it reaches its real home and leaves the animation set.
        settle(&mut s);
        assert!(s.position_of(2)[0] > 40.0, "member grew out to its real position");
    }

    #[test]
    fn fold_plan_reroutes_edges_to_representative() {
        let mut s = RenderStore::new();
        // 1 = rep; 2,3 fold into 1; 5 is an outside node.
        for id in [1u32, 2, 3, 5] {
            s.upsert(id, [id as f32, 0.0, 0.0], 0, 0.0, 0.0);
        }
        s.set_fold_plan(&[], &[2, 3], &[1, 1]);
        settle(&mut s); // members fold fully into the representative
        let _ = s.build_node_buffer(&[1, 2, 3, 5], 1.0, 0.7, 1.9); // drawn = {1,5}
        // Edges: (5→2) outside→member should re-route to (5→1); (2→3) intra-group
        // collapses to self and drops; a second outside edge (5→3) also maps to
        // (5→1) and must dedup against the first.
        let buf = s.build_edge_buffer(&[5, 2, 2, 3, 5, 3], 0.1);
        assert_eq!(
            buf.len(),
            EDGE_STRIDE_TYPED,
            "one representative edge after remap+dedup, intra-group dropped"
        );
    }

    #[test]
    fn clear_fold_plan_restores_full_density() {
        let mut s = RenderStore::new();
        for i in 1..=3u32 {
            s.upsert(i, [i as f32, 0.0, 0.0], 0, 0.0, 0.0);
        }
        s.set_fold_plan(&[3], &[2], &[1]);
        settle(&mut s); // fold-in completes
        assert_eq!(s.build_node_buffer(&[1, 2, 3], 1.0, 0.7, 1.9).len(), NODE_STRIDE);
        s.clear_fold_plan();
        settle(&mut s); // fold-out (grow from rep) completes
        // All three draw again; badge channel back to 0.
        let buf = s.build_node_buffer(&[1, 2, 3], 1.0, 0.7, 1.9);
        assert_eq!(buf.len(), 3 * NODE_STRIDE);
        assert!(approx(buf[17], 0.0), "no badge after clear");
    }

    #[test]
    fn fold_promotes_representative_into_drawn_set_when_budget_picks_only_members() {
        // Regression: a budgeted draw list containing only folded MEMBERS must
        // still draw their representative (promotion) — otherwise both the node
        // and its remapped edges vanish.
        let mut s = RenderStore::new();
        for id in [1u32, 2, 3, 5] {
            s.upsert(id, [id as f32, 0.0, 0.0], 0, 0.0, 0.0);
        }
        s.set_fold_plan(&[], &[2, 3], &[1, 1]); // 2,3 → rep 1
        settle(&mut s); // fold-in completes so members are fully folded
        // Budget selected only the members (rep 1 absent from the list).
        let buf = s.build_node_buffer(&[2, 3], 1.0, 0.7, 1.9);
        assert_eq!(buf.len(), NODE_STRIDE, "rep drawn once (promoted + deduped)");
        assert_eq!(s.render_ids(), &[1], "representative injected into drawn set");
        // And an edge from an outside node to a member now finds the rep on-screen.
        let ebuf = s.build_edge_buffer(&[5, 2], 0.1);
        // 5 wasn't drawn (not in the node list), so this still filters — but the
        // rep IS drawn, proving the endpoint remap resolves to a drawn node.
        let _ = ebuf; // (edge needs both endpoints drawn; asserted separately below)
        let buf2 = s.build_node_buffer(&[2, 3, 5], 1.0, 0.7, 1.9); // now 5 drawn too
        assert_eq!(buf2.len(), 2 * NODE_STRIDE, "rep(1) + outside(5)");
        let ebuf2 = s.build_edge_buffer(&[5, 2], 0.1);
        assert_eq!(ebuf2.len(), EDGE_STRIDE_TYPED, "edge 5→member reroutes to drawn rep");
    }

    #[test]
    fn search_and_nodes_near_canonicalise_through_fold() {
        let mut s = RenderStore::new();
        for id in [1u32, 2, 3] {
            s.upsert(id, [id as f32, 0.0, 0.0], 0, 0.0, 0.0);
            s.set_meta(id, "".into(), format!("Node {id}"), "page".into(), "".into());
        }
        // Fold 2 → rep 1; hide 3.
        s.set_fold_plan(&[3], &[2], &[1]);
        // Searching a folded member's label resolves to its visible representative.
        assert_eq!(s.search_labels("Node 2", 10), vec![1], "member → representative");
        // A hidden node is never a search target.
        assert!(s.search_labels("Node 3", 10).is_empty(), "hidden excluded from search");
        // The representative itself still matches directly.
        assert_eq!(s.search_labels("Node 1", 10), vec![1]);
        // nodes_near canonicalises the same way: a query at the member's position
        // returns the representative, and the hidden node never appears.
        let near = s.nodes_near([2.0, 0.0, 0.0], 5.0, 10);
        assert!(near.contains(&1), "member's neighbourhood resolves to rep");
        assert!(!near.contains(&2), "folded member not returned directly");
        assert!(!near.contains(&3), "hidden node excluded from proximity");
    }

    #[test]
    fn query_var_member_is_never_folded_away() {
        let mut s = RenderStore::new();
        for id in [1u32, 2, 3] {
            s.upsert(id, [id as f32, 0.0, 0.0], 0, 0.0, 0.0);
        }
        // Mark node 2 as a query variable, THEN apply a plan that would fold 2,3→1.
        s.set_query_var(2, 0);
        s.set_fold_plan(&[2], &[2, 3], &[1, 1]); // 2 also (wrongly) listed as hidden
        settle(&mut s); // member 3 folds fully; query-var 2 never enters the fold
        // Node 2 must be lifted out of both hide and fold — it draws as itself, so
        // build draws rep 1 (folding only member 3) plus the lifted query var 2.
        let buf = s.build_node_buffer(&[1, 2, 3], 1.0, 0.7, 1.9);
        assert_eq!(buf.len(), 2 * NODE_STRIDE, "rep + lifted query var draw");
        assert_eq!(s.render_ids(), &[1, 2], "query var 2 not hidden, not folded");
        assert_eq!(s.badge_of(1), 1, "only member 3 folded into rep 1");
    }

    #[test]
    fn search_cap_applies_to_unique_visible_representatives() {
        // Regression: several folded members of one group, all matching, must not
        // eat the search quota and starve other visible representatives.
        let mut s = RenderStore::new();
        for id in [1u32, 2, 3, 4, 10, 11] {
            s.upsert(id, [0.0, 0.0, 0.0], 0, 0.0, 0.0);
        }
        // Members 2,3 carry the HIGHEST centralities — under the old cap-then-fold
        // path they'd win both raw slots and collapse to a single rep, returning
        // just [1] and starving nodes 10/11.
        s.upsert(1, [0.0; 3], 0, 0.0, 0.50); // rep, modest centrality
        s.upsert(2, [0.0; 3], 0, 0.0, 0.99);
        s.upsert(3, [0.0; 3], 0, 0.0, 0.98);
        s.upsert(10, [0.0; 3], 0, 0.0, 0.70);
        s.upsert(11, [0.0; 3], 0, 0.0, 0.60);
        for (id, name) in [
            (1u32, "Match Rep"),
            (2, "Match A"),
            (3, "Match B"),
            (4, "Match C"),
            (10, "Match X"),
            (11, "Match Y"),
        ] {
            s.set_meta(id, "".into(), name.into(), "page".into(), "".into());
        }
        s.set_fold_plan(&[], &[2, 3, 4], &[1, 1, 1]); // 2,3,4 → rep 1
        // max=2 must return TWO distinct visible reps: rep 1 (best member centrality
        // 0.99) and node 10 (0.70) — NOT two members of the same fold.
        let hits = s.search_labels("match", 2);
        assert_eq!(hits, vec![1, 10], "cap bounds unique visible reps, not raw members");
    }

    #[test]
    fn clearing_query_var_refolds_without_refetch() {
        // Regression: the raw plan is retained, so a query-var lift-out is reversible
        // — clearing the mark re-folds the node with no server refetch.
        let mut s = RenderStore::new();
        for id in [1u32, 2, 3] {
            s.upsert(id, [id as f32, 0.0, 0.0], 0, 0.0, 0.0);
        }
        s.set_fold_plan(&[], &[2, 3], &[1, 1]); // 2,3 → rep 1
        settle(&mut s); // fold-in completes
        assert_eq!(s.build_node_buffer(&[1, 2, 3], 1.0, 0.7, 1.9).len(), NODE_STRIDE);
        assert_eq!(s.badge_of(1), 2);
        // Mark node 2 → lifted out (grows back out): rep 1 (folding only 3) + node 2.
        s.set_query_var(2, 0);
        settle(&mut s); // node 2 unfolds back to its position
        assert_eq!(s.build_node_buffer(&[1, 2, 3], 1.0, 0.7, 1.9).len(), 2 * NODE_STRIDE);
        assert_eq!(s.badge_of(1), 1);
        // Clear the mark → node 2 RE-FOLDS from the retained raw plan, no refetch.
        s.clear_query_var(2);
        settle(&mut s); // node 2 folds back in
        assert_eq!(s.build_node_buffer(&[1, 2, 3], 1.0, 0.7, 1.9).len(), NODE_STRIDE);
        assert_eq!(s.badge_of(1), 2, "cleared query var re-folds into rep");
    }

    #[test]
    fn community_color_is_deterministic_and_opaque() {
        let a = community_color(5, 0.0, 1);
        let b = community_color(5, 0.0, 1);
        assert_eq!(a, b);
        assert_eq!(a[3], 1.0);
        // Anomaly pushes toward warning red.
        let warn = community_color(5, 1.0, 1);
        assert!(warn[0] > a[0], "anomalous node reddens");
    }

    #[test]
    fn query_var_overlay_recolours_and_flags_then_restores() {
        let mut s = RenderStore::new();
        s.upsert(1, [0.0, 0.0, 0.0], 3, 0.0, 1.0); // community colour, max centrality
        s.upsert(2, [1.0, 0.0, 0.0], 3, 0.0, 1.0);
        let base = s.build_node_buffer(&[1, 2], 1.0, 0.7, 1.9);
        // Unmarked: colour == community colour, query flag (custom.b, offset 18) == 0.
        let community = community_color(3, 0.0, 1);
        assert!(approx(base[12], community[0]) && approx(base[13], community[1]));
        assert!(approx(base[18], 0.0), "unmarked node has no query flag");

        // Mark node 1 as palette 0.
        s.set_query_var(1, 0);
        assert!(s.is_query_var(1) && !s.is_query_var(2));
        let marked = s.build_node_buffer(&[1, 2], 1.0, 0.7, 1.9);
        let qv = query_var_color(0);
        // Node 1 now the query colour with custom.b flagged; node 2 untouched.
        assert!(approx(marked[12], qv[0]) && approx(marked[13], qv[1]) && approx(marked[14], qv[2]));
        assert!(approx(marked[18], 1.0), "marked node sets query flag");
        assert!(approx(marked[NODE_STRIDE + 18], 0.0), "other node unflagged");
        // Fold badge channel (custom.g, offset 17) is untouched by the overlay.
        assert!(approx(marked[17], 0.0));

        // Unmark restores the community colour and clears the flag.
        s.clear_query_var(1);
        let restored = s.build_node_buffer(&[1, 2], 1.0, 0.7, 1.9);
        assert!(approx(restored[12], community[0]) && approx(restored[18], 0.0));
    }

    #[test]
    fn query_var_color_cycles_and_is_opaque() {
        // Palette index wraps at QUERY_PALETTE_LEN and is always opaque.
        assert_eq!(query_var_color(0), query_var_color(QUERY_PALETTE_LEN));
        assert_eq!(query_var_color(3)[3], 1.0);
        assert_ne!(query_var_color(0), query_var_color(1));
    }

    #[test]
    fn clear_query_vars_unmarks_all() {
        let mut s = RenderStore::new();
        s.set_query_var(1, 0);
        s.set_query_var(2, 1);
        assert!(s.is_query_var(1) && s.is_query_var(2));
        s.clear_query_vars();
        assert!(!s.is_query_var(1) && !s.is_query_var(2));
    }

    // Co-proximate label fade: a labelled node leaves the opaque buffer for the
    // transparent one and eases to LABEL_FADE_ALPHA; COLOR.a is float 15.
    #[test]
    fn labelled_node_moves_to_faded_buffer_and_eases_to_label_alpha() {
        let mut s = RenderStore::new();
        s.upsert(1, [0.0, 0.0, 0.0], 0, 0.0, 0.0);
        s.upsert(2, [1.0, 0.0, 0.0], 0, 0.0, 0.0);
        s.set_labelled(&[1]);
        let main = s.build_node_buffer(&[1, 2], 1.0, 0.7, 1.9);
        assert_eq!(main.len(), NODE_STRIDE, "node 1 leaves the opaque buffer");
        assert!(approx(main[15], 1.0), "opaque node keeps alpha 1");
        let faded = s.faded_node_buffer().to_vec();
        assert_eq!(faded.len(), NODE_STRIDE, "node 1 is in the faded buffer");
        assert!(approx(faded[15], 1.0 - LABEL_FADE_STEP), "first build steps once");
        for _ in 0..20 {
            s.build_node_buffer(&[1, 2], 1.0, 0.7, 1.9);
        }
        assert!(approx(s.faded_node_buffer()[15], LABEL_FADE_ALPHA), "settles at 30 %");
        assert!(approx(s.label_alpha_of(1), LABEL_FADE_ALPHA));
        assert!(approx(s.label_alpha_of(2), 1.0));
        // Both endpoints still count as drawn, so the edge between them survives.
        assert_eq!(s.build_edge_buffer(&[1, 2], 1.0).len(), EDGE_STRIDE_TYPED);
        assert_eq!(s.render_ids().len(), 2, "faded node stays hittable");
    }

    #[test]
    fn unlabelled_node_fades_back_then_returns_to_opaque_buffer() {
        let mut s = RenderStore::new();
        s.upsert(1, [0.0, 0.0, 0.0], 0, 0.0, 0.0);
        s.upsert(2, [1.0, 0.0, 0.0], 0, 0.0, 0.0);
        s.set_labelled(&[1]);
        for _ in 0..20 {
            s.build_node_buffer(&[1, 2], 1.0, 0.7, 1.9);
        }
        s.set_labelled(&[]);
        let main = s.build_node_buffer(&[1, 2], 1.0, 0.7, 1.9);
        assert_eq!(main.len(), NODE_STRIDE, "still fading: stays in the faded pass");
        assert!(approx(s.faded_node_buffer()[15], LABEL_FADE_ALPHA + LABEL_FADE_STEP));
        for _ in 0..20 {
            s.build_node_buffer(&[1, 2], 1.0, 0.7, 1.9);
        }
        let main = s.build_node_buffer(&[1, 2], 1.0, 0.7, 1.9);
        assert_eq!(main.len(), 2 * NODE_STRIDE, "fully opaque again: back in main");
        assert!(s.faded_node_buffer().is_empty());
        assert!(approx(s.label_alpha_of(1), 1.0));
    }

    #[test]
    fn set_labelled_replaces_the_set_and_clear_resets_fades() {
        let mut s = RenderStore::new();
        s.upsert(1, [0.0, 0.0, 0.0], 0, 0.0, 0.0);
        s.upsert(2, [1.0, 0.0, 0.0], 0, 0.0, 0.0);
        s.set_labelled(&[1]);
        for _ in 0..3 {
            s.build_node_buffer(&[1, 2], 1.0, 0.7, 1.9); // 1 → 0.7
        }
        s.set_labelled(&[2]); // 1 drops out (fades back), 2 comes in
        s.build_node_buffer(&[1, 2], 1.0, 0.7, 1.9); // 1 → 0.8, 2 → 0.9
        assert!(approx(s.label_alpha_of(1), 0.8) && approx(s.label_alpha_of(2), 0.9));
        assert_eq!(s.faded_node_buffer().len(), 2 * NODE_STRIDE, "both mid-fade");
        s.clear();
        assert!(approx(s.label_alpha_of(1), 1.0) && approx(s.label_alpha_of(2), 1.0));
        assert!(s.faded_node_buffer().is_empty());
    }

    #[test]
    fn plane_node_buffer_offsets_y_and_ignores_overlays() {
        let mut s = RenderStore::new();
        s.upsert(1, [1.0, 2.0, 3.0], 4, 0.0, 1.0);
        // A query mark + fold badge must NOT affect the clean plane copy.
        s.set_query_var(1, 0);
        s.set_fold_plan(&[], &[], &[]);
        let buf = s.build_plane_node_buffer(&[1], 10.0, 1.0, 0.7, 1.9);
        assert_eq!(buf.len(), NODE_STRIDE);
        // Origin y lifted by the offset (transform row1 col3 = index 7).
        assert!(approx(buf[3], 1.0) && approx(buf[7], 12.0) && approx(buf[11], 3.0));
        // Colour is the community colour, NOT the query palette; custom.b flag clear.
        let community = community_color(4, 0.0, 1);
        assert!(approx(buf[12], community[0]) && approx(buf[13], community[1]));
        assert!(approx(buf[18], 0.0), "plane copy carries no query flag");
        // Unknown ids are skipped, not faked.
        assert!(s.build_plane_node_buffer(&[999], 0.0, 1.0, 0.7, 1.9).is_empty());
    }

    #[test]
    fn plane_edge_buffer_offsets_y_and_skips_unknown() {
        let mut s = RenderStore::new();
        s.upsert(1, [0.0, 0.0, 0.0], 0, 0.0, 0.0);
        s.upsert(2, [0.0, 4.0, 0.0], 0, 0.0, 0.0);
        // Both endpoints known → one edge (12 floats); lifted by y_offset.
        let buf = s.build_plane_edge_buffer(&[1, 2], 5.0, 0.1);
        assert_eq!(buf.len(), EDGE_STRIDE);
        // Midpoint y = (0+4)/2 + 5 = 7 (origin y at index 7).
        assert!(approx(buf[7], 7.0));
        // Unknown endpoint → skipped (no drawn-set requirement, just presence).
        assert!(s.build_plane_edge_buffer(&[1, 999], 0.0, 0.1).is_empty());
    }

    // --- Wave 2 Feature 4: relation-type edge grammar -----------------------

    #[test]
    fn edge_style_code_classifies_predicates() {
        // Subclass/taxonomy family → 2 (dashed + dimmer), bare and IRI-qualified.
        assert_eq!(edge_style_code("subclass_of"), 2);
        assert_eq!(edge_style_code("subClassOf"), 2);
        assert_eq!(edge_style_code("rdfs:subClassOf"), 2);
        assert_eq!(edge_style_code("http://www.w3.org/2000/01/rdf-schema#subClassOf"), 2);
        assert_eq!(edge_style_code("is_a"), 2);
        // Any other named predicate → 1 (typed, solid).
        assert_eq!(edge_style_code("references"), 1);
        assert_eq!(edge_style_code("relatedTo"), 1);
        // Empty/whitespace → 0 (untyped, faint).
        assert_eq!(edge_style_code(""), 0);
        assert_eq!(edge_style_code("   "), 0);
    }

    #[test]
    fn edge_style_code_prov_overrides_with_inferred() {
        // Asserted edges keep their predicate-derived code.
        assert_eq!(edge_style_code_prov("subclass_of", false), 2);
        assert_eq!(edge_style_code_prov("references", false), 1);
        assert_eq!(edge_style_code_prov("", false), 0);
        // Inferred edges are code 3 (amber-dashed) regardless of predicate —
        // epistemic status is the dominant visual channel.
        assert_eq!(edge_style_code_prov("subclass_of", true), STYLE_INFERRED);
        assert_eq!(edge_style_code_prov("references", true), STYLE_INFERRED);
        assert_eq!(edge_style_code_prov("", true), STYLE_INFERRED);
        assert_eq!(STYLE_INFERRED, 3);
    }

    #[test]
    fn edge_buffer_carries_style_in_custom_a() {
        let mut s = RenderStore::new();
        s.upsert(1, [0.0, 0.0, 0.0], 0, 0.0, 0.0);
        s.upsert(2, [0.0, 2.0, 0.0], 0, 0.0, 0.0);
        s.upsert(3, [0.0, 4.0, 0.0], 0, 0.0, 0.0);
        s.set_edge_styles(&[1, 2, 2, 3], &[2, 1]); // (1,2)=subclass, (2,3)=typed
        let _ = s.build_node_buffer(&[1, 2, 3], 1.0, 0.7, 1.9);
        let buf = s.build_edge_buffer(&[1, 2, 2, 3], 0.1);
        assert_eq!(buf.len(), 2 * EDGE_STRIDE_TYPED);
        // custom.a is the 16th float of each 16-float instance (index 15).
        assert!(approx(buf[15], 2.0), "subclass edge → style 2 in custom.a");
        assert!(approx(buf[EDGE_STRIDE_TYPED + 15], 1.0), "typed edge → style 1");
        // Direction-insensitive lookup: reversed pair resolves the same style.
        let buf_rev = s.build_edge_buffer(&[2, 1], 0.1);
        assert!(approx(buf_rev[15], 2.0), "style lookup ignores edge direction");
        // Unknown edge → untyped (0).
        s.upsert(4, [0.0, 6.0, 0.0], 0, 0.0, 0.0);
        let _ = s.build_node_buffer(&[3, 4], 1.0, 0.7, 1.9);
        let buf_unknown = s.build_edge_buffer(&[3, 4], 0.1);
        assert!(approx(buf_unknown[15], 0.0), "unregistered edge → untyped");
    }

    // --- Wave 2 Feature 3: type show/hide filter ----------------------------

    #[test]
    fn type_filter_hides_class_and_its_edges() {
        let mut s = RenderStore::new();
        s.upsert(1, [0.0, 0.0, 0.0], 0, 0.0, 0.0);
        s.upsert(2, [0.0, 2.0, 0.0], 0, 0.0, 0.0);
        s.upsert(3, [0.0, 4.0, 0.0], 0, 0.0, 0.0);
        s.set_node_kind(1, KIND_KNOWLEDGE);
        s.set_node_kind(2, KIND_ONTOLOGY);
        s.set_node_kind(3, KIND_KNOWLEDGE);
        // All visible initially.
        assert_eq!(s.build_node_buffer(&[1, 2, 3], 1.0, 0.7, 1.9).len(), 3 * NODE_STRIDE);
        // Hide ontology → node 2 drops; nodes 1,3 remain.
        s.set_type_visible(KIND_ONTOLOGY, false);
        assert!(!s.is_type_visible(KIND_ONTOLOGY));
        let buf = s.build_node_buffer(&[1, 2, 3], 1.0, 0.7, 1.9);
        assert_eq!(buf.len(), 2 * NODE_STRIDE, "hidden-class node dropped");
        assert_eq!(s.render_ids(), &[1, 3]);
        // Edge 1→2 loses an endpoint (2 hidden) → dropped; edge 1→3 survives.
        let ebuf = s.build_edge_buffer(&[1, 2, 1, 3], 0.1);
        assert_eq!(ebuf.len(), EDGE_STRIDE_TYPED, "edge to hidden node removed");
        // Re-show restores it.
        s.set_type_visible(KIND_ONTOLOGY, true);
        assert_eq!(s.build_node_buffer(&[1, 2, 3], 1.0, 0.7, 1.9).len(), 3 * NODE_STRIDE);
    }

    #[test]
    fn type_filter_unknown_kind_is_visible() {
        let mut s = RenderStore::new();
        s.upsert(1, [0.0, 0.0, 0.0], 0, 0.0, 0.0);
        // No kind recorded; hiding a class must not hide an un-kinded node.
        s.set_type_visible(KIND_AGENT, false);
        assert_eq!(s.build_node_buffer(&[1], 1.0, 0.7, 1.9).len(), NODE_STRIDE);
        // Out-of-range class code is a no-op, not a panic.
        s.set_type_visible(9, false);
        assert!(s.is_type_visible(9));
    }

    // --- Wave 2 Feature 1: additive expansion merge -------------------------

    #[test]
    fn append_new_edges_dedups_and_appends() {
        let mut flat = vec![1, 2, 2, 3];
        let mut weights = vec![1.0, 1.0];
        let mut types = vec!["references".to_string(), "".to_string()];
        // New: (2,3) already present (as-is), (3,2) present reversed, (3,4) fresh,
        // (5,5) self-loop dropped, (1,2) present.
        let added = append_new_edges(
            &mut flat,
            &mut weights,
            &mut types,
            &[2, 3, 3, 2, 3, 4, 5, 5, 1, 2],
            &[1.0, 1.0, 2.0, 1.0, 1.0],
            &[
                "x".into(),
                "x".into(),
                "subclass_of".into(),
                "x".into(),
                "x".into(),
            ],
        );
        assert_eq!(added, 1, "only (3,4) is genuinely new");
        assert_eq!(flat, vec![1, 2, 2, 3, 3, 4], "fresh edge appended once");
        assert_eq!(weights.len(), 3);
        assert_eq!(types[2], "subclass_of", "new edge's type carried through");
    }

    #[test]
    fn append_new_edges_registers_styles_on_the_tail() {
        // Simulates the client merge path: append, then register styles for the
        // appended tail so the new edge renders with its predicate style.
        let mut s = RenderStore::new();
        s.upsert(3, [0.0, 0.0, 0.0], 0, 0.0, 0.0);
        s.upsert(4, [0.0, 2.0, 0.0], 0, 0.0, 0.0);
        let mut flat: Vec<i32> = vec![];
        let mut weights: Vec<f32> = vec![];
        let mut types: Vec<String> = vec![];
        let before = flat.len();
        let added = append_new_edges(
            &mut flat,
            &mut weights,
            &mut types,
            &[3, 4],
            &[1.0],
            &["subclass_of".into()],
        );
        assert_eq!(added, 1);
        let tail_pairs = &flat[before..];
        let tail_codes: Vec<u8> = types[before / 2..].iter().map(|t| edge_style_code(t)).collect();
        s.merge_edge_styles(tail_pairs, &tail_codes);
        let _ = s.build_node_buffer(&[3, 4], 1.0, 0.7, 1.9);
        let buf = s.build_edge_buffer(&[3, 4], 0.1);
        assert!(approx(buf[15], 2.0), "merged subclass edge styled after append");
    }

    // --- Wave 2 Feature 2: top-by-centrality for search-teleport -------------

    #[test]
    fn top_by_centrality_ranks_labelled_nodes() {
        let mut s = RenderStore::new();
        for (id, cen) in [(1u32, 0.2), (2, 0.9), (3, 0.5), (4, 0.7)] {
            s.upsert(id, [0.0; 3], 0, 0.0, cen);
            s.set_meta(id, "".into(), format!("Node {id}"), "page".into(), "".into());
        }
        // A node with NO label is ineligible even at high centrality.
        s.upsert(5, [0.0; 3], 0, 0.0, 1.0);
        let top = s.top_by_centrality(3);
        assert_eq!(top, vec![2, 4, 3], "highest-centrality labelled nodes, desc");
        assert!(!s.top_by_centrality(10).contains(&5), "unlabelled node excluded");
        assert!(s.top_by_centrality(0).is_empty(), "max 0 → empty");
    }

    #[test]
    fn top_by_centrality_resolves_through_fold() {
        let mut s = RenderStore::new();
        for (id, cen) in [(1u32, 0.3), (2, 0.9), (3, 0.6)] {
            s.upsert(id, [0.0; 3], 0, 0.0, cen);
            s.set_meta(id, "".into(), format!("Node {id}"), "page".into(), "".into());
        }
        s.set_fold_plan(&[], &[2], &[1]); // member 2 → rep 1
        let top = s.top_by_centrality(10);
        assert!(top.contains(&1), "folded member resolves to its representative");
        assert!(!top.contains(&2), "member never returned directly");
        assert!(top.contains(&3));
    }

    // --- Per-node metadata sizing (desktop parity) --------------------------

    #[test]
    fn compute_degrees_counts_incident_edges() {
        let mut s = RenderStore::new();
        s.compute_degrees(&[1, 2, 1, 3, 2, 3]);
        assert_eq!(s.degree_of(1), 2);
        assert_eq!(s.degree_of(2), 2);
        assert_eq!(s.degree_of(3), 2);
        assert_eq!(s.degree_of(99), 0, "unknown node → degree 0");
        // Additive update (expansion merge path) bumps both endpoints.
        s.add_degrees(&[1, 4]);
        assert_eq!(s.degree_of(1), 3, "additive degree bump on merge");
        assert_eq!(s.degree_of(4), 1);
        // compute_degrees replaces (not accumulates).
        s.compute_degrees(&[1, 2]);
        assert_eq!(s.degree_of(1), 1, "compute_degrees resets before counting");
        assert_eq!(s.degree_of(3), 0);
    }

    #[test]
    fn set_file_size_merges_with_label_meta() {
        let mut s = RenderStore::new();
        s.set_meta(1, "".into(), "Page A".into(), "page".into(), "".into());
        s.set_file_size(1, 4096); // poll sets file_size AFTER set_meta
        assert_eq!(s.file_size_of(1), 4096);
        assert_eq!(s.label_of(1), "Page A", "label preserved alongside file_size");
        // file_size can also arrive before label meta without being clobbered:
        // set_meta replaces the entry, so the poll order (meta then size) is the
        // contract — but an early file_size on a fresh node is retained until then.
        s.set_file_size(2, 128);
        assert_eq!(s.file_size_of(2), 128);
        assert_eq!(s.file_size_of(3), 0, "unknown node → 0");
    }

    #[test]
    fn node_size_zero_metadata_equals_base_band() {
        let mut s = RenderStore::new();
        s.upsert(1, [0.0, 0.0, 0.0], 0, 0.0, 1.0); // max centrality
        s.upsert(2, [1.0, 0.0, 0.0], 0, 0.0, 0.0); // zero centrality
        let buf = s.build_node_buffer(&[1, 2], 2.0, 0.7, 1.9);
        // Zero-metadata node → multiplier 1.0 → exactly the previous size.
        assert!(approx(buf[0], 3.8), "zero-meta max-centrality node keeps base size");
        assert!(approx(buf[NODE_STRIDE], 1.4), "zero-meta min-centrality node keeps base");
    }

    #[test]
    fn node_size_grows_with_degree_and_filesize_then_caps() {
        let mut s = RenderStore::new();
        s.upsert(1, [0.0; 3], 0, 0.0, 0.0); // leaf, zero centrality
        s.upsert(2, [1.0, 0.0, 0.0], 0, 0.0, 1.0); // hub, max centrality
        s.set_file_size(2, 5_000_000);
        let mut edges: Vec<i32> = Vec::new();
        for other in 100..300i32 {
            edges.push(2);
            edges.push(other);
        }
        s.compute_degrees(&edges);
        let comp = 1.0_f32;
        let buf = s.build_node_buffer(&[1, 2], comp, 0.7, 1.9);
        let leaf = buf[0];
        let hub = buf[NODE_STRIDE];
        assert!(hub > leaf, "hub with degree+file is larger than a bare leaf");
        let cap = comp * 1.9 * META_SIZE_CAP_FACTOR;
        assert!(approx(hub, cap), "giant hub clamps to the VR occlusion cap");
    }

    #[test]
    fn node_size_representative_sizes_by_own_metadata_not_group() {
        let mut s = RenderStore::new();
        for id in [1u32, 2, 3] {
            s.upsert(id, [id as f32, 0.0, 0.0], 0, 0.0, 0.0);
        }
        s.set_file_size(2, 9_000_000);
        s.set_file_size(3, 9_000_000);
        let mut edges: Vec<i32> = Vec::new();
        for other in 100..200i32 {
            edges.push(2);
            edges.push(other);
            edges.push(3);
            edges.push(other);
        }
        s.compute_degrees(&edges);
        // Rep 1 drawn with NO fold → its own (zero) metadata size.
        let bare = s.build_node_buffer(&[1], 1.0, 0.7, 1.9)[0];
        // Fold 2,3 → rep 1. The representative is emitted first (index 0); under the
        // animated-fold model members may also stay visible in-transit, but the rep's
        // OWN size must be unchanged — it never inflates to the heavy members' size.
        s.set_fold_plan(&[], &[2, 3], &[1, 1]);
        let folded = s.build_node_buffer(&[1, 2, 3], 1.0, 0.7, 1.9);
        assert_eq!(s.render_ids()[0], 1, "representative drawn first");
        assert!(approx(folded[0], bare), "rep sizes by its own metadata, not the group's");
    }

    // ── ADR-2034: action/state precedence and evidence expiry ──────────────

    #[test]
    fn a_reordered_old_action_cannot_resurrect_a_completed_agent() {
        // The closeout finding: every action set WORKING without checking the
        // timestamp, so a delayed action arriving after a completion flipped the
        // agent back to working and re-drew its beam.
        let mut s = RenderStore::new();
        assert!(s.record_agent_action(5, 20, 0, 1_000, "reading"));
        assert!(s.set_agent_state_at(5, "done", "", 2_000));
        assert_eq!(s.agent_rec(5).unwrap().status, AGENT_DONE);

        // An action stamped BEFORE the completion arrives late. It is dropped
        // whole: status, target and action type all stand.
        assert!(
            !s.record_agent_action(5, 99, 3, 1_500, "stale work"),
            "an action older than the newest evidence must be refused"
        );
        let rec = s.agent_rec(5).unwrap();
        assert_eq!(rec.status, AGENT_DONE, "completion still stands");
        assert_eq!(rec.target_node_id, 20, "stale action did not retarget");
        assert_eq!(rec.task, "reading", "stale action did not rewrite the task");
        assert_eq!(s.agent_actions_stale(), 1);
        // The dropped action is not counted as ingested activity either.
        assert_eq!(s.agent_actions_total(), 1);
    }

    #[test]
    fn a_genuinely_newer_action_still_supersedes_a_completion() {
        // Precedence is by timestamp, not by channel: the state channel does not
        // get to freeze an agent for ever.
        let mut s = RenderStore::new();
        assert!(s.record_agent_action(5, 20, 0, 1_000, ""));
        assert!(s.set_agent_state_at(5, "done", "", 2_000));
        assert!(s.record_agent_action(5, 21, 1, 3_000, "next job"));
        let rec = s.agent_rec(5).unwrap();
        assert_eq!(rec.status, AGENT_WORKING);
        assert_eq!(rec.target_node_id, 21);
        assert_eq!(rec.task, "next job");
    }

    #[test]
    fn an_out_of_order_state_update_is_refused() {
        let mut s = RenderStore::new();
        assert!(s.record_agent_action(5, 20, 0, 5_000, ""));
        // A completion report stamped before the action we already applied.
        assert!(!s.set_agent_state_at(5, "done", "", 4_000));
        assert_eq!(s.agent_rec(5).unwrap().status, AGENT_WORKING);
        assert_eq!(s.agent_states_stale(), 1);
    }

    #[test]
    fn an_untimestamped_state_update_is_treated_as_current() {
        // The JSON channel carries no timestamp; "now" is the only honest reading,
        // so it supersedes evidence already applied but not a later action.
        let mut s = RenderStore::new();
        s.record_agent_action(5, 20, 0, 1_000, "");
        assert!(s.set_agent_state(5, "done", ""));
        assert_eq!(s.agent_rec(5).unwrap().status, AGENT_DONE);
        // A later action still wins.
        assert!(s.record_agent_action(5, 20, 0, 1_001, ""));
        assert_eq!(s.agent_rec(5).unwrap().status, AGENT_WORKING);
    }

    #[test]
    fn timestamp_comparison_survives_the_u32_wrap() {
        // Action timestamps are server ms % u32::MAX and wrap every ~49.7 days.
        // A naive `a > b` would freeze every agent at the wrap point.
        assert!(ts_is_newer(5, u32::MAX - 5), "just after the wrap is newer");
        assert!(!ts_is_newer(u32::MAX - 5, 5), "and the reverse is older");
        assert!(ts_is_newer(1_001, 1_000));
        assert!(!ts_is_newer(1_000, 1_000), "equal is not strictly newer");

        let mut s = RenderStore::new();
        assert!(s.record_agent_action(5, 20, 0, u32::MAX - 5, ""));
        // The clock wraps; this action is genuinely newer and must be applied.
        assert!(s.record_agent_action(5, 21, 0, 5, ""));
        assert_eq!(s.agent_rec(5).unwrap().target_node_id, 21);
    }

    #[test]
    fn stale_evidence_expires_a_live_status_and_removes_its_beam() {
        let mut s = RenderStore::new();
        s.upsert(5, [0.0, 0.0, 0.0], 0, 0.0, 0.0);
        s.upsert(20, [0.0, 4.0, 0.0], 0, 0.0, 0.0);
        s.record_agent_action(5, 20, 0, 1_000, "reading");
        s.build_node_buffer(&[5, 20], 1.0, 0.7, 1.9);
        assert_eq!(s.build_beam_buffer(1.0).len(), EDGE_STRIDE_TYPED, "beam while fresh");

        // Well inside the TTL: nothing changes.
        assert_eq!(s.expire_stale_agents(1_000 + AGENT_EVIDENCE_TTL_MS, AGENT_EVIDENCE_TTL_MS), 0);
        assert_eq!(s.agent_rec(5).unwrap().status, AGENT_WORKING);

        // Past the TTL: demoted to idle, target dropped, beam gone.
        assert_eq!(s.expire_stale_agents(1_001 + AGENT_EVIDENCE_TTL_MS, AGENT_EVIDENCE_TTL_MS), 1);
        let rec = s.agent_rec(5).unwrap();
        assert_eq!(rec.status, AGENT_IDLE);
        assert_eq!(rec.target_node_id, 0);
        assert!(rec.expired);
        assert!(s.build_beam_buffer(1.0).is_empty(), "expired agent draws no beam");
        assert_eq!(s.agent_expiries_total(), 1);
    }

    #[test]
    fn expiry_demotes_blocked_but_leaves_a_reported_completion_alone() {
        let mut s = RenderStore::new();
        s.record_agent_action(5, 20, 0, 1_000, "");
        s.set_agent_state_at(5, "blocked", "", 1_100);
        s.record_agent_action(6, 21, 0, 1_000, "");
        s.set_agent_state_at(6, "done", "", 1_100);

        assert_eq!(s.expire_stale_agents(100_000, AGENT_EVIDENCE_TTL_MS), 1);
        // BLOCKED is a live claim derived from stale evidence — it decays.
        assert_eq!(s.agent_rec(5).unwrap().status, AGENT_IDLE);
        // DONE is a reported outcome, not a live claim — it does not decay.
        assert_eq!(s.agent_rec(6).unwrap().status, AGENT_DONE);
    }

    #[test]
    fn a_future_stamped_action_is_not_expired_by_a_lagging_clock() {
        // If the render clock has not yet reached the action's timestamp the age
        // subtraction wraps; expiry must not read that as ancient.
        let mut s = RenderStore::new();
        s.record_agent_action(5, 20, 0, 50_000, "");
        assert_eq!(s.expire_stale_agents(1_000, AGENT_EVIDENCE_TTL_MS), 0);
        assert_eq!(s.agent_rec(5).unwrap().status, AGENT_WORKING);
    }

    #[test]
    fn refreshed_evidence_clears_the_expired_marker() {
        let mut s = RenderStore::new();
        s.record_agent_action(5, 20, 0, 1_000, "");
        s.expire_stale_agents(100_000, AGENT_EVIDENCE_TTL_MS);
        assert!(s.agent_rec(5).unwrap().expired);
        // The agent comes back: a fresh action re-arms it.
        assert!(s.record_agent_action(5, 22, 0, 100_001, ""));
        let rec = s.agent_rec(5).unwrap();
        assert!(!rec.expired);
        assert_eq!(rec.status, AGENT_WORKING);
        assert_eq!(rec.target_node_id, 22);
    }
}
