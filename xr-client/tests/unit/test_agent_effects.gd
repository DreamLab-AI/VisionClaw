extends "res://addons/gut/test.gd"

# Work-cue effects (scripts/agent_effects.gd): pooled rings, completion bursts
# and hand-off packet beads in two MultiMeshes under a unit-scale root.

const Effects := preload("res://scripts/agent_effects.gd")


func _make() -> Node3D:
	var e: Node3D = Effects.new()
	add_child(e)
	await get_tree().process_frame
	return e


func test_rings_and_bursts_are_instanced_and_bursts_expire() -> void:
	var e: Node3D = await _make()
	e.set_ring("a", Vector3(0, 1, -1))
	e.set_ring("b", Vector3(0.5, 1, -1))
	e.burst(Vector3(0, 1.2, -1))
	await get_tree().process_frame
	var mm: MultiMesh = (e.get_node("RingMulti") as MultiMeshInstance3D).multimesh
	assert_eq(mm.instance_count, 3, "two rings + one burst")
	e.clear_ring("a")
	await get_tree().create_timer(Effects.BURST_SEC + 0.1).timeout
	await get_tree().process_frame
	assert_eq(mm.instance_count, 1, "burst expired, one ring left")
	assert_eq(e.ring_count(), 1)
	e.queue_free()
	await get_tree().process_frame


func test_packets_travel_the_edge_then_vanish_and_respect_reduced_motion() -> void:
	var e: Node3D = await _make()
	e.send_packets(Vector3(0, 1, -1), Vector3(0.45, 1, -1))   # 1 s of travel at 0.45 m/s
	assert_eq(e.bead_count(), Effects.BEAD_COUNT, "one packet = three beads")
	await get_tree().process_frame
	var mm: MultiMesh = (e.get_node("BeadMulti") as MultiMeshInstance3D).multimesh
	assert_gte(mm.instance_count, 1, "first bead drawn immediately")
	assert_lte(mm.instance_count, Effects.BEAD_COUNT, "later beads wait on their spacing")
	await get_tree().create_timer(1.0 + 2.0 * Effects.BEAD_SPACING / Effects.BEAD_SPEED + 0.2).timeout
	await get_tree().process_frame
	assert_eq(e.bead_count(), 0, "all beads arrived and were dropped")
	e.reduced_motion = true
	e.send_packets(Vector3.ZERO, Vector3(1, 0, 0))
	assert_eq(e.bead_count(), 0, "no travelling packets under reduced motion")
	e.queue_free()
	await get_tree().process_frame
