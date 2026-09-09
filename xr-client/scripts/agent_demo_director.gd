extends RefCounted

## Agent demo director — ALL demo-mode code for the XR client lives here.
##
## The demo does not touch avatars, beams or the roster directly. It is a
## producer of synthetic `0x23 AGENT_ACTION` frames and JSON-channel state
## updates that enter the Rust registry through the same `ingest()` /
## `apply_agent_state()` doors a live swarm uses. Everything downstream —
## registry, status, targets, beams, Swarm roster, embodiment choreography — is
## the production path with no demo branch.
##
## The synthetic agents play as real agents: nothing they emit or display says
## "demo" — same frame layout, same registry, same names a swarm would use. The
## only visible sign of the demo is the Start/Stop Agent Demo button. Provenance
## lives here in code alone: wire ids are `0x80000000 | 0xD001..0xD006`
## (reserved range 0xD001–0xD0FF) and Stop retires exactly those ids from the
## registry (`retire_agents`) so no demo state can outlive the demo.
##
## Loop (multi-minute, per agent, independently seeded so nothing moves in
## lockstep): stagger in → work a real node 9–14 s (keep-alive actions every 5 s
## under the 30 s evidence TTL) → 60 % of the time follow a real edge to a
## neighbour and work 7–11 s → report done → rest 18–26 s → new target.

const AGENT_NODE_FLAG: int = 0x80000000
const DEMO_WIRE_BASE: int = 0xD001
const DEMO_WIRE_LAST: int = 0xD0FF
const MSG_AGENT_ACTION: int = 0x23
const EVENT_HEADER_BYTES: int = 15

const KEEPALIVE_SEC := 5.0
const WORK_MIN_SEC := 9.0
const WORK_MAX_SEC := 14.0
const FOLLOW_MIN_SEC := 7.0
const FOLLOW_MAX_SEC := 11.0
const REST_MIN_SEC := 18.0
const REST_MAX_SEC := 26.0
const FOLLOW_CHANCE := 0.6
const MIN_START_SEPARATION_M := 0.4
const CANDIDATE_POOL := 80
const STAGGER_SEC: Array[float] = [0.0, 1.3, 2.9, 4.6, 6.8, 9.1]

# Role, label, the AgentActionType it emits (0 Query, 1 Update, 2 Create,
# 3 Link, 4 Delete, 5 Transform — binary_protocol.rs) and its caption verb.
const ROLES: Array[Dictionary] = [
	{"suffix": 0xD001, "label": "Architect", "action": 2, "verb": "Mapping"},
	{"suffix": 0xD002, "label": "Analyst",   "action": 0, "verb": "Analysing"},
	{"suffix": 0xD003, "label": "Coder",     "action": 1, "verb": "Refactoring"},
	{"suffix": 0xD004, "label": "Reviewer",  "action": 3, "verb": "Reviewing"},
	{"suffix": 0xD005, "label": "Tester",    "action": 0, "verb": "Testing"},
	{"suffix": 0xD006, "label": "Optimizer", "action": 5, "verb": "Optimising"},
]

const PH_WAIT := 0
const PH_WORK := 1
const PH_FOLLOW := 2
const PH_REST := 3

var _client: Object = null        # BinaryProtocolClient (duck-typed; tests pass a fake)
var _world_of: Callable           # server-space Vector3 -> world Vector3
var _rng := RandomNumberGenerator.new()
var _running: bool = false
var _agents: Array = []           # per-role schedule records
var _adjacency: Dictionary = {}   # node id -> PackedInt32Array neighbours
var _candidates: PackedInt32Array = PackedInt32Array()
var _last_ts: int = 0
var _frames_sent: int = 0


func is_running() -> bool:
	return _running


func frames_sent() -> int:
	return _frames_sent


## Wire ids of the synthetic agents (with the agent flag) — used only to retire
## them on Stop and to supply names the registry has no label for.
static func demo_wire_ids() -> PackedInt32Array:
	var out := PackedInt32Array()
	for r: Dictionary in ROLES:
		out.append(int(AGENT_NODE_FLAG | int(r["suffix"])))
	return out


## Whether a masked/unmasked wire id is in the reserved demo range.
static func is_demo_id(wire_id: int) -> bool:
	var masked: int = wire_id & 0x03FFFFFF
	return masked >= DEMO_WIRE_BASE and masked <= DEMO_WIRE_LAST


static func display_name_for(wire_id: int) -> String:
	var masked: int = wire_id & 0x03FFFFFF
	for r: Dictionary in ROLES:
		if int(r["suffix"]) == masked:
			return String(r["label"])
	return ""


## `client` must expose ingest(PackedByteArray), apply_agent_state(id, status,
## task), retire_agents(PackedInt32Array), get_edges(), top_labels(max),
## get_render_ids(), node_position(id), label_of(id) and server_clock_ms().
## `world_of` maps a server-space position to world metres (GraphRoot.to_global).
func start(client: Object, world_of: Callable, seed: int = 0) -> bool:
	if _running:
		return true
	if client == null or not client.has_method("ingest"):
		return false
	_client = client
	_world_of = world_of
	_rng.seed = seed if seed != 0 else int(Time.get_ticks_usec())
	_build_adjacency()
	_candidates = _pick_candidates()
	if _candidates.is_empty():
		return false
	_agents.clear()
	var starts: PackedInt32Array = _separated_starts(ROLES.size())
	for i: int in range(ROLES.size()):
		var role: Dictionary = ROLES[i]
		_agents.append({
			"wire": int(AGENT_NODE_FLAG | int(role["suffix"])),
			"role": role,
			"phase": PH_WAIT,
			"t": 0.0,
			"dur": STAGGER_SEC[i % STAGGER_SEC.size()],
			"target": starts[i % starts.size()],
			"keepalive": 0.0,
		})
	_running = true
	return true


## Stop the demo and retire its registry records. Nothing else needs cleaning:
## avatars and effects follow the registry.
func stop() -> void:
	if not _running:
		return
	_running = false
	if _client != null and _client.has_method("retire_agents"):
		_client.retire_agents(demo_wire_ids())
	_agents.clear()


func tick(delta: float) -> void:
	if not _running:
		return
	for a: Dictionary in _agents:
		a["t"] = float(a["t"]) + delta
		match int(a["phase"]):
			PH_WAIT:
				if float(a["t"]) >= float(a["dur"]):
					_begin_work(a, PH_WORK, WORK_MIN_SEC, WORK_MAX_SEC)
			PH_WORK, PH_FOLLOW:
				a["keepalive"] = float(a["keepalive"]) + delta
				if float(a["keepalive"]) >= KEEPALIVE_SEC:
					a["keepalive"] = 0.0
					_send_action(a)
				if float(a["t"]) >= float(a["dur"]):
					var neighbour: int = _neighbour_of(int(a["target"]))
					if int(a["phase"]) == PH_WORK and neighbour >= 0 and _rng.randf() < FOLLOW_CHANCE:
						a["target"] = neighbour
						_begin_work(a, PH_FOLLOW, FOLLOW_MIN_SEC, FOLLOW_MAX_SEC)
					else:
						_complete(a)
			PH_REST:
				if float(a["t"]) >= float(a["dur"]):
					a["target"] = _next_target(int(a["target"]))
					_begin_work(a, PH_WORK, WORK_MIN_SEC, WORK_MAX_SEC)


# --- schedule ----------------------------------------------------------------

func _begin_work(a: Dictionary, phase: int, min_sec: float, max_sec: float) -> void:
	a["phase"] = phase
	a["t"] = 0.0
	a["dur"] = _rng.randf_range(min_sec, max_sec)
	a["keepalive"] = 0.0
	_send_action(a)


func _complete(a: Dictionary) -> void:
	a["phase"] = PH_REST
	a["t"] = 0.0
	a["dur"] = _rng.randf_range(REST_MIN_SEC, REST_MAX_SEC)
	if _client.has_method("apply_agent_state"):
		_client.apply_agent_state(int(a["wire"]), "done", "Done: %s" % _label(int(a["target"])))


func _send_action(a: Dictionary) -> void:
	var role: Dictionary = a["role"]
	var caption: String = "%s: %s" % [role["verb"], _label(int(a["target"]))]
	var payload: PackedByteArray = JSON.stringify({"intent": caption}).to_utf8_buffer()
	var frame: PackedByteArray = encode_action_frame([{
		"source": int(a["wire"]), "target": int(a["target"]), "action": int(role["action"]),
		"ts": _next_ts(), "duration_ms": 2000, "payload": payload,
	}])
	_client.ingest(frame)
	_frames_sent += 1


func _label(node_id: int) -> String:
	if _client != null and _client.has_method("label_of"):
		var s: String = String(_client.label_of(node_id))
		if s.length() > 0:
			return s
	return "node %d" % node_id


# Timestamps continue the server clock the registry already anchors on, so demo
# evidence orders/expires coherently next to live evidence; strictly increasing.
func _next_ts() -> int:
	var base: int = -1
	if _client != null and _client.has_method("server_clock_ms"):
		base = int(_client.server_clock_ms())
	if base < 0:
		base = Time.get_ticks_msec()
	var ts: int = maxi(base, _last_ts + 1)
	_last_ts = ts
	return ts & 0xFFFFFFFF


# --- graph queries ------------------------------------------------------------

func _build_adjacency() -> void:
	_adjacency.clear()
	if not _client.has_method("get_edges"):
		return
	var edges: PackedInt32Array = _client.get_edges()
	var i: int = 0
	while i + 1 < edges.size():
		var s: int = edges[i]
		var t: int = edges[i + 1]
		_adj_add(s, t)
		_adj_add(t, s)
		i += 2


func _adj_add(a: int, b: int) -> void:
	if not _adjacency.has(a):
		_adjacency[a] = PackedInt32Array()
	(_adjacency[a] as PackedInt32Array).append(b)


# High-centrality labelled nodes first (they sit toward the graph's mass
# centre), falling back to whatever is drawn.
func _pick_candidates() -> PackedInt32Array:
	var pool := PackedInt32Array()
	if _client.has_method("top_labels"):
		pool = _client.top_labels(CANDIDATE_POOL)
	if pool.is_empty() and _client.has_method("get_render_ids"):
		var drawn: PackedInt32Array = _client.get_render_ids()
		for i: int in range(mini(drawn.size(), CANDIDATE_POOL)):
			pool.append(drawn[i])
	return pool


func _separated_starts(n: int) -> PackedInt32Array:
	var out := PackedInt32Array()
	var placed: Array[Vector3] = []
	var order: Array = Array(_candidates)
	# Fisher–Yates on the director's own RNG so a seeded run is reproducible.
	for i: int in range(order.size() - 1, 0, -1):
		var j: int = _rng.randi_range(0, i)
		var tmp: Variant = order[i]
		order[i] = order[j]
		order[j] = tmp
	for id: Variant in order:
		if out.size() >= n:
			break
		var p: Vector3 = _world(int(id))
		var ok: bool = true
		for q: Vector3 in placed:
			if p.distance_to(q) < MIN_START_SEPARATION_M:
				ok = false
				break
		if ok:
			out.append(int(id))
			placed.append(p)
	# Dense graph: fill remaining slots without the separation rule.
	var k: int = 0
	while out.size() < n and k < order.size():
		if not out.has(int(order[k])):
			out.append(int(order[k]))
		k += 1
	if out.is_empty():
		out.append(_candidates[0])
	return out


func _world(node_id: int) -> Vector3:
	if _client.has_method("node_position") and _world_of.is_valid():
		return _world_of.call(_client.node_position(node_id))
	return Vector3.ZERO


func _neighbour_of(node_id: int) -> int:
	if not _adjacency.has(node_id):
		return -1
	var nb: PackedInt32Array = _adjacency[node_id]
	if nb.is_empty():
		return -1
	# Prefer a neighbour that is itself a candidate (visible, labelled).
	var tries: int = mini(8, nb.size())
	for _i: int in range(tries):
		var pick: int = nb[_rng.randi_range(0, nb.size() - 1)]
		if _candidates.has(pick) and pick != node_id:
			return pick
	var fallback: int = nb[_rng.randi_range(0, nb.size() - 1)]
	return fallback if fallback != node_id else -1


func _next_target(prev: int) -> int:
	var nb: int = _neighbour_of(prev)
	if nb >= 0 and _rng.randf() < 0.5:
		return nb
	return _candidates[_rng.randi_range(0, _candidates.size() - 1)]


# --- wire encoding --------------------------------------------------------------

## Build a `0x23` batch frame exactly as the server's `encode_agent_actions` does:
## `[0x23][u16 count]( [u16 ev_len][source u32|target u32|action u8|ts u32|
## dur u16|payload…] )*`, all little-endian. Each event: {source, target,
## action, ts, duration_ms, payload: PackedByteArray}.
static func encode_action_frame(events: Array) -> PackedByteArray:
	var out := PackedByteArray()
	out.resize(3)
	out.encode_u8(0, MSG_AGENT_ACTION)
	out.encode_u16(1, events.size())
	for ev: Dictionary in events:
		var payload: PackedByteArray = ev.get("payload", PackedByteArray())
		var body := PackedByteArray()
		body.resize(EVENT_HEADER_BYTES)
		body.encode_u32(0, int(ev["source"]) & 0xFFFFFFFF)
		body.encode_u32(4, int(ev["target"]) & 0xFFFFFFFF)
		body.encode_u8(8, int(ev.get("action", 0)) & 0xFF)
		body.encode_u32(9, int(ev.get("ts", 0)) & 0xFFFFFFFF)
		body.encode_u16(13, int(ev.get("duration_ms", 0)) & 0xFFFF)
		body.append_array(payload)
		var len_bytes := PackedByteArray()
		len_bytes.resize(2)
		len_bytes.encode_u16(0, body.size())
		out.append_array(len_bytes)
		out.append_array(body)
	return out
