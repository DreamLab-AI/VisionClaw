extends "res://addons/gut/test.gd"
const GraphScene := preload("res://scripts/graph_scene.gd")
const BackendCaps := preload("res://scripts/backend_caps.gd")

# Two-hand node grab in any VisionClaw volume (ADR-2166 amendment, ADR-2158 E9).
# The scene tests drive the real GraphScene headless (no XR, no backend) through
# its grab entry points (_begin_grab / _drive_grab / _end_grab), which
# _update_interaction calls per hand with a controller's pose; a fake client on
# the VisionClaw connection records every drag frame.


class FakeClient:
	extends RefCounted
	var calls: Array = []
	var pos: Dictionary = {}

	func send_drag_start(id: int, p: Vector3) -> void:
		calls.append(["start", id, p])

	func send_drag_update(id: int, p: Vector3) -> void:
		calls.append(["update", id, p])

	func send_drag_end(id: int) -> void:
		calls.append(["end", id])

	func send_node_unpin(id: int) -> void:
		calls.append(["unpin", id])

	func node_position(id: int) -> Vector3:
		return pos.get(id, Vector3.ZERO)

	func label_of(_id: int) -> String:
		return ""

	func of_kind(kind: String) -> Array:
		var out: Array = []
		for c: Array in calls:
			if c[0] == kind:
				out.append(c)
		return out


func _make_scene() -> Node3D:
	var packed: PackedScene = load("res://scenes/GraphScene.tscn")
	var scene: Node3D = packed.instantiate()
	add_child(scene)
	await get_tree().process_frame
	return scene


func _free(scene: Node3D) -> void:
	scene.queue_free()
	await get_tree().process_frame


func _settle(scene: Node3D) -> void:
	scene._portals.offline_check = func(_id: StringName) -> bool: return false
	scene._shim_pending_start = false


func _tick(scene: Node3D, seconds: float) -> void:
	var t := 0.0
	while t < seconds:
		scene._arrangement.tick(0.016)
		scene._portals.tick(0.016)
		t += 0.016


## Installs a recording client on the VisionClaw connection; returns it.
func _fake(scene: Node3D) -> FakeClient:
	var f := FakeClient.new()
	scene._vc_conn.client = f
	return f


# --- the gate ------------------------------------------------------------------

func test_claim_blocks_drag_allows_ontology_and_knowledge_nodes() -> void:
	assert_false(GraphScene.claim_blocks_drag("node", &"ontology", &"knowledge"), "an ontology node may drag")
	assert_false(GraphScene.claim_blocks_drag("node", &"knowledge", &"knowledge"), "a knowledge node may drag")
	assert_false(GraphScene.claim_blocks_drag("", &"", &"knowledge"), "no press: the Rust drag decides")
	assert_false(GraphScene.claim_blocks_drag("frame", &"ontology", &"knowledge"), "the ontology's own tube may still catch a node")


func test_claim_blocks_drag_still_blocks_link_frame_and_codescape() -> void:
	assert_true(GraphScene.claim_blocks_drag("link", &"", &"knowledge"), "a link press never drags")
	assert_true(GraphScene.claim_blocks_drag("node", &"codescape", &"knowledge"), "E9: codescape has no DRAG cap")
	assert_true(GraphScene.claim_blocks_drag("frame", &"codescape", &"knowledge"), "a codescape frame press never drags")
	assert_false(BackendCaps.supports(BackendCaps.for_mode(&"codescape"), BackendCaps.DRAG))
	assert_true(BackendCaps.supports(BackendCaps.for_mode(&"visionclaw"), BackendCaps.DRAG))


func test_grab_volume_allowed_only_in_the_entered_space_or_the_ring() -> void:
	assert_true(GraphScene.grab_volume_allowed(&"ontology", &""), "ring view: any volume")
	assert_true(GraphScene.grab_volume_allowed(&"knowledge", &""))
	assert_true(GraphScene.grab_volume_allowed(&"ontology", &"ontology"), "the entered room")
	assert_false(GraphScene.grab_volume_allowed(&"knowledge", &"ontology"), "a portal never grabs")
	assert_false(GraphScene.grab_volume_allowed(&"codescape", &"ontology"))


# --- two independent per-hand grabs ----------------------------------------------

func test_two_hands_hold_two_nodes_independently() -> void:
	var scene: Node3D = await _make_scene()
	var f := _fake(scene)
	assert_true(scene._begin_grab(0, &"knowledge", 11, Vector3(-0.2, 1.2, 0.0)))
	assert_true(scene._begin_grab(1, &"ontology", 22, Vector3(0.2, 1.2, 0.0)))
	assert_eq(scene._grab_id[0], 11)
	assert_eq(scene._grab_id[1], 22)
	assert_eq(scene._grab_vol[0], &"knowledge")
	assert_eq(scene._grab_vol[1], &"ontology")
	assert_eq(f.of_kind("start").size(), 2, "one nodeDragStart per hand")
	assert_eq(f.of_kind("start")[0][1], 11)
	assert_eq(f.of_kind("start")[1][1], 22)
	scene._end_grab(0)
	scene._end_grab(1)
	await _free(scene)


func test_releasing_one_hand_keeps_the_other_held() -> void:
	var scene: Node3D = await _make_scene()
	var f := _fake(scene)
	scene._begin_grab(0, &"knowledge", 11, Vector3(-0.2, 1.2, 0.0))
	scene._begin_grab(1, &"ontology", 22, Vector3(0.2, 1.2, 0.0))
	f.calls.clear()
	scene._end_grab(0)
	assert_eq(scene._grab_id[0], -1)
	assert_eq(scene._grab_id[1], 22, "the other hand still holds its node")
	assert_eq(f.of_kind("end").size(), 1)
	assert_eq(f.of_kind("end")[0][1], 11, "only the released node is ended")
	scene._drive_grab(1, Vector3(0.2, 1.2, 0.0), Vector3(0, 0, -1))
	assert_eq(f.of_kind("update").size(), 1, "the held hand keeps streaming")
	assert_eq(f.of_kind("update")[0][1], 22)
	scene._end_grab(1)
	assert_eq(scene._any_grab_id(), -1)
	await _free(scene)


func test_a_node_held_by_one_hand_is_refused_to_the_other() -> void:
	var scene: Node3D = await _make_scene()
	var f := _fake(scene)
	assert_true(scene._begin_grab(0, &"knowledge", 11, Vector3.ZERO))
	assert_false(scene._begin_grab(1, &"knowledge", 11, Vector3.ZERO), "one node, one hand")
	assert_eq(f.of_kind("start").size(), 1)
	scene._end_grab(0)
	await _free(scene)


func test_hunt_echo_pins_both_held_nodes() -> void:
	var scene: Node3D = await _make_scene()
	var client: RefCounted = scene._binary_client
	assert_true(client.has_method("hunt_grabs"), "the gdext must carry hunt_grabs (rebuild it)")
	if not client.has_method("hunt_grabs"):
		await _free(scene)
		return
	var f := PackedByteArray()
	f.resize(1 + 52 * 2)
	f[0] = 0x03
	for i: int in 2:
		f.encode_u32(1 + 52 * i, (i + 1) | 0x40000000)
	client.ingest(f)
	client.hunt_grabs(1.0, PackedInt32Array([1, 2]), PackedVector3Array([Vector3(5, 0, 0), Vector3(0, 7, 0)]))
	assert_eq(client.node_position(1), Vector3(5, 0, 0))
	assert_eq(client.node_position(2), Vector3(0, 7, 0))
	await _free(scene)


# --- routing and transforms --------------------------------------------------------

func test_drag_frames_use_each_volumes_own_content_transform() -> void:
	var scene: Node3D = await _make_scene()
	var f := _fake(scene)
	var origin := Vector3(0.0, 1.4, 0.0)
	var dir := Vector3(0, 0, -1)
	scene._begin_grab(0, &"knowledge", 11, origin)
	scene._begin_grab(1, &"ontology", 22, origin)
	f.calls.clear()
	scene._drive_grab(0, origin, dir)
	scene._drive_grab(1, origin, dir)
	var ups: Array = f.of_kind("update")
	assert_eq(ups.size(), 2)
	for hand: int in 2:
		var vid: StringName = [&"knowledge", &"ontology"][hand]
		var xf: Transform3D = scene._content_xf(vid)
		var world: Vector3 = origin + dir * float(scene._grab_dist[hand])
		var want: Vector3 = xf.affine_inverse() * world
		assert_true((ups[hand][2] as Vector3).is_equal_approx(want), "%s: %s vs %s" % [vid, ups[hand][2], want])
	scene._end_grab(0)
	scene._end_grab(1)
	await _free(scene)


func test_codescape_grab_is_refused_and_sends_nothing() -> void:
	var scene: Node3D = await _make_scene()
	var f := _fake(scene)
	assert_false(scene._begin_grab(0, &"codescape", 5, Vector3.ZERO))
	assert_eq(scene._grab_id[0], -1)
	assert_eq(f.calls.size(), 0)
	await _free(scene)


func test_entered_room_grabs_in_the_room_and_refuses_a_portal() -> void:
	var scene: Node3D = await _make_scene()
	_settle(scene)
	scene._on_hud_control("volume_enter:ontology")
	_tick(scene, 0.6)
	assert_eq(scene._portals.entered_id(), &"ontology")
	var f := _fake(scene)
	var origin := Vector3(0.0, 1.4, 0.0)
	assert_true(scene._begin_grab(0, &"ontology", 22, origin), "grab inside the entered room")
	assert_false(scene._begin_grab(1, &"knowledge", 11, origin), "pointing into a portal never grabs")
	assert_eq(scene._grab_id[1], -1)
	assert_eq(f.of_kind("start").size(), 1)
	f.calls.clear()
	scene._drive_grab(0, origin, Vector3(0, 0, -1))
	var xf: Transform3D = scene._content_xf(&"ontology")
	var want: Vector3 = xf.affine_inverse() * (origin + Vector3(0, 0, -1) * float(scene._grab_dist[0]))
	assert_true((f.of_kind("update")[0][2] as Vector3).is_equal_approx(want), "room content scale is in the conversion")
	scene._end_grab(0)
	await _free(scene)


func test_entering_a_room_releases_grabs_held_by_other_volumes() -> void:
	var scene: Node3D = await _make_scene()
	_settle(scene)
	var f := _fake(scene)
	scene._begin_grab(0, &"knowledge", 11, Vector3.ZERO)
	scene._on_hud_control("volume_enter:ontology")
	assert_eq(scene._grab_id[0], -1, "the held volume became a portal: released")
	assert_eq(f.of_kind("end").size(), 1)
	await _free(scene)


# --- the two-hand graph gesture stays on its own binding ----------------------------

func test_two_trigger_grabs_do_not_start_the_two_hand_gesture() -> void:
	var scene: Node3D = await _make_scene()
	_fake(scene)
	assert_true(GraphScene.two_hand_eligible(true, true, -1, false, false), "idle hands: the gesture may run")
	scene._begin_grab(0, &"knowledge", 11, Vector3.ZERO)
	scene._begin_grab(1, &"ontology", 22, Vector3.ZERO)
	assert_ne(scene._any_grab_id(), -1)
	assert_false(GraphScene.two_hand_eligible(true, true, scene._any_grab_id(), false, false), "a node grab owns a hand")
	scene._update_two_hand_manip(0.016)
	assert_false(scene._two_hand_active)
	scene._end_grab(0)
	assert_ne(scene._any_grab_id(), -1, "one hand still holds a node: still ineligible")
	scene._end_grab(1)
	assert_eq(scene._any_grab_id(), -1)
	await _free(scene)


# --- release rejoins the physics ------------------------------------------------------

func test_release_no_longer_records_a_pin() -> void:
	var scene: Node3D = await _make_scene()
	var f := _fake(scene)
	scene._begin_grab(0, &"knowledge", 11, Vector3.ZERO)
	scene._end_grab(0)
	assert_true(scene._pinned_ids.is_empty(), "the server unpins on release: nothing for Unpin All")
	assert_eq(f.of_kind("unpin").size(), 0)
	await _free(scene)


# --- the edge breathe ----------------------------------------------------------------

func _inc_text() -> String:
	return FileAccess.get_file_as_string("res://materials/edge_flow_common.gdshaderinc")


func test_breathe_is_36_seconds_and_deep() -> void:
	var t := _inc_text()
	assert_true(t.contains("uniform float breathe_period = 36.0;"), "36 s divides Godot's 3600 s TIME rollover")
	assert_true(t.contains("uniform float breathe_depth = 0.8;"))
	var mat := load("res://materials/edge_flow.tres") as ShaderMaterial
	assert_almost_eq(float(mat.get_shader_parameter("breathe_period")), 36.0, 1e-6)
	assert_almost_eq(float(mat.get_shader_parameter("breathe_depth")), 0.8, 1e-6)
	assert_eq(3600 % 36, 0)


func test_breathe_scales_brightness_with_the_alpha_swell_and_is_not_motion_gated() -> void:
	var t := _inc_text()
	assert_true(t.contains("albedo *= 1.0 - clamp(breathe_depth, 0.0, 1.0) * swell;"), "albedo swells with the alpha")
	assert_true(t.contains("alpha *= 1.0 - clamp(breathe_depth, 0.0, 1.0) * swell;"))
	var sw := t.find("float swell")
	assert_gt(sw, 0)
	assert_false(t.substr(sw, 400).contains("reduced"), "whole-edge breathe stays on under reduced motion")
