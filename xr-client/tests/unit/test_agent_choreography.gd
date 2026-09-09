extends "res://addons/gut/test.gd"

# Single-pose-owner choreography for work-layer embodiments
# (scripts/agent_choreography.gd). Pure GDScript: no scene, no gdext. These pin
# the behaviours the headset review demanded — agents materialise in place, only
# the choreography moves them, routes avoid the head, completion is explicit,
# and a parked agent stays put (and faded) until it is re-tasked.

const Choreo := preload("res://scripts/agent_choreography.gd")

const RIM := Vector3(1.5, 1.3, -1.6)
const TARGET := Vector3(0.0, 1.3, -1.6)


func _make(reduced: bool = false) -> RefCounted:
	var c: RefCounted = Choreo.new()
	c.reduced_motion = reduced
	c.set_head(Vector3(0.0, 1.6, 0.0))
	c.set_graph_centre(Vector3(0.0, 1.3, -1.6))
	return c


func _run(c: RefCounted, seconds: float, step: float = 1.0 / 90.0) -> void:
	var t := 0.0
	while t < seconds:
		c.tick(step)
		t += step


func test_materialises_in_place_then_travels_to_a_work_slot_near_the_target() -> void:
	var c: RefCounted = _make()
	c.add_agent("a", RIM, 0.0, RIM)
	c.update_registry("a", Choreo.ST_WORKING, true, 42, TARGET)
	# Materialise: alpha rises from 0 while the position does not move.
	_run(c, 0.4)
	var p: Dictionary = c.pose("a")
	assert_eq(p["phase"], Choreo.PH_MATERIALISE, "still materialising at 0.4 s")
	assert_eq(p["pos"], RIM, "materialises in place — no fly-in")
	assert_gt(float(p["alpha"]), 0.2, "alpha rising")
	# After materialise + hold it departs for the work slot.
	_run(c, 1.2)
	assert_eq(c.pose("a")["phase"], Choreo.PH_TRAVEL, "travel begins after settling")
	# It arrives within the travel cap and stops off the node, not on it.
	_run(c, Choreo.TRAVEL_MAX_SEC + Choreo.ARRIVE_SEC + 0.2)
	p = c.pose("a")
	assert_eq(p["phase"], Choreo.PH_WORK, "working after arrive")
	var d: float = (p["pos"] as Vector3).distance_to(TARGET)
	assert_between(d, 0.2, 0.5, "work slot stands ~0.32 m off the node (%.2f)" % d)
	assert_true(bool(p["working"]), "pose reports working")
	assert_gt((p["aim"] as Vector3).length(), 0.0, "pointer aims at the target")
	assert_almost_eq(float(p["alpha"]), 1.0, 0.01, "fully opaque while working")


func test_head_pose_changes_never_move_a_working_agent() -> void:
	var c: RefCounted = _make()
	c.add_agent("a", RIM, 0.0, RIM)
	c.update_registry("a", Choreo.ST_WORKING, true, 42, TARGET)
	_run(c, 10.0)
	var before: Vector3 = c.pose("a")["pos"]
	# Turning the head (a new head position/forward) is not a pose input for the
	# work layer — the slot is frozen on arrival.
	c.set_head(Vector3(0.8, 1.6, 0.5))
	_run(c, 1.0)
	var after: Vector3 = c.pose("a")["pos"]
	assert_lt(before.distance_to(after), 0.03, "only the 1 cm hover moves it (%.3f)" % before.distance_to(after))


func test_explicit_done_completes_parks_to_rim_fades_and_stays_selectable_state() -> void:
	var c: RefCounted = _make()
	c.add_agent("a", RIM, 0.0, RIM)
	c.update_registry("a", Choreo.ST_WORKING, true, 42, TARGET)
	_run(c, 10.0)
	c.take_events()
	c.update_registry("a", Choreo.ST_DONE, true, 42, TARGET)
	c.tick(0.01)
	assert_eq(c.pose("a")["phase"], Choreo.PH_COMPLETE, "done → complete immediately, no idle timeout")
	var evs: Array = c.take_events()
	assert_eq(evs.size(), 1, "one completion event")
	assert_eq(evs[0]["type"], "complete")
	_run(c, Choreo.COMPLETE_SEC + 0.1)
	assert_eq(c.pose("a")["phase"], Choreo.PH_PARK, "parks after the completion beat")
	_run(c, Choreo.PARK_MAX_SEC + 0.5)
	var p: Dictionary = c.pose("a")
	assert_eq(p["phase"], Choreo.PH_REST, "rests at the rim")
	assert_lt((p["pos"] as Vector3).distance_to(RIM), 0.05, "parked at its rim slot")
	assert_almost_eq(float(p["alpha"]), Choreo.ALPHA_PARKED, 0.01, "faded to 0.3, still present")
	assert_true(bool(p["done"]), "pose reports done")
	# Resting: nothing moves it until it is re-tasked.
	_run(c, 5.0)
	assert_eq(c.pose("a")["pos"], p["pos"], "rest holds position")
	c.update_registry("a", Choreo.ST_WORKING, true, 7, Vector3(0.4, 1.4, -1.9))
	c.tick(0.01)
	assert_eq(c.pose("a")["phase"], Choreo.PH_TRAVEL, "re-tasked agent brightens and travels from where it is")


func test_new_target_while_working_starts_a_new_trip_from_the_current_slot() -> void:
	var c: RefCounted = _make()
	c.add_agent("a", RIM, 0.0, RIM)
	c.update_registry("a", Choreo.ST_WORKING, true, 42, TARGET)
	_run(c, 10.0)
	var slot: Vector3 = c.pose("a")["pos"]
	c.update_registry("a", Choreo.ST_WORKING, true, 43, Vector3(-0.5, 1.2, -1.2))
	c.tick(0.01)
	var p: Dictionary = c.pose("a")
	assert_eq(p["phase"], Choreo.PH_TRAVEL, "target change → travel (edge hand-off)")
	assert_lt((p["pos"] as Vector3).distance_to(slot), 0.05, "departs from the current slot, never snaps")


func test_expired_evidence_parks_without_a_completion_burst() -> void:
	var c: RefCounted = _make()
	c.add_agent("a", RIM, 0.0, RIM)
	c.update_registry("a", Choreo.ST_WORKING, true, 42, TARGET)
	_run(c, 10.0)
	c.take_events()
	c.update_registry("a", Choreo.ST_IDLE, false, -1, Vector3.ZERO)
	c.tick(0.01)
	assert_eq(c.pose("a")["phase"], Choreo.PH_PARK, "stale agent drifts to the rim")
	for ev: Dictionary in c.take_events():
		assert_ne(ev["type"], "complete", "no completion burst for an expiry")


func test_routes_and_slots_keep_one_metre_from_the_head() -> void:
	var c: RefCounted = _make()
	# Head sits right on the straight line from rim to target.
	c.set_head(Vector3(0.75, 1.3, -1.6))
	c.add_agent("a", RIM, 0.0, RIM)
	c.update_registry("a", Choreo.ST_WORKING, true, 42, TARGET)
	_run(c, 1.4)
	var min_d := 10.0
	var t := 0.0
	while t < Choreo.TRAVEL_MAX_SEC:
		c.tick(1.0 / 90.0)
		t += 1.0 / 90.0
		min_d = minf(min_d, (c.pose("a")["pos"] as Vector3).distance_to(Vector3(0.75, 1.3, -1.6)))
	assert_gt(min_d, 0.6, "route bulges away from the head (closest %.2f m)" % min_d)
	_run(c, 1.0)
	var slot: Vector3 = c.pose("a")["pos"]
	assert_gte(slot.distance_to(Vector3(0.75, 1.3, -1.6)), 0.99, "work slot respects the exclusion sphere")


func test_reduced_motion_relocates_by_fading_instead_of_travelling() -> void:
	var c: RefCounted = _make(true)
	c.add_agent("a", RIM, 0.0, RIM)
	c.update_registry("a", Choreo.ST_WORKING, true, 42, TARGET)
	_run(c, 1.4)
	assert_eq(c.pose("a")["phase"], Choreo.PH_TRAVEL)
	var positions: Array = []
	var t := 0.0
	while t < Choreo.RM_FADE_SEC * 2.0 + 0.05:
		c.tick(1.0 / 90.0)
		t += 1.0 / 90.0
		positions.append(c.pose("a")["pos"])
	# Exactly two distinct positions: the origin and the destination.
	var distinct: Dictionary = {}
	for p: Vector3 in positions:
		distinct[p] = true
	assert_eq(distinct.size(), 2, "no intermediate positions under reduced motion")
	_run(c, Choreo.ARRIVE_SEC + 3.0)
	var a: Vector3 = c.pose("a")["pos"]
	c.tick(0.5)
	assert_eq(c.pose("a")["pos"], a, "no hover under reduced motion")


func test_staggered_delays_and_removal() -> void:
	var c: RefCounted = _make()
	c.add_agent("a", RIM, 0.0, RIM)
	c.add_agent("b", RIM + Vector3(0.5, 0, 0), 1.3, RIM)
	_run(c, 0.5)
	assert_gt(float(c.pose("a")["alpha"]), 0.0, "first agent is fading in")
	assert_eq(float(c.pose("b")["alpha"]), 0.0, "delayed agent is still invisible")
	c.remove_agent("a")
	assert_false(c.has_agent("a"))
	assert_true(c.pose("a").is_empty(), "no pose for a removed agent")
	assert_eq(c.ids().size(), 1)


func test_quintic_ease_endpoints_and_monotonic() -> void:
	assert_eq(Choreo.ease_quintic(0.0), 0.0)
	assert_eq(Choreo.ease_quintic(1.0), 1.0)
	var last := 0.0
	for i: int in range(1, 11):
		var v: float = Choreo.ease_quintic(float(i) / 10.0)
		assert_gte(v, last, "monotonic")
		last = v
