extends "res://addons/gut/test.gd"
const BackendCaps := preload("res://scripts/backend_caps.gd")

# ADR-2159: the exclusive backend switch is gone. GraphScene holds one
# BackendConnection per content source and three volumes bound to them; the
# HUD's global controls gate against the focused volume's connection.
# Instantiates the real GraphScene (as test_graph_agents.gd does).


func _make_scene() -> Node3D:
	var packed: PackedScene = load("res://scenes/GraphScene.tscn")
	var scene: Node3D = packed.instantiate()
	add_child(scene)
	await get_tree().process_frame
	return scene


func test_two_connections_three_volumes() -> void:
	var scene: Node3D = await _make_scene()
	assert_eq(scene._conns.size(), 2)
	var vc: Node = scene._conns[&"visionclaw"]
	var cs: Node = scene._conns[&"codescape"]
	assert_ne(vc.client, cs.client, "each connection has its own client")
	assert_eq(scene._binary_client, vc.client, "VisionClaw features read the VisionClaw client")
	assert_eq(scene._volumes.size(), 3)
	assert_eq(scene._conn_for_volume(&"knowledge"), vc)
	assert_eq(scene._conn_for_volume(&"ontology"), vc)
	assert_eq(scene._conn_for_volume(&"codescape"), cs)
	assert_true(cs.graph_ws_url().ends_with("/codescape/wss"))
	for vid: StringName in scene._volumes:
		assert_not_null(scene._volumes[vid].source(), "%s has its source attached" % vid)
	assert_false(scene.has_method("_switch_backend"), "the exclusive switch is deleted")
	scene.queue_free()
	await get_tree().process_frame


func test_focused_codescape_volume_refuses_visionclaw_routes() -> void:
	var scene: Node3D = await _make_scene()
	var cs: Node = scene._conns[&"codescape"]
	watch_signals(cs)
	scene._on_hud_control("volume_focus:codescape")
	assert_eq(scene._focused_volume, &"codescape")
	assert_false(scene.backend_supports(BackendCaps.FOLD))
	scene._on_hud_control("fold_plus")
	assert_false(scene._fold_pending, "no fold request sent to Codescape")
	assert_signal_emitted(cs, "refused", "refusal announced for the HUD notice")
	scene._on_hud_control("volume_focus:knowledge")
	assert_true(scene.backend_supports(BackendCaps.FOLD), "VisionClaw serves folds")
	scene.queue_free()
	await get_tree().process_frame


func test_codescape_drag_is_refused_and_never_sent() -> void:
	var scene: Node3D = await _make_scene()
	var cs: Node = scene._conns[&"codescape"]
	assert_false(cs.send_drag_start(3, Vector3.ONE), "E9: no nodeDrag on Codescape")
	assert_eq(cs.pinned_ids(), [])
	scene.queue_free()
	await get_tree().process_frame


func test_hidden_volume_stops_packing() -> void:
	var scene: Node3D = await _make_scene()
	scene._on_hud_control("volume_toggle:ontology")
	assert_false(scene._volumes[&"ontology"].visible_state)
	assert_eq(scene._sources[&"ontology"].demand(), {"nodes": 0, "edges": 0})
	scene._on_hud_control("volume_toggle:ontology")
	assert_true(scene._volumes[&"ontology"].visible_state)
	scene.queue_free()
	await get_tree().process_frame
