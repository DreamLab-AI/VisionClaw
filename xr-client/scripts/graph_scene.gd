extends Node3D

const AVATAR_TEMPLATE_PATH := "res://scenes/Avatar.tscn"
const AGENT_TEMPLATE_PATH := "res://scenes/AgentAvatar.tscn"

# Radius the selection arbiter uses for an agent core (mesh radius + a small
# acquisition margin). The Rust dwell resolver applies its own target-size floor.
const AGENT_SELECT_RADIUS: float = 0.2

# The dot(camera_forward, dir_to_agent) above which the user is deemed to be
# gazing at an agent — a ~25° head-gaze cone (cos 25° ≈ 0.906), feeding the
# agent's mutual-gaze model.
const MUTUAL_GAZE_DOT: float = 0.9

# HTTP origin for the intervention decide POST, derived from the ws:// backend
# base by scheme swap (ws→http, wss→https) unless XR_BACKEND_HTTP overrides.

# Reconnect with exponential backoff, unbounded: a Quest sleeping in its case
# must rejoin when it wakes, however long that takes.
const RECONNECT_BASE_DELAY_SEC: float = 2.0
const RECONNECT_MAX_DELAY_SEC: float = 60.0

# Quest render budgets. When the graph exceeds these, the most important
# nodes (by server-computed centrality) and the heaviest edges (by weight)
# are kept — same importance language as the desktop client.
# Instance budgets are now RUNTIME-DERIVED from the received topology (the backend
# serves a settings-driven initial load that can exceed any fixed constant), not
# hardcoded quality gates. Node budget = topology node count, edge budget =
# topology edge count, each bounded by an absolute safety ceiling so a runaway
# payload can't blow the Quest instance buffers. See _recompute_instance_budgets.
const NODE_SAFETY_CEILING: int = 20000
const EDGE_SAFETY_CEILING: int = 20000

# XR fit-to-view: the server streams graph coordinates spanning hundreds of
# metres centred nowhere near the user. A flat client frames its camera to the
# graph AABB; XR cannot move the user's physical floor, so instead we scale and
# recentre the whole graph (GraphRoot) into a room-sized volume anchored in
# front of the standing user. Recomputed while the layout is still settling,
# then latched so it does not jitter once stable.
const GRAPH_TARGET_SPAN: float = 2.4          # metres — longest AABB axis maps to this
const GRAPH_ANCHOR := Vector3(0.0, 1.3, -1.6) # in front of XROrigin, ~chest height
const GRAPH_REFIT_SETTLE_SEC: float = 6.0     # keep refitting for this long after first nodes

# GraphRoot's fit-scale shrinks node/edge geometry along with positions, so a
# raw 0.5 m node sphere becomes sub-pixel. Nodes and edges compensate by the
# inverse of the fit scale to hold a fixed apparent world size. Mesh radii come
# from GraphScene.tscn (SphereMesh r=0.5, CylinderMesh r=0.03).
const NODE_MESH_RADIUS: float = 0.5
const NODE_WORLD_RADIUS: float = 0.03         # ~3 cm node in the room
const EDGE_MESH_RADIUS: float = 0.03
const EDGE_WORLD_RADIUS: float = 0.0015       # ~1.5 mm edge tube
const BEAM_MESH_RADIUS: float = 0.02          # agent_beam CylinderMesh authoring radius

# Grab hysteresis: engagement is decided Rust-side (XrInteraction
# ACTIVATION_THRESHOLD = 0.7); release happens here when the trigger falls
# below this lower bound, so a half-pulled trigger can't flicker drag
# start/end at the boundary.
const GRAB_RELEASE: float = 0.4

# Controller locomotion: trackpad/stick slides the XR rig through the graph
# volume in the head-facing horizontal plane.
const LOCOMOTION_SPEED: float = 1.5   # metres/second at full deflection
const LOCOMOTION_DEADZONE: float = 0.15

# Backend endpoint resolution (env-overridable). The Rust hot-path crate owns the
# wire; GDScript only supplies URLs/credentials and pumps the inbox each frame.
const DEFAULT_BACKEND_WS := "ws://localhost:4000"
const GRAPH_STREAM_PATH := "/wss"
const PRESENCE_PATH := "/ws/presence"
const DEFAULT_ROOM_URN := "urn:visionclaw:room:sha256-12-deadbeefcafe"
const DEFAULT_DISPLAY_NAME := "Quest User"

# RES-a / ADR-130 D3 liveness canary the on-device selection loop fires once,
# the first time the selection arbiter resolves a non-origin agent-node
# selection. Recorded via POST /api/canary/observe/{id} (unauthenticated JSON).
const CANARY_M4_RAY := "CANARY-VC-M4-RAY"

var _binary_client: RefCounted = null
var _presence_client: RefCounted = null
var _interaction: RefCounted = null
var _lod_policy: RefCounted = null
var _voice_router: RefCounted = null
var _proxemics: RefCounted = null
var _selection: RefCounted = null
var _gaze_tracker: RefCounted = null
var _nostr_auth: RefCounted = null
# True when a real XR_NOSTR_SECRET was supplied — gates NIP-98 auth on the physics
# HTTP writes (the dev bearer 401s in release builds where the token literal is
# stripped from the binary).
var _nostr_secret_present: bool = false

# Agent embodiments (M3), keyed by agent id, distinct from human peer _avatars.
var _agents: Dictionary = {}
var _agent_order: PackedStringArray = PackedStringArray()
var _agent_cases: Dictionary = {}  # agent_id -> {case_id, summary}
# Integer handles bridge String agent ids to the Rust arbiter's u32 candidate
# ids. Handles start at 1 so 0 stays reserved for "no selection".
var _agent_by_handle: Dictionary = {}  # int handle -> agent_id
var _next_handle: int = 1
# Proxemics re-solve memo: last user pose + agent count the arc was solved for.
var _last_solve_pos: Vector3 = Vector3.ZERO
var _last_solve_forward: Vector3 = Vector3.FORWARD
var _last_solve_count: int = -1
var _eye_gaze_supported: bool = false
# Once-per-frame LOD recompute decision, set by _update_lod, read by _update_agents.
var _lod_recompute: bool = false

var _avatars: Dictionary = {}
# Node positions/targets now live in the Rust render store (BinaryProtocolClient):
# it owns the hunt and packs the MultiMesh buffers, so the per-frame 13k-entry
# GDScript loops are gone (PRD-008 perf). GDScript keeps only centrality (for the
# LOD/edge selection domain) mirrored from the throttled node_visuals signal.
var _node_centrality: Dictionary = {}
# Running max centrality, used to normalise per-node size/halo tells. Monotonic
# (never shrinks while a graph is loaded) so a settled top node keeps a stable
# reference; updated only when visuals arrive, never per frame. Seeded > 0 to
# avoid a divide-by-zero before the first analytics frame.
var _centrality_max: float = 0.0001
# Edge topology from initialGraphLoad, pre-capped by weight:
# flat [src0, tgt0, src1, tgt1, ...].
var _edge_pairs: PackedInt32Array = PackedInt32Array()

# LOD-selected drawn node ids (topology-biased, budget-capped). Cached and only
# recomputed when the selection domain changes (visuals/topology/budget) — the
# per-frame buffer build reuses it. Passed straight to build_node_buffer.
var _drawn_ids: PackedInt32Array = PackedInt32Array()
var _selection_dirty: bool = true
# Server-space target for the grabbed node, handed to the Rust hunt each frame so
# the dragged node tracks the wand. Updated by _update_interaction.
var _grab_target_server: Vector3 = Vector3.ZERO

# --- Proximity node-label overlay -------------------------------------------
# A small pool of world-space Label3D anchors shown only for the nodes closest to
# the camera, so text stays sparse. World-space (positioned at graph_root xform ×
# server_pos) so text size is constant in metres regardless of the fit scale.
const LABEL_POOL_SIZE: int = 12
const LABEL_UPDATE_SEC: float = 0.25          # 4 Hz refresh, not per frame
const LABEL_PROXIMITY_M: float = 0.5          # world-metre radius (outer fade edge)
const LABEL_INNER_M: float = 0.25             # full-opacity radius
const LABEL_TITLE_PX: float = 0.0006          # pixel_size → ~1.9 cm title text
const LABEL_TITLE_FONT: int = 32
const LABEL_DETAIL_FONT: int = 20
const LABEL_DETAIL_OFFSET_M: float = 0.028
# Label overlays with a distance-fade alpha at or below this are treated as not
# showing: the node's own fade (Rust LABEL_FADE_ALPHA, 30 %) tracks exactly the
# labels the user can actually read.
const LABEL_VISIBLE_MIN_A: float = 0.05
var _label_pool: Array = []                   # Array[Node3D] anchors, each Title+Detail
var _label_accum: float = 0.0

var _graph_ws_url: String = ""
var _presence_ws_url: String = ""
var _room_urn: String = ""
var _display_name: String = ""
var _nostr_secret_hex: String = ""
# Per-socket reconnect state. The graph (/wss) and presence (/ws/presence)
# sockets fail and recover independently, so each owns its own backoff counter
# and pending timer. Sharing one counter+timer (the old design) let a pending
# graph reconnect tear down a live presence socket and vice versa.
var _graph_reconnect_attempts: int = 0
var _graph_reconnect_timer: float = -1.0
var _presence_reconnect_attempts: int = 0
var _presence_reconnect_timer: float = -1.0

# One-shot latch for the M4-RAY canary (fires on the first resolved agent
# selection). The HTTPRequest is created in _ready so the POST never blocks the
# scene-tree thread.
var _m4_ray_observed: bool = false
var _observe_http: HTTPRequest = null

# Server-authoritative drag state.
var _grabbed_id: int = -1
var _grab_controller: XRController3D = null
var _grab_distance: float = 1.0
var _last_targeted_id: int = -1

# Double-click (node-open) detection: two grabs of the same node within the window.
const DOUBLE_CLICK_SEC: float = 0.45
var _dc_last_id: int = -1
var _dc_last_time: float = 0.0

# Movable world-anchored HUD state.
var _hud_grab_controller: XRController3D = null
var _hud_grab_offset: Transform3D = Transform3D.IDENTITY

# --- HUD control-panel state (M2+ operator controls) -------------------------
# Physics params tracked locally (server is authoritative but has no read-back on
# this path); seeded to the desktop defaults and mutated by Spread+/- presses.
var _repel_k: float = 190.0
var _rest_length: float = 85.0
var _physics_http: HTTPRequest = null
# One physics write in flight at a time: _physics_http is single-request, so gate
# presses until request_completed and only commit local repelK/restLength on a
# successful dispatch (a dropped ERR_BUSY press must not drift the tracked values).
var _physics_pending: bool = false
# Runtime visual factors (client-side only, no HTTP).
var _node_size_factor: float = 1.0
# Hierarchy / view physics state (server-routed via _physics_http, same one-in-flight
# gate as Spread). Seeded to the backend defaults: DAG rank-bias off, 60-unit shell
# spacing, full 3D (axisCompressionZ 1.0). Committed only on a successful PUT dispatch.
var _dag_bias_on: bool = false
var _dag_level_distance: float = 60.0
var _z_compression: float = 1.0
var _plane_bias_k := 0.0
var _plane_spacing := 60.0
const DAG_BIAS_ON_K: float = 0.6
const DAG_LEVEL_DISTANCE_MIN: float = 20.0
const DAG_LEVEL_DISTANCE_MAX: float = 200.0
const DAG_LEVEL_DISTANCE_STEP: float = 20.0
const PLANE_BIAS_ON_K: float = 1.0
const PLANE_SPACING_MIN: float = 0.0
const PLANE_SPACING_STEP: float = 20.0
const Z_COMPRESSION_FLAT: float = 0.3
const Z_COMPRESSION_FULL_3D: float = 1.0
# ADR-141 Phase 1 — constrained-layout engine. Cycling picker over the backend
# LayoutMode enum (serde camelCase); POSTed to /api/layout/mode via the same
# single-in-flight + auth-header path as the physics writes.
const LAYOUT_MODES: Array = ["forceDirected", "hierarchical", "radial", "spectral", "temporal", "clustered"]
const LAYOUT_MODE_LABELS: Array = ["Force", "Hierarchical", "Radial", "Spectral", "Temporal", "Clustered"]
var _layout_mode_idx: int = 0
# Node ids this session's operator has pinned via a drag (drag-end adds; Unpin All
# releases them but only removes each on its nodeUnpinAck so unacked ids can be
# retried). Dictionary used as a set (id → true) for O(1) add/dedupe.
var _pinned_ids: Dictionary = {}
# Physics values staged at PUT-dispatch time, keyed by the script member var name
# → intended value. They are committed (applied to the member vars) ONLY when the
# PUT returns 2xx in _on_physics_completed; a 401/timeout/500 discards them so the
# tracked state (and the HUD it drives) never diverges from the backend.
var _physics_staged: Dictionary = {}
# Operator-readable reason for the LAST failed server write ("" after a success).
# Shown on the Graph-tab status line and flashed in the HUD's bottom strip, so a
# rejected write (the dev-headset 401/403 trap) is visible in-HMD, not just in
# the log.
var _last_write_error: String = ""

# --- Fold-level ladder (Wave 3, Phase 2) ------------------------------------
# Per-view density level: 0 = ∅ (everything), 1 = hide low-signal, 2 = fold
# subclass chains, 3 = community fold. Local view state — NOT server-routed, so
# two viewers of the same session can hold different fold levels (same posture
# as _node_size_factor / _edge_show_count). Stepped via the HUD [Fold +]/[Fold -]
# buttons; each step GETs /api/graph/fold and applies the plan to the Rust store.
const FOLD_LEVEL_MIN: int = 0
const FOLD_LEVEL_MAX: int = 3
var _fold_level: int = 0
var _fold_http: HTTPRequest = null
# One fold fetch in flight at a time; the requested level is staged and committed
# to _fold_level only on a 2xx, mirroring the physics one-in-flight discipline.
var _fold_pending: bool = false
var _fold_staged_level: int = 0
# Full topology kept verbatim so the edge ranking can be recomputed once centrality
# analytics arrive (topology often precedes them, which would otherwise pin the
# ranking to the global-weight fallback forever).
var _edge_pairs_full: PackedInt32Array = PackedInt32Array()
var _edge_weights_full: PackedFloat32Array = PackedFloat32Array()
# Latches true after the one-shot re-rank fires (once centrality is populated) so
# the O(E log E) rank runs at most twice: once at topology, once when analytics land.
var _edge_rerank_done: bool = false
# Full weight-ranked edge list; Edges+/- re-slice the rendered subset from this
# without re-sorting or re-fetching. _edge_pairs is a prefix of it.
var _edge_pairs_ranked: PackedInt32Array = PackedInt32Array()
var _edge_show_count: int = 0
# Runtime instance budgets derived from the received topology (see
# _recompute_instance_budgets). Seeded to the safety ceilings so that before any
# topology arrives the client draws everything the position stream gives it (up to
# the ceiling) rather than an arbitrary constant.
var _node_budget: int = NODE_SAFETY_CEILING
var _edge_budget: int = EDGE_SAFETY_CEILING
# Alternating-frame phase for the node/edge multimesh rebuilds (see
# _physics_process): true → nodes, false → edges.
var _mm_phase: bool = false
# Set of node ids that appear as an edge endpoint (built from _edge_pairs_full on
# topology arrival). The LOD draw domain and the edge-ranking domain are both
# restricted to these, so drawn nodes and drawable edges stay coherent.
var _topo_ids: Dictionary = {}
# Centrality bias added to topo (edge-endpoint) nodes so the LOD cap fills with
# them first. Far above any real centrality (0..1) so ordering among topo nodes is
# still by their true centrality, and non-topo nodes only take leftover slots.
const TOPO_SELECT_BIAS: float = 1.0e6

# HUD pointer (controller-ray → SubViewport) state. Edge-detect the trigger so one
# pull = one click; track the last hovered controller for haptic feedback.
var _hud_trigger_was_down: bool = false
# Last valid viewport pixel the pointer sat at, so a click held while the ray
# leaves the panel (or the HUD is grabbed) can be released at the right spot
# instead of latching the SubViewport button-down state.
var _hud_last_px: Vector2 = Vector2.ZERO
# The controller whose aim ray is currently on the HUD panel (else null). Set each
# frame by _update_hud_pointer and read by _update_interaction to suppress node-grab
# engagement for that controller — a trigger pull aimed at the HUD is a button click,
# not a node grab.
var _hud_pointer_controller: XRController3D = null
# QuadMesh_hud size (HUD.tscn HudPanel) — the ray→UV mapping needs the panel extent.
const HUD_QUAD_W: float = 1.4
# HUD.tscn QuadMesh_hud height. 0.875 gives aspect 1.6, matching the 1280×800
# SubViewport exactly so the ray→UV mapping below is undistorted (task #20 redesign;
# was 0.9 against a 1024×640 viewport — a latent aspect mismatch).
const HUD_QUAD_H: float = 0.875
# Physics param clamps.
const REPEL_K_MIN: float = 20.0
const REPEL_K_MAX: float = 1000.0
const REST_LENGTH_MIN: float = 10.0
const REST_LENGTH_MAX: float = 400.0
const NODE_SIZE_FACTOR_MIN: float = 0.3
const NODE_SIZE_FACTOR_MAX: float = 3.0
const EDGE_SHOW_MIN: int = 200
# Dev bearer the codebase uses for physics settings writes.
const PHYSICS_BEARER: String = "Bearer dev-session-token"

# Radial node context menu (flagship visual query builder). Shared node menu:
# the A/X face button opens it on the wand-targeted node; its items combine the
# query-builder "Mark as ?vN / Clear variable" actions with future Wave-2 expand
# items. Instanced at runtime from RadialMenu.tscn so the scene file stays as-is.
const RADIAL_SCENE_PATH := "res://scenes/RadialMenu.tscn"
const RADIAL_QUAD: float = 0.6              # MenuPanel QuadMesh size (RadialMenu.tscn)
## Radial→mark chain instrumentation (retained, silenced). Flip true to trace the
## whole wand-click → item_selected → mark path in the HP log when diagnosing the
## marking flow; RadialMenu mirrors this via set_debug().
const QB_DEBUG: bool = false
var _radial: Node3D = null
const QueryBuilderScript := preload("res://scripts/query_builder.gd")
var _query := QueryBuilderScript.new()
var _radial_ax_was_down: bool = false       # A/X edge-detect (open/close toggle)
var _radial_pointer_controller: XRController3D = null  # controller aiming at the menu
var _radial_trigger_was_down: bool = false  # menu-click edge-detect for a clean release
var _radial_last_px: Vector2 = Vector2.ZERO
var _radial_pick_capture: int = -1          # node id captured during an on-demand hover pick
var _radial_picking: bool = false           # gate so _on_node_targeted routes to the pick

# Live count preview (Phase C). A debounced countOnly POST to
# /api/graph/query/pattern on every pattern change; mirrors the fold work's
# staged-request discipline (single in-flight superseded by cancel_request, finite
# timeout, staged commit on 2xx). -1 count = unknown/pending.
const QUERY_DEBOUNCE_SEC: float = 0.4
const QUERY_PREVIEW_LIMIT: int = 24
var _query_http: HTTPRequest = null
var _query_dirty: bool = false
var _query_debounce: float = 0.0
var _query_count: int = -1
var _query_truncated: bool = false
var _query_pending: bool = false
# Monotonic pattern revision: bumped on every pattern change, captured at POST
# time, and compared on completion so a stale in-flight count can't overwrite a
# newer pattern's result.
var _query_revision: int = 0
var _query_sent_revision: int = -1
# Per-frame pointer arbitration: a controller whose ray is nearer the radial menu
# owns the radial (and yields the HUD); nearer the HUD owns the HUD. One trigger
# pull can therefore never drive both overlapping panels.
var _hud_owner: XRController3D = null
var _radial_owner: XRController3D = null

# Semantic planes (Phase D). Execute POSTs the full pattern; each binding spawns a
# result subgraph on a +Y-stacked plane via PlaneManager. Separate HTTPRequest so a
# heavy execute never blocks the debounced count preview.
const PLANE_LIMIT: int = 24
const PLANE_GAP_M: float = 0.5   # target world-metre gap between layers (pre-fit-scaled)
const PlaneManagerScript := preload("res://scripts/plane_manager.gd")
var _planes = null  # PlaneManagerScript instance
var _exec_http: HTTPRequest = null
var _exec_pending: bool = false

# Wave 2, Feature 1 — predicate expansion in the node radial. When the radial opens
# on a node we GET its relations and repopulate the ring with "→ label (N)" /
# "← label (N)" items (action grammar "expand:<direction>:<edgeType>"). Selecting
# one POSTs /expand and additively merges the returned edges (no re-fit).
const EXPAND_LIMIT: int = 25
var _relations_http: HTTPRequest = null
var _expand_http: HTTPRequest = null
var _radial_node_id: int = -1              # node the radial is currently open on
var _radial_world_pos: Vector3 = Vector3.ZERO
var _relations_cache: Dictionary = {}      # node_id -> Array[extra_item dict]

# Wave 2, Feature 2 — search-and-teleport. A menu-button press on empty space opens
# a "top labels" radial (top centrality); selecting glides the XROrigin so the node
# sits TELEPORT_FRONT_M in front of the user, then pulse-highlights it.
const TOP_LABELS_COUNT: int = 10
const TELEPORT_FRONT_M: float = 1.0
const TELEPORT_GLIDE_SEC: float = 0.5
const TELEPORT_PULSE_SEC: float = 1.2
var _teleport_active: bool = false
var _teleport_from: Transform3D = Transform3D.IDENTITY
var _teleport_to: Transform3D = Transform3D.IDENTITY
var _teleport_t: float = 0.0
var _teleport_pulse_id: int = -1
var _teleport_pulse_t: float = 0.0
var _teleport_pulse_applied: bool = false

@onready var graph_root: Node3D = $GraphRoot
@onready var nodes_multi: MultiMeshInstance3D = $GraphRoot/NodesMulti
# Transparent twin of NodesMulti (gem_faded.tres): the nodes whose proximity/grab
# label is showing are packed here by Rust at 30 % alpha so the co-located text
# reads through the sphere. Optional — the scene works without it (no fade).
@onready var nodes_faded_multi: MultiMeshInstance3D = get_node_or_null("GraphRoot/NodesFadedMulti")
@onready var edges_multi: MultiMeshInstance3D = $GraphRoot/EdgesMulti
# Work-beam layer (ADR-140, Pillar 2 / P3): the reserved AgentMulti MultiMesh, now
# carrying one cylinder per active agent→target-node beam (agent_beam material).
@onready var agent_multi: MultiMeshInstance3D = $GraphRoot/AgentMulti
@onready var avatar_spawner: Node3D = $GraphRoot/AvatarSpawner
@onready var agent_spawner: Node3D = $GraphRoot/AgentSpawner
@onready var left_controller: XRController3D = $XROrigin3D/LeftController
@onready var right_controller: XRController3D = $XROrigin3D/RightController
@onready var hud: Node3D = get_node_or_null("XROrigin3D/XRCamera3D/HUD")

signal node_targeted_in_scene(node_id: int)
signal avatar_count_changed(count: int)
signal agent_selected(agent_id: String, did_nostr: String)
signal connection_status_changed(connected: bool)


func _ready() -> void:
	# The flat fallback camera is only for non-XR (desktop diagnostic / no-HMD
	# fallback). In an XR session the XRCamera3D must drive both eyes, so release
	# the flat camera's `current` flag to avoid it stealing the viewport.
	var flat_cam: Camera3D = get_node_or_null("FlatFallbackCamera")
	if flat_cam != null:
		# XR session drives both eyes via XRCamera3D — the flat camera must NOT be
		# current or it steals the viewport and the HMD layer goes empty (SteamVR
		# falls back to its home environment). Only make it current for a non-XR
		# desktop/diagnostic run.
		flat_cam.current = not get_viewport().use_xr
	# Detach the HUD from the camera so it stops following the gaze; re-anchor it
	# in the play space as a world-fixed panel the user can grab and reposition
	# with a wand (see _update_hud_grab). Deferred so we don't reparent while the
	# XR node tree is still being built.
	if hud != null:
		_reparent_hud_to_world.call_deferred()
	# gdext classes are #[class(no_init)] in Rust — construct via their static create() factory, not ClassDB.instantiate() (which cannot build a no_init class).
	_binary_client = BinaryProtocolClient.create()
	_presence_client = PresenceClientNode.create()
	_interaction = XrInteraction.create()
	_lod_policy = LodPolicy.create()
	_voice_router = SpatialVoiceRouter.create()
	_proxemics = ProxemicsSolver.create()
	_selection = SelectionArbiterNode.create()
	_gaze_tracker = GazeTracker.create()
	# One signing identity for both the graph socket and the intervention POST.
	# NostrAuth.create always returns a signer (ephemeral if the secret is empty),
	# so track whether a REAL secret was supplied: only then can NIP-98 authenticate
	# as a power user; otherwise we fall back to the dev bearer (dev flow).
	var _nostr_secret := OS.get_environment("XR_NOSTR_SECRET").strip_edges()
	_nostr_secret_present = not _nostr_secret.is_empty()
	_nostr_auth = NostrAuth.create(_nostr_secret)

	if _selection != null and _selection.has_signal("selection_made"):
		_selection.connect("selection_made", Callable(self, "_on_selection_made"))

	if _binary_client != null:
		# position_updated is intentionally NOT connected: positions live in the Rust
		# render store now (no per-node signal storm). Only the throttled
		# node_visuals_updated crosses the boundary, for the centrality mirror.
		_binary_client.connect("connection_changed", Callable(self, "_on_connection_changed"))
		_binary_client.connect("node_visuals_updated", Callable(self, "_on_node_visuals_updated"))
		_binary_client.connect("topology_updated", Callable(self, "_on_topology_updated"))
		if _binary_client.has_signal("text_message"):
			_binary_client.connect("text_message", Callable(self, "_on_graph_text"))
	if _presence_client != null:
		_presence_client.connect("avatar_joined", Callable(self, "_on_avatar_joined"))
		_presence_client.connect("avatar_left", Callable(self, "_on_avatar_left"))
		_presence_client.connect("avatar_pose_updated", Callable(self, "_on_avatar_pose_updated"))
		if _presence_client.has_signal("connection_changed"):
			_presence_client.connect("connection_changed", Callable(self, "_on_presence_connection_changed"))
		if _presence_client.has_signal("presence_kicked"):
			_presence_client.connect("presence_kicked", Callable(self, "_on_presence_kicked"))
	if _voice_router != null and _voice_router.has_signal("voice_activity"):
		_voice_router.connect("voice_activity", Callable(self, "_on_voice_activity"))
	if _interaction != null:
		_interaction.connect("node_targeted", Callable(self, "_on_node_targeted"))
		_interaction.connect("node_grabbed", Callable(self, "_on_node_grabbed"))

	# Instance the shared radial node menu (visual query builder + future expand).
	var radial_scene: PackedScene = load(RADIAL_SCENE_PATH)
	if radial_scene != null:
		_radial = radial_scene.instantiate()
		add_child(_radial)
		if _radial.has_signal("item_selected"):
			_radial.item_selected.connect(_on_radial_item_selected)
		if _radial.has_method("set_debug"):
			_radial.set_debug(QB_DEBUG)

	# Dedicated request for the live query-count preview (separate gate from the
	# fold/physics/observe requests so none can block the others).
	_query_http = HTTPRequest.new()
	add_child(_query_http)
	_query_http.request_completed.connect(_on_query_count_completed)
	_query_http.timeout = 10.0

	# Wave 2, Feature 1 — node-radial predicate expansion. Two dedicated requests so
	# neither the relations fetch nor the expand POST can block the query/fold gates.
	_relations_http = HTTPRequest.new()
	add_child(_relations_http)
	_relations_http.request_completed.connect(_on_relations_completed)
	_relations_http.timeout = 8.0
	_expand_http = HTTPRequest.new()
	add_child(_expand_http)
	_expand_http.request_completed.connect(_on_expand_completed)
	_expand_http.timeout = 12.0

	# Execute request + the semantic-plane manager (parented under GraphRoot so plane
	# +Y offsets ride the same fit transform as the graph).
	_exec_http = HTTPRequest.new()
	add_child(_exec_http)
	_exec_http.request_completed.connect(_on_execute_completed)
	_exec_http.timeout = 15.0
	_planes = PlaneManagerScript.new()
	_planes.name = "SemanticPlanes"
	if graph_root != null:
		graph_root.add_child(_planes)
		var node_mesh: Mesh = nodes_multi.multimesh.mesh if nodes_multi != null and nodes_multi.multimesh != null else null
		var edge_mesh: Mesh = edges_multi.multimesh.mesh if edges_multi != null and edges_multi.multimesh != null else null
		var node_mat: Material = nodes_multi.material_override if nodes_multi != null else null
		var edge_mat: Material = edges_multi.material_override if edges_multi != null else null
		_planes.configure(_binary_client, node_mesh, edge_mesh, node_mat, edge_mat)
	if hud != null:
		if hud.has_signal("query_execute_pressed"):
			hud.query_execute_pressed.connect(_execute_query)
		if hud.has_signal("query_clear_pressed"):
			hud.query_clear_pressed.connect(_clear_query)

	# HTTPRequest for the M4-RAY liveness observe POST (created at runtime so the
	# scene file stays script-only; the request is async and non-blocking).
	_observe_http = HTTPRequest.new()
	add_child(_observe_http)

	# Dedicated request for the HUD physics controls (separate from the observe
	# POST and the HUD decide POST so none of them can block the others).
	_physics_http = HTTPRequest.new()
	add_child(_physics_http)
	# Bound + gated: request_completed reopens the in-flight gate; a finite timeout
	# guarantees it fires even on a stalled connection so the gate never latches.
	_physics_http.request_completed.connect(_on_physics_completed)
	_physics_http.timeout = 10.0

	# Dedicated request for the fold-ladder GET (own gate, so a fold fetch never
	# blocks or is blocked by the physics PUT path).
	_fold_http = HTTPRequest.new()
	add_child(_fold_http)
	_fold_http.request_completed.connect(_on_fold_completed)
	_fold_http.timeout = 10.0

	_init_label_pool()
	_probe_eye_gaze()
	_wire_hud()
	_connect_from_env()


# Build the reusable Label3D pool once. Anchors live in world space (siblings of
# GraphRoot) so their metre-sized text is unaffected by the graph fit scale.
func _init_label_pool() -> void:
	for i: int in range(LABEL_POOL_SIZE):
		var anchor := Node3D.new()
		anchor.name = "NodeLabel%d" % i
		anchor.visible = false
		var title := Label3D.new()
		title.name = "Title"
		title.billboard = BaseMaterial3D.BILLBOARD_ENABLED
		title.pixel_size = LABEL_TITLE_PX
		title.font_size = LABEL_TITLE_FONT
		title.outline_size = 8
		title.modulate = Color(1.0, 1.0, 1.0, 1.0)
		anchor.add_child(title)
		var detail := Label3D.new()
		detail.name = "Detail"
		detail.billboard = BaseMaterial3D.BILLBOARD_ENABLED
		detail.pixel_size = LABEL_TITLE_PX
		detail.font_size = LABEL_DETAIL_FONT
		detail.outline_size = 6
		detail.position = Vector3(0.0, -LABEL_DETAIL_OFFSET_M, 0.0)
		detail.modulate = Color(0.8, 0.85, 0.95, 1.0)
		anchor.add_child(detail)
		add_child(anchor)
		_label_pool.append(anchor)


# Show labels for the nodes nearest the camera (plus the grabbed node), fading with
# distance. Runs at LABEL_UPDATE_SEC cadence; reuses the pool (no allocation churn
# beyond the one nodes_near query array).
func _update_proximity_labels() -> void:
	if _label_pool.is_empty():
		return
	if _binary_client == null or graph_root == null or not _binary_client.has_method("nodes_near"):
		_hide_all_labels()
		return
	var cam: XRCamera3D = _find_xr_camera()
	if cam == null:
		_hide_all_labels()
		return
	var cam_world: Vector3 = cam.global_position
	var gxf: Transform3D = graph_root.global_transform
	var cam_server: Vector3 = gxf.affine_inverse() * cam_world
	# Radius tracks the fit scale: a fixed world-metre reach maps to a larger
	# server-space radius when the graph is scaled down.
	var radius_server: float = LABEL_PROXIMITY_M / maxf(_graph_scale, 0.0001)
	# Query more than the pool so unlabelled near-nodes don't starve labelled ones.
	var near: PackedInt32Array = _binary_client.nodes_near(cam_server, radius_server, LABEL_POOL_SIZE * 2)
	# Build the shown list: grabbed node first (always labelled), then nearest
	# labelled nodes up to the pool size.
	var shown: Array = []
	if _grabbed_id != -1:
		shown.append(_grabbed_id)
	# Query variables are always labelled (with their ?vN badge), regardless of
	# proximity, so the assembled pattern stays legible while the user works.
	for mid: int in _query.marked_ids():
		if shown.size() >= LABEL_POOL_SIZE:
			break
		if not shown.has(mid):
			shown.append(mid)
	for id: int in near:
		if shown.size() >= LABEL_POOL_SIZE:
			break
		if shown.has(id):
			continue
		if _binary_client.label_of(id) != "":
			shown.append(id)
	var labelled: PackedInt32Array = PackedInt32Array()
	for i: int in range(LABEL_POOL_SIZE):
		var anchor: Node3D = _label_pool[i]
		if i >= shown.size():
			anchor.visible = false
			continue
		var node_id: int = shown[i]
		var server_pos: Vector3 = _binary_client.node_position(node_id)
		var world_pos: Vector3 = gxf * server_pos
		anchor.global_position = world_pos
		anchor.visible = true
		var title: Label3D = anchor.get_node("Title")
		var detail: Label3D = anchor.get_node("Detail")
		# Prefix a query variable's label with its ?vN badge so a marked node reads
		# as the variable it stands for.
		var vlabel: String = _query.var_name(node_id)
		if vlabel != "":
			title.text = "%s  %s" % [vlabel, _binary_client.label_of(node_id)]
		else:
			title.text = _binary_client.label_of(node_id)
		# Fold badge: a representative shows "(+N)" for the members collapsed into
		# it — the label path is the primary consumer of the fold badge count (the
		# halo shader also rings it via INSTANCE_CUSTOM.g).
		if _binary_client.has_method("fold_badge_of"):
			var badge: int = _binary_client.fold_badge_of(node_id)
			if badge > 0:
				title.text += "  (+%d)" % badge
		var d: String = _binary_client.detail_of(node_id)
		# Agent task line (ADR-140, Pillar 3 / P4): if this node is an agent with a
		# current task, surface it in the proximity Detail line so the swarm's work
		# reads at a glance up close — takes precedence over the generic node detail.
		if _binary_client.has_method("agent_task"):
			var task: String = _binary_client.agent_task(node_id)
			if task != "":
				d = "⚙ %s" % task
		detail.text = d
		detail.visible = d != ""
		# Fade: full opacity within LABEL_INNER_M, linear to 0 at the radius edge.
		# The grabbed node and every query variable are always fully shown.
		var a: float = 1.0
		if node_id != _grabbed_id and not _query.is_marked(node_id):
			var dist: float = cam_world.distance_to(world_pos)
			a = clampf(
				1.0 - (dist - LABEL_INNER_M) / maxf(LABEL_PROXIMITY_M - LABEL_INNER_M, 0.001),
				0.0, 1.0
			)
		title.modulate = Color(1.0, 1.0, 1.0, a)
		detail.modulate = Color(0.8, 0.85, 0.95, a)
		if a > LABEL_VISIBLE_MIN_A:
			labelled.append(node_id)
	_set_labelled_nodes(labelled)


func _hide_all_labels() -> void:
	for anchor: Node3D in _label_pool:
		anchor.visible = false
	_set_labelled_nodes(PackedInt32Array())


# Co-proximate label fade: the nodes whose label overlay is showing drop to 30 %
# alpha (Rust LABEL_FADE_ALPHA) for exactly the duration of the display, so the
# text sitting inside the sphere reads through it. Rust eases the alpha and routes
# those nodes to the transparent NodesFadedMulti pass (see _update_multimesh); the
# opaque NodesMulti material never changes.
func _set_labelled_nodes(ids: PackedInt32Array) -> void:
	if _binary_client == null or not _binary_client.has_method("set_labelled"):
		return
	_binary_client.set_labelled(ids)


# Eye-gaze capability probe (copresence brief §Godot API availability; the
# godot #113717 hazard). Query support ONLY here — after OpenXR init in
# XRBoot — and feed the flag to the Rust gaze resolver, which keeps head-gaze
# primary and degrades eye-gaze to head unless supported. We never enable the
# XR_EXT_eye_gaze_interaction action-map binding blindly: the extension stays
# off on Quest 3 (which returns false), so the action-map error can never fire.
func _probe_eye_gaze() -> void:
	var xr: XRInterface = XRServer.find_interface("OpenXR")
	if xr != null and xr.has_method("is_eye_gaze_interaction_supported"):
		_eye_gaze_supported = xr.is_eye_gaze_interaction_supported()
	else:
		_eye_gaze_supported = false
	if _gaze_tracker != null and _gaze_tracker.has_method("set_eye_gaze_supported"):
		_gaze_tracker.set_eye_gaze_supported(_eye_gaze_supported)
	if not _eye_gaze_supported:
		print("GraphScene: eye-gaze unsupported (Quest 3 floor device) -- head-gaze primary")


func _wire_hud() -> void:
	if hud == null:
		return
	connection_status_changed.connect(hud._on_connection_status)
	avatar_count_changed.connect(hud.set_avatar_count)
	if hud.has_signal("join_requested"):
		hud.join_requested.connect(_on_hud_join_requested)
	if hud.has_method("configure_intervention"):
		hud.configure_intervention(_http_base(), _nostr_auth)
	if hud.has_signal("control_pressed"):
		hud.control_pressed.connect(_on_hud_control)
	if hud.has_signal("reconnect_pressed"):
		hud.reconnect_pressed.connect(_on_hud_reconnect)
	_refresh_controls_status()


# Operator pressed Reconnect on the Session tab: force-reconnect both sockets now.
func _on_hud_reconnect() -> void:
	_reset_reconnect_state()
	_attempt_connect()


# HUD control-panel actions. Physics actions POST/PUT to the backend on the
# dedicated _physics_http; visual actions mutate a runtime factor and re-slice /
# re-scale on the spot (no HTTP, work only on press). Status label refreshed after
# every press.
func _on_hud_control(action: String) -> void:
	# Feature 3 — type show/hide filter. "type_toggle:<class>:<1|0>" (1 = visible).
	if action.begins_with("type_toggle:"):
		_apply_type_toggle(action.substr(12))
		return
	match action:
		"reset_layout":
			_request_physics_reset()
		"spread_plus":
			_request_spread(1.2, 1.15)
		"spread_minus":
			_request_spread(0.8, 0.85)
		"edges_plus":
			_edge_show_count = int(round(float(_edge_show_count) * 1.4))
			_apply_edge_slice()
		"edges_minus":
			_edge_show_count = int(round(float(_edge_show_count) * 0.7))
			_apply_edge_slice()
		"node_size_plus":
			_node_size_factor = clampf(_node_size_factor * 1.25, NODE_SIZE_FACTOR_MIN, NODE_SIZE_FACTOR_MAX)
		"node_size_minus":
			_node_size_factor = clampf(_node_size_factor * 0.8, NODE_SIZE_FACTOR_MIN, NODE_SIZE_FACTOR_MAX)
		"hierarchy_toggle":
			_request_hierarchy_toggle()
		"shells_plus":
			_request_shells(DAG_LEVEL_DISTANCE_STEP)
		"shells_minus":
			_request_shells(-DAG_LEVEL_DISTANCE_STEP)
		"planes_toggle":
			_request_planes_toggle()
		"plane_gap_plus":
			_request_plane_gap(PLANE_SPACING_STEP)
		"plane_gap_minus":
			_request_plane_gap(-PLANE_SPACING_STEP)
		"radial_dag":
			_post_radial({"mode": "dagRank", "transitionMs": 800})
		"radial_type":
			_post_radial({"mode": "typeTier", "transitionMs": 800})
		"radial_ego":
			if _radial_node_id < 0:
				push_warning("GraphScene: Ego Focus needs a selected node — open a node's radial first")
			else:
				_post_radial({"mode": "ego", "focusNode": _radial_node_id, "transitionMs": 800})
		"radial_off":
			if _put_physics_body({"dagBiasK": 0.0}):
				_physics_staged = {"_dag_bias_on": false}
		"flat_toggle":
			_request_flat_toggle()
		"layout_mode_cycle":
			_request_layout_mode_cycle()
		"fold_plus":
			_request_fold(1)
		"fold_minus":
			_request_fold(-1)
		"unpin_all":
			_unpin_all()
		_:
			push_warning("GraphScene: unknown HUD control '%s'" % action)
	_refresh_controls_status()


# Apply a type show/hide toggle "<class>:<1|0>" to the render store. Class codes
# mirror render_store::KIND_* (0 knowledge / 1 ontology / 2 agent). A rebuild of
# the draw list happens on the next frame via _recompute_drawn_ids.
func _apply_type_toggle(spec: String) -> void:
	if _binary_client == null or not _binary_client.has_method("set_type_visible"):
		return
	var parts := spec.split(":")
	if parts.size() < 2:
		return
	var class_code: int = int({"knowledge": 0, "ontology": 1, "agent": 2}.get(parts[0], -1))
	if class_code < 0:
		return
	var visible: bool = parts[1] == "1"
	_binary_client.set_type_visible(class_code, visible)
	# Force the draw domain to rebuild so hidden-class nodes drop immediately.
	_selection_dirty = true


func _refresh_controls_status() -> void:
	if hud == null:
		return
	if hud.has_method("set_controls_status"):
		var busy: String = "  [busy]" if _physics_pending else ""
		var dag: String = "on" if _dag_bias_on else "off"
		var fold: String = "  [fold busy]" if _fold_pending else ""
		var err: String = "" if _last_write_error.is_empty() else "\n⚠ " + _last_write_error
		hud.set_controls_status("repelK %.0f  restLen %.0f  edges %d/%d  nodes<=%d  node×%.2f  dag %s/%.0f  z%.2f  fold L%d  pin %d%s%s%s" % [
			_repel_k, _rest_length, _edge_show_count, _edge_budget, _node_budget, _node_size_factor,
			dag, _dag_level_distance, _z_compression, _fold_level, _pinned_ids.size(), busy, fold, err])
	# Reflect the toggle/pin state on the button faces (press-only, no per-frame cost).
	if hud.has_method("set_control_states"):
		hud.set_control_states(_dag_bias_on, _z_compression < Z_COMPRESSION_FULL_3D, _pinned_ids.size(), not is_zero_approx(_plane_bias_k))
	if hud.has_method("set_layout_mode_label"):
		hud.set_layout_mode_label(LAYOUT_MODE_LABELS[_layout_mode_idx])
	if hud.has_method("set_fold_state"):
		hud.set_fold_state(_fold_level)
	# Populate the Pins tab list (cheap; press/ack-only, no per-frame cost).
	if hud.has_method("set_pinned_ids"):
		hud.set_pinned_ids(_pinned_ids.keys())


# Fold ±: step the density ladder by `delta`, clamped [0,3]. GETs the server fold
# plan for the target level (passing the current pinned ids so pinned nodes are
# promoted to representatives) and commits _fold_level only on a 2xx, mirroring the
# physics one-in-flight discipline. Level 0 short-circuits: clear the plan locally,
# no HTTP needed.
func _request_fold(delta: int) -> void:
	if _fold_pending:
		push_warning("GraphScene: fold change already in flight; ignoring")
		return
	# Suppress ladder steps while a node grab or a two-hand graph manipulation owns
	# the hands: a fold transition re-seeds/animates positions, which would fight the
	# grab's optimistic pin and the manip's reference-frame lock (Phase-0 design).
	if _grabbed_id != -1 or _two_hand_active:
		push_warning("GraphScene: fold suppressed during grab / two-hand manip")
		return
	var target: int = clampi(_fold_level + delta, FOLD_LEVEL_MIN, FOLD_LEVEL_MAX)
	if target == _fold_level:
		return  # already at a ladder rail — no redundant fetch
	if target == 0:
		# ∅: no groups to fetch — clear the plan directly and force one rebuild.
		if _binary_client != null and _binary_client.has_method("clear_fold_plan"):
			_binary_client.clear_fold_plan()
		_fold_level = 0
		_selection_dirty = true
		return
	if _fold_http == null:
		return
	var pinned_csv := _pinned_ids_csv()
	var url := "%s/api/graph/fold?level=%d" % [_http_base(), target]
	if pinned_csv != "":
		url += "&pinned=" + pinned_csv
	var headers := _auth_headers(url, "GET")
	var err := _fold_http.request(url, headers, HTTPClient.METHOD_GET)
	if err != OK:
		push_warning("GraphScene: fold GET failed to start (%d)" % err)
		return
	_fold_pending = true
	_fold_staged_level = target


# Comma-separated list of this session's pinned node ids (empty when none), for
# the fold endpoint's ?pinned= param so pinned nodes are promoted, never folded.
func _pinned_ids_csv() -> String:
	if _pinned_ids.is_empty():
		return ""
	var parts := PackedStringArray()
	for id: int in _pinned_ids.keys():
		parts.push_back(str(id))
	return ",".join(parts)


# Fold GET resolved. On 2xx, parse {hidden, groups:[{representativeId, memberIds,
# ...}]} into flat (hidden, members, reps) arrays and hand them to the Rust store,
# then commit _fold_level. Snap transition this phase (members vanish / appear
# instantly); animation is Phase 3. A failed/timed-out fetch discards the staged
# level so the tracked state never diverges from what was applied.
func _on_fold_completed(result: int, response_code: int, _headers: PackedStringArray, body: PackedByteArray) -> void:
	_fold_pending = false
	var ok := result == HTTPRequest.RESULT_SUCCESS and response_code >= 200 and response_code < 300
	if not ok:
		push_warning("GraphScene: fold request failed (result=%d code=%d)" % [result, response_code])
		_refresh_controls_status()
		return
	var parsed: Variant = JSON.parse_string(body.get_string_from_utf8())
	if typeof(parsed) != TYPE_DICTIONARY:
		push_warning("GraphScene: fold response was not a JSON object")
		_refresh_controls_status()
		return
	var hidden := PackedInt32Array()
	for h: Variant in parsed.get("hidden", []):
		hidden.push_back(int(h))
	var members := PackedInt32Array()
	var reps := PackedInt32Array()
	for g: Variant in parsed.get("groups", []):
		if typeof(g) != TYPE_DICTIONARY:
			continue
		var rep: int = int(g.get("representativeId", 0))
		for m: Variant in g.get("memberIds", []):
			members.push_back(int(m))
			reps.push_back(rep)
	if _binary_client != null and _binary_client.has_method("set_fold_plan"):
		_binary_client.set_fold_plan(hidden, members, reps)
	_fold_level = _fold_staged_level
	_selection_dirty = true  # force one node/edge buffer rebuild with the new plan
	_refresh_controls_status()


# Hierarchy toggle: PUT dagBiasK (0.6 on / 0.0 off). Same one-in-flight gate and
# commit-only-on-dispatch discipline as Spread, so a busy/failed PUT never drifts
# the tracked toggle state away from what the server last received.
func _request_hierarchy_toggle() -> void:
	if _physics_pending:
		push_warning("GraphScene: physics change already in flight; ignoring")
		return
	var want_on := not _dag_bias_on
	var k: float = DAG_BIAS_ON_K if want_on else 0.0
	if _put_physics_body({"dagBiasK": k}):
		_physics_staged = {"_dag_bias_on": want_on}


# Shells ±: nudge dagLevelDistance by ±20, clamped [20, 200]; commit only on dispatch.
func _request_shells(delta: float) -> void:
	if _physics_pending:
		push_warning("GraphScene: physics change already in flight; ignoring")
		return
	var d := clampf(_dag_level_distance + delta, DAG_LEVEL_DISTANCE_MIN, DAG_LEVEL_DISTANCE_MAX)
	if is_equal_approx(d, _dag_level_distance):
		return  # already at the clamp rail — no redundant PUT
	if _put_physics_body({"dagLevelDistance": d}):
		_physics_staged = {"_dag_level_distance": d}


# Planes toggle: PUT planeBiasK (1.0 on / 0.0 off). Same one-in-flight gate and
# commit-only-on-dispatch discipline as Hierarchy, so a busy/failed PUT never drifts
# the tracked toggle state away from what the server last received.
func _request_planes_toggle() -> void:
	if _physics_pending:
		push_warning("GraphScene: physics change already in flight; ignoring")
		return
	var k: float = PLANE_BIAS_ON_K if is_zero_approx(_plane_bias_k) else 0.0
	if _put_physics_body({"planeBiasK": k}):
		_physics_staged = {"_plane_bias_k": k}


# Plane Gap ±: nudge planeSpacing by ±20, clamped ≥ 0; commit only on dispatch.
func _request_plane_gap(delta: float) -> void:
	if _physics_pending:
		push_warning("GraphScene: physics change already in flight; ignoring")
		return
	var s := maxf(_plane_spacing + delta, PLANE_SPACING_MIN)
	if is_equal_approx(s, _plane_spacing):
		return  # already at the clamp rail — no redundant PUT
	if _put_physics_body({"planeSpacing": s}):
		_physics_staged = {"_plane_spacing": s}


# 3D ↔ Flat: toggle axisCompressionZ between full 3D (1.0) and flat discs (0.3).
func _request_flat_toggle() -> void:
	if _physics_pending:
		push_warning("GraphScene: physics change already in flight; ignoring")
		return
	var z: float = Z_COMPRESSION_FLAT if _z_compression >= Z_COMPRESSION_FULL_3D else Z_COMPRESSION_FULL_3D
	if _put_physics_body({"axisCompressionZ": z}):
		_physics_staged = {"_z_compression": z}


# Layout Mode cycle: step to the next backend LayoutMode and POST it. Uses the same
# single-in-flight gate + staging discipline as the physics writes — the tracked
# index flips only on a 2xx ack (see _on_physics_completed), so a busy/failed POST
# never drifts the HUD label away from what the server last accepted.
func _request_layout_mode_cycle() -> void:
	if _physics_pending:
		push_warning("GraphScene: physics change already in flight; ignoring")
		return
	var next_idx := (_layout_mode_idx + 1) % LAYOUT_MODES.size()
	if _post_layout_mode(LAYOUT_MODES[next_idx]):
		_physics_staged = {"_layout_mode_idx": next_idx}


# POST {base}/api/layout/mode {"mode":<value>,"transitionMs":800}. Returns true only
# if the request was dispatched (gate held until _on_physics_completed). Same
# HTTPRequest + auth-header path as the physics PUT/POST helpers.
func _post_layout_mode(mode: String) -> bool:
	if _physics_http == null:
		return false
	if _physics_pending:
		return false
	var url := "%s/api/layout/mode" % _http_base()
	var headers := _auth_headers(url, "POST")
	var body := {"mode": mode, "transitionMs": 800}
	var err := _physics_http.request(url, headers, HTTPClient.METHOD_POST, JSON.stringify(body))
	if err != OK:
		push_warning("GraphScene: layout mode POST failed to start (%d)" % err)
		return false
	_physics_pending = true
	return true


# ADR-141 Phase 3 — POST {base}/api/layout/radial with a radial-shell body
# ({"mode":<dagRank|typeTier|ego>, "focusNode":<u32?>, "transitionMs":<u64>}).
# Same single-in-flight gate, auth-header path, and commit-on-2xx discipline as
# _post_layout_mode. Returns true only if the request was dispatched.
func _post_radial(body: Dictionary) -> bool:
	if _physics_http == null:
		return false
	if _physics_pending:
		push_warning("GraphScene: physics change already in flight; ignoring")
		return false
	var url := "%s/api/layout/radial" % _http_base()
	var headers := _auth_headers(url, "POST")
	var err := _physics_http.request(url, headers, HTTPClient.METHOD_POST, JSON.stringify(body))
	if err != OK:
		push_warning("GraphScene: radial POST failed to start (%d)" % err)
		return false
	_physics_pending = true
	return true


# Unpin every node this session pinned via a drag. No HTTP: each unpin rides the
# existing outbound graph socket through the Rust client's send_node_unpin, which
# sends {"type":"nodeUnpin",...} to hand the node back to physics. (drag-end now
# PINS persistently, so releasing must go through this explicit unpin path.)
func _unpin_all() -> void:
	if _binary_client == null or not _binary_client.has_method("send_node_unpin"):
		return
	# Send an unpin for every tracked id but do NOT clear the set here: an id is
	# removed only when its nodeUnpinAck arrives (see _on_graph_text). A
	# disconnected/dropped send therefore keeps the id, so pressing Unpin All again
	# retries the still-unacked ones.
	for id: int in _pinned_ids.keys():
		_binary_client.send_node_unpin(id)


# Spread ±: compute the candidate params, and only COMMIT the tracked values if the
# PUT actually dispatched — a busy/failed request must not drift repelK/restLength
# away from what the server last received.
func _request_spread(repel_mul: float, rest_mul: float) -> void:
	if _physics_pending:
		push_warning("GraphScene: physics change already in flight; ignoring")
		return
	var rk := clampf(_repel_k * repel_mul, REPEL_K_MIN, REPEL_K_MAX)
	var rl := clampf(_rest_length * rest_mul, REST_LENGTH_MIN, REST_LENGTH_MAX)
	# Stage, don't commit: the tracked values flip only on a 2xx ack (see
	# _on_physics_completed) so a failed PUT can't drift them off the server state.
	if _put_physics_params(rk, rl):
		_physics_staged = {"_repel_k": rk, "_rest_length": rl}


func _request_physics_reset() -> void:
	if _physics_pending:
		push_warning("GraphScene: physics change already in flight; ignoring")
		return
	_post_physics_reset()


# Fired when any physics request resolves (success, timeout, or failure) — reopens
# the gate so the next press can dispatch.
func _on_physics_completed(result: int, response_code: int, _headers: PackedStringArray, _body: PackedByteArray) -> void:
	_physics_pending = false
	var ok := result == HTTPRequest.RESULT_SUCCESS and response_code >= 200 and response_code < 300
	if ok:
		# Commit the staged values only now that the server has accepted them, so the
		# tracked state matches the backend exactly.
		for field: String in _physics_staged:
			set(field, _physics_staged[field])
		_last_write_error = ""
	else:
		_last_write_error = _describe_write_failure(result, response_code)
		push_warning("GraphScene: physics request failed (result=%d code=%d) — staged change discarded: %s" % [result, response_code, _last_write_error])
		if hud != null and hud.has_method("flash_notice"):
			hud.flash_notice(_last_write_error)
	# Discard staged on either outcome: on failure the member vars were never
	# touched, so the HUD stays in sync with the backend (no divergence).
	_physics_staged = {}
	_refresh_controls_status()


# Authorization headers for a physics write to `url` with `method` (upper-case).
# Primary: NIP-98 (`Nostr <b64>`) minted per request via the shared NostrAuth —
# the URL must be the exact request URL incl. query so the server's tag check
# passes. This is the same signing path hud.gd's decide POST uses. Falls back to
# the legacy dev bearer (+ X-Nostr-Pubkey, which the server's Bearer path requires)
# only when no real secret was supplied — the dev flow. In release builds the dev
# bearer 401s, so a real secret is mandatory there.
func _auth_headers(url: String, method: String) -> PackedStringArray:
	var headers := PackedStringArray()
	if _nostr_auth != null and _nostr_secret_present and _nostr_auth.has_method("nip98_header"):
		headers.push_back("Authorization: %s" % str(_nostr_auth.nip98_header(url, method)))
	else:
		headers.push_back("Authorization: %s" % PHYSICS_BEARER)
		if _nostr_auth != null and _nostr_auth.has_method("pubkey_hex"):
			headers.push_back("X-Nostr-Pubkey: %s" % str(_nostr_auth.pubkey_hex()))
	headers.push_back("Content-Type: application/json")
	return headers


# POST {base}/api/settings/physics/reset-layout. Returns true if the request was
# dispatched (gate then held until _on_physics_completed).
func _post_physics_reset() -> bool:
	if _physics_http == null:
		return false
	var url := "%s/api/settings/physics/reset-layout" % _http_base()
	var headers := _auth_headers(url, "POST")
	var err := _physics_http.request(url, headers, HTTPClient.METHOD_POST, "{}")
	if err != OK:
		push_warning("GraphScene: reset-layout POST failed to start (%d)" % err)
		return false
	_physics_pending = true
	return true


# PUT {base}/api/settings/physics?graph=knowledge with the given params. Returns true
# only if the request was dispatched. Thin wrapper over _put_physics_body.
func _put_physics_params(repel_k: float, rest_length: float) -> bool:
	return _put_physics_body({"repelK": repel_k, "restLength": rest_length})


# Operator-readable reason (with the remedy) for a failed server write. 401/403
# is the dev-headset trap: the HP client is not a loopback peer, so the dev bearer
# is always refused and only VISIONCLAW_DEV_MODE=1 on a dev-build backend (ADR-2039
# / ADR-2108) or a power-user NIP-98 identity (XR_NOSTR_SECRET) unlocks the
# server-routed HUD buttons (View 3D/Flat, Hierarchy, Shells, Spread, Planes,
# Radial, Layout Mode, Reset).
func _describe_write_failure(result: int, response_code: int) -> String:
	if result != HTTPRequest.RESULT_SUCCESS:
		return "Backend unreachable (result %d) — layout write dropped" % result
	match response_code:
		401, 403:
			var cred: String = "NIP-98 identity" if _nostr_secret_present else "dev bearer"
			return "Layout write denied (HTTP %d via %s): set VISIONCLAW_DEV_MODE=1 on the dev backend, or a power-user XR_NOSTR_SECRET" % [response_code, cred]
		_:
			return "Layout write failed (HTTP %d) — change discarded" % response_code


# PUT {base}/api/settings/physics?graph=knowledge with an arbitrary physics body.
# Returns true only if the request was dispatched (gate then held until
# _on_physics_completed). Shared by Spread and the Hierarchy/View controls so they
# all honour the single-in-flight gate identically.
func _put_physics_body(body: Dictionary) -> bool:
	if _physics_http == null:
		return false
	if _physics_pending:
		return false
	var url := "%s/api/settings/physics?graph=knowledge" % _http_base()
	var headers := _auth_headers(url, "PUT")
	var err := _physics_http.request(url, headers, HTTPClient.METHOD_PUT, JSON.stringify(body))
	if err != OK:
		push_warning("GraphScene: physics PUT failed to start (%d)" % err)
		return false
	_physics_pending = true
	return true


func _on_hud_join_requested(room_urn: String) -> void:
	_room_urn = room_urn
	_reset_reconnect_state()
	_attempt_connect()


# Resolve graph/presence endpoints and credentials from the environment and open
# both sockets. `XR_BACKEND_WS` is the base (scheme+host+port); the two well-known
# paths are appended. Empty secret => anonymous graph stream / ephemeral Nostr
# identity.
func _connect_from_env() -> void:
	var base: String = _env_or("XR_BACKEND_WS", DEFAULT_BACKEND_WS).rstrip("/")
	connect_to_server(
		base + GRAPH_STREAM_PATH,
		base + PRESENCE_PATH,
		_env_or("XR_ROOM_URN", DEFAULT_ROOM_URN),
		_env_or("XR_DISPLAY_NAME", DEFAULT_DISPLAY_NAME),
		OS.get_environment("XR_NOSTR_SECRET")
	)


func _env_or(name: String, fallback: String) -> String:
	var value: String = OS.get_environment(name)
	return value if value != "" else fallback


# HTTP origin for the intervention decide POST. Prefer an explicit override,
# else swap the ws:// backend scheme to http:// (wss → https).
func _http_base() -> String:
	var override: String = OS.get_environment("XR_BACKEND_HTTP")
	if override != "":
		return override.rstrip("/")
	var ws: String = _env_or("XR_BACKEND_WS", DEFAULT_BACKEND_WS).rstrip("/")
	if ws.begins_with("wss://"):
		return "https://" + ws.substr(6)
	if ws.begins_with("ws://"):
		return "http://" + ws.substr(5)
	return ws


func connect_to_server(
	graph_ws_url: String,
	presence_ws_url: String,
	room_urn: String,
	display_name: String,
	nostr_secret_hex: String = ""
) -> void:
	_graph_ws_url = graph_ws_url
	_presence_ws_url = presence_ws_url
	_room_urn = room_urn
	_display_name = display_name
	_nostr_secret_hex = nostr_secret_hex
	_reset_reconnect_state()
	_attempt_connect()


func _reset_reconnect_state() -> void:
	_graph_reconnect_attempts = 0
	_graph_reconnect_timer = -1.0
	_presence_reconnect_attempts = 0
	_presence_reconnect_timer = -1.0


func _attempt_connect() -> void:
	# Initial connect (and full re-join): bring up both sockets. Reconnects are
	# per-socket via _connect_graph / _connect_presence so one socket's backoff
	# never disturbs the other.
	_connect_graph()
	_connect_presence()


func _connect_graph() -> void:
	# Tear down any prior graph socket so a reconnect never leaks a detached
	# tokio task, then re-open. The Nostr secret also NIP-98-authenticates the
	# graph socket so server-authoritative drag/pin messages are accepted.
	if _binary_client == null:
		return
	if _binary_client.has_method("close"):
		_binary_client.close()
	# A reconnect may land on a restarted server with a different id space:
	# stale positions/topology would mix with the fresh snapshot as orphan
	# gems and phantom edges, so drop all graph state before re-dialling. The
	# Rust store is cleared inside connect_to_url (below); clear the GDScript
	# mirror + selection here.
	_node_centrality.clear()
	_centrality_max = 0.0001
	_drawn_ids = PackedInt32Array()
	_selection_dirty = true
	_edge_pairs = PackedInt32Array()
	_edge_pairs_ranked = PackedInt32Array()
	_edge_pairs_full = PackedInt32Array()
	_edge_weights_full = PackedFloat32Array()
	_edge_rerank_done = false
	_topo_ids = {}
	_edge_show_count = 0
	# Back to "draw everything up to the ceiling" until the next topology sets the
	# real budgets.
	_node_budget = NODE_SAFETY_CEILING
	_edge_budget = EDGE_SAFETY_CEILING
	# Graph-state clear: drop any two-hand manual latch so the fresh snapshot is
	# re-fitted from scratch rather than inheriting a stale manual transform.
	_manual_transform = false
	_two_hand_active = false
	_manip_session_base_scale = -1.0
	if _binary_client.has_method("connect_to_url"):
		_binary_client.connect_to_url(_graph_ws_url, _nostr_secret_hex)


func _connect_presence() -> void:
	if _presence_client == null:
		return
	if _presence_client.has_method("close"):
		_presence_client.close()
	if _presence_client.has_method("join"):
		_presence_client.join(_presence_ws_url, _room_urn, _display_name, _nostr_secret_hex)


func _physics_process(delta: float) -> void:
	# Drain network inboxes on the scene-tree thread; both clients emit their
	# signals from inside poll().
	if _binary_client != null and _binary_client.has_method("poll"):
		_binary_client.poll()
	if _presence_client != null and _presence_client.has_method("poll"):
		_presence_client.poll()
	_update_lod()
	_update_locomotion(delta)
	_update_hud_grab()
	_arbitrate_pointers()
	_update_hud_pointer()
	_update_radial_menu()
	_update_query_count(delta)
	_update_teleport(delta)
	# Two-hand pinch scale/rotate/move must run BEFORE _fit_graph_to_view: on its
	# first engage it latches _manual_transform, which makes the adaptive fit
	# early-return this same frame so the gesture — not the auto-fit — owns
	# graph_root's transform.
	_update_two_hand_manip(delta)
	# Rust owns the hunt now: one call eases all render positions toward their
	# targets and pins the grabbed node to the wand — replaces the old GDScript
	# per-node lerp over a 13k Dictionary.
	if _binary_client != null and _binary_client.has_method("hunt"):
		_binary_client.hunt(POSITION_HUNT_EASE, _grabbed_id, _grab_target_server)
	_fit_graph_to_view(delta)
	# At the desktop-Vive instance budgets the two multimesh rebuilds are the
	# frame-cost hot spot (GDScript loops over ~6k instances). Alternate them so
	# each runs at 45 Hz while the compositor holds 90 — position hunting eases
	# per frame, so half-rate transform refresh is visually indistinguishable.
	_mm_phase = not _mm_phase
	if _mm_phase:
		_update_multimesh()
	else:
		_update_edge_multimesh()
	# Work beams (ADR-140, Pillar 2 / P3) refresh every frame: the buffer is a short
	# walk of the agent registry (tens of instances), not the node/edge domain, so it
	# is not part of the 45 Hz alternation — the flowing stream stays crisp at 90 Hz.
	_update_beam_multimesh()
	_update_interaction()
	_update_agents(delta)
	_update_selection(delta)
	_update_voice_listener()
	_tick_reconnect(delta)
	# Proximity labels refresh at ~4 Hz (not per frame) — the query + text/fade
	# assignment is the only work and it reuses the pool, so zero per-frame cost.
	_label_accum += delta
	if _label_accum >= LABEL_UPDATE_SEC:
		_label_accum -= LABEL_UPDATE_SEC
		_update_proximity_labels()
		_update_swarm_roster()


# Trackpad/stick locomotion: slide the XR rig through the graph. Either wand's
# primary axis drives movement in the camera's horizontal facing plane (push
# up = move the way you look, sideways = strafe). Vertical fly is on the same
# stick's push when the trigger is not held, kept simple and comfortable.
func _update_locomotion(delta: float) -> void:
	var origin: XROrigin3D = get_node_or_null("XROrigin3D")
	var camera: XRCamera3D = _find_xr_camera()
	if origin == null or camera == null:
		return
	var move := Vector2.ZERO
	for controller: XRController3D in [left_controller, right_controller]:
		if controller == null or not controller.get_is_active():
			continue
		var v: Vector2 = controller.get_vector2("primary")
		if v.length() > move.length():
			move = v
	if move.length() < LOCOMOTION_DEADZONE:
		return
	var fwd: Vector3 = -camera.global_transform.basis.z
	fwd.y = 0.0
	fwd = fwd.normalized()
	var right: Vector3 = camera.global_transform.basis.x
	right.y = 0.0
	right = right.normalized()
	var step: Vector3 = (fwd * move.y + right * move.x) * LOCOMOTION_SPEED * delta
	origin.global_position += step


# Move the HUD out of the camera (gaze-locked) and anchor it in the play space
# as a world-fixed, wand-movable panel.
func _reparent_hud_to_world() -> void:
	if hud == null:
		return
	var origin: XROrigin3D = get_node_or_null("XROrigin3D")
	if origin == null:
		return
	var old_parent: Node = hud.get_parent()
	if old_parent != null and old_parent != origin:
		old_parent.remove_child(hud)
		origin.add_child(hud)
	# Comfortable default: down and to the left, angled toward the user.
	hud.transform = Transform3D(Basis(Vector3.UP, deg_to_rad(25.0)), Vector3(-0.6, 1.0, -0.9))
	hud.visible = true


# Grab the HUD with a wand: hold the grip button while the wand is near the panel
# to pick it up; it then rides the controller until grip releases.
func _update_hud_grab() -> void:
	if hud == null:
		return
	if _hud_grab_controller != null:
		var grip_now: float = _hud_grab_controller.get_float("grip")
		if grip_now < 0.5:
			_hud_grab_controller = null
		else:
			hud.global_transform = _hud_grab_controller.global_transform * _hud_grab_offset
		return
	# While a two-hand graph gesture owns both grips, don't let a hand that drifts
	# near the panel start a HUD grab (which would abort the gesture next frame).
	if _two_hand_active:
		return
	# Measure to the VISIBLE panel, not the HUD root: HudPanel sits ~0.7 m out on
	# the root's local -Z, so a wand touching the panel is well beyond 0.4 m from
	# the root origin (grip-move was dead). Fall back to the root if the panel is
	# missing.
	var panel := hud.get_node_or_null("HudPanel") as Node3D
	var panel_pos: Vector3 = panel.global_position if panel != null else hud.global_position
	for controller: XRController3D in [left_controller, right_controller]:
		if controller == null or not controller.get_is_active():
			continue
		if controller.get_float("grip") < 0.7:
			continue
		if controller.global_position.distance_to(panel_pos) < 0.4:
			_hud_grab_controller = controller
			_hud_grab_offset = controller.global_transform.affine_inverse() * hud.global_transform
			_pulse(controller, 0.4, 0.05)
			break


# Wand-ray → HUD SubViewport input forwarding. The HUD is a SubViewport rendered
# onto a quad (HudPanel), so Godot's GUI (Buttons/LineEdit) never sees VR input by
# itself. Each frame we intersect the active controller's aim ray with the panel
# quad, convert the hit to viewport pixels, and push a mouse-motion event; the
# trigger is edge-detected into a single left click. This is THE activation path
# for every HUD button — Approve/Deny and the new control grid alike.
func _update_hud_pointer() -> void:
	# Cleared each frame; set below only if a ray is actually on the panel, so
	# _update_interaction sees a fresh answer every frame.
	_hud_pointer_controller = null
	if hud == null:
		return
	var vp := hud.get_node_or_null("HudViewport") as SubViewport
	var panel := hud.get_node_or_null("HudPanel") as Node3D
	if vp == null or panel == null:
		return
	# While the panel is being physically grabbed (grip), don't also drive the
	# cursor — moving it would spray clicks. Release any held click FIRST (at the
	# last valid spot) so the SubViewport button doesn't latch pressed.
	if _hud_grab_controller != null:
		if _hud_trigger_was_down:
			_push_hud_mouse(vp, _hud_last_px, false)
			_hud_trigger_was_down = false
		return
	# Prefer the right controller; fall back to the left. First one whose ray hits
	# the panel wins. Skip the hand that is mid node-drag — its trigger is held for
	# the drag, so crossing the panel must not fire button clicks.
	var hit_uv := Vector2(-1.0, -1.0)
	var hit_ctrl: XRController3D = null
	for controller: XRController3D in [right_controller, left_controller]:
		if controller == null or not controller.get_is_active():
			continue
		if _grabbed_id != -1 and controller == _grab_controller:
			continue
		# Arbitration: this controller's ray is nearer the radial menu — it owns the
		# radial, so the HUD yields it this frame.
		if controller == _radial_owner:
			continue
		var uv := _ray_hit_hud_uv(controller, panel)
		if uv.x >= 0.0:
			hit_uv = uv
			hit_ctrl = controller
			break
	if hit_ctrl == null:
		# Ray left the panel: release a held click at the last valid position so a
		# button can't latch pressed.
		if _hud_trigger_was_down:
			_push_hud_mouse(vp, _hud_last_px, false)
			_hud_trigger_was_down = false
		return
	# Ray is on the panel: claim this controller so node-grab engagement skips it.
	_hud_pointer_controller = hit_ctrl
	var px := Vector2(hit_uv.x * float(vp.size.x), hit_uv.y * float(vp.size.y))
	_hud_last_px = px
	var motion := InputEventMouseMotion.new()
	motion.position = px
	motion.global_position = px
	vp.push_input(motion)
	var down: bool = _controller_trigger(hit_ctrl) > 0.6
	if down and not _hud_trigger_was_down:
		_push_hud_mouse(vp, px, true)
	elif not down and _hud_trigger_was_down:
		_push_hud_mouse(vp, px, false)
		_pulse(hit_ctrl, 0.3, 0.03)
	_hud_trigger_was_down = down


func _push_hud_mouse(vp: SubViewport, px: Vector2, pressed: bool) -> void:
	var click := InputEventMouseButton.new()
	click.button_index = MOUSE_BUTTON_LEFT
	click.pressed = pressed
	click.position = px
	click.global_position = px
	vp.push_input(click)


# Intersect a controller aim ray with the HUD quad; returns viewport UV (0..1,
# y-down) or (-1,-1) on a miss. Works in the panel's local space so the maths is
# independent of where the movable HUD currently sits.
func _ray_hit_hud_uv(controller: XRController3D, panel: Node3D) -> Vector2:
	var to_local: Transform3D = panel.global_transform.affine_inverse()
	var o: Vector3 = to_local * controller.global_position
	var d: Vector3 = (to_local.basis * (-controller.global_transform.basis.z)).normalized()
	# QuadMesh lies in the panel's local XY plane (z = 0), facing +Z.
	if absf(d.z) < 0.00001:
		return Vector2(-1.0, -1.0)
	var t: float = -o.z / d.z
	if t < 0.0:
		return Vector2(-1.0, -1.0)
	var hit: Vector3 = o + d * t
	var half_w: float = HUD_QUAD_W * 0.5
	var half_h: float = HUD_QUAD_H * 0.5
	if absf(hit.x) > half_w or absf(hit.y) > half_h:
		return Vector2(-1.0, -1.0)
	# Reach is a WORLD distance: the local param t scales with HUD scale, so compare
	# the world-space hit distance from the wand instead (scale-invariant reach).
	var hit_world: Vector3 = panel.global_transform * hit
	if controller.global_position.distance_to(hit_world) > RAY_LENGTH:
		return Vector2(-1.0, -1.0)
	# Local +x → right (u 0→1); local +y is up, viewport v is down → invert.
	var u: float = (hit.x + half_w) / HUD_QUAD_W
	var v: float = 1.0 - (hit.y + half_h) / HUD_QUAD_H
	return Vector2(u, v)


func _update_lod() -> void:
	# should_recompute() ticks a frame counter, so it must be called EXACTLY once
	# per frame. Cache the decision here for _update_agents to reuse — a second
	# call would double-tick and desynchronise both LOD passes.
	_lod_recompute = false
	if _lod_policy == null or not _lod_policy.has_method("should_recompute"):
		return
	if not _lod_policy.should_recompute():
		return
	_lod_recompute = true
	var camera: XRCamera3D = _find_xr_camera()
	if camera == null:
		return
	var cam_pos: Vector3 = camera.global_position
	for avatar_id: String in _avatars:
		var av: Node3D = _avatars[avatar_id]
		var dist: float = cam_pos.distance_to(av.global_position)
		var level: int = _lod_policy.classify_distance(dist)
		if av.has_method("set_lod_level"):
			av.set_lod_level(level)


var _graph_scale: float = 1.0
var _fit_frame: int = 0
var _fit_target_scale: float = 0.0
var _fit_target_centre: Vector3 = Vector3.ZERO
# Frames between AABB recomputes (a full pass over _node_positions is O(N)).
const FIT_RECOMPUTE_FRAMES: int = 15
# Per-frame easing toward the fit target — small enough that a moving/re-settling
# graph rescales smoothly rather than snapping.
const FIT_EASE: float = 0.06


# Scale + recentre GraphRoot so the streamed node cloud always fills a room-sized
# volume in front of the user. Adaptive (not latched): the AABB is re-measured
# periodically and the transform eased toward the new target, so the graph stays
# room-sized as the force layout spreads, re-settles, or reheats after a physics
# change. Node/edge geometry compensates for `_graph_scale` so it never shrinks
# to specks.
func _fit_graph_to_view(_delta: float) -> void:
	# A two-hand manual gesture takes permanent authority over graph_root until a
	# new topology (or graph-state clear) re-enables the fit. While latched, the
	# adaptive easing must not fight the user's transform.
	if _manual_transform:
		return
	if graph_root == null or _binary_client == null or _binary_client.node_count() == 0:
		return
	_fit_frame += 1
	if _fit_frame % FIT_RECOMPUTE_FRAMES == 0 or _fit_target_scale == 0.0:
		# Robust 5th–95th percentile AABB, computed in Rust over the render store
		# (excluding the grabbed node so a dragged outlier can't inflate the fit —
		# "edges scaling off when moved"). Returns [minx,miny,minz,maxx,maxy,maxz]
		# or empty.
		var bb: PackedFloat32Array = _binary_client.render_aabb(0.05, 0.95, _grabbed_id)
		if bb.size() == 6:
			var mn := Vector3(bb[0], bb[1], bb[2])
			var mx := Vector3(bb[3], bb[4], bb[5])
			var span: Vector3 = mx - mn
			var longest: float = maxf(span.x, maxf(span.y, span.z))
			if longest >= 0.001:
				_fit_target_scale = GRAPH_TARGET_SPAN / longest
				_fit_target_centre = (mn + mx) * 0.5
	if _fit_target_scale <= 0.0:
		return
	# Ease current scale/centre toward the target, then rebuild the transform:
	# world = anchor + scale * (server_pos - centre) → graph centre sits at anchor.
	var ease: float = FIT_EASE if _graph_scale > 0.0 else 1.0
	_graph_scale = lerpf(_graph_scale, _fit_target_scale, ease)
	graph_root.transform = Transform3D(
		Basis.IDENTITY.scaled(Vector3(_graph_scale, _graph_scale, _graph_scale)),
		GRAPH_ANCHOR - _fit_target_centre * _graph_scale
	)


# Linear-interpolated percentile (q in [0,1]) of an unsorted float array. Used to
# derive robust AABB bounds for the fit so a single outlier node can't drive the
# whole-graph rescale. Copies before sorting so caller data is untouched.
func _percentile(values: PackedFloat32Array, q: float) -> float:
	var n: int = values.size()
	if n == 0:
		return 0.0
	if n == 1:
		return values[0]
	var sorted: PackedFloat32Array = values.duplicate()
	sorted.sort()
	var rank: float = clampf(q, 0.0, 1.0) * float(n - 1)
	var lo: int = int(floor(rank))
	var hi: int = int(ceil(rank))
	if lo == hi:
		return sorted[lo]
	var frac: float = rank - float(lo)
	return lerpf(sorted[lo], sorted[hi], frac)


# --- Two-hand pinch: whole-graph scale / rotate / move -----------------------
# Ported conceptually from Graph2VR's SphereInteraction.cs. When BOTH grips are
# held — and neither hand is grabbing a node (_grabbed_id) nor the HUD panel
# (_hud_grab_controller) — the graph locks into a manipulation session. At
# gesture start we capture: the left→right controller vector, the hand midpoint,
# graph_root's current world transform, and a world-space pivot at the render
# AABB centre (server-space centre mapped through graph_root.global_transform).
# Each frame the graph is:
#   • scaled by (current inter-hand distance / initial), clamped to a sane band;
#   • rotated by the full-3D shortest-arc rotation between the initial and
#     current inter-hand vectors;
#   • translated by the hand-midpoint delta,
# all composed about the fixed world pivot onto the captured start transform:
#   world = T(midDelta) · T(pivot) · R · S · T(-pivot) · startXform
# The first engage latches `_manual_transform`, permanently disabling the
# adaptive auto-fit easing until a new topology / graph-state clear re-enables
# it. Accumulated rotation past ~90° re-baselines the reference frame — the same
# drift guard Graph2VR applies — so quaternion error can't build up over a long
# twist. Single-grip behaviour (HUD grab) is untouched: this only engages when
# BOTH grips are past the engage threshold and no HUD/node grab is active.
const GRIP_MANIP_ENGAGE: float = 0.7
const GRIP_MANIP_RELEASE: float = 0.5
const MANIP_DRIFT_RESET_RAD: float = 1.5707963   # ~90°
# ABSOLUTE scale band, measured against the SESSION baseline (the auto-fit scale
# captured the moment the manual latch first engages, held until the latch
# clears). Clamping the absolute scale — not a per-gesture factor — stops
# repeated 0.1× gestures from compounding the graph toward zero while auto-fit is
# off, which would leave it unreachable with no way to grow it back.
const MANIP_ABS_SCALE_MIN: float = 0.05          # ×session (fitted) baseline
const MANIP_ABS_SCALE_MAX: float = 50.0          # ×session (fitted) baseline
const MANIP_MIN_SPAN: float = 0.02               # hands too coincident → no axis

var _two_hand_active: bool = false
# Permanent authority latch: while true the adaptive fit is disabled and the
# graph transform is whatever the last manual gesture left it at.
var _manual_transform: bool = false
# Reference frame captured at gesture start (re-baselined on drift reset).
var _manip_start_xform: Transform3D = Transform3D.IDENTITY
var _manip_pivot_world: Vector3 = Vector3.ZERO
var _manip_init_vec: Vector3 = Vector3.ZERO
var _manip_init_dist: float = 1.0
var _manip_init_mid: Vector3 = Vector3.ZERO
# graph_root's uniform scale at the moment the latch FIRST engaged (i.e. the
# auto-fit scale). Persists across every gesture in the session until the latch
# clears (new topology / graph-state clear); the absolute scale clamp is measured
# against this so gestures can't ratchet the graph past the reachable band.
# -1 => not yet captured this session.
var _manip_session_base_scale: float = -1.0


func _update_two_hand_manip(_delta: float) -> void:
	var lc := left_controller
	var rc := right_controller
	# Eligible only when both wands are tracking and no competing grab owns a hand.
	# A node grab uses the trigger and the HUD grab uses a single grip; either one
	# taking a hand blocks the two-hand gesture (and vice versa).
	var eligible: bool = lc != null and rc != null \
		and lc.get_is_active() and rc.get_is_active() \
		and _grabbed_id == -1 and _hud_grab_controller == null
	if _two_hand_active:
		# End when either grip relaxes below the release threshold, a wand drops
		# out, or a grab claims a hand. Hysteresis: release < engage so a grip held
		# near the threshold can't flicker the session on and off.
		if not eligible \
				or lc.get_float("grip") < GRIP_MANIP_RELEASE \
				or rc.get_float("grip") < GRIP_MANIP_RELEASE:
			_two_hand_active = false
			return
		_apply_two_hand_manip()
		return
	if not eligible:
		return
	if lc.get_float("grip") < GRIP_MANIP_ENGAGE or rc.get_float("grip") < GRIP_MANIP_ENGAGE:
		return
	_begin_two_hand_manip()


# Lock the reference frame for a new manipulation session and take fit authority.
func _begin_two_hand_manip() -> void:
	if graph_root == null:
		return
	var lp: Vector3 = left_controller.global_position
	var rp: Vector3 = right_controller.global_position
	var vec: Vector3 = rp - lp
	if vec.length() < MANIP_MIN_SPAN:
		return  # hands coincident: no stable axis yet, wait a frame to engage
	var start_xform: Transform3D = graph_root.global_transform
	# Pivot = render AABB centre in server space → world through the live graph
	# transform. -1 includes every node (nothing is grabbed during a manip).
	var pivot: Vector3 = _manip_aabb_centre_world(start_xform)
	_manip_start_xform = start_xform
	_manip_pivot_world = pivot
	_manip_init_vec = vec
	_manip_init_dist = vec.length()
	_manip_init_mid = (lp + rp) * 0.5
	# Capture the session baseline ONCE, on the transition into the latched state —
	# this is the auto-fit scale. Subsequent gestures within the same session reuse
	# it, so the absolute clamp band is fixed relative to the fitted size and can't
	# drift with gesture count.
	if not _manual_transform or _manip_session_base_scale <= 0.0:
		_manip_session_base_scale = _uniform_scale(start_xform)
	_two_hand_active = true
	_manual_transform = true  # permanently disable auto-fit until new topology
	_pulse(left_controller, 0.4, 0.05)
	_pulse(right_controller, 0.4, 0.05)


# Recompute + apply the graph transform for the current controller poses.
func _apply_two_hand_manip() -> void:
	if graph_root == null:
		return
	var lp: Vector3 = left_controller.global_position
	var rp: Vector3 = right_controller.global_position
	var vec: Vector3 = rp - lp
	var dist: float = vec.length()
	if dist < MANIP_MIN_SPAN or _manip_init_dist < 0.001:
		return
	# Scale: change in inter-hand distance, then clamp the ABSOLUTE resulting scale
	# to [MIN,MAX] × the SESSION baseline (not a per-gesture factor). start_scale
	# folds in any prior drift-reset bake; the resulting absolute scale is
	# start_scale × scale_factor, which we clamp and back-convert to a factor.
	var start_scale: float = _uniform_scale(_manip_start_xform)
	var scale_factor: float = dist / _manip_init_dist
	if start_scale > 0.0001 and _manip_session_base_scale > 0.0:
		var abs_target: float = start_scale * scale_factor
		var abs_lo: float = MANIP_ABS_SCALE_MIN * _manip_session_base_scale
		var abs_hi: float = MANIP_ABS_SCALE_MAX * _manip_session_base_scale
		abs_target = clampf(abs_target, abs_lo, abs_hi)
		scale_factor = abs_target / start_scale
	# Full-3D shortest-arc rotation between the initial and current hand vectors.
	var rot: Quaternion = Quaternion(_manip_init_vec.normalized(), vec.normalized())
	# Translation from the hand-midpoint delta.
	var mid: Vector3 = (lp + rp) * 0.5
	var translate: Vector3 = mid - _manip_init_mid
	# Compose about the world pivot: T(translate) · T(p) · R · S · T(-p) · start.
	var p: Vector3 = _manip_pivot_world
	var rs: Basis = Basis(rot) * Basis.IDENTITY.scaled(
		Vector3(scale_factor, scale_factor, scale_factor))
	var anchored := Transform3D(rs, p - rs * p)                 # T(p)·R·S·T(-p)
	var manip := Transform3D(Basis.IDENTITY, translate) * anchored
	# Apply THIS frame's full delta first (no dropped frame), then keep the
	# node/edge apparent-size compensation in step with the new scale.
	graph_root.global_transform = manip * _manip_start_xform
	_graph_scale = _uniform_scale(graph_root.global_transform)
	# Drift guard: once the accumulated twist passes ~90°, re-baseline from the
	# JUST-APPLIED transform (not the previous frame's) so no delta is lost — the
	# graph doesn't pop. init_vec := current vector, so next frame's rotation
	# starts from identity again. Graph2VR resets its frame the same way.
	if rot.get_angle() > MANIP_DRIFT_RESET_RAD:
		_manip_start_xform = graph_root.global_transform
		_manip_init_vec = vec
		_manip_init_dist = dist
		_manip_init_mid = mid
		_manip_pivot_world = _manip_aabb_centre_world(graph_root.global_transform)


# Render AABB centre (5th–95th pct, all nodes) mapped to world through `xform`.
# Falls back to the transform origin if the store can't supply a box yet.
func _manip_aabb_centre_world(xform: Transform3D) -> Vector3:
	if _binary_client != null and _binary_client.has_method("render_aabb"):
		var bb: PackedFloat32Array = _binary_client.render_aabb(0.05, 0.95, -1)
		if bb.size() == 6:
			var centre_server := Vector3(
				(bb[0] + bb[3]) * 0.5, (bb[1] + bb[4]) * 0.5, (bb[2] + bb[5]) * 0.5)
			return xform * centre_server
	return xform.origin


# Uniform scale magnitude of a transform's basis. The graph is only ever scaled
# uniformly, so the x-axis length is representative (get_scale is always +ve).
func _uniform_scale(xform: Transform3D) -> float:
	return xform.basis.get_scale().x


# Client-side optimistic position hunting: the server streams authoritative
# positions as targets; each frame the rendered position eases toward its target
# so motion stays smooth between (and independent of) update ticks — the same
# pattern the desktop client uses. The grabbed node is exempt (it tracks the
# hand locally and the server echoes it back).
const POSITION_HUNT_EASE: float = 0.06


# Recompute the drawn-node selection (LOD budget + topology bias). Cached in
# _drawn_ids and only rebuilt when the domain changes (visuals/topology/budget),
# so the per-frame buffer build reuses it. Topology-coherence + budget logic is
# unchanged; only the id SOURCE moved from _node_positions.keys() to the Rust store.
func _recompute_drawn_ids() -> void:
	if _binary_client == null:
		_drawn_ids = PackedInt32Array()
		return
	var all_ids: PackedInt32Array = _binary_client.get_node_ids()
	var n: int = all_ids.size()
	if n <= _node_budget or _lod_policy == null or not _lod_policy.has_method("visible_subset"):
		_drawn_ids = all_ids
		return
	# Over budget: bias edge-endpoint (topo) nodes so they win cap slots first
	# (keeps drawn nodes coherent with drawable edges), spare slots to the highest
	# centrality non-topo nodes.
	var have_topo: bool = not _topo_ids.is_empty()
	var centrality := PackedFloat32Array()
	centrality.resize(n)
	for i: int in range(n):
		var c: float = _node_centrality.get(all_ids[i], 0.0)
		if have_topo and _topo_ids.has(all_ids[i]):
			c += TOPO_SELECT_BIAS
		centrality[i] = c
	var subset: PackedInt32Array = _lod_policy.visible_subset(centrality, _node_budget)
	var out := PackedInt32Array()
	out.resize(subset.size())
	for j: int in range(subset.size()):
		out[j] = all_ids[subset[j]]
	_drawn_ids = out


# Node MultiMesh: Rust packs the whole instance buffer (transform + colour +
# custom) from the drawn ids; GDScript does a single buffer assignment. scale_comp
# folds the GraphRoot fit-scale and the HUD node-size factor; the centrality size
# tell + halo custom channel are computed in Rust.
func _update_multimesh() -> void:
	if nodes_multi == null or nodes_multi.multimesh == null or _binary_client == null:
		return
	if _selection_dirty:
		_recompute_drawn_ids()
		_selection_dirty = false
	var comp: float = NODE_WORLD_RADIUS * _node_size_factor / (NODE_MESH_RADIUS * _graph_scale)
	var buf: PackedFloat32Array = _binary_client.build_node_buffer(_drawn_ids, comp, 0.7, 1.9)
	var mm: MultiMesh = nodes_multi.multimesh
	var count: int = buf.size() / 20
	if mm.instance_count != count:
		mm.instance_count = count
	if count > 0:
		mm.buffer = buf
	# Transparent pass: the labelled / fading nodes the same build routed away from
	# the opaque buffer (COLOR.a = fade alpha). Usually 0-13 instances.
	if nodes_faded_multi == null or nodes_faded_multi.multimesh == null:
		return
	if not _binary_client.has_method("faded_node_buffer"):
		return
	var fbuf: PackedFloat32Array = _binary_client.faded_node_buffer()
	var fmm: MultiMesh = nodes_faded_multi.multimesh
	var fcount: int = fbuf.size() / 20
	if fmm.instance_count != fcount:
		fmm.instance_count = fcount
	if fcount > 0:
		fmm.buffer = fbuf


# Edge MultiMesh: Rust filters the ranked pairs to both-endpoints-drawn and packs
# the rotated+scaled cylinder transforms; GDScript does a single buffer assignment.
func _update_edge_multimesh() -> void:
	if edges_multi == null or edges_multi.multimesh == null or _binary_client == null:
		return
	var er: float = EDGE_WORLD_RADIUS / (EDGE_MESH_RADIUS * _graph_scale)
	var buf: PackedFloat32Array = _binary_client.build_edge_buffer(_edge_pairs, er)
	var mm: MultiMesh = edges_multi.multimesh
	# 16 floats/instance: 12 transform + 4 INSTANCE_CUSTOM (style code in .a).
	# MultiMesh_edges has use_custom_data=true, so the resource stride is 16 — the
	# builder packs 16 (see render_store::build_edge_buffer). A /12 divisor here
	# mis-sizes instance_count, so set_buffer rejects every frame and edges vanish.
	var count: int = buf.size() / 16
	if mm.instance_count != count:
		mm.instance_count = count
	if count > 0:
		mm.buffer = buf


# Work-beam MultiMesh (ADR-140, Pillar 2 / P3): Rust packs one cylinder per active
# agent→target-node link (16 floats/instance, status code in INSTANCE_CUSTOM.a);
# GDScript does a single buffer assignment. Beam count = live working/blocked agents
# (tens, not thousands), so this runs every frame — the flowing stream stays crisp
# and the build is a short walk of the agent registry, not the node domain.
func _update_beam_multimesh() -> void:
	if agent_multi == null or agent_multi.multimesh == null or _binary_client == null:
		return
	if not _binary_client.has_method("build_beam_buffer"):
		return
	# Beam cylinder mesh radius is 0.02; fold the GraphRoot fit-scale in so the beam
	# keeps a constant world thickness as the graph is scaled by the two-hand gesture.
	var br: float = EDGE_WORLD_RADIUS / (BEAM_MESH_RADIUS * _graph_scale)
	var buf: PackedFloat32Array = _binary_client.build_beam_buffer(br)
	var mm: MultiMesh = agent_multi.multimesh
	var count: int = buf.size() / 16
	if mm.instance_count != count:
		mm.instance_count = count
	if count > 0:
		mm.buffer = buf


# Feed controller rays into the Rust interaction policy and drive the
# server-authoritative drag protocol. Idle cost is two trigger reads; the
# candidate arrays are only consulted while a trigger is live or a grab is
# in flight.
func _update_interaction() -> void:
	_ensure_controller_rays()
	if _interaction == null or _binary_client == null:
		return

	# A two-hand graph gesture owns both hands: don't let a same-frame grip+trigger
	# combination start a node grab underneath it. (An already-in-flight grab
	# blocks the gesture from engaging, so this only suppresses NEW grabs.)
	if _two_hand_active:
		return

	# Active drag: the grabbed node rides the grab controller's aim point.
	if _grabbed_id != -1:
		var trigger_now: float = _controller_trigger(_grab_controller)
		if trigger_now < GRAB_RELEASE or _grab_controller == null:
			if _binary_client.has_method("send_drag_end"):
				_binary_client.send_drag_end(_grabbed_id)
			# Track the node this drag pinned so Unpin All can release it later. The
			# server pins on drag; drag-end above is our own release of THIS node, but
			# the operator may re-pin via a fresh drag — recording it keeps the Unpin
			# All set authoritative for the session.
			if _grabbed_id >= 0:
				_pinned_ids[_grabbed_id] = true
				_refresh_controls_status()
			_pulse(_grab_controller, 0.3, 0.05)
			_grabbed_id = -1
			_grab_controller = null
			_grab_target_server = Vector3.ZERO
		else:
			# Keep the node at the distance it was grabbed at (ride the ray), rather
			# than snapping it to a fixed point in front of the wand. World→server
			# because the store works in GraphRoot-local space. The Rust hunt pins
			# the grabbed node to _grab_target_server each frame (optimistic echo).
			var ray_origin: Vector3 = _grab_controller.global_position
			var ray_dir: Vector3 = -_grab_controller.global_transform.basis.z
			var hand_world: Vector3 = ray_origin + ray_dir * _grab_distance
			var hand_server: Vector3 = graph_root.global_transform.affine_inverse() * hand_world
			_grab_target_server = hand_server
			if _binary_client.has_method("send_drag_update"):
				_binary_client.send_drag_update(_grabbed_id, hand_server)
		return

	for controller: XRController3D in [left_controller, right_controller]:
		if controller == null or not controller.get_is_active():
			continue
		# Don't start a node grab with a controller that's pointing at the HUD —
		# that trigger pull is a button click. (Release of an already-held grab is
		# handled above and is unaffected.)
		if controller == _hud_pointer_controller:
			continue
		# Likewise a trigger pull aimed at the open radial menu is a menu click,
		# not a node grab.
		if controller == _radial_pointer_controller:
			continue
		var pinch: float = _controller_trigger(controller)
		if pinch < 0.05:
			continue
		_grab_controller = controller
		# Candidate ids + render positions come from the Rust store (the drawn
		# subset from the last node buffer build). Positions are GraphRoot-local
		# scaled space; transform to world so the world-space wand ray and the
		# interaction's metre-space thresholds intersect the nodes.
		var render_ids: PackedInt32Array = _binary_client.get_render_ids()
		var render_positions: PackedVector3Array = _binary_client.get_render_positions()
		var gxf: Transform3D = graph_root.global_transform
		var world_positions := PackedVector3Array()
		world_positions.resize(render_positions.size())
		for i: int in range(render_positions.size()):
			world_positions[i] = gxf * render_positions[i]
		_interaction.evaluate_ray(
			controller.global_position,
			-controller.global_transform.basis.z,
			pinch,
			render_ids,
			world_positions
		)
		break


func _controller_trigger(controller: XRController3D) -> float:
	if controller == null or not controller.get_is_active():
		return 0.0
	return controller.get_float("trigger")


# Desktop-parity laser pointers. A thin emissive beam is parented to each wand,
# extending down the aim pose's -Z (the same ray fed to the Rust interaction
# policy). Created once, then shown only while the controller is tracking and
# tinted green as the trigger engages so the user sees the selection ray.
const RAY_LENGTH: float = 5.0
const RAY_IDLE_COLOR := Color(0.35, 0.7, 1.0)   # cyan when tracking, idle
const RAY_ACTIVE_COLOR := Color(0.3, 1.0, 0.4)  # green as the trigger pulls


func _ensure_controller_rays() -> void:
	for controller: XRController3D in [left_controller, right_controller]:
		if controller == null:
			continue
		var ray: MeshInstance3D = controller.get_node_or_null("AimRay") as MeshInstance3D
		if ray == null:
			var mesh := BoxMesh.new()
			# Thin beam down -Z; the box is centred so offset it forward by half.
			mesh.size = Vector3(0.006, 0.006, RAY_LENGTH)
			var mat := StandardMaterial3D.new()
			mat.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
			mat.emission_enabled = true
			mat.emission = RAY_IDLE_COLOR
			mat.albedo_color = RAY_IDLE_COLOR
			ray = MeshInstance3D.new()
			ray.name = "AimRay"
			ray.mesh = mesh
			ray.material_override = mat
			ray.position = Vector3(0.0, 0.0, -RAY_LENGTH * 0.5)
			controller.add_child(ray)
		var active: bool = controller.get_is_active()
		ray.visible = active
		if active:
			var cur_mat := ray.material_override as StandardMaterial3D
			if cur_mat != null:
				var col: Color = RAY_ACTIVE_COLOR if _controller_trigger(controller) > 0.05 else RAY_IDLE_COLOR
				cur_mat.emission = col
				cur_mat.albedo_color = col


func _pulse(controller: XRController3D, amplitude: float, duration_sec: float) -> void:
	if controller != null and controller.get_is_active():
		controller.trigger_haptic_pulse("haptic", 0.0, amplitude, duration_sec, 0.0)


func _update_voice_listener() -> void:
	if _voice_router == null or not _voice_router.has_method("update_listener"):
		return
	var camera: XRCamera3D = _find_xr_camera()
	if camera == null:
		return
	var cam_pos: Vector3 = camera.global_position
	var cam_fwd: Vector3 = -camera.global_transform.basis.z
	var cam_up: Vector3 = camera.global_transform.basis.y
	_voice_router.update_listener(cam_pos, cam_fwd, cam_up)


# --- Agent embodiment (M3) + selection (M4) ---------------------------------

## Spawn a geometric agent embodiment. Distinct from a human presence peer: an
## agent is a utility actor placed on the proxemics arc, keyed by did:nostr.
func spawn_agent(agent_id: String, display_name: String, did: String, verified: bool) -> void:
	if _agents.has(agent_id):
		return
	var template: PackedScene = load(AGENT_TEMPLATE_PATH)
	if template == null:
		push_warning("AgentAvatar template missing")
		return
	var agent: Node3D = template.instantiate()
	agent_spawner.add_child(agent)
	var handle: int = _next_handle
	_next_handle += 1
	agent.set_meta("agent_id", agent_id)
	agent.set_meta("handle", handle)
	agent.set_meta("did", did)
	if agent.has_method("set_avatar_identity"):
		agent.set_avatar_identity(display_name, did, verified)
	_agents[agent_id] = agent
	_agent_by_handle[handle] = agent_id
	_agent_order.append(agent_id)
	if _selection != null and _selection.has_method("register_identity"):
		_selection.register_identity(handle, did)
	_last_solve_count = -1  # force an arc re-solve on the next frame
	_resolve_agent_arc()


func despawn_agent(agent_id: String) -> void:
	if not _agents.has(agent_id):
		return
	var agent: Node3D = _agents[agent_id]
	var handle: int = agent.get_meta("handle", 0)
	_agent_by_handle.erase(handle)
	_agents.erase(agent_id)
	_agent_cases.erase(agent_id)
	var idx: int = _agent_order.find(agent_id)
	if idx != -1:
		_agent_order.remove_at(idx)
	agent.queue_free()
	_last_solve_count = -1
	_resolve_agent_arc()
	_publish_case_count()


## An agent raised a broker case awaiting the operator's approval. Drives the
## agent's own state (saturated colour + attention motion) and, if selected,
## the HUD intervention panel.
func set_agent_case(agent_id: String, case_id: String, summary: String) -> void:
	if not _agents.has(agent_id):
		return
	_agent_cases[agent_id] = {"case_id": case_id, "summary": summary}
	var agent: Node3D = _agents[agent_id]
	if agent.has_method("apply_signal"):
		agent.apply_signal(agent_avatar_signal_approval_requested())
	_publish_case_count()


func clear_agent_case(agent_id: String) -> void:
	if not _agent_cases.has(agent_id):
		return
	_agent_cases.erase(agent_id)
	if _agents.has(agent_id):
		var agent: Node3D = _agents[agent_id]
		if agent.has_method("apply_signal"):
			agent.apply_signal(agent_avatar_signal_approval_granted())
	_publish_case_count()


static func agent_avatar_signal_approval_requested() -> int:
	return 2  # AgentSignal::ApprovalRequested (avatar_state.rs:418)


static func agent_avatar_signal_approval_granted() -> int:
	return 3  # AgentSignal::ApprovalGranted


func _publish_case_count() -> void:
	if hud != null and hud.has_method("set_case_count"):
		hud.set_case_count(_agent_cases.size())


# Place agents on a forward arc in the social band via the Rust proxemics solver
# (Hall's zones, ±60°, 1.5–2.5 m). Re-solve only on a membership or user-pose
# change (the solver's own gate keeps it off the per-frame path).
func _resolve_agent_arc() -> void:
	if _proxemics == null or _agent_order.is_empty():
		return
	var camera: XRCamera3D = _find_xr_camera()
	var user_pos: Vector3 = camera.global_position if camera != null else Vector3.ZERO
	var user_fwd: Vector3 = -camera.global_transform.basis.z if camera != null else Vector3.FORWARD
	var count: int = _agent_order.size()

	if count == _last_solve_count and _proxemics.has_method("should_resolve"):
		if not _proxemics.should_resolve(
			_last_solve_pos, _last_solve_forward, _last_solve_count,
			user_pos, user_fwd, count
		):
			return

	var positions: PackedVector3Array = _proxemics.solve(user_pos, user_fwd, count)
	for i: int in range(mini(positions.size(), count)):
		var agent_id: String = _agent_order[i]
		if _agents.has(agent_id):
			(_agents[agent_id] as Node3D).global_position = positions[i]
	_last_solve_pos = user_pos
	_last_solve_forward = user_fwd
	_last_solve_count = count


# Swarm tab roster (ADR-140, Pillar 4 / P5). Built from the Rust agent registry
# (#[func]s agent_ids/agent_status/agent_target_node/agent_task) and pushed to the
# HUD at the ~4 Hz label cadence, but only when the roster actually changed (a cheap
# signature diff avoids rebuilding the row UI every tick). Agent wire ids ARE node
# ids, so each row's tap → "teleport:<id>" reuses the existing node-teleport glide.
var _swarm_sig: String = ""


func _update_swarm_roster() -> void:
	if hud == null or not hud.has_method("set_swarm_roster"):
		return
	if _binary_client == null or not _binary_client.has_method("agent_ids"):
		return
	var ids: PackedInt32Array = _binary_client.agent_ids()
	var rows: Array = []
	var sig := PackedStringArray()
	for id: int in ids:
		var target_id: int = _binary_client.agent_target_node(id)
		var target_label: String = ""
		if target_id >= 0:
			target_label = _binary_client.label_of(target_id)
		var status: int = _binary_client.agent_status(id)
		var task: String = _binary_client.agent_task(id)
		rows.append({
			"id": id,
			"name": _binary_client.label_of(id),
			"status": status,
			"target": target_label,
			"task": task,
		})
		# Name is in the signature so a late-arriving label refreshes its row.
		sig.append("%d:%s:%d:%s:%s" % [id, _binary_client.label_of(id), status, target_label, task])
	var joined := "|".join(sig)
	if joined == _swarm_sig:
		return
	_swarm_sig = joined
	hud.set_swarm_roster(rows)


func _update_agents(delta: float) -> void:
	if _agents.is_empty():
		return
	_resolve_agent_arc()
	var camera: XRCamera3D = _find_xr_camera()
	if camera == null:
		return
	var cam_pos: Vector3 = camera.global_position
	var cam_fwd: Vector3 = -camera.global_transform.basis.z
	var dt_us: int = int(delta * 1_000_000.0)
	# Reuse _update_lod's once-per-frame recompute decision (never re-tick here).
	var recompute_lod: bool = _lod_recompute and _lod_policy != null \
		and _lod_policy.has_method("agent_feature_mask")

	for agent_id: String in _agents:
		var agent: Node3D = _agents[agent_id]
		var to_agent: Vector3 = agent.global_position - cam_pos
		var dist: float = to_agent.length()
		# Mutual-gaze test: is the user's head-gaze pointed at this agent?
		var gazing: bool = dist > 0.001 and cam_fwd.dot(to_agent / dist) > MUTUAL_GAZE_DOT
		if agent.has_method("tick_attention"):
			agent.tick_attention(gazing, cam_pos - agent.global_position, false, 0, Vector3.ZERO, dt_us)
		# LOD feature mask (badge drops first, then cone, core billboards).
		if recompute_lod and agent.has_method("set_feature_mask"):
			var level: int = _lod_policy.classify_distance(dist)
			agent.set_feature_mask(_lod_policy.agent_feature_mask(level))


# Feed the three-resolver arbiter this frame's controller rays, smoothed gaze,
# and agent candidates; drive the dwell reticle from the charge ratio.
func _update_selection(delta: float) -> void:
	if _selection == null or _agents.is_empty():
		return
	var camera: XRCamera3D = _find_xr_camera()
	if camera == null:
		return

	_selection.begin_frame()
	var any_controller: bool = false
	var hand: int = 0
	for controller: XRController3D in [left_controller, right_controller]:
		if controller != null and controller.get_is_active():
			any_controller = true
			var trig: float = controller.get_float("trigger")
			_selection.push_controller(
				hand,
				controller.global_position,
				-controller.global_transform.basis.z,
				trig,
				true,
				trig > 0.7
			)
		hand += 1

	# Smoothed head-gaze ray (eye-gaze only if the probe found hardware support).
	var cam_pos: Vector3 = camera.global_position
	var cam_fwd: Vector3 = -camera.global_transform.basis.z
	if _gaze_tracker != null and _gaze_tracker.has_method("resolve"):
		var gdir: Vector3 = _gaze_tracker.resolve(cam_pos, cam_fwd, delta, _eye_gaze_supported)
		_selection.set_gaze(cam_pos, gdir)
	else:
		_selection.set_gaze(cam_pos, cam_fwd)

	# Candidates = the agents (handles), with the core radius + margin.
	var ids := PackedInt32Array()
	var positions := PackedVector3Array()
	var radii := PackedFloat32Array()
	for agent_id: String in _agents:
		var agent: Node3D = _agents[agent_id]
		ids.append(agent.get_meta("handle", 0))
		positions.append(agent.global_position)
		radii.append(AGENT_SELECT_RADIUS)
	_selection.set_candidates(ids, positions, radii)

	_selection.tick(not any_controller, Time.get_ticks_usec(), int(delta * 1_000_000.0))

	if hud != null and hud.has_method("set_dwell_charge") and _selection.has_method("charge_ratio"):
		hud.set_dwell_charge(_selection.charge_ratio())


func _on_selection_made(handle: int, did_nostr: String, _resolver: int) -> void:
	if not _agent_by_handle.has(handle):
		return
	var agent_id: String = _agent_by_handle[handle]
	emit_signal("agent_selected", agent_id, did_nostr)
	# M4-RAY: the arbiter resolved a non-origin agent-node selection — fire the
	# liveness canary once (RES-a / ADR-130 D3).
	_fire_m4_ray_canary(agent_id, did_nostr)
	# If the selected agent is awaiting approval, open the intervention panel.
	if _agent_cases.has(agent_id) and hud != null and hud.has_method("show_case"):
		var c: Dictionary = _agent_cases[agent_id]
		hud.show_case(c.get("case_id", ""), c.get("summary", ""))


# One-shot POST /api/canary/observe/CANARY-VC-M4-RAY, latched so it fires exactly
# once per session — on the first resolved agent selection. The route is
# unauthenticated (a thin JSON adapter over the LivenessHarness), so no NIP-98
# header is needed. Fail-open: a failed dispatch un-latches so a later selection
# can retry, and never blocks the scene.
func _fire_m4_ray_canary(agent_id: String, did_nostr: String) -> void:
	if _m4_ray_observed or _observe_http == null:
		return
	_m4_ray_observed = true
	var url: String = "%s/api/canary/observe/%s" % [_http_base(), CANARY_M4_RAY]
	var body: Dictionary = {
		"evidence": "xr-client selection arbiter resolved agent %s (%s)" % [agent_id, did_nostr],
	}
	var headers := PackedStringArray(["Content-Type: application/json"])
	var err: int = _observe_http.request(url, headers, HTTPClient.METHOD_POST, JSON.stringify(body))
	if err != OK:
		push_warning("GraphScene: M4-RAY observe POST failed to start (%d)" % err)
		_m4_ray_observed = false


# Inbound non-topology JSON on the multiplexed /wss graph socket (M2). The Rust
# BinaryProtocolClient forwards these verbatim via text_message; we parse the
# envelope and route broker:new_case to the HUD intervention panel's entry point.
func _on_graph_text(json: String) -> void:
	var parsed: Variant = JSON.parse_string(json)
	if typeof(parsed) != TYPE_DICTIONARY:
		return
	var msg: Dictionary = parsed
	match str(msg.get("type", "")):
		"broker:new_case":
			# A malformed frame can carry a non-Dictionary payload (string, null,
			# array); passing that to a Dictionary-typed param crashes
			# _physics_process. Guard the type before routing.
			var p: Variant = msg.get("payload", {})
			if typeof(p) == TYPE_DICTIONARY:
				_handle_broker_new_case(p)
		"nodeUnpinAck":
			# Server confirmed the release (position_updates.rs handle_node_unpin →
			# {"type":"nodeUnpinAck","data":{"nodeId":N}}). Drop the id from the
			# retry set only now; unacked ids stay so Unpin All can retry them.
			var d: Variant = msg.get("data", {})
			if typeof(d) == TYPE_DICTIONARY and d.has("nodeId"):
				var nid: int = int(d["nodeId"])
				if _pinned_ids.erase(nid):
					_refresh_controls_status()


func _handle_broker_new_case(payload: Dictionary) -> void:
	var case_id: String = str(payload.get("caseId", ""))
	if case_id.is_empty():
		return
	var summary: String = str(payload.get("title", ""))
	# Surface the case through the HUD's existing intervention entry point.
	if hud != null and hud.has_method("show_case"):
		hud.show_case(case_id, summary)


func _tick_reconnect(delta: float) -> void:
	# Each socket owns its own countdown; firing one reconnects only that socket
	# so a pending graph reconnect can't tear down a live presence socket.
	if _graph_reconnect_timer >= 0.0:
		_graph_reconnect_timer -= delta
		if _graph_reconnect_timer <= 0.0:
			_graph_reconnect_timer = -1.0
			_connect_graph()
	if _presence_reconnect_timer >= 0.0:
		_presence_reconnect_timer -= delta
		if _presence_reconnect_timer <= 0.0:
			_presence_reconnect_timer = -1.0
			_connect_presence()


func _backoff_delay(attempts: int) -> float:
	return minf(
		RECONNECT_BASE_DELAY_SEC * pow(2.0, float(attempts - 1)),
		RECONNECT_MAX_DELAY_SEC
	)


func _schedule_graph_reconnect() -> void:
	_graph_reconnect_attempts += 1
	_graph_reconnect_timer = _backoff_delay(_graph_reconnect_attempts)
	push_warning("GraphScene: graph reconnect attempt %d in %.1fs" % [
		_graph_reconnect_attempts, _graph_reconnect_timer])


func _schedule_presence_reconnect() -> void:
	_presence_reconnect_attempts += 1
	_presence_reconnect_timer = _backoff_delay(_presence_reconnect_attempts)
	push_warning("GraphScene: presence reconnect attempt %d in %.1fs" % [
		_presence_reconnect_attempts, _presence_reconnect_timer])


func _find_xr_camera() -> XRCamera3D:
	# During an active OpenXR session the XRCamera3D is the viewport's current
	# camera — that is the canonical lookup.
	var viewport_cam: Camera3D = get_viewport().get_camera_3d()
	if viewport_cam is XRCamera3D:
		return viewport_cam as XRCamera3D
	# Fallback before the session is current: locate the XROrigin3D in the tree
	# and return its XRCamera3D child.
	var origin: Node = get_tree().current_scene.find_child("XROrigin3D", true, false) if get_tree().current_scene != null else null
	if origin != null:
		for child: Node in origin.get_children():
			if child is XRCamera3D:
				return child as XRCamera3D
	return null


# Graph (/wss) socket state. `connected` is emitted only once the subscribe
# handshake completes (transport.rs), so a live signal always means a usable
# stream. Clear any pending graph reconnect timer on connect, else a queued
# _tick_reconnect would tear the fresh socket back down.
func _on_connection_changed(connected: bool) -> void:
	emit_signal("connection_status_changed", connected)
	if connected:
		_graph_reconnect_attempts = 0
		_graph_reconnect_timer = -1.0
	else:
		_schedule_graph_reconnect()


# Presence (/ws/presence) socket state, on its own independent backoff.
func _on_presence_connection_changed(connected: bool) -> void:
	if connected:
		_presence_reconnect_attempts = 0
		_presence_reconnect_timer = -1.0
	else:
		_schedule_presence_reconnect()


# position_updated is no longer emitted by the Rust client (the store owns
# positions), so there is no _on_position_updated handler. Colours + centrality
# arrive via the throttled node_visuals_updated below; positions never touch
# GDScript.
func _on_node_visuals_updated(node_id: int, community_id: int, centrality: float, anomaly: float) -> void:
	# Colour is now computed Rust-side inside build_node_buffer; GDScript keeps only
	# centrality (for the LOD/edge selection domain) and marks the selection dirty
	# so a new/changed node re-enters the drawn set.
	_node_centrality[node_id] = centrality
	_centrality_max = maxf(_centrality_max, centrality)
	_selection_dirty = true
	# Topology often arrives before centrality analytics, forcing the edge ranking
	# onto its global-weight fallback. Re-rank ONCE — not per-frame — the moment
	# centrality covers the nodes the cap will actually draw, so the rendered edge
	# subset finally reflects the visible subgraph.
	if not _edge_rerank_done and not _edge_pairs_full.is_empty():
		var node_total: int = _binary_client.node_count() if _binary_client != null else 0
		var need: int = mini(_node_budget, maxi(1, node_total))
		if _node_centrality.size() >= need:
			_rerank_edges(true)
			_edge_rerank_done = true


# Deterministic community palette: golden-ratio hue walk gives well-separated
# colours for any community count (same approach as the desktop renderer).
# When the server has not computed communities (all community_id == 0, which
# collapses every node to hue 0 / red), fall back to a per-node golden-ratio hue
# so the graph still reads as varied rather than a uniform mass. Anomalous nodes
# blend toward warning red regardless.
func _community_color(community_id: int, anomaly: float, node_id: int = 0) -> Color:
	var key: int = community_id if community_id != 0 else node_id
	var hue: float = fmod(float(key) * 0.61803398875, 1.0)
	var base: Color = Color.from_hsv(hue, 0.6, 0.95)
	if anomaly > 0.5:
		return base.lerp(Color(1.0, 0.15, 0.1), clampf((anomaly - 0.5) * 2.0, 0.0, 0.85))
	return base


func _on_topology_updated(_edge_count: int) -> void:
	# Keep the full topology verbatim so the ranking can be recomputed once
	# centrality analytics arrive (see _on_node_visuals_updated).
	_edge_pairs_full = _binary_client.get_edges()
	_edge_weights_full = _binary_client.get_edge_weights()
	_edge_rerank_done = false
	# A fresh topology is a new layout: release any two-hand manual latch so the
	# adaptive fit re-frames the new graph automatically.
	_manual_transform = false
	_two_hand_active = false
	_manip_session_base_scale = -1.0
	# Endpoint-id set (built once here, not per frame): the LOD draw domain and the
	# edge ranking domain are both restricted to these so they stay coherent.
	_topo_ids = {}
	for i: int in range(_edge_pairs_full.size()):
		_topo_ids[_edge_pairs_full[i]] = true
	_recompute_instance_budgets()
	_selection_dirty = true
	var known: int = _binary_client.node_count() if _binary_client != null else 0
	print("GraphScene DEBUG: topology arrived edges=%d nodes=%d positions_known=%d budgets=%d/%d" % [
		_edge_weights_full.size(), _topo_ids.size(), known, _node_budget, _edge_budget])
	# preserve_show=false: a fresh topology defaults to showing all ranked edges.
	_rerank_edges(false)


# Derive the runtime instance budgets from the received topology, each bounded by
# its absolute safety ceiling so a runaway settings-driven load can't overrun the
# Quest instance buffers. Node budget follows the count of topology (edge-endpoint)
# nodes; edge budget follows the topology edge count. When topology carries no
# edges the node budget stays at the ceiling so the position stream still draws.
func _recompute_instance_budgets() -> void:
	var topo_nodes: int = _topo_ids.size()
	_node_budget = NODE_SAFETY_CEILING if topo_nodes <= 0 else mini(NODE_SAFETY_CEILING, topo_nodes)
	_edge_budget = mini(EDGE_SAFETY_CEILING, _edge_weights_full.size())
	_selection_dirty = true
	if hud != null:
		_refresh_controls_status()


# The node ids the LOD cap will actually draw: all edge-endpoint (topo) nodes when
# they fit the cap, else the top-cap of them by centrality. Mirrors the TOPO bias
# in _update_multimesh so the edge ranking domain equals the drawn domain. Empty
# when no topology has arrived. Event-driven (topology / analytics), not per frame.
func _topo_top_ids() -> Dictionary:
	var out := {}
	if _topo_ids.is_empty():
		return out
	var domain: Array = _topo_ids.keys()
	if domain.size() <= _node_budget:
		for id: int in domain:
			out[id] = true
		return out
	domain.sort_custom(func(a: int, b: int) -> bool:
		return _node_centrality.get(a, 0.0) > _node_centrality.get(b, 0.0))
	for i: int in range(_node_budget):
		out[domain[i]] = true
	return out


# (Re)compute the weight-ranked edge list from the kept full topology. When over
# budget, an edge earns budget only if BOTH endpoints are in the top-centrality
# node set the node cap will actually draw (visible-subgraph ranking); it falls
# back to global-weight ranking only when no draw domain exists yet. Runs on
# topology and once more when centrality lands — never per frame.
func _rerank_edges(preserve_show: bool) -> void:
	var pairs: PackedInt32Array = _edge_pairs_full
	var weights: PackedFloat32Array = _edge_weights_full
	var total: int = weights.size()
	if total <= 0:
		_edge_pairs_ranked = PackedInt32Array()
		_edge_show_count = 0
		_apply_edge_slice()
		return
	if total <= _edge_budget:
		_edge_pairs_ranked = pairs
		if not preserve_show:
			_edge_show_count = total
		_apply_edge_slice()
		return
	# Rank over the SAME domain the LOD path draws: the top-cap topo (edge-endpoint)
	# nodes by centrality. This keeps the ranking domain and the drawn domain equal,
	# so both-endpoints-drawn edges actually render instead of being ranked in but
	# never shown.
	var eligible: Array = []
	var top_ids: Dictionary = _topo_top_ids()
	if not top_ids.is_empty():
		for e: int in range(total):
			if top_ids.has(pairs[e * 2]) and top_ids.has(pairs[e * 2 + 1]):
				eligible.append(e)
	# Fallback to global-weight ranking only when there is no usable draw domain yet
	# (no topology endpoints) — a genuinely sparse-but-real subgraph is kept as-is
	# rather than padded with edges whose endpoints will never be drawn.
	if eligible.is_empty():
		eligible = range(total)
	eligible.sort_custom(func(a: int, b: int) -> bool: return weights[a] > weights[b])
	var kept: int = mini(_edge_budget, eligible.size())
	var capped := PackedInt32Array()
	capped.resize(kept * 2)
	for i: int in range(kept):
		var e: int = eligible[i]
		capped[i * 2] = pairs[e * 2]
		capped[i * 2 + 1] = pairs[e * 2 + 1]
	_edge_pairs_ranked = capped
	if not preserve_show:
		_edge_show_count = kept
	_apply_edge_slice()
	push_warning("GraphScene: %d edges exceed budget; rendering %d (visible-subgraph ranked)" % [total, kept])


# Render the first _edge_show_count pairs of the ranked edge list (heaviest-first),
# clamped to [EDGE_SHOW_MIN, ranked size]. Zero-copy when showing all.
func _apply_edge_slice() -> void:
	var ranked_pairs: int = _edge_pairs_ranked.size() / 2
	if ranked_pairs <= 0:
		_edge_pairs = PackedInt32Array()
		_edge_show_count = 0
		return
	_edge_show_count = clampi(_edge_show_count, mini(EDGE_SHOW_MIN, ranked_pairs), ranked_pairs)
	if _edge_show_count >= ranked_pairs:
		_edge_pairs = _edge_pairs_ranked
	else:
		_edge_pairs = _edge_pairs_ranked.slice(0, _edge_show_count * 2)


func _on_avatar_joined(did: String, display_name: String, avatar_id: String) -> void:
	var template: PackedScene = load(AVATAR_TEMPLATE_PATH)
	if template == null:
		push_warning("Avatar template missing")
		return
	var avatar: Node = template.instantiate()
	avatar_spawner.add_child(avatar)
	avatar.set_meta("avatar_id", avatar_id)
	avatar.set_meta("did", did)
	# M1 (PRD-023 WP-9): render the did:nostr the presence join carried. The
	# badge reads unverified until the client runs the Schnorr-challenge check
	# (ADR-130 Decision 6) — the server-vouched roster DID is not client-trusted
	# on presentation (invariant 2).
	if avatar.has_method("set_avatar_identity"):
		avatar.set_avatar_identity(display_name, did, false)
	elif avatar.has_method("set_display_name"):
		avatar.set_display_name(display_name)
	_avatars[avatar_id] = avatar

	if _voice_router != null and _voice_router.has_method("attach_track"):
		_voice_router.attach_track(did, avatar.global_position)

	emit_signal("avatar_count_changed", _avatars.size())


func _on_avatar_left(avatar_id: String) -> void:
	if not _avatars.has(avatar_id):
		return
	var av: Node = _avatars[avatar_id]
	var did: String = av.get_meta("did", "")
	if _voice_router != null and _voice_router.has_method("detach_track") and did != "":
		_voice_router.detach_track(did)
	_avatars.erase(avatar_id)
	av.queue_free()
	emit_signal("avatar_count_changed", _avatars.size())


func _on_avatar_pose_updated(
	avatar_id: String,
	head_pos: Vector3,
	head_rot: Quaternion,
	has_left: bool,
	has_right: bool,
	left_pos: Vector3,
	left_rot: Quaternion,
	right_pos: Vector3,
	right_rot: Quaternion
) -> void:
	if not _avatars.has(avatar_id):
		return
	var av: Node3D = _avatars[avatar_id]
	if av.has_method("apply_pose"):
		# Forward articulated hand transforms so remote avatars get real hands;
		# has_left/has_right gate whether the pos/rot are meaningful.
		av.apply_pose(
			head_pos, head_rot, has_left, has_right,
			left_pos, left_rot, right_pos, right_rot
		)


func _on_voice_activity(avatar_id: String, active: bool) -> void:
	if not _avatars.has(avatar_id):
		return
	var av: Node3D = _avatars[avatar_id]
	if av.has_method("set_speaking"):
		av.set_speaking(active)


func _on_node_targeted(node_id: int, _distance: float) -> void:
	# During an on-demand radial pick (A/X pressed), route the hit to the capture
	# and skip the normal target side effects (haptic/print) — this is a query, not
	# a hover.
	if _radial_picking:
		_radial_pick_capture = node_id
		return
	emit_signal("node_targeted_in_scene", node_id)
	if node_id != _last_targeted_id:
		print("GraphScene DEBUG: node_targeted id=%d dist=%.2f" % [node_id, _distance])
		_last_targeted_id = node_id
		_pulse(_grab_controller, 0.15, 0.02)


func _on_node_grabbed(node_id: int, _position: Vector3) -> void:
	if _grabbed_id == node_id:
		return
	# Double-click = two grabs of the SAME node within the window → open the node's
	# linked page in the HUD instead of grabbing. The grab this second press would
	# start is suppressed cleanly (we return before setting _grabbed_id / drag_start).
	var now: float = float(Time.get_ticks_msec()) / 1000.0
	if node_id == _dc_last_id and (now - _dc_last_time) <= DOUBLE_CLICK_SEC:
		_dc_last_id = -1
		_open_node_document(node_id)
		return
	_dc_last_id = node_id
	_dc_last_time = now
	_grabbed_id = node_id
	print("GraphScene DEBUG: node_grabbed id=%d" % node_id)
	# Node render position now lives in the Rust store.
	var pos: Vector3 = _binary_client.node_position(node_id) if _binary_client != null else Vector3.ZERO
	_grab_target_server = pos
	# Remember how far along the ray the node was so the drag preserves depth.
	if _grab_controller != null:
		var node_world: Vector3 = graph_root.global_transform * pos
		_grab_distance = clampf(_grab_controller.global_position.distance_to(node_world), 0.2, 6.0)
	if _binary_client != null and _binary_client.has_method("send_drag_start"):
		_binary_client.send_drag_start(node_id, pos)
	_pulse(_grab_controller, 0.6, 0.08)


# Resolve a node to its narrativegoldmine slug and hand it to the HUD document
# view. Slug = slugify(metadata_id if present else label). Slugify is idempotent,
# so slugifying a real slug is a no-op and a title-shaped metadata_id is fixed.
func _open_node_document(node_id: int) -> void:
	if hud == null or not hud.has_method("show_document") or _binary_client == null:
		return
	var meta_id: String = _binary_client.meta_id_of(node_id) if _binary_client.has_method("meta_id_of") else ""
	var label: String = _binary_client.label_of(node_id) if _binary_client.has_method("label_of") else ""
	var raw: String = meta_id if meta_id != "" else label
	var slug: String = _slugify(raw)
	if slug == "":
		return
	var title: String = label if label != "" else slug
	hud.show_document(title, slug)


# Port of client/src/features/graph/utils/pageLinks.ts slugifyLabel: lowercase,
# collapse every run of non [a-z0-9] to a single '-', trim leading/trailing '-'.
func _slugify(s: String) -> String:
	var lower: String = s.to_lower()
	var out: String = ""
	var prev_dash: bool = true  # start true so a leading run doesn't emit a dash
	for i: int in range(lower.length()):
		var ch: String = lower[i]
		if (ch >= "a" and ch <= "z") or (ch >= "0" and ch <= "9"):
			out += ch
			prev_dash = false
		elif not prev_dash:
			out += "-"
			prev_dash = true
	if out.ends_with("-"):
		out = out.substr(0, out.length() - 1)
	return out


func _on_presence_kicked(reason: String) -> void:
	push_warning("GraphScene: kicked from presence -- %s" % reason)
	_schedule_presence_reconnect()


# --- Radial node context menu (visual query builder, flagship) --------------
#
# The MENU button (or A/X on Index/Touch-class wands — Vive has no A/X) opens a
# wand-operated radial menu ON the currently targeted node; the menu items combine
# the query-builder mark/clear/execute actions with any extra (future Wave-2
# expand) items. While open, the wand ray drives the menu's SubViewport via its
# pointer_input API (trigger = click), exactly the way the HUD pointer works. The
# same button toggles the menu closed.
func _update_radial_menu() -> void:
	if _radial == null:
		return
	# Edge-detect A/X on either controller.
	var ax_down := false
	var ax_ctrl: XRController3D = null
	for controller: XRController3D in [right_controller, left_controller]:
		if controller == null or not controller.get_is_active():
			continue
		# Vive wands have no A/X — their menu button (above the trackpad) opens
		# the node radial; A/X kept for Index/Touch-class controllers.
		if controller.is_button_pressed("menu_button") or controller.is_button_pressed("ax_button"):
			ax_down = true
			ax_ctrl = controller
			break
	if ax_down and not _radial_ax_was_down:
		if _radial.visible:
			_radial.close()
		else:
			_open_radial_on_target(ax_ctrl)
	_radial_ax_was_down = ax_down

	# Drive the menu with the wand ray while it is open.
	_radial_pointer_controller = null
	if not _radial.visible:
		return
	var panel := _radial.get_node_or_null("MenuPanel") as Node3D
	if panel == null:
		return
	var hit_uv := Vector2(-1.0, -1.0)
	var hit_ctrl: XRController3D = null
	for controller: XRController3D in [right_controller, left_controller]:
		if controller == null or not controller.get_is_active():
			continue
		# Arbitration: this controller's ray is nearer the HUD — it owns the HUD, so
		# the radial yields it this frame.
		if controller == _hud_owner:
			continue
		var uv := _ray_hit_menu_uv(controller, panel)
		if uv.x >= 0.0:
			hit_uv = uv
			hit_ctrl = controller
			break
	if hit_ctrl == null:
		# Ray left the panel: release any held click at the last spot so the
		# SubViewport button doesn't latch pressed.
		if _radial_trigger_was_down:
			_radial.pointer_input(_radial_last_px, false)
			_radial_trigger_was_down = false
		return
	_radial_pointer_controller = hit_ctrl
	_radial_last_px = hit_uv
	var click := _controller_trigger(hit_ctrl) >= 0.6
	if QB_DEBUG and click != _radial_trigger_was_down:
		print("[QB] radial pointer %s at uv=(%.2f,%.2f)" % ["PRESS" if click else "RELEASE", hit_uv.x, hit_uv.y])
	_radial.pointer_input(hit_uv, click)
	_radial_trigger_was_down = click


# Open the radial on the node the given controller's ray is aimed at. No-op when
# the ray isn't on a node.
func _open_radial_on_target(controller: XRController3D) -> void:
	if controller == null or _binary_client == null or graph_root == null:
		return
	var node_id := _pick_node_under_ray(controller)
	if node_id < 0:
		# Empty space: open the search-and-teleport "top labels" radial instead
		# (Feature 2 — the wand-friendly, keyboardless search path).
		_open_top_labels_radial(controller)
		return
	_radial_node_id = node_id
	# Include any cached predicate-expansion items via the query builder's
	# extra_items seam; a fresh relations fetch (below) repopulates when it lands.
	var extra: Array = _relations_cache.get(node_id, [])
	var items: Array = _query.build_node_menu_items(node_id, extra)
	var world_pos: Vector3 = graph_root.global_transform * _binary_client.node_position(node_id)
	var cam: XRCamera3D = _find_xr_camera()
	if cam != null:
		# Float the panel slightly toward the user so it isn't buried in the node.
		var to_cam: Vector3 = (cam.global_position - world_pos)
		if to_cam.length() > 0.001:
			world_pos += to_cam.normalized() * 0.15
	_radial_world_pos = world_pos
	_radial.open(items, world_pos)
	# Face the user: the QuadMesh faces +Z, so aim -Z at the camera then flip.
	if cam != null:
		_radial.look_at(cam.global_position, Vector3.UP)
		_radial.rotate_object_local(Vector3.UP, PI)
	_pulse(controller, 0.3, 0.04)
	# Kick a relations fetch so the ring gains "→ label (N)" expansion items.
	_request_node_relations(node_id)


# Feature 2 — open a "top labels" radial (top TOP_LABELS_COUNT nodes by centrality)
# on empty space; selecting an item teleports to that node. Wand-friendly: no
# keyboard needed. The panel floats 1.5 m in front of the controller.
func _open_top_labels_radial(controller: XRController3D) -> void:
	if _binary_client == null or not _binary_client.has_method("top_labels"):
		return
	var ids: PackedInt32Array = _binary_client.top_labels(TOP_LABELS_COUNT)
	if ids.is_empty():
		return
	var items: Array = []
	for i: int in range(ids.size()):
		var nid: int = ids[i]
		var label: String = str(_binary_client.label_of(nid))
		if label.is_empty():
			label = "Node %d" % nid
		items.append({"label": "⌖ %s" % label, "action": "teleport:%d" % nid})
	_radial_node_id = -1
	var cam: XRCamera3D = _find_xr_camera()
	var world_pos: Vector3 = controller.global_position - controller.global_transform.basis.z * 1.5
	_radial_world_pos = world_pos
	_radial.open(items, world_pos)
	if cam != null:
		_radial.look_at(cam.global_position, Vector3.UP)
		_radial.rotate_object_local(Vector3.UP, PI)
	_pulse(controller, 0.3, 0.04)


# --- Feature 1: predicate expansion in the node radial ----------------------

# GET {base}/api/graph/node/{id}/relations. On success the ring is repopulated with
# "→ label (N)" outgoing / "← label (N)" incoming items via the extra_items seam.
func _request_node_relations(node_id: int) -> void:
	if _relations_http == null:
		return
	var url := "%s/api/graph/node/%d/relations" % [_http_base(), node_id]
	var headers := _auth_headers(url, "GET")
	_relations_http.cancel_request()  # supersede any in-flight fetch for a prior node
	var err := _relations_http.request(url, headers, HTTPClient.METHOD_GET)
	if err != OK:
		push_warning("GraphScene: relations GET failed to start (%d)" % err)


func _on_relations_completed(result: int, code: int, _headers: PackedStringArray, body: PackedByteArray) -> void:
	if result != HTTPRequest.RESULT_SUCCESS or code < 200 or code >= 300:
		return
	var parsed: Variant = JSON.parse_string(body.get_string_from_utf8())
	if typeof(parsed) != TYPE_DICTIONARY:
		return
	var node_id: int = int(parsed.get("nodeId", parsed.get("node_id", _radial_node_id)))
	var extra: Array = _build_expansion_items(parsed)
	_relations_cache[node_id] = extra
	# If the radial is still open on this node, repopulate the ring in place.
	# BUGFIX (live): do NOT repopulate while a wand click is in progress on the
	# menu. `_radial.open()` frees + rebuilds every Button; if that lands between the
	# synthesized press and release of a trigger-click, the pressed button is
	# destroyed and `item_selected` never fires — the "first mark doesn't persist"
	# bug. Deferring the async relations repopulate until the trigger is released
	# keeps the click intact. The items are cached, so the next open shows them.
	if _radial != null and _radial.visible and _radial_node_id == node_id and not _radial_trigger_was_down:
		var items: Array = _query.build_node_menu_items(node_id, extra)
		_radial.open(items, _radial_world_pos)
		var cam: XRCamera3D = _find_xr_camera()
		if cam != null:
			_radial.look_at(cam.global_position, Vector3.UP)
			_radial.rotate_object_local(Vector3.UP, PI)


# Build the radial expansion items from a relations response. Expected shape:
# {"outgoing":[{"edgeType":str,"count":int,"label":str?}], "incoming":[...]}. Falls
# back to the edgeType as the label. Action grammar "expand:<direction>:<edgeType>".
func _build_expansion_items(data: Dictionary) -> Array:
	var items: Array = []
	for dir_key: String in ["outgoing", "incoming"]:
		var arrow: String = "→" if dir_key == "outgoing" else "←"
		var rels: Variant = data.get(dir_key, [])
		if typeof(rels) != TYPE_ARRAY:
			continue
		for rel: Variant in rels:
			if typeof(rel) != TYPE_DICTIONARY:
				continue
			var edge_type: String = str(rel.get("edgeType", rel.get("edge_type", "")))
			if edge_type.is_empty():
				continue
			var count: int = int(rel.get("count", 0))
			var label: String = str(rel.get("label", edge_type))
			items.append({
				"label": "%s %s (%d)" % [arrow, label, count],
				"action": "expand:%s:%s" % [dir_key, edge_type],
			})
	return items


# POST {base}/api/graph/node/{id}/expand {edgeType,direction,limit}. The returned
# nodes are already in the position stream; the response's edges are additively
# merged into the topology (no re-fit) on completion.
func _expand_node(node_id: int, direction: String, edge_type: String) -> void:
	if _expand_http == null or node_id < 0:
		return
	var url := "%s/api/graph/node/%d/expand" % [_http_base(), node_id]
	var headers := _auth_headers(url, "POST")
	var payload := {"edgeType": edge_type, "direction": direction, "limit": EXPAND_LIMIT}
	_expand_http.cancel_request()
	var err := _expand_http.request(url, headers, HTTPClient.METHOD_POST, JSON.stringify(payload))
	if err != OK:
		push_warning("GraphScene: expand POST failed to start (%d)" % err)


func _on_expand_completed(result: int, code: int, _headers: PackedStringArray, body: PackedByteArray) -> void:
	if result != HTTPRequest.RESULT_SUCCESS or code < 200 or code >= 300:
		return
	var parsed: Variant = JSON.parse_string(body.get_string_from_utf8())
	if typeof(parsed) != TYPE_DICTIONARY:
		return
	# Collect the returned edges into a flat [s0,t0,…] pair list + parallel types.
	var edges: Variant = parsed.get("edges", [])
	if typeof(edges) != TYPE_ARRAY:
		return
	var pairs := PackedInt32Array()
	var types := PackedStringArray()
	for e: Variant in edges:
		if typeof(e) != TYPE_DICTIONARY:
			continue
		var s: int = int(e.get("source", e.get("source_id", -1)))
		var t: int = int(e.get("target", e.get("target_id", -1)))
		if s < 0 or t < 0:
			continue
		pairs.push_back(s)
		pairs.push_back(t)
		types.push_back(str(e.get("edgeType", e.get("edge_type", ""))))
	if pairs.is_empty() or _binary_client == null or not _binary_client.has_method("merge_expansion"):
		return
	var added: int = _binary_client.merge_expansion(pairs, types)
	if added > 0:
		_refresh_topology_additive()


# Additively fold newly merged edges into the draw pipeline: re-pull the edge list,
# extend the endpoint-id domain, recompute budgets and re-rank — WITHOUT re-fitting
# the view (GraphDBViewerWeb additive-merge principle: no rebuild, no re-fit).
func _refresh_topology_additive() -> void:
	if _binary_client == null:
		return
	_edge_pairs_full = _binary_client.get_edges()
	_edge_weights_full = _binary_client.get_edge_weights()
	for i: int in range(_edge_pairs_full.size()):
		_topo_ids[_edge_pairs_full[i]] = true
	_recompute_instance_budgets()
	_selection_dirty = true
	_edge_rerank_done = false
	# preserve_show=true: keep the user's current density; do not reset the fit.
	_rerank_edges(true)


# --- Feature 2: teleport to a node ------------------------------------------

# Glide the XROrigin so `node_id` sits TELEPORT_FRONT_M in front of the user's gaze,
# then pulse-highlight it. Reuses the locomotion frame (origin translation only; no
# rotation) so it composes with the existing manual transform.
func _teleport_to_node(node_id: int) -> void:
	if _binary_client == null or graph_root == null:
		return
	var cam: XRCamera3D = _find_xr_camera()
	var origin: XROrigin3D = _find_xr_origin()
	if cam == null or origin == null:
		return
	# World position of the target node under the current fit transform.
	var target_world: Vector3 = graph_root.global_transform * _binary_client.node_position(node_id)
	# Desired camera position: TELEPORT_FRONT_M back along the camera's forward from
	# the node, keeping the camera height. Forward is -Z of the camera basis.
	var fwd: Vector3 = -cam.global_transform.basis.z
	fwd.y = 0.0
	if fwd.length() < 0.001:
		fwd = Vector3.FORWARD
	fwd = fwd.normalized()
	var desired_cam: Vector3 = target_world - fwd * TELEPORT_FRONT_M
	# Move the origin by the delta between where the camera should be and where it is
	# (origin-space translation = world delta; camera offset within origin is fixed).
	var delta: Vector3 = desired_cam - cam.global_position
	_teleport_from = origin.transform
	_teleport_to = origin.transform.translated(delta)
	_teleport_t = 0.0
	_teleport_active = true
	# Pulse-highlight the target briefly by borrowing the query-var overlay channel
	# (recolour + rim-flag). Guarded: skip if the node is already a genuine query
	# var so the transient highlight never clobbers a user's mark.
	_teleport_pulse_id = node_id
	_teleport_pulse_t = 0.0
	_teleport_pulse_applied = false
	if _binary_client.has_method("is_query_var") and not _binary_client.is_query_var(node_id):
		if _binary_client.has_method("set_query_var"):
			_binary_client.set_query_var(node_id, 0)
			_teleport_pulse_applied = true


# Advance the teleport glide + pulse each frame; call from _process.
func _update_teleport(delta: float) -> void:
	if _teleport_active:
		var origin: XROrigin3D = _find_xr_origin()
		if origin == null:
			_teleport_active = false
		else:
			_teleport_t += delta / maxf(TELEPORT_GLIDE_SEC, 0.0001)
			var a: float = clampf(_teleport_t, 0.0, 1.0)
			# Smoothstep ease for a comfortable (nausea-safe) glide.
			var e: float = a * a * (3.0 - 2.0 * a)
			origin.transform = _teleport_from.interpolate_with(_teleport_to, e)
			if a >= 1.0:
				_teleport_active = false
	if _teleport_pulse_id >= 0:
		_teleport_pulse_t += delta
		if _teleport_pulse_t >= TELEPORT_PULSE_SEC:
			# Retire the transient highlight (only if we applied it, never a real mark).
			if _teleport_pulse_applied and _binary_client != null and _binary_client.has_method("clear_query_var"):
				_binary_client.clear_query_var(_teleport_pulse_id)
			_teleport_pulse_applied = false
			_teleport_pulse_id = -1


func _find_xr_origin() -> XROrigin3D:
	var cam: XRCamera3D = _find_xr_camera()
	var n: Node = cam
	while n != null:
		if n is XROrigin3D:
			return n as XROrigin3D
		n = n.get_parent()
	return null


# Ray-pick the node under a controller without engaging a grab: evaluate the
# interaction ray at zero pinch (emits node_targeted on a hit, never node_grabbed)
# and capture the hit via _on_node_targeted. Returns the node id or -1 on a miss.
func _pick_node_under_ray(controller: XRController3D) -> int:
	if _interaction == null or _binary_client == null or graph_root == null:
		return -1
	var render_ids: PackedInt32Array = _binary_client.get_render_ids()
	var render_positions: PackedVector3Array = _binary_client.get_render_positions()
	var gxf: Transform3D = graph_root.global_transform
	var world_positions := PackedVector3Array()
	world_positions.resize(render_positions.size())
	for i: int in range(render_positions.size()):
		world_positions[i] = gxf * render_positions[i]
	_radial_pick_capture = -1
	_radial_picking = true
	_interaction.evaluate_ray(
		controller.global_position,
		-controller.global_transform.basis.z,
		0.0,
		render_ids,
		world_positions
	)
	_radial_picking = false
	return _radial_pick_capture


# Intersect a controller aim ray with the radial MenuPanel quad; returns viewport
# UV (0..1, y-down) or (-1,-1) on a miss. Mirrors _ray_hit_hud_uv for the smaller
# square panel.
func _ray_hit_menu_uv(controller: XRController3D, panel: Node3D) -> Vector2:
	var to_local: Transform3D = panel.global_transform.affine_inverse()
	var o: Vector3 = to_local * controller.global_position
	var d: Vector3 = (to_local.basis * (-controller.global_transform.basis.z)).normalized()
	if absf(d.z) < 0.00001:
		return Vector2(-1.0, -1.0)
	var t: float = -o.z / d.z
	if t < 0.0:
		return Vector2(-1.0, -1.0)
	var hit: Vector3 = o + d * t
	var half: float = RADIAL_QUAD * 0.5
	if absf(hit.x) > half or absf(hit.y) > half:
		return Vector2(-1.0, -1.0)
	var hit_world: Vector3 = panel.global_transform * hit
	if controller.global_position.distance_to(hit_world) > RAY_LENGTH:
		return Vector2(-1.0, -1.0)
	var u: float = (hit.x + half) / RADIAL_QUAD
	var v: float = 1.0 - (hit.y + half) / RADIAL_QUAD
	return Vector2(u, v)


# Route a chosen radial action. Action grammar: "qb_mark:<id>", "qb_unmark:<id>",
# "qb_execute", "qb_clear" (query builder); other prefixes belong to future
# Wave-2 providers. The menu closes itself after emitting (radial_menu.gd).
func _on_radial_item_selected(action: String) -> void:
	if QB_DEBUG:
		print("[QB] item_selected action='%s'" % action)
	if action.begins_with("qb_mark:"):
		_mark_query_var(int(action.substr(8)))
	elif action.begins_with("qb_unmark:"):
		_unmark_query_var(int(action.substr(10)))
	elif action == "qb_toggle_edges":
		_query.use_concrete_edges = not _query.use_concrete_edges
		_mark_query_dirty()  # pattern predicates changed → re-count
	elif action == "qb_clear":
		_clear_query()
	elif action == "qb_execute":
		_execute_query()
	elif action.begins_with("expand:"):
		# "expand:<direction>:<edgeType>" — Feature 1 predicate expansion.
		var rest := action.substr(7)
		var sep := rest.find(":")
		if sep > 0:
			_expand_node(_radial_node_id, rest.substr(0, sep), rest.substr(sep + 1))
	elif action.begins_with("teleport:"):
		_teleport_to_node(int(action.substr(9)))


func _mark_query_var(node_id: int) -> void:
	if _binary_client == null:
		return
	var assigned := _query.mark(node_id)
	var has_fn: bool = _binary_client.has_method("set_query_var")
	if QB_DEBUG:
		print("[QB] mark node=%d -> %s (var_count=%d) set_query_var_bound=%s" % [
			node_id, assigned, _query.var_count(), str(has_fn)
		])
	if has_fn:
		_binary_client.set_query_var(node_id, _query.palette_index(node_id))
	_update_proximity_labels()
	_mark_query_dirty()


func _unmark_query_var(node_id: int) -> void:
	if not _query.unmark(node_id):
		return
	if _binary_client != null and _binary_client.has_method("clear_query_var"):
		_binary_client.clear_query_var(node_id)
	_update_proximity_labels()
	_mark_query_dirty()


func _clear_query() -> void:
	_query.clear()
	if _binary_client != null and _binary_client.has_method("clear_query_vars"):
		_binary_client.clear_query_vars()
	if _planes != null:
		_planes.clear()  # discard any result planes
	_update_proximity_labels()
	# Reset the preview and hide the HUD panel (no active query).
	_query_dirty = false
	_query_count = -1
	_query_pending = false
	_query_truncated = false
	_refresh_query_hud()


# --- Live count preview (Phase C) -------------------------------------------
#
# The active pattern is the set of visible-graph edges connecting marked nodes,
# with marked nodes as query variables. On every pattern change we debounce ~400ms
# then POST it countOnly to /api/graph/query/pattern; the returned bindingCount is
# staged onto the HUD query panel. Edges carry their concrete predicate (from the
# edge-type wire) unless the "Edges: any" toggle is on — see
# query_builder.gd::derive_triples.
# Decide, once per frame, which panel each active controller's ray owns (nearer
# panel wins), so the HUD and radial pointer handlers never both consume the same
# trigger. Each controller is assigned to at most one panel; a full miss leaves it
# unassigned. Right controller is considered first for a stable tie.
func _arbitrate_pointers() -> void:
	_hud_owner = null
	_radial_owner = null
	var hud_panel: Node3D = null
	if hud != null:
		hud_panel = hud.get_node_or_null("HudPanel") as Node3D
	var radial_panel: Node3D = null
	if _radial != null and _radial.visible:
		radial_panel = _radial.get_node_or_null("MenuPanel") as Node3D
	if hud_panel == null and radial_panel == null:
		return
	for controller: XRController3D in [right_controller, left_controller]:
		if controller == null or not controller.get_is_active():
			continue
		var hud_d := _panel_ray_distance(controller, hud_panel, HUD_QUAD_W, HUD_QUAD_H)
		var rad_d := _panel_ray_distance(controller, radial_panel, RADIAL_QUAD, RADIAL_QUAD)
		if hud_d == INF and rad_d == INF:
			continue
		# The radial is a MODAL the user explicitly summoned at a node, so any ray
		# that hits it wins outright — even if the HUD happens to sit nearer along
		# that ray. Without this, a node summoned roughly toward the parked HUD lets
		# the HUD claim the controller and STARVES the radial of all wand input
		# (the "clicks never land / first mark never registers" bug). When the ray
		# misses the radial, fall back to nearer-panel between HUD and (closed)
		# radial. Assign _hud_owner explicitly in the else so both panels can't
		# consume one trigger.
		if rad_d != INF:
			if _radial_owner == null:
				_radial_owner = controller
		elif hud_d != INF:
			if _hud_owner == null:
				_hud_owner = controller


# World distance from `controller` to where its aim ray hits the quad `panel`
# (full sizes quad_w/quad_h), or INF on a miss / beyond reach. Shared by the
# pointer arbitration; the per-panel UV helpers keep their own hit maths.
func _panel_ray_distance(controller: XRController3D, panel: Node3D, quad_w: float, quad_h: float) -> float:
	if controller == null or panel == null:
		return INF
	var to_local: Transform3D = panel.global_transform.affine_inverse()
	var o: Vector3 = to_local * controller.global_position
	var d: Vector3 = (to_local.basis * (-controller.global_transform.basis.z)).normalized()
	if absf(d.z) < 0.00001:
		return INF
	var t: float = -o.z / d.z
	if t < 0.0:
		return INF
	var hit: Vector3 = o + d * t
	if absf(hit.x) > quad_w * 0.5 or absf(hit.y) > quad_h * 0.5:
		return INF
	var hit_world: Vector3 = panel.global_transform * hit
	var dist: float = controller.global_position.distance_to(hit_world)
	if dist > RAY_LENGTH:
		return INF
	return dist


func _update_query_count(delta: float) -> void:
	if not _query_dirty:
		return
	_query_debounce += delta
	if _query_debounce < QUERY_DEBOUNCE_SEC:
		return
	_query_dirty = false
	_send_query_count()


# Mark the pattern changed: invalidate the shown count, bump the revision (so any
# in-flight count is now stale), and restart the debounce.
func _mark_query_dirty() -> void:
	_query_dirty = true
	_query_debounce = 0.0
	_query_count = -1
	_query_pending = true
	_query_revision += 1
	_refresh_query_hud()


func _send_query_count() -> void:
	if _query_http == null or _binary_client == null or not _query.is_active():
		return
	var payload: Dictionary = _query.build_pattern_payload(
		_binary_client.get_edges(), _binary_client.get_edge_types(), true, QUERY_PREVIEW_LIMIT
	)
	var triples: Array = payload["triples"]
	if triples.is_empty():
		# Variables marked but none connected by a visible edge yet: 0 matches, no
		# round-trip needed.
		_query_count = 0
		_query_truncated = false
		_query_pending = false
		_refresh_query_hud()
		return
	var url := "%s/api/graph/query/pattern" % _http_base()
	var headers := _auth_headers(url, "POST")
	_query_http.cancel_request()  # supersede any in-flight preview
	var err := _query_http.request(url, headers, HTTPClient.METHOD_POST, JSON.stringify(payload))
	if err != OK:
		push_warning("GraphScene: query count POST failed to start (%d)" % err)
		return
	_query_sent_revision = _query_revision
	_query_pending = true
	_refresh_query_hud()


func _on_query_count_completed(result: int, response_code: int, _headers: PackedStringArray, body: PackedByteArray) -> void:
	# Drop a stale completion: the pattern changed after this request was sent, so a
	# newer count is pending/in-flight and owns the display.
	if _query_sent_revision != _query_revision:
		return
	_query_pending = false
	var ok := result == HTTPRequest.RESULT_SUCCESS and response_code >= 200 and response_code < 300
	if ok:
		var parsed: Variant = JSON.parse_string(body.get_string_from_utf8())
		if typeof(parsed) == TYPE_DICTIONARY:
			_query_count = int(parsed.get("bindingCount", 0))
			_query_truncated = bool(parsed.get("truncated", false))
	else:
		push_warning("GraphScene: query count failed (result=%d code=%d)" % [result, response_code])
	_refresh_query_hud()


# Push the current summary + count to the HUD query panel (or hide it when no
# query is active). Recomputes triples so the edge count in the summary is fresh.
func _refresh_query_hud() -> void:
	if hud == null or not hud.has_method("set_query_preview"):
		return
	if not _query.is_active():
		if hud.has_method("hide_query_preview"):
			hud.hide_query_preview()
		if hud.has_method("set_marked_vars"):
			hud.set_marked_vars([])
		return
	if _binary_client != null:
		_query.derive_triples(_binary_client.get_edges(), _binary_client.get_edge_types())
	hud.set_query_preview(_query.pattern_summary(), _query_count, _query_pending, _query_truncated)
	# Populate the marked-variable chip list (var name without the leading '?', +label).
	if hud.has_method("set_marked_vars"):
		var rows: Array = []
		for mid: int in _query.marked_ids():
			var vname: String = _query.var_name(mid)
			rows.append({
				"var": vname.substr(1) if vname.begins_with("?") else vname,
				"label": _binary_client.label_of(mid) if _binary_client != null else "",
			})
		hud.set_marked_vars(rows)


func _execute_query() -> void:
	if _exec_http == null or _binary_client == null or not _query.is_active():
		return
	if _exec_pending:
		return
	var payload: Dictionary = _query.build_pattern_payload(
		_binary_client.get_edges(), _binary_client.get_edge_types(), false, PLANE_LIMIT
	)
	if (payload["triples"] as Array).is_empty():
		return
	var url := "%s/api/graph/query/pattern" % _http_base()
	var headers := _auth_headers(url, "POST")
	var err := _exec_http.request(url, headers, HTTPClient.METHOD_POST, JSON.stringify(payload))
	if err != OK:
		push_warning("GraphScene: query execute POST failed to start (%d)" % err)
		return
	_exec_pending = true


func _on_execute_completed(result: int, response_code: int, _headers: PackedStringArray, body: PackedByteArray) -> void:
	_exec_pending = false
	var ok := result == HTTPRequest.RESULT_SUCCESS and response_code >= 200 and response_code < 300
	if not ok:
		push_warning("GraphScene: query execute failed (result=%d code=%d)" % [result, response_code])
		return
	var parsed: Variant = JSON.parse_string(body.get_string_from_utf8())
	if typeof(parsed) != TYPE_DICTIONARY:
		return
	var bindings: Array = (parsed as Dictionary).get("bindings", [])
	if _planes == null:
		return
	# Server-space gap so layers land ~PLANE_GAP_M apart after the fit scale.
	var gap_server: float = PLANE_GAP_M / maxf(_graph_scale, 0.0001)
	var node_comp: float = NODE_WORLD_RADIUS * _node_size_factor / (NODE_MESH_RADIUS * maxf(_graph_scale, 0.0001))
	var edge_comp: float = EDGE_WORLD_RADIUS / (EDGE_MESH_RADIUS * maxf(_graph_scale, 0.0001))
	# size_lo/size_hi match the main node buffer build (graph_scene.gd:_update_multimesh).
	var spawned: int = _planes.build(
		bindings, _query.triples, gap_server, node_comp,
		0.7, 1.9, edge_comp,
		Callable(self, "_plane_binding_label")
	)
	print("GraphScene: query executed → %d planes" % spawned)


# Header label for a result plane: the first variable's node label.
func _plane_binding_label(binding: Dictionary) -> String:
	for k in binding.keys():
		var id := int(binding[k])
		var lbl: String = _binary_client.label_of(id) if _binary_client != null else ""
		return lbl if lbl != "" else str(id)
	return ""
