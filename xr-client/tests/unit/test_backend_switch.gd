extends "res://addons/gut/test.gd"
const BackendCaps := preload("res://scripts/backend_caps.gd")

# ADR-2143 A6: a backend switch abandons what the previous server already put
# on screen, not just the requests still in flight. Instantiates the real
# GraphScene (as test_graph_agents.gd does); needs the gdext library.


func _make_scene() -> Node3D:
	var packed: PackedScene = load("res://scenes/GraphScene.tscn")
	var scene: Node3D = packed.instantiate()
	add_child(scene)
	await get_tree().process_frame
	return scene


func test_switch_clears_applied_fold_pins_and_pending_state() -> void:
	var scene: Node3D = await _make_scene()
	scene._fold_level = 2
	scene._fold_pending = true
	scene._physics_pending = true
	scene._physics_staged = {"_repel_k": 1.0}
	scene._pinned_ids = {5: true, 9: true}
	scene._abandon_backend_requests()
	assert_eq(scene._fold_level, 0, "applied fold plan dropped with the old epoch's ids")
	assert_false(scene._fold_pending, "fold request abandoned")
	assert_false(scene._physics_pending, "physics latch reopened")
	assert_eq(scene._physics_staged.size(), 0, "staged physics discarded")
	assert_eq(scene._pinned_ids.size(), 0, "pin set cleared after the unpins were sent")
	assert_true(scene._selection_dirty, "a rebuild is forced")
	scene.queue_free()
	await get_tree().process_frame


func test_switch_keeps_the_once_per_session_canary_latched() -> void:
	var scene: Node3D = await _make_scene()
	scene._m4_ray_observed = true
	scene._abandon_backend_requests()
	assert_true(scene._m4_ray_observed, "canary stays latched across a switch")
	scene.queue_free()
	await get_tree().process_frame


func test_same_mode_switch_is_a_no_op() -> void:
	var scene: Node3D = await _make_scene()
	scene._fold_level = 1
	scene._switch_backend(scene._backend_mode)
	assert_eq(scene._fold_level, 1, "switching to the active backend touches nothing")
	scene.queue_free()
	await get_tree().process_frame
