extends "res://addons/gut/test.gd"
const VolumePicker := preload("res://scripts/volume_picker.gd")
const SpatialVolume := preload("res://scripts/spatial_volume.gd")

# Spatial-volume UX feature 2: node picking in every box. Each volume reads its
# own render-store slot through its own Content transform; hidden, closed,
# non-interactable and offline volumes are skipped. Sources and clients are
# duck-typed fakes; volumes are real SpatialVolumes.


class FakeClient extends RefCounted:
	var slots := {}  # slot → [PackedInt32Array, PackedVector3Array]
	var last_slot := 0

	func get_render_ids_for_slot(slot: int) -> PackedInt32Array:
		return slots.get(slot, [PackedInt32Array(), PackedVector3Array()])[0]

	func get_render_positions_for_slot(slot: int) -> PackedVector3Array:
		return slots.get(slot, [PackedInt32Array(), PackedVector3Array()])[1]

	# The single-slot accessors read the last-packed slot only: the picker must
	# never use them.
	func get_render_ids() -> PackedInt32Array:
		return get_render_ids_for_slot(last_slot)

	func get_render_positions() -> PackedVector3Array:
		return get_render_positions_for_slot(last_slot)


class FakeConn extends RefCounted:
	var conn_id: StringName
	var client: RefCounted
	var connected := true

	func _init(id: StringName, c: RefCounted) -> void:
		conn_id = id
		client = c

	func is_connected_now() -> bool:
		return connected


class FakeSource extends RefCounted:
	var conn: Object
	var slot := 0

	func _init(c: Object, s: int) -> void:
		conn = c
		slot = s


# A source on a client without the per-slot accessors (older gdext build).
class LegacySource extends RefCounted:
	var conn: Object
	var slot := 0
	var pos := {}

	func _init(c: Object) -> void:
		conn = c

	func content_ids() -> PackedInt32Array:
		return PackedInt32Array(pos.keys())

	func position_of(id: int) -> Vector3:
		return pos[id]


class BareClient extends RefCounted:
	pass


var client: FakeClient
var vc: FakeConn
var volumes: Dictionary
var sources: Dictionary


func _volume(id: StringName, at: Vector3) -> Node3D:
	var v: Node3D = SpatialVolume.new()
	v.volume_id = id
	add_child_autofree(v)
	v.position = at
	return v


func before_each() -> void:
	client = FakeClient.new()
	vc = FakeConn.new(&"visionclaw", client)
	# Knowledge box 2 m ahead, ontology box 2 m ahead and 1 m right, both
	# packed into their own slots on one client.
	var know := _volume(&"knowledge", Vector3(0, 0, -2))
	var onto := _volume(&"ontology", Vector3(1, 0, -2))
	volumes = {&"knowledge": know, &"ontology": onto}
	sources = {&"knowledge": FakeSource.new(vc, 1), &"ontology": FakeSource.new(vc, 2)}
	client.slots[1] = [PackedInt32Array([10, 11]), PackedVector3Array([Vector3.ZERO, Vector3(0, 0.3, 0)])]
	client.slots[2] = [PackedInt32Array([20]), PackedVector3Array([Vector3.ZERO])]
	client.last_slot = 2


func test_picks_the_node_in_each_volumes_own_slot() -> void:
	var hit := VolumePicker.pick(Vector3.ZERO, Vector3(0, 0, -1), volumes, sources)
	assert_eq(hit.get("volume"), &"knowledge")
	assert_eq(hit.get("conn"), &"visionclaw")
	assert_eq(hit.get("node_id"), 10)
	assert_almost_eq(float(hit["distance"]), 2.0, 1e-4)
	assert_true((hit["world_pos"] as Vector3).is_equal_approx(Vector3(0, 0, -2)))
	var to_onto := (Vector3(1, 0, -2)).normalized()
	hit = VolumePicker.pick(Vector3.ZERO, to_onto, volumes, sources)
	assert_eq(hit.get("volume"), &"ontology")
	assert_eq(hit.get("node_id"), 20, "slot 2 read, not the last-packed slot by accident")


func test_slot_one_is_read_even_when_slot_two_packed_last() -> void:
	var up := Vector3(0, 0.3, -2).normalized()
	var hit := VolumePicker.pick(Vector3.ZERO, up, volumes, sources)
	assert_eq(hit.get("node_id"), 11, "parked slot 1's second node")


func test_positions_go_through_the_content_transform() -> void:
	var know: Node3D = volumes[&"knowledge"]
	know.content.transform = Transform3D(Basis().scaled(Vector3.ONE * 0.5), Vector3(0, 0.1, 0))
	# Node 11 at graph (0, 0.3, 0) → content (0, 0.25, 0) → world (0, 0.25, -2).
	var hit := VolumePicker.pick(Vector3.ZERO, Vector3(0, 0.25, -2).normalized(), volumes, sources)
	assert_eq(hit.get("node_id"), 11)
	assert_true((hit["world_pos"] as Vector3).is_equal_approx(Vector3(0, 0.25, -2)))


func test_miss_outside_the_cone_returns_empty() -> void:
	assert_eq(VolumePicker.pick(Vector3.ZERO, Vector3(0, 1, 0), volumes, sources), {})
	# 0.1 m off at 2 m is ~2.9°: outside 1.5°, inside 5°.
	var off := Vector3(-0.1, 0, -2).normalized()
	assert_eq(VolumePicker.pick(Vector3.ZERO, off, volumes, sources), {})
	assert_eq(VolumePicker.pick(Vector3.ZERO, off, volumes, sources, 5.0).get("node_id"), 10)
	assert_eq(VolumePicker.pick(Vector3.ZERO, Vector3.ZERO, volumes, sources), {}, "zero direction")


func test_nearest_along_the_ray_wins() -> void:
	(volumes[&"ontology"] as Node3D).position = Vector3(0, 0, -1)
	var hit := VolumePicker.pick(Vector3.ZERO, Vector3(0, 0, -1), volumes, sources)
	assert_eq(hit.get("volume"), &"ontology")
	assert_almost_eq(float(hit["distance"]), 1.0, 1e-4)


func test_nodes_behind_or_beyond_range_are_ignored() -> void:
	assert_eq(VolumePicker.pick(Vector3(0, 0, -5), Vector3(0, 0, -1), volumes, sources), {}, "behind")
	assert_eq(VolumePicker.pick(Vector3(0, 0, 40), Vector3(0, 0, -1), volumes, sources), {}, "past 30 m")


func test_hidden_closed_non_interactable_and_offline_volumes_are_skipped() -> void:
	var know: Node3D = volumes[&"knowledge"]
	(volumes[&"ontology"] as Node3D).position = Vector3(0, 0, -3)
	know.request_visible(false)
	assert_eq(VolumePicker.pick(Vector3.ZERO, Vector3(0, 0, -1), volumes, sources).get("volume"), &"ontology")
	know.request_visible(true)
	know.request_interactable(false)
	assert_eq(VolumePicker.pick(Vector3.ZERO, Vector3(0, 0, -1), volumes, sources).get("volume"), &"ontology")
	know.request_interactable(true)
	know.close()
	assert_eq(VolumePicker.pick(Vector3.ZERO, Vector3(0, 0, -1), volumes, sources).get("volume"), &"ontology")
	vc.connected = false
	assert_eq(VolumePicker.pick(Vector3.ZERO, Vector3(0, 0, -1), volumes, sources), {}, "offline")


func test_pickable_rules() -> void:
	var know: Node3D = volumes[&"knowledge"]
	assert_true(VolumePicker.pickable(know, sources[&"knowledge"]))
	assert_false(VolumePicker.pickable(know, null), "no source")
	assert_false(VolumePicker.pickable(null, sources[&"knowledge"]), "no volume")
	assert_false(VolumePicker.pickable(know, FakeSource.new(null, 1)), "no connection")
	vc.connected = false
	assert_false(VolumePicker.pickable(know, sources[&"knowledge"]), "offline")


func test_volume_without_a_source_is_skipped() -> void:
	sources.erase(&"knowledge")
	assert_eq(VolumePicker.pick(Vector3.ZERO, Vector3(0, 0, -1), volumes, sources), {})


func test_candidates_prefer_slot_accessors_and_fall_back_to_content_ids() -> void:
	var c := VolumePicker.candidates(sources[&"ontology"])
	assert_eq(c[0], PackedInt32Array([20]))
	var legacy := LegacySource.new(FakeConn.new(&"codescape", BareClient.new()))
	legacy.pos = {7: Vector3(0, 0, 0), 8: Vector3(0, 0.5, 0)}
	c = VolumePicker.candidates(legacy)
	assert_eq((c[0] as PackedInt32Array).size(), 2)
	assert_eq((c[1] as PackedVector3Array).size(), 2)
	var cs := _volume(&"codescape", Vector3(0, 0.5, -1))
	var hit := VolumePicker.pick(Vector3.ZERO, Vector3(0, 0.5, -1).normalized(), {&"codescape": cs}, {&"codescape": legacy})
	assert_eq(hit.get("conn"), &"codescape")
	assert_eq(hit.get("node_id"), 7)


# The real gdext client exposes the per-slot readers, so the content_ids
# fallback in candidates() never hides a missing binding in a live build.
func test_gdext_client_binds_the_per_slot_readers() -> void:
	assert_true(ClassDB.class_exists("BinaryProtocolClient"), "gdext library loaded")
	assert_true(ClassDB.class_has_method("BinaryProtocolClient", "get_render_ids_for_slot"))
	assert_true(ClassDB.class_has_method("BinaryProtocolClient", "get_render_positions_for_slot"))
