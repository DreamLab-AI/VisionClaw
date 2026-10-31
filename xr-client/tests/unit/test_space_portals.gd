extends "res://addons/gut/test.gd"
# ADR-2166: spatial volumes can be entered; the others become portals. The
# script is load()ed inside each helper, so a missing script fails every test
# (rather than GUT skipping an unparseable file).
const VolumeArrangement := preload("res://scripts/volume_arrangement.gd")
const SpatialVolume := preload("res://scripts/spatial_volume.gd")
const VolumeScene := preload("res://scenes/EmulatedVolume.tscn")
const FixtureContentSource := preload("res://tests/fixtures/fixture_content_source.gd")
const PREFS_PATH := "res://scripts/xr_prefs.gd"
const FramePicker := preload("res://scripts/frame_picker.gd")
const BackendCaps := preload("res://scripts/backend_caps.gd")
const PORTALS_PATH := "res://scripts/space_portals.gd"
const SHADER_PATH := "res://materials/portal_membrane.gdshader"

const DT := 1.0 / 90.0
const IDS: Array[StringName] = [&"knowledge", &"ontology", &"codescape"]

var _calls := 0          # entered_changed emissions
var _offline: Dictionary = {}
var _native := false


func _portals_script() -> Script:
	return load(PORTALS_PATH)


func _arrangement() -> Node3D:
	var arr: Node3D = VolumeArrangement.new()
	add_child_autofree(arr)
	for id: StringName in IDS:
		var v: Node3D = VolumeScene.instantiate()
		v.volume_id = id
		v.bounds = VolumeArrangement.DEFAULT_BOUNDS
		arr.add_volume(v, VolumeArrangement.DEFAULT_YAWS[id])
	return arr


func _pts() -> PackedVector3Array:
	var rng := RandomNumberGenerator.new()
	rng.seed = 2166
	var pts := PackedVector3Array()
	for i: int in 200:
		pts.append(Vector3(rng.randf_range(-100.0, 300.0), rng.randf_range(40.0, 60.0), rng.randf_range(-50.0, 50.0)))
	pts.append(Vector3(-100.0, 40.0, -50.0))
	pts.append(Vector3(300.0, 60.0, 50.0))
	return pts


# An arrangement with a fixture source in every volume, ticked until fitted.
func _scene() -> Dictionary:
	var arr := _arrangement()
	for id: StringName in IDS:
		arr.volume(id).attach(FixtureContentSource.new(_pts(), StringName("fx:%s" % id)))
	for k: int in 3:
		for id: StringName in IDS:
			arr.volume(id).tick(DT)
	var p: Node3D = _portals_script().new()
	p.bind(arr)
	_calls = 0
	_offline = {}
	_native = false
	p.native_check = func() -> bool: return _native
	p.offline_check = func(id: StringName) -> bool: return bool(_offline.get(id, false))
	p.entered_changed.connect(func(_id: StringName) -> void: _calls += 1)
	return {"arr": arr, "p": p}


func _settle(p: Node3D, arr: Node3D, seconds := 1.0) -> void:
	var t := 0.0
	while t < seconds:
		arr.tick(DT)
		p.tick(DT)
		for id: StringName in IDS:
			arr.volume(id).tick(DT)
		t += DT


func _enter(p: Node3D, id: StringName, at := Vector3(2.0, 1.6, -1.0), heading := Vector3(0.0, 0.0, -1.0)) -> bool:
	return p.enter(id, at, heading)


# --- enter, leave, swap --------------------------------------------------------

func test_enter_makes_the_room() -> void:
	var s := _scene()
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	assert_false(p.is_entered())
	assert_true(_enter(p, &"ontology"))
	assert_eq(p.entered_id(), &"ontology")
	_settle(p, arr)
	var room: Node3D = arr.volume(&"ontology")
	assert_eq(room.bounds, p.ROOM_BOUNDS, "room-scale bounds, past the 4 m emulated clamp")
	assert_eq(room.bounds_mode, SpatialVolume.BoundsMode.BOUNDED, "never IMMERSIVE")
	assert_true(room.portal_room)
	assert_almost_eq(room.position.x, 2.0, 1.0e-4, "centred on the user's x")
	assert_almost_eq(room.position.z, -1.0, 1.0e-4, "centred on the user's z")
	assert_almost_eq(room.position.y - room.bounds.y * 0.5, 0.0, 1.0e-4, "box bottom on the floor")
	assert_false(room.frame.visible, "frame hidden while entered")
	assert_false(room._handle_marks.visible, "handle markers hidden")
	assert_eq(room.handle_at(room.position + room.bounds * 0.5, 0.5), -1, "no handle can be picked")
	for id: StringName in [&"knowledge", &"codescape"]:
		assert_eq(arr.volume(id).bounds, VolumeArrangement.DEFAULT_BOUNDS, "%s keeps its size" % id)
		assert_true(arr.volume(id).frame.visible, "%s frame still drawn" % id)
	assert_eq(p.portal_ids().size(), 2)
	assert_true(arr.layout_locked)
	assert_eq(_calls, 1)


func test_resize_grip_is_refused_on_the_room() -> void:
	var s := _scene()
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	_enter(p, &"knowledge")
	_settle(p, arr)
	var room: Node3D = arr.volume(&"knowledge")
	var hand := Node3D.new()
	add_child_autofree(hand)
	hand.position = room.position + room.bounds * 0.5
	room.begin_resize(7, hand)
	assert_false(room.is_resizing(), "the room cannot be resized by a handle")
	assert_eq(room.bounds, p.ROOM_BOUNDS)


func test_leave_restores_the_ring_exactly() -> void:
	var s := _scene()
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	# A messy starting layout: a grip move, a bring-to-front, a resize, and a
	# two-hand transform on the arrangement itself.
	arr.move_volume(&"ontology", Transform3D(Basis(Vector3.UP, 0.4), Vector3(0.9, 1.1, -0.7)))
	arr.bring_to_front(&"knowledge")
	_settle(p, arr)
	arr.volume(&"codescape").set_runtime_bounds(Vector3(1.5, 0.8, 2.0), SpatialVolume.BoundsMode.BOUNDED)
	arr.transform = Transform3D(Basis(Vector3.UP, 0.3).scaled(Vector3.ONE * 0.6), Vector3(0.2, 0.0, 0.5))
	var before: Dictionary = arr.snapshot()
	assert_true(arr.is_moved(&"ontology"))
	assert_true(_enter(p, &"codescape"))
	_settle(p, arr)
	assert_true(arr.transform.is_equal_approx(Transform3D.IDENTITY), "entered on an untouched frame")
	assert_true(p.leave())
	_settle(p, arr)
	var after: Dictionary = arr.snapshot()
	assert_eq(after["arr_xf"], before["arr_xf"], "arrangement transform back")
	for id: StringName in IDS:
		assert_eq(after["xf"][id], before["xf"][id], "%s transform" % id)
		assert_eq(after["bounds"][id], before["bounds"][id], "%s bounds" % id)
		assert_eq(after["modes"][id], before["modes"][id], "%s mode" % id)
	assert_eq(after["slots"], before["slots"], "slots")
	assert_eq(after["moved"], before["moved"], "moved flags")
	assert_true(arr.is_moved(&"ontology"))
	assert_false(arr.layout_locked)
	assert_false(p.is_entered())
	assert_false(arr.volume(&"codescape").portal_room)
	assert_true(arr.volume(&"codescape").frame.visible, "frame back")
	assert_false(p.leave(), "nothing to leave")


func test_swap_makes_the_old_room_a_portal() -> void:
	var s := _scene()
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	var before: Dictionary = arr.snapshot()
	_enter(p, &"knowledge")
	_settle(p, arr)
	assert_true(_enter(p, &"ontology", Vector3(-1.0, 1.7, 3.0)))
	_settle(p, arr)
	assert_eq(p.entered_id(), &"ontology")
	assert_eq(arr.volume(&"ontology").bounds, p.ROOM_BOUNDS)
	assert_eq(arr.volume(&"knowledge").bounds, VolumeArrangement.DEFAULT_BOUNDS, "old room shrinks back")
	assert_false(arr.volume(&"knowledge").portal_room)
	assert_true(p.portal_ids().has(&"knowledge"), "the old room is a portal")
	assert_almost_eq(arr.volume(&"ontology").position.x, -1.0, 1.0e-4)
	assert_eq(_calls, 2)
	p.leave()
	_settle(p, arr)
	var after: Dictionary = arr.snapshot()
	for id: StringName in IDS:
		assert_eq(after["xf"][id], before["xf"][id], "%s back after swap + leave" % id)
		assert_eq(after["bounds"][id], before["bounds"][id])


func test_entering_the_entered_volume_is_a_no_op() -> void:
	var s := _scene()
	var p: Node3D = s["p"]
	_enter(p, &"knowledge")
	_settle(p, s["arr"])
	assert_false(_enter(p, &"knowledge"))
	assert_eq(_calls, 1)


func test_bring_to_front_is_refused_while_entered() -> void:
	var s := _scene()
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	_enter(p, &"knowledge")
	_settle(p, arr)
	var before: Transform3D = arr.volume(&"ontology").transform
	assert_false(arr.bring_to_front(&"ontology"), "the ring is not the layout while entered")
	assert_eq(arr.volume(&"ontology").transform, before)
	assert_false(arr.is_easing())


# --- the invariant: no portal box meets the room -------------------------------

func test_sat_helper_agrees_with_obvious_cases() -> void:
	var P = _portals_script()
	assert_true(P.boxes_intersect(Transform3D.IDENTITY, Vector3.ONE, Transform3D(Basis.IDENTITY, Vector3(0.9, 0.0, 0.0)), Vector3.ONE))
	assert_false(P.boxes_intersect(Transform3D.IDENTITY, Vector3.ONE, Transform3D(Basis.IDENTITY, Vector3(1.1, 0.0, 0.0)), Vector3.ONE))
	# A turned box reaches further along the diagonal than its half-width.
	var turned := Transform3D(Basis(Vector3.UP, PI / 4.0), Vector3(1.2, 0.0, 0.0))
	assert_true(P.boxes_intersect(Transform3D.IDENTITY, Vector3.ONE, turned, Vector3.ONE))
	assert_false(P.boxes_intersect(Transform3D.IDENTITY, Vector3.ONE, Transform3D(Basis(Vector3.UP, PI / 4.0), Vector3(1.3, 0.0, 0.0)), Vector3.ONE))


func test_portal_ring_never_meets_the_room() -> void:
	var cases := [
		{"user": Vector3(0.0, 1.6, 0.0), "head": Vector3(0.0, 0.0, -1.0), "big": &""},
		{"user": Vector3(3.0, 1.2, -4.0), "head": Vector3(1.0, 0.0, 1.0), "big": &"ontology"},
		{"user": Vector3(-2.0, 1.8, 2.0), "head": Vector3(-1.0, 0.0, 0.3), "big": &"knowledge"},
		{"user": Vector3(0.5, 1.6, 0.5), "head": Vector3(0.7, 0.0, -0.7), "big": &"codescape"},
	]
	for c: Dictionary in cases:
		for room_id: StringName in IDS:
			var s := _scene()
			var arr: Node3D = s["arr"]
			var p: Node3D = s["p"]
			if c["big"] != &"":
				arr.volume(c["big"]).set_runtime_bounds(Vector3(4.0, 4.0, 4.0), SpatialVolume.BoundsMode.BOUNDED)
			var snapshot_bounds := {}
			for id: StringName in IDS:
				snapshot_bounds[id] = arr.volume(id).bounds
			if c["big"] == room_id:
				# The room is resized to ROOM_BOUNDS; the big box is not a portal.
				pass
			assert_true(_enter(p, room_id, c["user"], c["head"]), "enter %s" % room_id)
			_settle(p, arr)
			var room: Node3D = arr.volume(room_id)
			assert_eq(p.portal_ids().size(), 2)
			for pid: StringName in p.portal_ids():
				var q: Node3D = arr.volume(pid)
				assert_false(p.boxes_intersect(room.transform, room.bounds, q.transform, q.bounds),
					"%s portal clear of the %s room (user %s)" % [pid, room_id, c["user"]])
				# Faces the user: +Z points back at the floor point.
				var to_user := Vector3(c["user"].x - q.position.x, 0.0, c["user"].z - q.position.z).normalized()
				assert_true(q.transform.basis.z.is_equal_approx(to_user), "%s faces the user" % pid)
				assert_almost_eq(q.position.y, clampf(c["user"].y, 0.8, 2.2), 1.0e-4, "at eye height")
				var d := Vector2(q.position.x - c["user"].x, q.position.z - c["user"].z).length()
				var floor_r: float = p.half_diagonal_xz(p.ROOM_BOUNDS) + p.half_diagonal_xz(q.bounds) + p.PORTAL_MARGIN
				assert_true(d >= floor_r - 1.0e-4, "ring radius %.3f >= %.3f" % [d, floor_r])
			p.leave()
			_settle(p, arr)
			for id: StringName in IDS:
				assert_eq(arr.volume(id).bounds, snapshot_bounds[id], "%s bounds restored" % id)


func test_swap_keeps_the_invariant() -> void:
	var s := _scene()
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	arr.volume(&"codescape").set_runtime_bounds(Vector3(4.0, 3.0, 4.0), SpatialVolume.BoundsMode.BOUNDED)
	_enter(p, &"knowledge", Vector3(0.0, 1.6, 0.0))
	_settle(p, arr)
	_enter(p, &"ontology", Vector3(1.5, 1.6, 1.0), Vector3(1.0, 0.0, 0.0))
	_settle(p, arr)
	var room: Node3D = arr.volume(&"ontology")
	for pid: StringName in p.portal_ids():
		var q: Node3D = arr.volume(pid)
		assert_false(p.boxes_intersect(room.transform, room.bounds, q.transform, q.bounds), "%s clear after swap" % pid)


func test_arrangement_scale_does_not_shrink_the_room() -> void:
	var s := _scene()
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	arr.transform = Transform3D(Basis.IDENTITY.scaled(Vector3.ONE * 0.5), Vector3(1.0, 0.0, 0.0))
	_enter(p, &"knowledge")
	_settle(p, arr)
	var room: Node3D = arr.volume(&"knowledge")
	assert_true(arr.transform.is_equal_approx(Transform3D.IDENTITY), "unscaled while entered")
	assert_true((room.global_transform.basis.get_scale() * room.bounds).is_equal_approx(p.ROOM_BOUNDS), "6 m of world")


func test_floor_is_the_origin_height_not_zero() -> void:
	var s := _scene()
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	assert_true(p.enter(&"knowledge", Vector3(0.0, 2.1, 0.0), Vector3(0.0, 0.0, -1.0), 0.5))
	_settle(p, arr)
	var room: Node3D = arr.volume(&"knowledge")
	assert_almost_eq(room.position.y - room.bounds.y * 0.5, 0.5, 1.0e-4, "box bottom on the origin's floor")
	for pid: StringName in p.portal_ids():
		assert_almost_eq(arr.volume(pid).position.y, 0.5 + 1.6, 1.0e-4, "eye height measured from that floor")


func test_rays_from_inside_the_room_pick_portals_not_the_room() -> void:
	var s := _scene()
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	_enter(p, &"knowledge", Vector3(0.0, 1.6, 0.0))
	_settle(p, arr)
	var vols := {}
	for id: StringName in IDS:
		vols[id] = arr.volume(id)
	var portal: Node3D = arr.volume(p.portal_ids()[0])
	var origin := Vector3(0.0, 1.5, 0.0)
	var dir := (portal.global_position - origin).normalized()
	var hit: Dictionary = FramePicker.pick(origin, dir, vols, 0.04)
	assert_false(hit.is_empty(), "the portal is hit")
	assert_eq(hit["volume"], p.portal_ids()[0], "pointing at a portal focuses the portal, not the room")
	var away: Dictionary = FramePicker.pick(origin, Vector3.UP, vols, 0.04)
	assert_true(away.is_empty(), "empty space picks nothing: grip falls through to two-hand")


# --- E7 for the room -----------------------------------------------------------

func test_e7_holds_for_the_room() -> void:
	var s := _scene()
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	_enter(p, &"knowledge")
	# Check on every frame of the entry, dark and after.
	var t := 0.0
	var room: Node3D = arr.volume(&"knowledge")
	var src: RefCounted = room.source()
	while t < 1.0:
		arr.tick(DT)
		p.tick(DT)
		room.tick(DT)
		for pt: Vector3 in src.points:
			assert_true(room.contains_world(room.world_from_content(pt)), "node centre inside the room at t=%.2f" % t)
		t += DT
	# The fit is also tight: it fills the room on its binding axis.
	assert_almost_eq(room.target_scale(), room.bounds.x / 400.0, 1.0e-6)  # x binds: 6 / 400


# --- refusals ------------------------------------------------------------------

func _state(arr: Node3D, p: Node3D) -> Dictionary:
	var out: Dictionary = arr.snapshot()
	out["entered"] = p.entered_id()
	out["fade"] = p.fade_alpha()
	out["calls"] = _calls
	out["locked"] = arr.layout_locked
	out["rooms"] = IDS.map(func(id: StringName) -> bool: return arr.volume(id).portal_room)
	out["weights"] = p.budget_weights()
	return out


func _assert_refused(s: Dictionary, id: StringName, why: String) -> void:
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	var before := _state(arr, p)
	assert_false(p.can_enter(id), "can_enter false: %s" % why)
	assert_false(p.enter(id, Vector3.ZERO, Vector3(0.0, 0.0, -1.0)), "enter false: %s" % why)
	_settle(p, arr, 0.3)
	assert_eq(_state(arr, p), before, "nothing changed: %s" % why)


func test_refused_when_native_is_active() -> void:
	var s := _scene()
	_native = true
	_assert_refused(s, &"knowledge", "native container arm active")
	_native = false
	assert_true(_enter(s["p"], &"knowledge"), "accepted once the native arm is off")


func test_refused_when_the_target_is_closed() -> void:
	var s := _scene()
	s["arr"].volume(&"ontology").close()
	_assert_refused(s, &"ontology", "closed")


func test_refused_when_the_target_is_hidden() -> void:
	var s := _scene()
	s["arr"].volume(&"ontology").request_visible(false)
	_assert_refused(s, &"ontology", "hidden")


func test_refused_when_the_target_is_offline() -> void:
	var s := _scene()
	_offline[&"codescape"] = true
	_assert_refused(s, &"codescape", "offline")


func test_refused_for_an_unknown_volume_and_mid_transition() -> void:
	var s := _scene()
	_assert_refused(s, &"nope", "unknown id")
	var p: Node3D = s["p"]
	_enter(p, &"knowledge")
	assert_true(p.is_transitioning())
	assert_false(_enter(p, &"ontology"), "second enter during the dim")
	assert_eq(p.entered_id(), &"knowledge")


func test_refused_enter_while_inside_leaves_the_space_untouched() -> void:
	var s := _scene()
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	_enter(p, &"knowledge")
	_settle(p, arr)
	_offline[&"codescape"] = true
	_assert_refused(s, &"codescape", "offline target while inside another space")


func test_room_going_away_ends_the_space() -> void:
	var s := _scene()
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	var before: Dictionary = arr.snapshot()
	_enter(p, &"ontology")
	_settle(p, arr)
	arr.volume(&"ontology").request_visible(false)
	_settle(p, arr, 0.2)
	assert_false(p.is_entered(), "a hidden room is left")
	assert_false(arr.layout_locked)
	assert_eq(arr.snapshot()["xf"][&"knowledge"], before["xf"][&"knowledge"])


# --- walk-through --------------------------------------------------------------

func test_walkthrough_is_off_by_default_and_pref_round_trips() -> void:
	var s := _scene()
	assert_false(s["p"].walkthrough_enabled)
	var tmp := "user://xr_prefs.portal_test.cfg"
	DirAccess.remove_absolute(ProjectSettings.globalize_path(tmp))
	var prefs = load(PREFS_PATH)
	prefs.path_override = tmp
	assert_false(prefs.portal_walkthrough(), "default off")
	prefs.set_portal_walkthrough(true)
	assert_true(prefs.portal_walkthrough())
	prefs.set_portal_walkthrough(false)
	assert_false(prefs.portal_walkthrough())
	prefs.path_override = ""
	DirAccess.remove_absolute(ProjectSettings.globalize_path(tmp))


func test_walkthrough_does_nothing_when_disabled() -> void:
	var s := _scene()
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	_enter(p, &"knowledge")
	_settle(p, arr)
	var q: Node3D = arr.volume(p.portal_ids()[0])
	for i: int in 90:
		assert_eq(p.update_walkthrough(q.global_position, Vector3(0.0, 0.0, -1.0), DT), &"")
	assert_eq(p.entered_id(), &"knowledge")


func test_walkthrough_needs_a_dwell() -> void:
	var s := _scene()
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	p.walkthrough_enabled = true
	_enter(p, &"knowledge")
	_settle(p, arr)
	assert_false(p.is_walkthrough_armed(), "disarmed by the enter itself")
	p.update_walkthrough(arr.volume(&"knowledge").global_position, Vector3(0.0, 0.0, -1.0), DT)
	assert_true(p.is_walkthrough_armed(), "armed once the head is clear of every portal")
	var target: StringName = p.portal_ids()[0]
	var inside: Vector3 = arr.volume(target).global_position
	var t := 0.0
	while t < p.DWELL_SEC - 0.05:
		assert_eq(p.update_walkthrough(inside, Vector3(0.0, 0.0, -1.0), DT), &"")
		t += DT
	assert_eq(p.entered_id(), &"knowledge", "short of the dwell")
	# Stepping out resets the clock.
	p.update_walkthrough(Vector3(0.0, 1.5, 0.0), Vector3(0.0, 0.0, -1.0), DT)
	t = 0.0
	while t < p.DWELL_SEC - 0.05:
		p.update_walkthrough(inside, Vector3(0.0, 0.0, -1.0), DT)
		t += DT
	assert_eq(p.entered_id(), &"knowledge", "the clock restarted after stepping out")
	var got := &""
	for i: int in 60:
		got = p.update_walkthrough(inside, Vector3(0.0, 0.0, -1.0), DT)
		if got != &"":
			break
	assert_eq(got, target, "dwell reached: enters the portal")
	assert_eq(p.entered_id(), target)
	assert_false(p.is_walkthrough_armed())


func test_walkthrough_rearms_only_after_leaving_every_portal() -> void:
	var s := _scene()
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	p.walkthrough_enabled = true
	_enter(p, &"knowledge", Vector3(0.0, 1.6, 0.0))
	_settle(p, arr)
	var first: StringName = p.portal_ids()[0]
	p.update_walkthrough(arr.volume(&"knowledge").global_position, Vector3(0.0, 0.0, -1.0), DT)
	var got := &""
	var inside: Vector3 = arr.volume(first).global_position
	for i: int in 60:
		got = p.update_walkthrough(inside, Vector3(0.0, 0.0, -1.0), DT)
		if got != &"":
			break
	assert_eq(got, first)
	_settle(p, arr)
	# Disarmed: the head dropped into another portal does nothing, however long.
	var other: StringName = p.portal_ids()[0]
	var spot: Vector3 = arr.volume(other).global_position
	for i: int in 180:
		assert_eq(p.update_walkthrough(spot, Vector3(0.0, 0.0, -1.0), DT), &"", "not re-armed")
	assert_false(p.is_walkthrough_armed())
	# Hovering 0.2 m off a box keeps it disarmed (inside the 0.3 m hysteresis).
	var q: Node3D = arr.volume(other)
	var edge: Vector3 = q.global_position + q.global_transform.basis.x * (q.bounds.x * 0.5 + 0.2)
	p.update_walkthrough(edge, Vector3(0.0, 0.0, -1.0), DT)
	assert_false(p.is_walkthrough_armed(), "0.2 m outside is not clear")
	# Well clear of every portal re-arms.
	p.update_walkthrough(arr.volume(p.entered_id()).global_position, Vector3(0.0, 0.0, -1.0), DT)
	assert_true(p.is_walkthrough_armed(), "re-armed")
	got = &""
	for i: int in 60:
		got = p.update_walkthrough(spot, Vector3(0.0, 0.0, -1.0), DT)
		if got != &"":
			break
	assert_eq(got, other, "enters again once re-armed and dwelt")


# --- comfort: no world motion while visible ------------------------------------

func _world_state(arr: Node3D) -> Array:
	var out: Array = [arr.transform, arr.scale]
	for id: StringName in IDS:
		var v: Node3D = arr.volume(id)
		out.append([v.transform, v.bounds, v.content.transform])
	return out


func test_reduced_motion_dims_then_snaps_with_no_visible_motion() -> void:
	var s := _scene()
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	var cam := Node3D.new()
	add_child_autofree(cam)
	p.attach_fade(cam)
	p.reduced_motion = true
	var before := _world_state(arr)
	assert_true(_enter(p, &"ontology"))
	assert_false(arr.is_easing(), "no ease machinery in reduced motion")
	var moved_while_visible := 0
	var saw_dark := false
	var dark_done := false
	var guard := 0
	while p.is_transitioning() and guard < 200:
		arr.tick(DT)
		p.tick(DT)
		for id: StringName in IDS:
			arr.volume(id).tick(DT)
		var a: float = p.fade_alpha()
		assert_false(arr.is_easing(), "never easing")
		assert_eq(p.fade_quad.visible, a > 0.001, "dim quad follows alpha")
		if a >= 0.999:
			saw_dark = true
		if not saw_dark and _world_state(arr) != before:
			moved_while_visible += 1
		if saw_dark and not dark_done and _world_state(arr) != before:
			dark_done = true
			assert_true(a >= 0.999, "the snap lands in full dark")
		guard += 1
	assert_true(saw_dark, "the view went fully dark")
	assert_eq(moved_while_visible, 0, "nothing moved or scaled before the view was dark")
	assert_true(dark_done, "the snap happened")
	assert_almost_eq(p.fade_alpha(), 0.0, 1.0e-6)
	assert_false(p.fade_quad.visible, "dim quad gone")
	# Content is already at its fitted size once the dim lifts (no grow-in).
	var room: Node3D = arr.volume(&"ontology")
	var settled: Transform3D = room.content.transform
	for i: int in 30:
		room.tick(DT)
	assert_true(room.content.transform.is_equal_approx(settled), "no content growth after the dim")
	# And again on the way out.
	var entered_state := _world_state(arr)
	assert_true(p.leave())
	var visible_motion := 0
	saw_dark = false
	guard = 0
	while p.is_transitioning() and guard < 200:
		arr.tick(DT)
		p.tick(DT)
		if p.fade_alpha() >= 0.999:
			saw_dark = true
		if not saw_dark and _world_state(arr) != entered_state:
			visible_motion += 1
		guard += 1
	assert_eq(visible_motion, 0, "leaving also snaps in the dark")
	assert_true(saw_dark)


func test_eased_motion_stays_inside_the_budget_and_never_scales_the_arrangement() -> void:
	var s := _scene()
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	p.reduced_motion = false
	assert_true(_enter(p, &"knowledge"))
	assert_false(p.is_transitioning(), "no dim when motion is allowed")
	assert_eq(p.fade_alpha(), 0.0)
	assert_true(arr.is_easing(&"knowledge"), "the room eases")
	var t := 0.0
	var steps := 0
	while arr.is_easing() and t < 2.0:
		arr.tick(DT)
		p.tick(DT)
		assert_true(arr.transform.is_equal_approx(Transform3D.IDENTITY), "arrangement never moved or scaled")
		t += DT
		steps += 1
	assert_true(t <= p.EASE_SEC + 2.0 * DT, "done within %.2f s (took %.3f)" % [p.EASE_SEC, t])
	assert_true(steps > 5, "it was an ease, not a snap")
	var room: Node3D = arr.volume(&"knowledge")
	assert_almost_eq(room.position.y - room.bounds.y * 0.5, 0.0, 1.0e-4)


func test_an_arrangement_with_a_two_hand_transform_takes_the_dim_path() -> void:
	var s := _scene()
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	p.reduced_motion = false
	arr.transform = Transform3D(Basis.IDENTITY.scaled(Vector3.ONE * 0.7), Vector3(0.4, 0.0, 0.0))
	assert_true(_enter(p, &"knowledge"))
	assert_true(p.is_transitioning(), "dim instead of a visible rescale")
	assert_false(arr.is_easing())


# --- the membranes -------------------------------------------------------------

func test_membranes_are_one_multimesh_one_draw_call() -> void:
	var s := _scene()
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	var mmis: Array[Node] = p.find_children("*", "MultiMeshInstance3D", true, false)
	assert_eq(mmis.size(), 1, "one MultiMeshInstance3D under the portals node")
	assert_false(p.membranes.visible, "nothing drawn on the ring")
	_enter(p, &"knowledge")
	_settle(p, arr)
	var mm: MultiMesh = p.membranes.multimesh
	assert_true(p.membranes.visible)
	assert_eq(mm.visible_instance_count, 2, "both other volumes are portals")
	assert_true(mm.use_colors, "per-instance tint and focus")
	assert_eq(mm.mesh.get_surface_count(), 1, "one surface")
	assert_true(mm.mesh.surface_get_material(0) is ShaderMaterial)
	var mat: ShaderMaterial = mm.mesh.surface_get_material(0)
	assert_eq(mat.shader.resource_path, SHADER_PATH)
	# Drawn call count: 1 MultiMesh node, 1 surface, 1 material.
	var draw_calls := 0
	for n: Node in arr.find_children("*", "MultiMeshInstance3D", true, false):
		if n == p.membranes and n.visible:
			draw_calls += n.multimesh.mesh.get_surface_count()
	assert_eq(draw_calls, 1, "all membranes in one draw call")
	# Tints: knowledge/ontology/codescape differ; the focused portal is brighter.
	arr.volume(&"codescape").set_focused(true)
	arr.volume(&"ontology").set_focused(false)
	p.tick(DT)
	var ids: Array[StringName] = p.portal_ids()
	for i: int in ids.size():
		var c: Color = p.membrane_color(i)
		var want: Color = p.TINTS[ids[i]]
		assert_almost_eq(c.r, want.r, 1.0e-3, "%s tint" % ids[i])
		assert_almost_eq(c.a, 1.0 if ids[i] == &"codescape" else p.UNFOCUSED, 1.0e-3, "%s focus" % ids[i])
	# Each membrane sits on its portal's user-facing face.
	for i: int in ids.size():
		var v: Node3D = arr.volume(ids[i])
		var xf: Transform3D = p.membrane_transform(i)
		assert_true(xf.origin.is_equal_approx(v.transform * Vector3(0.0, 0.0, v.bounds.z * 0.5 + p.MEMBRANE_LIFT)), "%s membrane on the +Z face" % ids[i])
	p.leave()
	_settle(p, arr)
	assert_false(p.membranes.visible, "ring mode draws no membranes")


func test_membrane_motion_uniform_follows_reduced_motion() -> void:
	var s := _scene()
	var p: Node3D = s["p"]
	var mat: ShaderMaterial = p.membranes.multimesh.mesh.surface_get_material(0)
	p.reduced_motion = true
	assert_eq(float(mat.get_shader_parameter("motion")), 0.0)
	p.reduced_motion = false
	assert_eq(float(mat.get_shader_parameter("motion")), 1.0)


func test_membrane_shader_stays_off_the_screen_and_depth_textures() -> void:
	var src := FileAccess.get_file_as_string(SHADER_PATH)
	assert_false(src.is_empty())
	for banned: String in ["SCREEN_TEXTURE", "hint_screen_texture", "hint_depth_texture", "DEPTH_TEXTURE", "SubViewport"]:
		assert_false(src.contains(banned), "no %s" % banned)
	for token: String in ["unshaded", "blend_add", "depth_draw_never", "cull_disabled", "uniform float motion"]:
		assert_true(src.contains(token), "has %s" % token)
	assert_false(src.contains("depth_test_disabled"), "depth test stays on")
	var code := ""
	for line: String in FileAccess.get_file_as_string(PORTALS_PATH).split("\n"):
		if not line.strip_edges().begins_with("#"):
			code += line + "\n"
	for banned: String in ["SubViewport", "SCREEN_TEXTURE", "Camera3D.new", "stencil", "hint_depth_texture"]:
		assert_false(code.contains(banned), "portals code uses no %s" % banned)


# --- budget hook ---------------------------------------------------------------

func test_budget_weights() -> void:
	var s := _scene()
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	for id: StringName in IDS:
		assert_eq(p.budget_weights()[id], 1.0, "ring: all 1.0")
	_enter(p, &"ontology")
	var w: Dictionary = p.budget_weights()
	assert_eq(w[&"ontology"], 1.0)
	assert_eq(w[&"knowledge"], 0.15)
	assert_eq(w[&"codescape"], 0.15)
	_settle(p, arr)
	p.leave()
	for id: StringName in IDS:
		assert_eq(p.budget_weights()[id], 1.0, "left: all 1.0 again")


# --- GraphScene wiring ---------------------------------------------------------

func _make_scene() -> Node3D:
	var packed: PackedScene = load("res://scenes/GraphScene.tscn")
	var scene: Node3D = packed.instantiate()
	add_child(scene)
	await get_tree().process_frame
	return scene


func _scene_settle(scene: Node3D, seconds := 0.6) -> void:
	var t := 0.0
	while t < seconds:
		scene._arrangement.tick(DT)
		scene._portals.tick(DT)
		t += DT


func test_scene_enter_leave_actions_and_budget_hook() -> void:
	var scene := await _make_scene()
	assert_not_null(scene._portals, "GraphScene builds SpacePortals")
	assert_not_null(scene._portals.fade_quad, "the dim quad exists (reduced motion dims, not just snaps)")
	# Headless has no connections up: lift the offline refusal for this test.
	scene._portals.offline_check = func(_id: StringName) -> bool: return false
	scene._shim_pending_start = false
	scene._on_hud_control("volume_enter:ontology")
	assert_eq(scene._portals.entered_id(), &"ontology")
	_scene_settle(scene)
	assert_true(scene._volume_states()[&"ontology"]["entered"], "HUD sees the entered volume")
	scene._on_hud_control("volume_front:knowledge")
	assert_false(scene._arrangement.is_easing(), "Front is ignored while entered")
	scene._on_hud_control("volume_leave")
	_scene_settle(scene)
	assert_false(scene._portals.is_entered())
	scene._on_hud_control("volume_enter:knowledge")
	_scene_settle(scene)
	scene._on_hud_control("reset_layout")
	assert_false(scene._portals.is_entered(), "Reset Layout leaves first")
	assert_false(scene._arrangement.layout_locked)
	scene.queue_free()
	await get_tree().process_frame


func test_scene_refuses_enter_while_native_is_pending() -> void:
	var scene := await _make_scene()
	scene._portals.offline_check = func(_id: StringName) -> bool: return false
	scene._shim_pending_start = true
	scene._on_hud_control("volume_enter:ontology")
	assert_false(scene._portals.is_entered(), "native chosen but not started: refused")
	scene._shim_pending_start = false
	scene.queue_free()
	await get_tree().process_frame


func test_scene_budget_hook_is_guarded_and_called_when_present() -> void:
	var scene := await _make_scene()
	scene._portals.offline_check = func(_id: StringName) -> bool: return false
	scene._shim_pending_start = false
	var seen: Array = []
	var fake := FakeBudget.new()
	fake.seen = seen
	var real = scene._budget
	scene._budget = fake
	scene._on_hud_control("volume_enter:codescape")
	assert_eq(seen.size(), 1)
	assert_eq(seen[0][&"codescape"], 1.0)
	assert_eq(seen[0][&"knowledge"], 0.15)
	_scene_settle(scene)
	scene._on_hud_control("volume_leave")
	assert_eq(seen.size(), 2)
	assert_eq(seen[1][&"knowledge"], 1.0)
	# A budget without the method (XE not merged yet) is simply skipped.
	scene._budget = real
	if not real.has_method("set_volume_weights"):
		_scene_settle(scene)
		scene._on_hud_control("volume_enter:knowledge")
		assert_true(scene._portals.is_entered())
	scene.queue_free()
	await get_tree().process_frame


func test_codescape_drag_is_still_refused_while_entered() -> void:
	var scene := await _make_scene()
	scene._portals.offline_check = func(_id: StringName) -> bool: return false
	scene._shim_pending_start = false
	scene._on_hud_control("volume_enter:codescape")
	_scene_settle(scene)
	assert_eq(scene._portals.entered_id(), &"codescape")
	assert_true(scene.claim_blocks_drag("node", &"codescape", scene.OVERLAY_VOLUME), "the press is claimed: no Rust drag")
	assert_eq(scene.OVERLAY_VOLUME, &"knowledge", "overlay volume unchanged")
	assert_false(BackendCaps.supports(BackendCaps.for_mode(&"codescape"), BackendCaps.DRAG), "Codescape does not serve DRAG")
	assert_false(scene._cs_conn.gate(BackendCaps.DRAG, "drag a node", true), "the gate itself refuses")
	assert_false(scene._cs_conn.send_drag_start(7, Vector3.ZERO), "the connection refuses nodeDragStart")
	assert_false(scene._cs_conn.send_drag_update(7, Vector3.ZERO))
	assert_false(scene._cs_conn.send_drag_end(7))
	scene.queue_free()
	await get_tree().process_frame


func test_scene_grip_is_refused_while_entered() -> void:
	var scene := await _make_scene()
	scene._portals.offline_check = func(_id: StringName) -> bool: return false
	scene._shim_pending_start = false
	scene._on_hud_control("volume_enter:knowledge")
	_scene_settle(scene)
	var ctrl := Node3D.new()
	scene.add_child(ctrl)
	var q: Node3D = scene._volumes[&"ontology"]
	var before := q.transform
	scene.begin_box_grip(ctrl, {"volume": &"ontology", "part": "frame", "handle": -1, "distance": 1.0, "world_pos": q.global_position})
	assert_eq(scene._box_grab_vid, &"", "no grab while entered")
	assert_eq(q.transform, before)
	ctrl.queue_free()
	scene.queue_free()
	await get_tree().process_frame


# --- R10 follow-ups (F2-F7) ----------------------------------------------------

# One frame of everything the scene ticks, in the scene's order.
func _step(p: Node3D, arr: Node3D) -> void:
	arr.tick(DT)
	p.tick(DT)
	for id: StringName in IDS:
		arr.volume(id).tick(DT)


# Steps until `_world_state` first differs from `before`; returns the dim's
# opacity on that frame, or -1.0 when the world never changed.
func _alpha_at_first_change(p: Node3D, arr: Node3D, before: Array, frames := 120) -> float:
	for i: int in frames:
		_step(p, arr)
		if _world_state(arr) != before:
			return p.fade_alpha()
	return -1.0


func _dimmed_scene(reduced: bool) -> Dictionary:
	var s := _scene()
	var cam := Node3D.new()
	add_child_autofree(cam)
	s["p"].attach_fade(cam)
	s["p"].reduced_motion = reduced
	return s


func test_room_loss_ends_the_space_through_the_dim() -> void:
	for reduced: bool in [true, false]:
		for cause: String in ["offline", "hidden", "closed"]:
			var s := _dimmed_scene(reduced)
			var arr: Node3D = s["arr"]
			var p: Node3D = s["p"]
			_enter(p, &"ontology")
			_settle(p, arr, 1.5)
			assert_false(p.is_transitioning())
			match cause:
				"offline":
					_offline[&"ontology"] = true
				"hidden":
					arr.volume(&"ontology").request_visible(false)
				"closed":
					arr.volume(&"ontology").close()
			var before := _world_state(arr)
			var a := _alpha_at_first_change(p, arr, before)
			var why := "room %s, reduced_motion=%s" % [cause, reduced]
			assert_true(a >= 0.999, "the world changes only in full dark (alpha %.3f): %s" % [a, why])
			assert_false(p.is_entered(), why)
			_settle(p, arr, 0.5)
			assert_false(arr.layout_locked, why)
			assert_false(p.is_transitioning(), why)


func test_room_loss_during_the_enter_dim_still_ends_in_the_dark() -> void:
	var s := _dimmed_scene(true)
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	var start: Dictionary = arr.snapshot()
	_enter(p, &"ontology")
	for i: int in 4:
		_step(p, arr)
	assert_true(p.fade_alpha() < 0.999 and p.is_transitioning(), "part-way into the dim")
	var before := _world_state(arr)
	_offline[&"ontology"] = true
	assert_true(_alpha_at_first_change(p, arr, before) >= 0.999 or _world_state(arr) == before)
	_settle(p, arr, 0.6)
	assert_false(p.is_entered())
	assert_false(arr.layout_locked)
	assert_eq(arr.snapshot()["xf"], start["xf"], "the ring is back exactly")


func test_room_loss_during_the_dim_back_restarts_from_the_current_alpha() -> void:
	var s := _dimmed_scene(true)
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	_enter(p, &"ontology")
	var guard := 0
	while p.fade_alpha() < 0.999 and guard < 100:
		_step(p, arr)
		guard += 1
	_step(p, arr)
	_step(p, arr)
	assert_true(p.fade_alpha() < 0.999 and p.fade_alpha() > 0.0, "coming back from dark")
	var a0: float = p.fade_alpha()
	var before := _world_state(arr)
	_offline[&"ontology"] = true
	_step(p, arr)
	assert_true(p.fade_alpha() >= a0 - 0.2, "the dim does not drop to clear: %.3f -> %.3f" % [a0, p.fade_alpha()])
	assert_true(_alpha_at_first_change(p, arr, before) >= 0.999 or _world_state(arr) == before)
	_settle(p, arr, 0.6)
	assert_false(p.is_entered())


func test_hiding_or_showing_a_portal_relayouts_through_the_dim() -> void:
	var s := _dimmed_scene(true)
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	_enter(p, &"ontology")
	_settle(p, arr, 1.0)
	arr.volume(&"knowledge").request_visible(false)
	var a := _alpha_at_first_change(p, arr, _world_state(arr))
	assert_true(a >= 0.999, "hiding a portal re-lays the ring only in the dark (alpha %.3f)" % a)
	_settle(p, arr, 0.5)
	assert_eq(p.portal_ids(), [&"codescape"] as Array[StringName])
	var one: Transform3D = arr.volume(&"codescape").transform
	arr.volume(&"knowledge").request_visible(true)
	a = _alpha_at_first_change(p, arr, _world_state(arr))
	assert_true(a >= 0.999, "showing one re-lays the ring only in the dark (alpha %.3f)" % a)
	_settle(p, arr, 0.5)
	assert_eq(p.portal_ids().size(), 2)
	assert_false(arr.volume(&"codescape").transform.is_equal_approx(one), "the ring was re-laid")
	var room: Node3D = arr.volume(&"ontology")
	for pid: StringName in p.portal_ids():
		var q: Node3D = arr.volume(pid)
		assert_false(p.boxes_intersect(room.transform, room.bounds, q.transform, q.bounds), "%s clear of the room" % pid)


func test_a_second_portal_change_during_the_relayout_dim_is_not_lost() -> void:
	var s := _dimmed_scene(true)
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	_enter(p, &"ontology")
	_settle(p, arr, 1.0)
	arr.volume(&"knowledge").request_visible(false)
	for i: int in 3:
		_step(p, arr)
	arr.volume(&"codescape").request_visible(false)
	_settle(p, arr, 1.0)
	assert_eq(p.portal_ids().size(), 0)
	assert_false(p.membranes.visible)


func test_eased_relayout_when_motion_is_allowed() -> void:
	var s := _dimmed_scene(false)
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	_enter(p, &"ontology")
	_settle(p, arr, 1.0)
	var q: Node3D = arr.volume(&"codescape")
	var from := q.transform.origin
	arr.volume(&"knowledge").request_visible(false)
	_step(p, arr)
	_step(p, arr)
	var early := (q.transform.origin - from).length()
	assert_false(p.is_transitioning(), "no dim when motion is allowed")
	assert_true(arr.is_easing(&"codescape"), "the portal eases")
	_settle(p, arr, 1.0)
	var total := (q.transform.origin - from).length()
	assert_true(total > 0.5, "it did move (%.3f m)" % total)
	assert_true(early < 0.25 * total, "and not in one frame (%.3f of %.3f m)" % [early, total])


func test_leave_and_swap_always_go_through_the_dim_even_with_motion_allowed() -> void:
	# E7 forbids an eased shrink, and leaving/swapping shrinks a room's content.
	var s := _dimmed_scene(false)
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	_enter(p, &"knowledge")
	_settle(p, arr, 1.5)
	var entered := _world_state(arr)
	assert_true(_enter(p, &"ontology", Vector3(-1.0, 1.6, 3.0)))
	assert_true(p.is_transitioning(), "a swap dims")
	assert_false(arr.is_easing(), "no ease machinery")
	var a := _alpha_at_first_change(p, arr, entered)
	assert_true(a >= 0.999, "the swap lands in full dark (alpha %.3f)" % a)
	_settle(p, arr, 1.0)
	entered = _world_state(arr)
	assert_true(p.leave())
	assert_true(p.is_transitioning(), "leaving dims")
	assert_false(arr.is_easing())
	a = _alpha_at_first_change(p, arr, entered)
	assert_true(a >= 0.999, "leaving lands in full dark (alpha %.3f)" % a)
	_settle(p, arr, 1.0)
	assert_false(arr.layout_locked)
	assert_false(p.is_transitioning())


func test_mid_front_ease_enter_and_leave_restore_the_new_slots() -> void:
	var s := _scene()
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	arr.bring_to_front(&"knowledge")
	for i: int in 5:
		arr.tick(DT)
	assert_true(arr.is_easing(), "the Front ease is running")
	_enter(p, &"ontology")
	_settle(p, arr)
	assert_true(p.leave())
	_settle(p, arr)
	for id: StringName in IDS:
		assert_true(arr.volume(id).transform.is_equal_approx(VolumeArrangement.ring_transform(arr.slot_yaw(id))),
			"%s lands on its new slot, not where the ease had got to" % id)


func test_walkthrough_enter_uses_the_floor() -> void:
	var s := _scene()
	var arr: Node3D = s["arr"]
	var p: Node3D = s["p"]
	var args: Array = []
	for m: Dictionary in p.get_script().get_script_method_list():
		if m["name"] == "update_walkthrough":
			args = m["args"]
	assert_eq(args.size(), 4, "update_walkthrough takes the floor height")
	if args.size() < 4:
		return
	p.walkthrough_enabled = true
	var head := Vector3(0.0, 6.6, 0.0)
	var fwd := Vector3(0.0, 0.0, -1.0)
	assert_true(p.enter(&"knowledge", head, fwd, 5.0))
	_settle(p, arr)
	p.update_walkthrough(head, fwd, DT, 5.0)
	var inside: Vector3 = arr.volume(p.portal_ids()[0]).global_position
	var got := &""
	for i: int in 60:
		got = p.update_walkthrough(inside, fwd, DT, 5.0)
		if got != &"":
			break
	assert_ne(got, &"", "the walk-through entered a portal")
	_settle(p, arr)
	var room: Node3D = arr.volume(got)
	assert_almost_eq(room.position.y - room.bounds.y * 0.5, 5.0, 1.0e-3, "room floor on the origin's floor, not world 0")
	for pid: StringName in p.portal_ids():
		assert_almost_eq(arr.volume(pid).position.y, 5.0 + 1.6, 1.0e-3, "portals at eye height above that floor")


func test_scene_grip_is_refused_during_the_leave_dim() -> void:
	var scene := await _make_scene()
	scene._portals.offline_check = func(_id: StringName) -> bool: return false
	scene._shim_pending_start = false
	scene._on_hud_control("volume_enter:knowledge")
	_scene_settle(scene)
	scene._on_hud_control("volume_leave")
	assert_false(scene._portals.is_entered(), "already marked as left")
	assert_true(scene._portals.arrangement_locked() or scene._portals.is_transitioning(), "but the ring is not back yet")
	var ctrl := Node3D.new()
	scene.add_child(ctrl)
	var q: Node3D = scene._volumes[&"ontology"]
	var before := q.transform
	scene.begin_box_grip(ctrl, {"volume": &"ontology", "part": "frame", "handle": -1, "distance": 1.0, "world_pos": q.global_position})
	assert_eq(scene._box_grab_vid, &"", "no grab while the ring is being restored")
	assert_eq(q.transform, before)
	ctrl.queue_free()
	scene.queue_free()
	await get_tree().process_frame


func test_portals_never_meet_each_other_at_the_maximum_count() -> void:
	var P = _portals_script()
	var heading := Vector3(0.0, 0.0, -1.0)
	for edge: float in [1.0, 2.5, 4.0]:
		var n: int = P.MAX_PORTALS
		var sizes: Array = []
		for i: int in n:
			sizes.append(Vector3(edge, edge, edge))
		var room_xf: Transform3D = P.room_transform(Vector3.ZERO, heading, P.ROOM_BOUNDS)
		var xfs: Array[Transform3D] = P.portal_layout(Vector3.ZERO, heading, P.ROOM_BOUNDS, sizes, 1.6)
		assert_eq(xfs.size(), n)
		for i: int in n:
			assert_false(P.boxes_intersect(room_xf, P.ROOM_BOUNDS, xfs[i], sizes[i]), "portal %d clear of the room (%.1f m boxes)" % [i, edge])
			for j: int in range(i + 1, n):
				assert_false(P.boxes_intersect(xfs[i], sizes[i], xfs[j], sizes[j]),
					"portals %d and %d clear of each other (%.1f m boxes, %d portals)" % [i, j, edge, n])


class FakeBudget extends RefCounted:
	var seen: Array = []
	func set_volume_weights(w: Dictionary) -> void:
		seen.append(w.duplicate())
