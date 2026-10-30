extends "res://addons/gut/test.gd"
const GraphScene := preload("res://scripts/graph_scene.gd")
const VolumeArrangement := preload("res://scripts/volume_arrangement.gd")

# Spatial-volume UX (UX contract features 1-5, ADR-2158 implementation note):
# trigger and grip precedence, select/focus/highlight routing, follow-a-link,
# box move and resize, Front and Reset Layout. The precedence statics take the
# pickers' own result shapes, so fake hits here are what the real pickers
# return; the scene tests drive the real GraphScene headless (no XR, no
# backend), as test_backend_switch.gd does.


func _make_scene() -> Node3D:
	var packed: PackedScene = load("res://scenes/GraphScene.tscn")
	var scene: Node3D = packed.instantiate()
	add_child(scene)
	await get_tree().process_frame
	return scene


func _free(scene: Node3D) -> void:
	scene.queue_free()
	await get_tree().process_frame


func _node_hit(vid: StringName, conn: StringName, id: int, d: float) -> Dictionary:
	return {"volume": vid, "conn": conn, "node_id": id, "world_pos": Vector3.ZERO, "distance": d}


func _frame_hit(vid: StringName, part: String, handle: int, d: float, p := Vector3.ZERO) -> Dictionary:
	return {"volume": vid, "part": part, "handle": handle, "distance": d, "world_pos": p}


func _link_hit(t: float, d: float) -> Dictionary:
	return {
		"record": {"kind": "bridge"},
		"from": {"volume": &"knowledge", "conn": &"visionclaw", "node_id": 11},
		"to": {"volume": &"ontology", "conn": &"visionclaw", "node_id": 22},
		"t": t, "distance": d, "world_pos": Vector3.ZERO,
	}


# --- precedence (pure) ---------------------------------------------------------

func test_trigger_precedence() -> void:
	var node := _node_hit(&"ontology", &"visionclaw", 5, 1.2)
	var frame := _frame_hit(&"ontology", "frame", -1, 1.0)
	assert_eq(GraphScene.resolve_trigger(true, _link_hit(0.2, 0.5), node, frame), "hud", "HUD/menu wins outright")
	assert_eq(GraphScene.resolve_trigger(false, _link_hit(0.2, 0.5), node, frame), "link", "a nearer link beats the node")
	assert_eq(GraphScene.resolve_trigger(false, _link_hit(0.2, 1.5), node, frame), "node", "a farther link loses to the node")
	assert_eq(GraphScene.resolve_trigger(false, {}, node, frame), "node", "node beats the frame it sits in")
	assert_eq(GraphScene.resolve_trigger(false, _link_hit(0.2, 9.0), {}, frame), "link", "with no node a link beats the frame")
	assert_eq(GraphScene.resolve_trigger(false, {}, {}, frame), "frame")
	assert_eq(GraphScene.resolve_trigger(false, {}, {}, {}), "none")


func test_grip_precedence() -> void:
	var handle := _frame_hit(&"knowledge", "handle", 3, 1.0)
	var frame := _frame_hit(&"knowledge", "frame", -1, 1.0)
	assert_eq(GraphScene.resolve_grip(true, handle, true), "hud", "HUD grab first")
	assert_eq(GraphScene.resolve_grip(false, handle, true), "handle")
	assert_eq(GraphScene.resolve_grip(false, frame, true), "frame")
	assert_eq(GraphScene.resolve_grip(false, _frame_hit(&"knowledge", "handle", -1, 1.0), false), "frame", "a handle hit without an index is a frame")
	assert_eq(GraphScene.resolve_grip(false, {}, true), "two_hand")
	assert_eq(GraphScene.resolve_grip(false, {}, false), "none")


func test_drag_stays_knowledge_only() -> void:
	assert_false(GraphScene.claim_blocks_drag("", &"", &"knowledge"), "no press: the Rust drag decides")
	assert_false(GraphScene.claim_blocks_drag("none", &"", &"knowledge"))
	assert_false(GraphScene.claim_blocks_drag("node", &"knowledge", &"knowledge"), "a knowledge node may drag")
	assert_true(GraphScene.claim_blocks_drag("node", &"ontology", &"knowledge"), "select only in other boxes")
	assert_true(GraphScene.claim_blocks_drag("node", &"codescape", &"knowledge"), "E9")
	assert_true(GraphScene.claim_blocks_drag("link", &"", &"knowledge"))
	assert_true(GraphScene.claim_blocks_drag("frame", &"ontology", &"knowledge"), "a frame press elsewhere never drags")
	assert_false(GraphScene.claim_blocks_drag("frame", &"knowledge", &"knowledge"), "the overlay's own tube may still catch a node")


func test_box_drag_global_translates_only() -> void:
	var g0 := Transform3D(Basis(Vector3.UP, 0.7), Vector3(1.0, 1.0, -1.0))
	var g := GraphScene.box_drag_global(g0, Vector3(1.0, 1.0, -0.5), Vector3(1.5, 0.8, -0.5))
	assert_true(g.basis.is_equal_approx(g0.basis), "rotation kept")
	assert_true(g.origin.is_equal_approx(Vector3(1.5, 0.8, -1.0)), "moved by the grab point's travel")


# --- trigger routing on the real scene -------------------------------------------

func test_node_pick_selects_focuses_and_shows_info() -> void:
	var scene: Node3D = await _make_scene()
	scene.apply_trigger("node", _node_hit(&"codescape", &"codescape", 7, 1.0), {}, {})
	assert_eq(scene._focused_volume, &"codescape", "the box is focused")
	assert_eq(scene._volumes[&"codescape"].selected_node, 7)
	assert_eq(scene._volumes[&"codescape"].selected_conn, &"codescape", "selection carries the box's own connection")
	assert_true(scene._volumes[&"codescape"].focused)
	assert_eq(scene._link_layer.highlight_keys(), [{"conn": &"codescape", "node_id": 7}], "its links are highlighted")
	assert_eq(scene._hl_count, 1)
	assert_string_contains(scene.hud.selection_line(), "Codescape")
	assert_string_contains(scene.hud.selection_line(), "#7", "label from the Codescape client (none headless → id)")
	# A second pick in another box moves the selection there.
	scene.apply_trigger("node", _node_hit(&"ontology", &"visionclaw", 3, 1.0), {}, {})
	assert_eq(scene._volumes[&"codescape"].selected_node, -1, "one selection across boxes")
	assert_eq(scene._volumes[&"ontology"].selected_node, 3)
	assert_eq(scene._focused_volume, &"ontology")
	scene.clear_volume_selection()
	assert_eq(scene._volumes[&"ontology"].selected_node, -1)
	assert_eq(scene._link_layer.highlight_keys(), [])
	assert_eq(scene.hud.selection_line(), "")
	await _free(scene)


func test_follow_link_focuses_far_end_and_brings_it_front() -> void:
	var scene: Node3D = await _make_scene()
	# Ray met the link near `from` (knowledge): follow to `to` (ontology).
	scene.apply_trigger("link", {}, _link_hit(0.1, 0.8), {})
	assert_eq(scene._focused_volume, &"ontology")
	assert_eq(scene._volumes[&"ontology"].selected_node, 22)
	assert_eq(scene._arrangement.occupant(0.0), &"ontology", "the far box takes the front slot")
	assert_true(scene._arrangement.is_easing(&"ontology"), "and eases there")
	var keys: Array = scene._link_layer.highlight_keys()
	assert_eq(keys.size(), 2, "both ends highlighted: %s" % [keys])
	assert_eq(scene._hl_count, 2, "markers on both ends")
	# Met near `to`: follow back to knowledge.
	scene.apply_trigger("link", {}, _link_hit(0.9, 0.8), {})
	assert_eq(scene._focused_volume, &"knowledge")
	assert_eq(scene._volumes[&"knowledge"].selected_node, 11)
	assert_eq(scene._arrangement.occupant(0.0), &"knowledge")
	await _free(scene)


func test_frame_pick_focuses() -> void:
	var scene: Node3D = await _make_scene()
	scene.apply_trigger("frame", {}, {}, _frame_hit(&"codescape", "frame", -1, 1.5))
	assert_eq(scene._focused_volume, &"codescape")
	assert_true(scene._volumes[&"codescape"].focused and not scene._volumes[&"knowledge"].focused)
	await _free(scene)


func test_trigger_pick_with_the_real_pickers() -> void:
	var scene: Node3D = await _make_scene()
	var eye := Vector3(0.0, VolumeArrangement.RING_HEIGHT, 0.0)
	# Straight ahead: the codescape box (0°), empty headless, so its frame.
	assert_eq(scene.trigger_pick(eye, Vector3.FORWARD, false, 1), "frame")
	assert_eq(scene._focused_volume, &"codescape", "point to focus (feature 1)")
	assert_eq(scene._trigger_claim[1], "frame", "the press is held as the hand's claim")
	assert_eq(scene._trigger_claim_vol[1], &"codescape")
	# At the knowledge box (−60°).
	var to_k: Vector3 = scene._volumes[&"knowledge"].global_position - eye
	assert_eq(scene.trigger_pick(eye, to_k, false, 0), "frame")
	assert_eq(scene._focused_volume, &"knowledge")
	# Up at the sky: nothing; the HUD always wins.
	assert_eq(scene.trigger_pick(eye, Vector3.UP, false, 0), "none")
	assert_eq(scene._focused_volume, &"knowledge", "a miss changes nothing")
	assert_eq(scene.trigger_pick(eye, Vector3.FORWARD, true, 0), "hud")
	assert_eq(scene._focused_volume, &"knowledge", "a HUD press never reaches the boxes")
	await _free(scene)


func test_new_generation_clears_the_selection() -> void:
	var scene: Node3D = await _make_scene()
	scene.select_volume_node(&"codescape", &"codescape", 4)
	scene._on_conn_topology_changed(&"visionclaw", 2)
	assert_eq(scene._volumes[&"codescape"].selected_node, 4, "another connection's generation leaves it")
	scene._on_conn_topology_changed(&"codescape", 2)
	assert_eq(scene._volumes[&"codescape"].selected_node, -1, "its own generation voids the wire id")
	scene.select_volume_node(&"ontology", &"visionclaw", 9)
	scene._on_conn_connected_changed(&"visionclaw", false)
	assert_eq(scene._volumes[&"ontology"].selected_node, -1, "a disconnect clears it")
	scene.select_volume_node(&"ontology", &"visionclaw", 9)
	scene._on_hud_control("volume_toggle:ontology")
	assert_eq(scene._sel_node, -1, "hiding the box clears it")
	await _free(scene)


# --- Front and Reset (features 5 and 4) ----------------------------------------

func test_front_action_brings_focused_box_forward() -> void:
	var scene: Node3D = await _make_scene()
	scene._on_hud_control("volume_front:knowledge")
	assert_eq(scene._arrangement.occupant(0.0), &"knowledge")
	assert_almost_eq(scene._arrangement.slot_yaw(&"codescape"), -60.0, 1.0e-6)
	for i: int in 40:
		scene._arrangement.tick(1.0 / 90.0)
	assert_true(scene._volumes[&"knowledge"].transform.is_equal_approx(VolumeArrangement.ring_transform(0.0)))
	scene._on_hud_control("volume_front:nope")  # unknown: ignored
	await _free(scene)


func test_reset_layout_restores_the_arrangement_even_on_codescape() -> void:
	var scene: Node3D = await _make_scene()
	scene._on_hud_control("volume_focus:codescape")
	scene._on_hud_control("volume_front:knowledge")
	scene._arrangement.move_volume(&"ontology", Transform3D(Basis.IDENTITY, Vector3(3.0, 0.0, 0.0)))
	scene._volumes[&"codescape"].request_bounds(Vector3(2.0, 1.5, 1.0))
	scene._on_hud_control("reset_layout")
	for vid: StringName in scene._volumes:
		var v: Node3D = scene._volumes[vid]
		assert_true(v.transform.is_equal_approx(scene._arrangement.default_transform(vid)), "%s on its ring slot" % vid)
		assert_eq(v.bounds, VolumeArrangement.DEFAULT_BOUNDS, "%s at default size" % vid)
	assert_false(scene._arrangement.is_easing())
	await _free(scene)


# --- box grip: move and resize ---------------------------------------------------

func _hand(scene: Node3D, at: Vector3, look: Vector3) -> Node3D:
	var h := Node3D.new()
	scene.add_child(h)
	h.global_transform = Transform3D(Basis.looking_at(look), at)
	return h


func test_grip_frame_moves_one_box() -> void:
	var scene: Node3D = await _make_scene()
	scene._arrangement.set_arrangement_scale(0.5)
	var c: Node3D = scene._volumes[&"codescape"]
	var k0: Transform3D = scene._volumes[&"knowledge"].global_transform
	var g0: Transform3D = c.global_transform
	var eye := Vector3(0.0, 1.0, 0.0)
	var to_c: Vector3 = c.global_position - eye
	var hand := _hand(scene, eye, to_c)
	scene.begin_box_grip(hand, _frame_hit(&"codescape", "frame", -1, to_c.length(), c.global_position))
	assert_eq(scene._box_grab_ctrl, hand)
	assert_eq(scene._focused_volume, &"codescape", "the gripped box is focused")
	assert_true(scene._arrangement.is_moved(&"codescape"), "it leaves its ring slot")
	# Hand slides 0.3 m right: the box follows by the same world offset.
	hand.global_position = eye + Vector3(0.3, 0.0, 0.0)
	scene.drive_box_grip()
	assert_true(c.global_position.is_equal_approx(g0.origin + Vector3(0.3, 0.0, 0.0)), "%s" % c.global_position)
	assert_true(c.global_transform.basis.is_equal_approx(g0.basis), "rotation and the arrangement scale held")
	assert_almost_eq(c.transform.basis.get_scale().x, 1.0, 1.0e-5, "unit scale in the arrangement")
	assert_true(scene._volumes[&"knowledge"].global_transform.is_equal_approx(k0), "only that box moves")
	scene._end_box_grip()
	assert_null(scene._box_grab_ctrl)
	assert_true(c.global_position.is_equal_approx(g0.origin + Vector3(0.3, 0.0, 0.0)), "the move holds")
	await _free(scene)


func test_grip_handle_resizes() -> void:
	var scene: Node3D = await _make_scene()
	var c: Node3D = scene._volumes[&"codescape"]
	var corner: Vector3 = c.global_transform * (Vector3(0.5, 0.5, 0.5))  # handle 7
	var hand := _hand(scene, corner, corner + Vector3.FORWARD)
	scene.begin_box_grip(hand, _frame_hit(&"codescape", "handle", 7, 0.0, corner))
	assert_true(c.is_resizing())
	assert_false(scene._arrangement.is_moved(&"codescape"), "a resize is not a move")
	hand.global_position = c.global_transform * Vector3(0.6, 0.6, 0.6)
	scene.drive_box_grip()
	assert_true(c.bounds.is_equal_approx(Vector3(1.2, 1.2, 1.2)), "grows by twice the hand's travel: %s" % c.bounds)
	scene._end_box_grip()
	assert_false(c.is_resizing())
	await _free(scene)


func test_hidden_box_ends_its_grip() -> void:
	var scene: Node3D = await _make_scene()
	var o: Node3D = scene._volumes[&"ontology"]
	var hand := _hand(scene, Vector3(0.0, 1.0, 0.0), o.global_position)
	scene.begin_box_grip(hand, _frame_hit(&"ontology", "frame", -1, 1.6, o.global_position))
	scene._on_hud_control("volume_toggle:ontology")
	assert_null(scene._box_grab_ctrl, "hiding the gripped box releases it")
	scene.begin_box_grip(hand, _frame_hit(&"ontology", "frame", -1, 1.6, o.global_position))
	assert_null(scene._box_grab_ctrl, "a hidden box cannot be gripped")
	await _free(scene)


func test_box_grip_blocks_two_hand_gesture() -> void:
	var scene: Node3D = await _make_scene()
	var c: Node3D = scene._volumes[&"codescape"]
	var hand := _hand(scene, Vector3(0.0, 1.0, 0.0), c.global_position)
	scene.begin_box_grip(hand, _frame_hit(&"codescape", "frame", -1, 1.6, c.global_position))
	scene._update_two_hand_manip(0.011)
	assert_false(scene._two_hand_active, "a held box owns a hand")
	await _free(scene)
