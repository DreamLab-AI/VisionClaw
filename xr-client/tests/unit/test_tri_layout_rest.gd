extends "res://addons/gut/test.gd"

# ADR-2135: in the separated layout the work agents rest at the triangle's
# centroid (server origin) plus their activity drift; back at separation 0 the
# front-arc rim slots return. The choreography stays the only pose writer: the
# scene only moves the slot an agent parks at (Invariant 8).


func _make_scene() -> Node3D:
	var scene: Node3D = (load("res://scenes/GraphScene.tscn") as PackedScene).instantiate()
	add_child(scene)
	await get_tree().process_frame
	return scene


func test_rest_slot_is_the_centroid_plus_drift_on_a_small_ring() -> void:
	var scene: Node3D = await _make_scene()
	var client := FakeDriftClient.new()
	client.offset = Vector3(40, 0, -20)
	var real: RefCounted = scene._binary_client
	scene._binary_client = client  # no frame runs while the fake is in place
	scene._graph_separation = 300.0
	var slot: Vector3 = scene._drift_rest_slot(7, 0)
	var centre: Vector3 = scene._server_to_world(Vector3(40, 0, -20))
	assert_almost_eq(slot.distance_to(centre), scene.DRIFT_REST_SPREAD_M, 1e-4, "on the ring around centroid + drift")
	assert_eq(client.asked, [[7, 300.0]], "the agent's own drift at this separation")
	var other: Vector3 = scene._drift_rest_slot(7, 1)
	assert_gt(slot.distance_to(other), 0.01, "slot index spreads agents")
	scene._binary_client = real
	scene._graph_separation = 0.0
	scene.queue_free()
	await get_tree().process_frame


class FakeDriftClient extends RefCounted:
	var offset := Vector3.ZERO
	var asked: Array = []
	func agent_drift_offset(id: int, separation: float) -> Vector3:
		asked.append([id, separation])
		return offset
	func step_agent_drift() -> void: pass
