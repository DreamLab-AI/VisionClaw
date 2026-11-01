extends "res://addons/gut/test.gd"

# The done beat of the single pose writer (scripts/agent_choreography.gd): a
# finished agent eases out from where it was to the edge of the visual scene in
# 30 s and holds there. This is the headset half of one rule that both clients
# follow (ADR-2135 "Done rule (both clients)" bullet, ADR-2109): radius
# r0 + (edge - r0) * easeOutCubic(elapsed / 30 s), measured from the moment the
# agent is recorded as done, never past the edge, constant once on it.

const Choreo := preload("res://scripts/agent_choreography.gd")
const SpatialVolume := preload("res://scripts/spatial_volume.gd")

const CENTRE := Vector3(0.0, 1.3, -1.6)
const EDGE := 0.9
const START := Vector3(0.18, 1.3, -1.6)   # a freshly spawned agent: centroid + 0.18 m ring
const TARGET := Vector3(0.0, 1.3, -1.6)


func _make(reduced: bool = false, edge: float = EDGE) -> RefCounted:
	var c: RefCounted = Choreo.new()
	c.reduced_motion = reduced
	c.set_head(Vector3(0.0, 1.6, 0.0))
	c.set_graph_centre(CENTRE)
	c.set_scene_edge(edge)
	return c


func _radius(c: RefCounted, id: String = "a") -> float:
	return ((c.pose(id)["pos"] as Vector3) - CENTRE).length()


func _run(c: RefCounted, seconds: float, hz: float = 90.0) -> void:
	var n: int = int(round(seconds * hz))
	for i: int in n:
		c.tick(1.0 / hz)


# A working agent at its work slot, ten seconds after spawning.
func _working(reduced: bool = false, edge: float = EDGE) -> RefCounted:
	var c: RefCounted = _make(reduced, edge)
	c.add_agent("a", START, 0.0, START)
	c.update_registry("a", Choreo.ST_WORKING, true, 42, TARGET)
	_run(c, 10.0)
	assert_eq(c.pose("a")["phase"], Choreo.PH_WORK, "fixture: working")
	return c


func test_a_done_agent_moves_outward_then_holds_still_at_the_edge() -> void:
	var c: RefCounted = _working()
	var r0: float = _radius(c)
	assert_lt(r0, EDGE - 0.3, "fixture: it starts well inside the edge (%.2f)" % r0)
	c.update_registry("a", Choreo.ST_DONE, true, 42, TARGET)
	var last: float = r0
	var steps: int = int(Choreo.EDGE_SECONDS * 90.0)
	for i: int in steps:
		c.tick(1.0 / 90.0)
		var r: float = _radius(c)
		assert_gte(r, last - 1.0e-6, "never moves inward (step %d: %.5f < %.5f)" % [i, r, last])
		last = r
	assert_gt(last, EDGE - 0.01, "on the edge after 30 s (%.4f)" % last)
	var held: Vector3 = c.pose("a")["pos"]
	for i: int in 900:   # ten more seconds
		c.tick(1.0 / 90.0)
		assert_eq(c.pose("a")["pos"], held, "bit-constant once on the edge")
		if c.pose("a")["pos"] != held:
			break
	assert_almost_eq(_radius(c), EDGE, 1.0e-4, "holds exactly on the edge")


func test_a_done_agent_holds_at_the_edge_for_ten_minutes_until_the_roster_removes_it() -> void:
	# F2 (owner, 2026-11-01): done, drifts to the edge, then holds until it leaves the roster.
	# Nothing in the choreography times the hold out: only remove_agent (the roster retiring it) ends it.
	var c: RefCounted = _working()
	c.update_registry("a", Choreo.ST_DONE, true, 42, TARGET)
	_run(c, Choreo.EDGE_SECONDS + 1.0, 72.0)
	var held: Vector3 = c.pose("a")["pos"]
	assert_almost_eq(_radius(c), EDGE, 1.0e-4, "fixture: on the edge")
	for i: int in 43200:   # ten minutes at 72 Hz
		c.tick(1.0 / 72.0)
		if i % 5000 == 0:
			c.update_registry("a", Choreo.ST_DONE, true, 42, TARGET)   # the 5 s keep-alive
		if c.pose("a")["pos"] != held or not c.has_agent("a"):
			fail_test("left the edge or the roster after %d ticks" % i)
			return
	assert_true(c.has_agent("a"), "still embodied after ten minutes")
	c.remove_agent("a")
	assert_false(c.has_agent("a"), "gone once the roster retires it")


func test_the_run_follows_the_closed_form_in_time() -> void:
	var c: RefCounted = _working()
	var r0: float = _radius(c)
	c.update_registry("a", Choreo.ST_DONE, true, 42, TARGET)
	var t: float = 0.0
	for i: int in 1350:   # 15 s
		c.tick(1.0 / 90.0)
		t += 1.0 / 90.0
	var u: float = t / Choreo.EDGE_SECONDS
	var want: float = r0 + (EDGE - r0) * (1.0 - pow(1.0 - u, 3.0))
	assert_almost_eq(_radius(c), want, 0.002, "radius is r0 + (edge - r0) * easeOutCubic(t / 30 s)")


func test_a_done_agent_never_passes_the_edge() -> void:
	var c: RefCounted = _working()
	c.update_registry("a", Choreo.ST_DONE, true, 42, TARGET)
	for i: int in int(40.0 * 90.0):
		c.tick(1.0 / 90.0)
		assert_lte(_radius(c), EDGE + 1.0e-5, "inside the edge at step %d" % i)


func test_an_agent_already_beyond_the_edge_is_put_onto_it_never_further_out() -> void:
	# The scene shrank (or the agent finished far out): it goes straight onto the edge.
	var c: RefCounted = _working(false, 5.0)
	var r_far: float = _radius(c)
	c.set_scene_edge(r_far * 0.5)
	c.update_registry("a", Choreo.ST_DONE, true, 42, TARGET)
	c.tick(1.0 / 90.0)
	assert_lte(_radius(c), r_far * 0.5 + 1.0e-5, "onto the smaller edge at once")
	_run(c, 35.0)
	assert_almost_eq(_radius(c), r_far * 0.5, 1.0e-4, "and it stays there")


func test_a_shrinking_edge_mid_run_clamps_the_agent() -> void:
	var c: RefCounted = _working()
	c.update_registry("a", Choreo.ST_DONE, true, 42, TARGET)
	_run(c, 12.0)
	c.set_scene_edge(0.6)
	for i: int in int(30.0 * 90.0):
		c.tick(1.0 / 90.0)
		assert_lte(_radius(c), 0.6 + 1.0e-5, "never past the new edge")
	assert_almost_eq(_radius(c), 0.6, 1.0e-4, "arrives on the new edge")


func test_arrival_takes_thirty_seconds_at_72_and_90_hz() -> void:
	# "Arrived" = within 0.1 mm of the edge (far under a pixel). easeOutCubic is
	# flat near the end, so that happens a little before 30 s; the closed form says when:
	# t* = 30 * (1 - cbrt(0.0001 / (edge - r0))). Both rates must hit it to within a frame,
	# and so each other. Past 30 s the position is exactly centre + dir * edge.
	var eps := 1.0e-4
	var arrival := {}
	for hz: float in [72.0, 90.0]:
		var c: RefCounted = _working()
		var r0: float = _radius(c)
		var expect: float = Choreo.EDGE_SECONDS * (1.0 - pow(eps / (EDGE - r0), 1.0 / 3.0))
		c.update_registry("a", Choreo.ST_DONE, true, 42, TARGET)
		var t: float = 0.0
		var landed: float = -1.0
		var frame: float = 1.0 / hz
		for i: int in int(40.0 * hz):
			c.tick(frame)
			t += frame
			if landed < 0.0 and EDGE - _radius(c) < eps:
				landed = t
		arrival[hz] = landed
		assert_gt(landed, 0.0, "reaches the edge at %d Hz" % int(hz))
		assert_almost_eq(landed, expect, frame + 1.0e-6, "%d Hz: arrives when the closed form says (%.3f vs %.3f s)" % [int(hz), landed, expect])
		assert_lt(landed, Choreo.EDGE_SECONDS + 1.0e-6, "and not after 30 s")
		assert_gt(landed, Choreo.EDGE_SECONDS - 6.0, "and not wildly early: about 30 s")
		assert_almost_eq(_radius(c), EDGE, 1.0e-5, "exactly on the edge afterwards")
	assert_almost_eq(float(arrival[72.0]), float(arrival[90.0]), 1.0 / 72.0 + 1.0e-6, "same arrival time at 72 and 90 Hz")
	# Midway the two rates sit at the same radius too (a function of time, not frames).
	var radii := {}
	for hz: float in [72.0, 90.0]:
		var c: RefCounted = _working()
		c.update_registry("a", Choreo.ST_DONE, true, 42, TARGET)
		_run(c, 10.0, hz)
		radii[hz] = _radius(c)
	assert_almost_eq(float(radii[72.0]), float(radii[90.0]), 0.002, "10 s in, 72 Hz and 90 Hz are at the same radius")


func test_an_agent_named_again_during_the_run_leaves_it() -> void:
	var c: RefCounted = _working()
	c.update_registry("a", Choreo.ST_DONE, true, 42, TARGET)
	_run(c, 12.0)
	var mid: float = _radius(c)
	assert_gt(mid, 0.5, "fixture: part-way out")
	assert_lt(mid, EDGE - 0.05, "fixture: not yet on the edge")
	c.update_registry("a", Choreo.ST_WORKING, true, 7, Vector3(0.1, 1.4, -1.5))
	c.tick(1.0 / 90.0)
	assert_eq(c.pose("a")["phase"], Choreo.PH_TRAVEL, "named again: it travels, the run is over")
	_run(c, Choreo.TRAVEL_MAX_SEC + Choreo.ARRIVE_SEC + 1.0)
	assert_eq(c.pose("a")["phase"], Choreo.PH_WORK, "and works again")
	assert_lt(_radius(c), EDGE - 0.2, "it is back at a work slot, not out on the edge")
	# Its next finish restarts the clock from where it is now.
	var r1: float = _radius(c)
	c.update_registry("a", Choreo.ST_DONE, true, 7, TARGET)
	_run(c, 1.0)
	assert_lt(_radius(c), EDGE - 0.3, "a new clock: one second in, still near the start")
	assert_gte(_radius(c), r1 - 1.0e-4, "and moving outward from r0")


func test_a_run_begun_mid_travel_still_runs_and_takes_thirty_seconds() -> void:
	var c: RefCounted = _make()
	c.add_agent("a", START, 0.0, START)
	c.update_registry("a", Choreo.ST_WORKING, true, 42, Vector3(0.4, 1.3, -1.9))
	_run(c, 2.0)
	assert_eq(c.pose("a")["phase"], Choreo.PH_TRAVEL, "fixture: mid-travel")
	c.update_registry("a", Choreo.ST_DONE, true, 42, Vector3(0.4, 1.3, -1.9))
	c.tick(1.0 / 90.0)
	assert_eq(c.pose("a")["phase"], Choreo.PH_EDGE, "done at any phase starts the run")
	_run(c, Choreo.EDGE_SECONDS + 0.1)
	assert_almost_eq(_radius(c), EDGE, 1.0e-4, "on the edge 30 s after it was recorded done")


func test_an_agent_that_is_done_when_it_materialises_runs_to_the_edge() -> void:
	var c: RefCounted = _make()
	c.add_agent("a", START, 0.0, START)
	c.update_registry("a", Choreo.ST_DONE, false, -1, Vector3.ZERO)
	_run(c, 1.5)   # materialise + hold
	assert_eq(c.pose("a")["phase"], Choreo.PH_EDGE, "straight into the run once it exists")
	_run(c, Choreo.EDGE_SECONDS + 0.5)
	assert_almost_eq(_radius(c), EDGE, 1.0e-4, "on the edge")


func test_the_run_runs_under_reduced_motion_without_a_lift_or_bulge() -> void:
	# Reduced motion is the headset default (graph_scene._reduced_motion = true).
	var c: RefCounted = _working(true)
	var dir: Vector3 = ((c.pose("a")["pos"] as Vector3) - CENTRE).normalized()
	c.update_registry("a", Choreo.ST_DONE, true, 42, TARGET)
	var last: float = _radius(c)
	var moved: bool = false
	for i: int in int(Choreo.EDGE_SECONDS * 90.0):
		c.tick(1.0 / 90.0)
		var r: float = _radius(c)
		assert_gte(r, last - 1.0e-6, "outward only")
		if r > last + 1.0e-7:
			moved = true
		last = r
		var off: Vector3 = (c.pose("a")["pos"] as Vector3) - CENTRE
		assert_lt(off.cross(dir).length(), 1.0e-4, "a straight radial line: no lift, no bulge (step %d)" % i)
	assert_true(moved, "it moves under reduced motion (the default on the headset)")
	assert_almost_eq(last, EDGE, 1.0e-4, "and lands on the edge")


func test_the_edge_is_measured_from_the_live_centre() -> void:
	# Move the volume after the agent has arrived: the agent is carried with it.
	var c: RefCounted = _working()
	c.update_registry("a", Choreo.ST_DONE, true, 42, TARGET)
	_run(c, 35.0)
	var shift := Vector3(0.5, 0.0, 0.25)
	c.set_graph_centre(CENTRE + shift)
	c.tick(1.0 / 90.0)
	var p: Vector3 = c.pose("a")["pos"]
	assert_almost_eq((p - (CENTRE + shift)).length(), EDGE, 1.0e-4, "still on the edge of the moved volume")


func test_the_run_never_enters_the_head_exclusion_sphere() -> void:
	# The old park routed around the head and kept its rest point 1 m off it; the run
	# must too. Put the head on the agent's outward ray, 1.6 m from the centre: the
	# edge (0.9) would leave the agent 0.7 m from the head, inside HEAD_EXCLUSION.
	var c: RefCounted = _working()
	var dir: Vector3 = ((c.pose("a")["pos"] as Vector3) - CENTRE).normalized()
	var head: Vector3 = CENTRE + dir * 1.6
	c.set_head(head)
	c.update_registry("a", Choreo.ST_DONE, true, 42, TARGET)
	var closest: float = 10.0
	for i: int in int(40.0 * 90.0):
		c.tick(1.0 / 90.0)
		closest = minf(closest, (c.pose("a")["pos"] as Vector3).distance_to(head))
	assert_gte(closest, Choreo.HEAD_EXCLUSION - 1.0e-4, "never closer than the exclusion radius to the head (%.4f)" % closest)
	var held: Vector3 = c.pose("a")["pos"]
	assert_lt(_radius(c), EDGE - 0.1, "it stopped short of the edge, at the exclusion sphere (%.3f)" % _radius(c))
	# The stop is frozen when the run begins: turning or moving the head later moves nothing.
	c.set_head(head + Vector3(0.3, 0.0, 0.2))
	_run(c, 2.0)
	assert_eq(c.pose("a")["pos"], held, "bit-constant while held, whatever the head does")


func test_a_head_off_the_outward_ray_leaves_the_run_alone() -> void:
	var c: RefCounted = _working()
	var dir: Vector3 = ((c.pose("a")["pos"] as Vector3) - CENTRE).normalized()
	c.set_head(CENTRE - dir * 1.6)   # behind the centre, on the far side
	c.update_registry("a", Choreo.ST_DONE, true, 42, TARGET)
	_run(c, 35.0)
	assert_almost_eq(_radius(c), EDGE, 1.0e-4, "the edge is the stop when the head is not in the way")


func test_the_clock_does_not_run_while_the_edge_is_unknown() -> void:
	# The desktop returns before it starts the clock when it has no edge.
	var c: RefCounted = _working(false, 0.0)
	var r0: float = _radius(c)
	c.update_registry("a", Choreo.ST_DONE, true, 42, TARGET)
	_run(c, 20.0)
	c.set_scene_edge(EDGE)
	c.tick(1.0 / 90.0)
	assert_lt(_radius(c), r0 + 0.01, "an edge that appears late starts the run from where it is, no jump")
	_run(c, Choreo.EDGE_SECONDS + 0.5)
	assert_almost_eq(_radius(c), EDGE, 1.0e-4, "and it takes the full 30 s from then")


func test_without_a_known_edge_a_done_agent_holds_still() -> void:
	var c: RefCounted = _working(false, 0.0)
	var held: Vector3 = c.pose("a")["pos"]
	c.update_registry("a", Choreo.ST_DONE, true, 42, TARGET)
	_run(c, 5.0)
	assert_lt((c.pose("a")["pos"] as Vector3).distance_to(held), 0.02, "no edge known: it stays at its slot (hover only)")


func test_ease_out_cubic_endpoints_and_monotonic() -> void:
	assert_eq(Choreo.ease_out_cubic(0.0), 0.0)
	assert_eq(Choreo.ease_out_cubic(1.0), 1.0)
	assert_eq(Choreo.ease_out_cubic(2.0), 1.0, "clamped")
	var last := 0.0
	for i: int in range(1, 11):
		var v: float = Choreo.ease_out_cubic(float(i) / 10.0)
		assert_gte(v, last, "monotonic")
		last = v
	assert_almost_eq(Choreo.ease_out_cubic(0.5), 0.875, 1.0e-6, "1 - (1 - u)^3")


# --- the edge value: one existing extent, no new constant ----------------------

func test_the_edge_radius_is_the_inscribed_sphere_of_the_volume_box_in_world_metres() -> void:
	assert_almost_eq(SpatialVolume.edge_radius(Vector3(1, 1, 1), 1.0), 0.5, 1.0e-6, "a cube: half the side")
	assert_almost_eq(SpatialVolume.edge_radius(Vector3(2.0, 0.8, 1.5), 1.0), 0.4, 1.0e-6, "half the smallest extent, so inside every face")
	assert_almost_eq(SpatialVolume.edge_radius(Vector3(1, 1, 1), 2.0), 1.0, 1.0e-6, "the arrangement scale applies")
	assert_eq(SpatialVolume.edge_radius(Vector3(0.6, 0.8, 0.0), 1.0), 0.0, "a flat box has no inscribed sphere, so no edge")
	var box := Vector3(2.0, 0.8, 1.5)
	var r := SpatialVolume.edge_radius(box, 1.0)
	for axis in [Vector3.RIGHT, Vector3.UP, Vector3.BACK]:
		assert_true(r <= 0.5 * absf((box * axis).length()) + 1.0e-6, "the edge sphere never crosses a face")
	assert_eq(SpatialVolume.edge_radius(Vector3(1, 1, 1), 0.0), 0.0, "no scale, no edge")
	assert_eq(SpatialVolume.edge_radius(Vector3(NAN, 1, 1), 1.0), 0.0, "non-finite bounds give no edge")


# --- sources: one definition, no parked path -----------------------------------

func _read(rel: String) -> String:
	var path: String = ProjectSettings.globalize_path("res://").path_join("..").path_join(rel).simplify_path()
	var f := FileAccess.open(path, FileAccess.READ)
	assert_not_null(f, "can read %s" % path)
	return f.get_as_text() if f != null else ""


func test_the_thirty_seconds_are_the_desktops_thirty_seconds() -> void:
	var ts: String = _read("client/src/features/graph/utils/agentNudge.ts")
	var re := RegEx.new()
	re.compile("export const AGENT_EDGE_SECONDS\\s*=\\s*(\\d+(?:\\.\\d+)?)")
	var m: RegExMatch = re.search(ts)
	assert_not_null(m, "agentNudge.ts defines AGENT_EDGE_SECONDS")
	if m != null:
		assert_eq(float(m.get_string(1)), Choreo.EDGE_SECONDS, "both clients use the same 30 s (ADR-2135)")
	var gd: String = _read("xr-client/scripts/agent_choreography.gd")
	assert_true(gd.contains("ADR-2135"), "the GDScript constant cites ADR-2135")
	assert_true(ts.contains("easeOutCubic"), "the desktop easing is easeOutCubic")


func test_the_park_beat_is_gone() -> void:
	var gd: String = _read("xr-client/scripts/agent_choreography.gd")
	for word: String in ["PH_PARK", "PH_COMPLETE", "PARK_SPEED", "PARK_MIN_SEC", "PARK_MAX_SEC", "ALPHA_PARKED", "_begin_park", "COMPLETE_SEC", "parking", "\"park\""]:
		assert_false(gd.contains(word), "agent_choreography.gd no longer has %s" % word)
	var gs: String = _read("xr-client/scripts/graph_scene.gd")
	assert_false(gs.contains("parks at"), "graph_scene.gd comments no longer describe a parked slot")
	assert_true(gs.contains("set_scene_edge(SpatialVolume.edge_radius("), "GraphScene feeds the edge from the overlay volume's box")
