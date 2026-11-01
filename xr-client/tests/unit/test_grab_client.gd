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
	assert_false(scene._content_xf(&"knowledge").is_equal_approx(scene._content_xf(&"ontology")), "precondition: the two volumes frame differently")
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
	# The room's content is fitted up to room size; the empty headless graph fits at 1,
	# so scale it by hand (no tick runs before the drive below).
	(scene._volumes[&"ontology"].content as Node3D).scale = Vector3.ONE * 4.0
	assert_almost_eq(scene._content_xf(&"ontology").basis.get_scale().x, 4.0, 1e-3, "precondition: a scaled room")
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


# --- no jump on grab: real client, real packed slots ---------------------------------

## Ingests one knowledge node (wire flag 0x4000_0000) and one ontology-class node
## (0x0400_0000) at known positions and packs each volume's own slot, so the
## picker's candidates are the real per-slot render positions. Returns
## {volume: [node id, store position]}.
func _seed_real_nodes(scene: Node3D) -> Dictionary:
	var client: RefCounted = scene._vc_conn.client
	scene._vc_conn._connected = true
	var b := PackedByteArray()
	b.append(0x03)
	var spec := [[0x40000000 | 5, Vector3(3.0, 2.0, -1.5)], [0x04000000 | 7, Vector3(-2.0, 1.0, 4.0)]]
	for sp: Array in spec:
		var rec := PackedByteArray()
		rec.resize(52)
		rec.encode_u32(0, int(sp[0]))
		var p: Vector3 = sp[1]
		rec.encode_float(4, p.x)
		rec.encode_float(8, p.y)
		rec.encode_float(12, p.z)
		b.append_array(rec)
	client.ingest(b)
	var out := {}
	for vid: StringName in [&"knowledge", &"ontology"]:
		var src = scene._sources[vid]
		src.invalidate()  # the first frame cached an empty id list
		var ids: PackedInt32Array = src.content_ids()
		client.set_pack_slot(src.slot)
		client.build_node_buffer_lod(ids, 1.0, 0.7, 1.9, Vector3.ZERO, 8, 100.0)
		out[vid] = [ids[0] if ids.size() > 0 else -1, client.node_position(ids[0]) if ids.size() > 0 else Vector3.ZERO]
	return out


## Grabs the volume's node from a hand 1 m straight behind it along -Z and checks
## the first drag target is the node's own store position: no jump, whatever the
## volume's Content transform.
func _assert_no_jump(scene: Node3D, vid: StringName, hand: int, seeded: Dictionary) -> void:
	var node_id: int = seeded[vid][0]
	assert_ne(node_id, -1, "%s: the class mask routed a node here" % vid)
	var cand: Array = scene._grab_candidates(vid)
	assert_eq((cand[0] as PackedInt32Array).size(), 1, "%s: one drawn candidate" % vid)
	if (cand[0] as PackedInt32Array).size() != 1:
		return
	var world: Vector3 = (cand[1] as PackedVector3Array)[0]
	var origin: Vector3 = world + Vector3(0, 0, 1)
	assert_true(scene._begin_grab(hand, vid, node_id, origin), "%s: grab begins" % vid)
	assert_almost_eq(float(scene._grab_dist[hand]), 1.0, 1e-3, "%s: depth is the hand-to-node distance" % vid)
	scene._drive_grab(hand, origin, Vector3(0, 0, -1))
	var want: Vector3 = seeded[vid][1]
	assert_true((scene._grab_target[hand] as Vector3).is_equal_approx(want), "%s: first target %s is the node's position %s" % [vid, scene._grab_target[hand], want])


func test_no_jump_on_grab_in_the_ring_for_both_volumes_and_hands() -> void:
	var scene: Node3D = await _make_scene()
	var seeded := _seed_real_nodes(scene)
	assert_ne(seeded[&"knowledge"][0], seeded[&"ontology"][0], "disjoint class masks: two different nodes")
	_assert_no_jump(scene, &"knowledge", 0, seeded)
	_assert_no_jump(scene, &"ontology", 1, seeded)
	assert_ne(scene._any_grab_id(), -1)
	scene._release_grab()
	await _free(scene)


func test_no_jump_on_grab_inside_a_scaled_entered_room() -> void:
	var scene: Node3D = await _make_scene()
	_settle(scene)
	var seeded := _seed_real_nodes(scene)
	scene._on_hud_control("volume_enter:ontology")
	_tick(scene, 0.6)
	assert_eq(scene._portals.entered_id(), &"ontology")
	(scene._volumes[&"ontology"].content as Node3D).scale = Vector3.ONE * 4.0
	_assert_no_jump(scene, &"ontology", 0, seeded)
	assert_false(scene._begin_grab(1, &"knowledge", seeded[&"knowledge"][0], Vector3.ZERO), "the knowledge portal never grabs")
	scene._release_grab()
	await _free(scene)


# --- overlays placed through GraphRoot see only knowledge-held nodes -----------------

func test_label_pool_and_focus_marker_follow_the_volume_of_the_node() -> void:
	var scene: Node3D = await _make_scene()
	_fake(scene)
	scene._begin_grab(0, &"knowledge", 11, Vector3.ZERO)
	scene._begin_grab(1, &"ontology", 22, Vector3.ZERO)
	assert_eq(scene._held_ids(), [11, 22])
	assert_eq(scene._held_ids(scene.OVERLAY_VOLUME), [11], "an ontology-held node is not a knowledge label")
	assert_true(scene.focus_frame_for(22).is_equal_approx(scene._content_xf(&"ontology")), "held ontology node: its own frame")
	assert_true(scene.focus_frame_for(11).is_equal_approx(scene._content_xf(&"knowledge")))
	scene._release_grab()
	# A hover target is placed in the volume the ray targeted it in.
	scene._grab_eval_hand = 1
	scene._grab_eval_vol = &"knowledge"
	scene._grab_eval_map = {33: &"ontology"}
	scene._on_node_targeted(33, 2.0)
	scene._grab_eval_hand = -1
	scene._grab_eval_map = {}
	assert_eq(scene._last_targeted_vol, &"ontology")
	assert_true(scene.focus_frame_for(33).is_equal_approx(scene._content_xf(&"ontology")), "hover ring lands in the ontology box")
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


# ================================================================================
# ADR-2168 review fixes (R15 → F13). Each test drives the scene through instance
# members only, so a missing new member fails an assertion, never a file-level
# parse error that GUT would skip silently.
# ================================================================================

const VolumePicker := preload("res://scripts/volume_picker.gd")


## Ingests `know` knowledge nodes (ids 1..know) and `onto` ontology-class nodes
## (ids 10001..) on a deterministic lattice, then packs each volume's own slot.
## Returns {volume: [first node id, packed candidate count]}.
func _seed_many(scene: Node3D, know: int, onto: int, spread: float = 1.0) -> Dictionary:
	var client: RefCounted = scene._vc_conn.client
	scene._vc_conn._connected = true
	var b := PackedByteArray()
	b.append(0x03)
	var total: int = know + onto
	var recs := PackedByteArray()
	recs.resize(52 * total)
	for n: int in total:
		var is_know: bool = n < know
		var id: int = n + 1 if is_know else 10001 + (n - know)
		var flag: int = 0x40000000 if is_know else 0x04000000
		var o: int = 52 * n
		recs.encode_u32(o, flag | id)
		recs.encode_float(o + 4, float((n % 23) - 11) * spread)
		recs.encode_float(o + 8, float((n / 23) % 17 - 8) * spread)
		recs.encode_float(o + 12, float((n / 391) % 11 - 5) * spread)
	b.append_array(recs)
	client.ingest(b)
	var out := {}
	for vid: StringName in [&"knowledge", &"ontology"]:
		var src = scene._sources[vid]
		src.invalidate()
		var ids: PackedInt32Array = src.content_ids()
		client.set_pack_slot(src.slot)
		client.build_node_buffer_lod(ids, 1.0, 0.7, 1.9, Vector3.ZERO, 8, 100.0)
		out[vid] = [ids[0] if ids.size() > 0 else -1, (client.get_render_ids_for_slot(src.slot) as PackedInt32Array).size()]
	return out


# --- F1: a held node must not refit the volume it is held in -------------------------

func test_a_held_node_beyond_the_face_does_not_run_away_through_the_refit() -> void:
	var scene: Node3D = await _make_scene()
	_settle(scene)
	var client: RefCounted = scene._vc_conn.client
	_seed_many(scene, 40, 0, 2.0)
	var v: Node3D = scene._volumes[&"knowledge"]
	var dt := 1.0 / 90.0
	for i: int in 12:
		scene._tick_volumes(dt)
	var nid: int = 5
	var node_world: Vector3 = scene._content_xf(&"knowledge") * client.node_position(nid)
	assert_true(scene._begin_grab(0, &"knowledge", nid, node_world + Vector3(0, 0, 1)), "grab begins")
	# The hand rests 0.4 m beyond the +X face of the 1 m box and does not move.
	var dir := Vector3(0, 0, -1)
	var face: Vector3 = v.global_transform * Vector3(v.bounds.x * 0.5 + 0.4, 0.0, 0.0)
	var origin: Vector3 = face - dir * float(scene._grab_dist[0])
	var base_t := 0.0
	var base_s := 0.0
	var max_t := 0.0
	for f: int in 102:
		scene._drive_grab(0, origin, dir)
		client.hunt_grabs(scene.chase_ease(dt), PackedInt32Array([nid]), PackedVector3Array([scene._grab_target[0]]))
		scene._tick_volumes(dt)
		if f == 11:
			base_t = (scene._grab_target[0] as Vector3).length()
			base_s = v.target_scale()
		elif f > 11:
			max_t = maxf(max_t, (scene._grab_target[0] as Vector3).length())
	assert_gt(base_t, 0.0)
	assert_lt(max_t, base_t * 1.5, "the drag target stays bounded over 90 frames (base %.1f, max %.1f)" % [base_t, max_t])
	assert_gt(v.target_scale(), base_s * 0.9, "the content scale stays stable (%.4f vs %.4f)" % [v.target_scale(), base_s])
	scene._release_grab()
	await _free(scene)


func test_held_ids_are_excluded_from_their_own_volumes_refit() -> void:
	var scene: Node3D = await _make_scene()
	_fake(scene)
	var know = scene._sources[&"knowledge"]
	var onto = scene._sources[&"ontology"]
	assert_true("grabbed_ids" in know, "ConnectionVolumeSource carries the exclusion set")
	if not ("grabbed_ids" in know):
		await _free(scene)
		return
	scene._begin_grab(0, &"knowledge", 11, Vector3.ZERO)
	scene._begin_grab(1, &"knowledge", 12, Vector3.ZERO)
	assert_eq(know.grabbed_ids.size(), 2, "both hands' nodes leave the knowledge fit")
	assert_true(know.grabbed_ids.has(11) and know.grabbed_ids.has(12))
	assert_eq(onto.grabbed_ids.size(), 0, "the other volume's fit is untouched")
	scene._end_grab(0)
	assert_eq(know.grabbed_ids, PackedInt32Array([12]), "releasing one hand keeps the other node excluded")
	scene._end_grab(1)
	assert_eq(know.grabbed_ids.size(), 0, "nothing held: the whole extent again")
	scene._begin_grab(0, &"ontology", 22, Vector3.ZERO)
	assert_eq(onto.grabbed_ids, PackedInt32Array([22]))
	assert_eq(know.grabbed_ids.size(), 0)
	scene._release_grab()
	assert_eq(onto.grabbed_ids.size(), 0, "a forced release clears it too")
	await _free(scene)


# --- F2: the unclaimed-press candidate merge ------------------------------------------

func test_candidate_merge_resolves_each_nodes_volume_and_skips_held_ids() -> void:
	var scene: Node3D = await _make_scene()
	_settle(scene)
	var seeded := _seed_many(scene, 3, 2)
	var kid: int = seeded[&"knowledge"][0]
	var oid: int = seeded[&"ontology"][0]
	assert_true(scene.has_method("_merge_grab_candidates") and scene.has_method("_grab_volume_of"), "the merge is its own function")
	if not scene.has_method("_merge_grab_candidates"):
		await _free(scene)
		return
	var vols: Array[StringName] = [&"knowledge", &"ontology"]
	var m: Array = scene._merge_grab_candidates(vols, -1, -1)
	var ids: PackedInt32Array = m[0]
	var world: PackedVector3Array = m[1]
	assert_eq(ids.size(), 5, "every drawn candidate of both volumes")
	assert_eq(world.size(), ids.size())
	for i: int in ids.size():
		var want_vol: StringName = &"knowledge" if ids[i] <= 3 else &"ontology"
		assert_eq(scene._grab_volume_of(ids[i]), want_vol, "node %d resolves to %s" % [ids[i], want_vol])
		var local: Vector3 = scene._vc_conn.client.node_position(ids[i])
		var xf: Transform3D = scene._volumes[want_vol].world_from_content(Vector3.ZERO) * Vector3.ZERO
		assert_true(world[i].is_equal_approx(scene._volumes[want_vol].world_from_content(local)), "node %d: world position through its own Content transform" % ids[i])
		assert_not_null(xf)
	assert_eq(scene._grab_volume_of(kid), &"knowledge")
	assert_eq(scene._grab_volume_of(oid), &"ontology")
	# M2b: a node the other hand holds never reaches the Rust tube, so it cannot shadow a free neighbour.
	m = scene._merge_grab_candidates(vols, kid, oid)
	ids = m[0]
	assert_eq(ids.size(), 3)
	assert_false(ids.has(kid) or ids.has(oid), "both held ids are filtered out")
	assert_eq((m[1] as PackedVector3Array).size(), 3, "positions stay aligned with ids")
	m = scene._merge_grab_candidates(vols, -1, oid)
	assert_false((m[0] as PackedInt32Array).has(oid))
	assert_true((m[0] as PackedInt32Array).has(kid))
	assert_eq(scene._grab_volume_of(kid), &"knowledge", "offsets follow the filtered array")
	await _free(scene)


func _legacy_merge(scene: Node3D, vols: Array[StringName]) -> int:
	# The merge as shipped at b36fa0b3a, verbatim in shape: a method call per node into
	# world_from_content, an _is_held call, two appends and a Dictionary write.
	var ids := PackedInt32Array()
	var wp := PackedVector3Array()
	var map := {}
	for vid: StringName in vols:
		var v: Node3D = scene._volumes[vid]
		var cand: Array = VolumePicker.candidates(scene._sources[vid])
		var cids: PackedInt32Array = cand[0]
		var pos: PackedVector3Array = cand[1]
		var world := PackedVector3Array()
		world.resize(pos.size())
		for i: int in pos.size():
			world[i] = v.world_from_content(pos[i])
		for i: int in mini(cids.size(), world.size()):
			if scene._is_held(cids[i]):
				continue
			ids.append(cids[i])
			wp.append(world[i])
			map[cids[i]] = vid
	return ids.size()


func test_candidate_merge_costs_well_under_a_frame_at_13k_nodes() -> void:
	var scene: Node3D = await _make_scene()
	_settle(scene)
	var seeded := _seed_many(scene, 9000, 4000, 0.05)
	var n: int = int(seeded[&"knowledge"][1]) + int(seeded[&"ontology"][1])
	assert_gt(n, 10000, "a 13k-node population is packed (%d candidates)" % n)
	var vols: Array[StringName] = [&"knowledge", &"ontology"]
	var legacy_ms := 0.0
	for r: int in 35:
		var t0 := Time.get_ticks_usec()
		_legacy_merge(scene, vols)
		if r >= 5:
			legacy_ms += float(Time.get_ticks_usec() - t0) / 1000.0
	legacy_ms /= 30.0
	assert_true(scene.has_method("_merge_grab_candidates"), "the merge is its own function")
	if not scene.has_method("_merge_grab_candidates"):
		print("F2 BENCH legacy merge: %.3f ms per hand per frame at %d candidates" % [legacy_ms, n])
		await _free(scene)
		return
	var new_ms := 0.0
	var got := 0
	for r: int in 35:
		var t0 := Time.get_ticks_usec()
		var m: Array = scene._merge_grab_candidates(vols, 3, 10001)
		if r >= 5:
			new_ms += float(Time.get_ticks_usec() - t0) / 1000.0
		got = (m[0] as PackedInt32Array).size()
	new_ms /= 30.0
	print("F2 BENCH merge at %d candidates: legacy %.3f ms, new %.3f ms per hand per frame" % [n, legacy_ms, new_ms])
	assert_eq(got, n - 2, "the two held ids are the only ones dropped")
	# Loose on purpose: the HP is shared. R15 measured the hoisted loop at 0.49 ms; the legacy one at 8.1 ms.
	assert_lt(new_ms, 2.5, "two hands must fit the 11.1 ms frame budget with room to spare")
	await _free(scene)


# --- F4: no grab while an arrangement ease is in flight -----------------------------------

func test_no_grab_starts_during_the_first_room_entry_ease() -> void:
	var scene: Node3D = await _make_scene()
	_settle(scene)
	scene._portals.reduced_motion = false
	_fake(scene)
	scene._on_hud_control("volume_enter:ontology")
	_tick(scene, 0.1)
	assert_eq(scene._portals.entered_id(), &"ontology")
	assert_false(scene._portals.is_transitioning(), "no dim: the first entry only eases")
	assert_true(scene._arrangement.is_easing(), "precondition: the arrangement is mid-ease")
	assert_false(scene._begin_grab(0, &"ontology", 22, Vector3(0, 1.4, 0)), "an easing Content transform is a locked one")
	assert_eq(scene._grab_id[0], -1)
	_tick(scene, 0.6)
	assert_false(scene._arrangement.is_easing(), "the ease is done")
	assert_true(scene._begin_grab(0, &"ontology", 22, Vector3(0, 1.4, 0)), "positive control: the same grab begins once it settles")
	scene._release_grab()
	await _free(scene)


# --- F8: a link press with a node held must not move volumes ---------------------------------

func test_link_press_while_a_node_is_held_moves_no_volume() -> void:
	var scene: Node3D = await _make_scene()
	_fake(scene)
	assert_true(scene._begin_grab(0, &"knowledge", 11, Vector3.ZERO))
	var front_before: StringName = scene._arrangement.occupant(0.0)
	var xf_before: Transform3D = scene._content_xf(&"knowledge")
	var link := {
		"record": {"kind": "bridge"},
		"from": {"volume": &"knowledge", "conn": &"visionclaw", "node_id": 11},
		"to": {"volume": &"ontology", "conn": &"visionclaw", "node_id": 22},
		"t": 0.1, "distance": 0.8, "world_pos": Vector3.ZERO,
	}
	# The free hand follows a link whose far end sits in the box that is not at the front.
	if front_before == &"ontology":
		link["from"] = {"volume": &"ontology", "conn": &"visionclaw", "node_id": 22}
		link["to"] = {"volume": &"knowledge", "conn": &"visionclaw", "node_id": 11}
	scene.apply_trigger("link", {}, link, {})
	assert_false(scene._arrangement.is_easing(), "no box eases while a node is in hand")
	assert_eq(scene._arrangement.occupant(0.0), front_before, "the front slot is unchanged")
	_tick(scene, 0.5)
	assert_true(scene._content_xf(&"knowledge").is_equal_approx(xf_before), "the held node's frame did not move")
	scene._release_grab()
	# Positive control: the same press with both hands free brings the far box to the front.
	scene.apply_trigger("link", {}, link, {})
	assert_true(scene._arrangement.is_easing(), "free hands: the link press still follows the link")
	await _free(scene)


# --- F6: double-click state is per hand --------------------------------------------------------

func _press(scene: Node3D, hand: int, node_id: int) -> void:
	scene._grab_eval_hand = hand
	scene._grab_eval_vol = &"knowledge"
	scene._on_node_grabbed(node_id, Vector3.ZERO)
	scene._grab_eval_hand = -1
	scene._grab_eval_vol = &""


func test_interleaved_presses_do_not_defeat_a_per_hand_double_click() -> void:
	var scene: Node3D = await _make_scene()
	_fake(scene)
	_press(scene, 0, 11)
	assert_eq(scene._grab_id[0], 11, "first press grabs")
	scene._end_grab(0)
	_press(scene, 1, 22)
	assert_eq(scene._grab_id[1], 22)
	scene._end_grab(1)
	_press(scene, 0, 11)
	assert_eq(scene._grab_id[0], -1, "left A, right B, left A: the second left press is a double-click, not a grab")
	await _free(scene)


func test_handing_a_node_to_the_other_hand_grabs_it_and_does_not_double_click() -> void:
	var scene: Node3D = await _make_scene()
	_fake(scene)
	_press(scene, 0, 11)
	scene._end_grab(0)
	_press(scene, 1, 11)
	assert_eq(scene._grab_id[1], 11, "a different hand pressing the same node is a hand-off")
	scene._release_grab()
	await _free(scene)


# --- F7: the hand-local offset ----------------------------------------------------------------------

func test_an_off_axis_catch_does_not_snap_the_node() -> void:
	var scene: Node3D = await _make_scene()
	var seeded := _seed_real_nodes(scene)
	for case_basis: Basis in [Basis(), Basis(Vector3.UP, deg_to_rad(35.0)) * Basis(Vector3.RIGHT, deg_to_rad(-20.0))]:
		var vid: StringName = &"knowledge"
		var node_id: int = seeded[vid][0]
		var cand: Array = scene._grab_candidates(vid)
		var node_world: Vector3 = (cand[1] as PackedVector3Array)[0]
		# The tube catches within 5 cm; the hand sits 3 cm off the node's axis in its own frame.
		var aim: Vector3 = -case_basis.z
		var origin: Vector3 = node_world - aim * 1.0 + case_basis.x * 0.03
		assert_true(scene._begin_grab(0, vid, node_id, origin, case_basis), "off-axis grab begins")
		scene._drive_grab(0, origin, aim, case_basis)
		var want: Vector3 = seeded[vid][1]
		assert_true((scene._grab_target[0] as Vector3).is_equal_approx(want), "first target %s is the node's own position %s, not 3 cm aside" % [scene._grab_target[0], want])
		# The hand then moves 0.5 m right without turning: the node moves with it, rigidly.
		scene._drive_grab(0, origin + Vector3(0.5, 0, 0), aim, case_basis)
		var moved: Vector3 = scene._content_xf(vid) * (scene._grab_target[0] as Vector3)
		assert_true(moved.is_equal_approx(node_world + Vector3(0.5, 0, 0)), "the node rides the hand with its catch offset: %s" % moved)
		scene._release_grab()
	await _free(scene)


func test_the_shader_comment_states_the_true_trough() -> void:
	var t := _inc_text()
	assert_false(t.contains("dim to a fifth"), "the old claim: a fifth of peak is wrong under blend_mix")
	assert_true(t.contains("0.04"), "(1 - 0.8)^2: albedo and alpha both fall to a fifth, so the trough is ~0.04 of peak")
