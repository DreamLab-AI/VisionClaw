extends RefCounted

## Single pose owner for work-layer agent embodiments (ADR-140 work layer: agents
## keyed by their `0x23` wire id). Every embodied agent has exactly ONE writer of
## its position and alpha — this class. The proxemics arc never touches these
## agents; it only serves conversation-layer avatars (keyed by did:nostr).
##
## Inputs are the Rust registry's view of an agent (status, target node, target
## world position) plus the head position; the output is a pose per agent that
## GraphScene applies to the avatar. Pure GDScript, no scene references, so the
## whole state machine is unit-testable under GUT without a headset or gdext.
##
## Presentation phases (separate from the activity states the avatar shows):
##   MATERIALISE → TRAVEL → ARRIVE → WORK → (TRAVEL to a new target …)
##   WORK → COMPLETE → PARK → REST → (brighten + TRAVEL when re-tasked)
## Timings, speeds and comfort rules follow the embodiment plan
## (~/.claude/plans/agent-embodiment-redesign.md §2.3–2.5).

const PH_MATERIALISE := 0
const PH_TRAVEL := 1
const PH_ARRIVE := 2
const PH_WORK := 3
const PH_COMPLETE := 4
const PH_PARK := 5
const PH_REST := 6

# Registry status codes (render_store.rs AGENT_*): 0 idle, 1 working, 2 blocked, 3 done.
const ST_IDLE := 0
const ST_WORKING := 1
const ST_BLOCKED := 2
const ST_DONE := 3

const MATERIALISE_SEC := 0.8
const ARRIVE_SEC := 0.7
const COMPLETE_SEC := 1.2
const TRAVEL_SPEED := 0.32        # m/s, medium — legible but not a dart
const TRAVEL_MIN_SEC := 1.0
const TRAVEL_MAX_SEC := 6.0
const PARK_SPEED := 0.10          # m/s, the slow drift to the rim
const PARK_MIN_SEC := 2.0
const PARK_MAX_SEC := 12.0
const SLOT_DISTANCE := 0.32       # work slot stands this far off the node …
const SLOT_LIFT := 0.10           # … and this much above it
const HOVER_AMP := 0.01           # 1 cm hover while working
const HEAD_EXCLUSION := 1.0       # routes and slots keep this far from the head
const ALPHA_PARKED := 0.3
const FADE_SEC := 2.0
const BRIGHTEN_SEC := 0.8
const RM_FADE_SEC := 0.4          # reduced motion: fade-out / relocate / fade-in
const MAX_LIFT := 0.15

## When true, travel becomes fade → relocate → fade and hover is disabled
## (comfort node's reduced_motion; defaults ON in the avatar).
var reduced_motion: bool = false

var _head: Vector3 = Vector3.ZERO
var _centre: Vector3 = Vector3.ZERO
var _agents: Dictionary = {}   # id -> record Dictionary
var _events: Array = []
var _next_idx: int = 0


## Register an agent. It materialises in place at `spawn_pos` after `delay`
## seconds (the demo staggers entrances) and parks back to `rim_slot` when done.
func add_agent(id: String, spawn_pos: Vector3, delay: float, rim_slot: Vector3) -> void:
	if _agents.has(id):
		return
	var seed_f: float = fposmod(float(hash(id)) * 0.000001, 1.0)
	_agents[id] = {
		"phase": PH_MATERIALISE, "t": 0.0, "dur": MATERIALISE_SEC, "delay": maxf(delay, 0.0),
		"pos": spawn_pos, "from": spawn_pos, "to": spawn_pos,
		"alpha": 0.0, "alpha_to": 0.0, "alpha_rate": 0.0, "materialised": false,
		"status": ST_IDLE, "has_target": false, "target_id": -1, "target_world": spawn_pos,
		"slot": spawn_pos, "rim": rim_slot, "seed": seed_f, "idx": _next_idx,
		"hover_t": 0.0, "lift": 0.0, "lateral": Vector3.ZERO, "rm_jumped": false,
		"working_target": -1,
	}
	_next_idx += 1


func remove_agent(id: String) -> void:
	_agents.erase(id)


func has_agent(id: String) -> bool:
	return _agents.has(id)


func ids() -> Array:
	return _agents.keys()


func set_head(p: Vector3) -> void:
	_head = p


func set_graph_centre(c: Vector3) -> void:
	_centre = c


func set_rim_slot(id: String, p: Vector3) -> void:
	if _agents.has(id):
		_agents[id]["rim"] = p


## Feed the registry's current view of an agent. `target_world` is only read
## when `has_target` is true.
func update_registry(id: String, status: int, has_target: bool, target_id: int, target_world: Vector3) -> void:
	if not _agents.has(id):
		return
	var a: Dictionary = _agents[id]
	a["status"] = status
	a["has_target"] = has_target
	a["target_id"] = target_id if has_target else -1
	if has_target:
		a["target_world"] = target_world


## Advance every agent by `delta` seconds.
func tick(delta: float) -> void:
	for id: String in _agents:
		_tick_agent(id, _agents[id], delta)


## The pose GraphScene applies this frame.
func pose(id: String) -> Dictionary:
	var a: Dictionary = _agents.get(id, {})
	if a.is_empty():
		return {}
	var phase: int = a["phase"]
	var engaged: bool = phase == PH_TRAVEL or phase == PH_ARRIVE or phase == PH_WORK
	var aim: Vector3 = Vector3.ZERO
	if engaged and a["has_target"]:
		aim = (a["target_world"] as Vector3) - (a["pos"] as Vector3)
	return {
		"pos": a["pos"], "alpha": a["alpha"], "phase": phase, "aim": aim,
		"has_target": a["has_target"], "target_id": a["target_id"], "target_world": a["target_world"],
		"working": phase == PH_ARRIVE or phase == PH_WORK,
		"done": phase == PH_COMPLETE or phase == PH_PARK or phase == PH_REST,
	}


## Drain the transition events since the last call — GraphScene turns them
## into effects (arrive ring, completion burst). Each: {type, id, target_id, pos}.
func take_events() -> Array:
	var out: Array = _events
	_events = []
	return out


static func ease_quintic(u: float) -> float:
	var x: float = clampf(u, 0.0, 1.0)
	return x * x * x * (x * (x * 6.0 - 15.0) + 10.0)


# --- internals ---------------------------------------------------------------

func _tick_agent(id: String, a: Dictionary, delta: float) -> void:
	a["t"] = float(a["t"]) + delta
	a["hover_t"] = float(a["hover_t"]) + delta
	match int(a["phase"]):
		PH_MATERIALISE:
			var t: float = float(a["t"]) - float(a["delay"])
			if t < 0.0:
				a["alpha"] = 0.0
			else:
				if not bool(a["materialised"]):
					a["materialised"] = true
					_alpha_goal(a, 1.0, MATERIALISE_SEC)
				if t >= MATERIALISE_SEC + 0.5:
					a["alpha"] = 1.0
					_after_settled(id, a)
		PH_TRAVEL:
			_advance_move(id, a, delta, false)
		PH_ARRIVE:
			a["pos"] = _slot_with_hover(a)
			if float(a["t"]) >= ARRIVE_SEC:
				_enter(a, PH_WORK)
			_check_work_transitions(id, a)
		PH_WORK:
			a["pos"] = _slot_with_hover(a)
			_check_work_transitions(id, a)
		PH_COMPLETE:
			a["pos"] = _slot_with_hover(a)
			if float(a["t"]) >= COMPLETE_SEC:
				_begin_park(id, a)
		PH_PARK:
			_advance_move(id, a, delta, true)
			_check_rest_transitions(id, a)
		PH_REST:
			_check_rest_transitions(id, a)
	_step_alpha(a, delta)


func _enter(a: Dictionary, phase: int) -> void:
	a["phase"] = phase
	a["t"] = 0.0


func _after_settled(id: String, a: Dictionary) -> void:
	if _is_live(a) and a["has_target"]:
		_begin_travel(id, a)
	else:
		_enter(a, PH_REST)
		_alpha_goal(a, ALPHA_PARKED, FADE_SEC)


func _is_live(a: Dictionary) -> bool:
	var s: int = a["status"]
	return s == ST_WORKING or s == ST_BLOCKED


func _check_work_transitions(id: String, a: Dictionary) -> void:
	if int(a["status"]) == ST_DONE:
		_enter(a, PH_COMPLETE)
		_emit(id, a, "complete")
		return
	if not a["has_target"] or int(a["status"]) == ST_IDLE:
		# Evidence expired or the agent went idle without reporting completion.
		_begin_park(id, a)
		return
	if int(a["target_id"]) != int(a["working_target"]):
		_begin_travel(id, a)


func _check_rest_transitions(id: String, a: Dictionary) -> void:
	if _is_live(a) and a["has_target"]:
		_alpha_goal(a, 1.0, BRIGHTEN_SEC)
		_begin_travel(id, a)


func _begin_travel(id: String, a: Dictionary) -> void:
	a["working_target"] = a["target_id"]
	a["slot"] = _work_slot(a)
	_setup_move(a, a["slot"], TRAVEL_SPEED, TRAVEL_MIN_SEC, TRAVEL_MAX_SEC)
	_alpha_goal(a, 1.0, BRIGHTEN_SEC)
	_enter(a, PH_TRAVEL)
	_emit(id, a, "depart")


func _begin_park(id: String, a: Dictionary) -> void:
	a["working_target"] = -1
	var rim: Vector3 = _away_from_head(a["rim"])
	_setup_move(a, rim, PARK_SPEED, PARK_MIN_SEC, PARK_MAX_SEC)
	_alpha_goal(a, ALPHA_PARKED, FADE_SEC)
	_enter(a, PH_PARK)
	_emit(id, a, "park")


func _setup_move(a: Dictionary, to: Vector3, speed: float, min_sec: float, max_sec: float) -> void:
	var from: Vector3 = a["pos"]
	a["from"] = from
	a["to"] = to
	a["rm_jumped"] = false
	var dist: float = from.distance_to(to)
	if reduced_motion:
		a["dur"] = RM_FADE_SEC * 2.0
		a["lift"] = 0.0
		a["lateral"] = Vector3.ZERO
		return
	a["dur"] = clampf(dist / speed, min_sec, max_sec)
	a["lift"] = minf(MAX_LIFT, dist * 0.2)
	a["lateral"] = _head_avoidance(from, to)


func _advance_move(id: String, a: Dictionary, _delta: float, parking: bool) -> void:
	var t: float = a["t"]
	var dur: float = maxf(float(a["dur"]), 0.001)
	var from: Vector3 = a["from"]
	var to: Vector3 = a["to"]
	if reduced_motion:
		# Comfort: no continuous translation. Fade out, relocate, fade in.
		if t < RM_FADE_SEC:
			a["alpha_to"] = 0.0
			a["alpha_rate"] = 1.0 / RM_FADE_SEC
			a["pos"] = from
		else:
			if not bool(a["rm_jumped"]):
				a["rm_jumped"] = true
				a["pos"] = to
				_alpha_goal(a, ALPHA_PARKED if parking else 1.0, RM_FADE_SEC)
		if t >= dur:
			a["pos"] = to
			_finish_move(id, a, parking)
		return
	var u: float = clampf(t / dur, 0.0, 1.0)
	var e: float = ease_quintic(u)
	var bulge: float = sin(u * PI)
	a["pos"] = from.lerp(to, e) + Vector3.UP * float(a["lift"]) * bulge + (a["lateral"] as Vector3) * bulge
	if u >= 1.0:
		a["pos"] = to
		_finish_move(id, a, parking)


func _finish_move(id: String, a: Dictionary, parking: bool) -> void:
	if parking:
		_enter(a, PH_REST)
	else:
		_enter(a, PH_ARRIVE)
		_emit(id, a, "arrive")


func _slot_with_hover(a: Dictionary) -> Vector3:
	var p: Vector3 = a["slot"]
	if reduced_motion:
		return p
	var period: float = 5.0 + 2.0 * float(a["seed"])
	return p + Vector3.UP * HOVER_AMP * sin(TAU * float(a["hover_t"]) / period + TAU * float(a["seed"]))


# Work slot: off the node toward the graph exterior (so the body never sits
# inside the cloud), lifted slightly, with a small deterministic per-agent swing
# so two agents on neighbouring nodes do not overlap. Frozen for the whole stay.
func _work_slot(a: Dictionary) -> Vector3:
	var target: Vector3 = a["target_world"]
	var outward: Vector3 = target - _centre
	if outward.length_squared() < 0.0025:
		outward = target - _head
		outward.y = 0.0
	if outward.length_squared() < 0.0001:
		outward = Vector3.RIGHT
	outward = outward.normalized()
	var idx: int = a["idx"]
	var swing: float = (0.25 + 0.15 * float(idx / 2)) * (1.0 if idx % 2 == 0 else -1.0)
	outward = outward.rotated(Vector3.UP, swing)
	var slot: Vector3 = target + outward * SLOT_DISTANCE + Vector3.UP * SLOT_LIFT
	return _away_from_head(slot)


func _away_from_head(p: Vector3) -> Vector3:
	var d: Vector3 = p - _head
	var len: float = d.length()
	if len >= HEAD_EXCLUSION:
		return p
	if len < 0.001:
		d = Vector3.FORWARD
		len = 1.0
	return _head + d / len * HEAD_EXCLUSION


# Lateral bulge that keeps a straight route outside the head-exclusion sphere.
func _head_avoidance(from: Vector3, to: Vector3) -> Vector3:
	var seg: Vector3 = to - from
	var len2: float = seg.length_squared()
	if len2 < 0.0001:
		return Vector3.ZERO
	var u: float = clampf((_head - from).dot(seg) / len2, 0.0, 1.0)
	var closest: Vector3 = from + seg * u
	var away: Vector3 = closest - _head
	var dist: float = away.length()
	if dist >= HEAD_EXCLUSION:
		return Vector3.ZERO
	if dist < 0.001:
		away = seg.cross(Vector3.UP)
		if away.length_squared() < 0.0001:
			away = Vector3.RIGHT
	return away.normalized() * (HEAD_EXCLUSION - dist + 0.1)


func _alpha_goal(a: Dictionary, goal: float, seconds: float) -> void:
	a["alpha_to"] = goal
	a["alpha_rate"] = absf(goal - float(a["alpha"])) / maxf(seconds, 0.01)


func _step_alpha(a: Dictionary, delta: float) -> void:
	var cur: float = a["alpha"]
	var goal: float = a["alpha_to"]
	if is_equal_approx(cur, goal):
		return
	a["alpha"] = move_toward(cur, goal, float(a["alpha_rate"]) * delta)


func _emit(id: String, a: Dictionary, type: String) -> void:
	_events.append({"type": type, "id": id, "target_id": a["target_id"], "pos": a["target_world"] if a["has_target"] else a["pos"]})
