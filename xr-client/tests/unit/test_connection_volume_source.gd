extends "res://addons/gut/test.gd"
const BackendConnection := preload("res://scripts/backend_connection.gd")
const ConnectionVolumeSource := preload("res://scripts/connection_volume_source.gd")

# XR phase-1 contract §3.3 / §8: the real content source. A fake client holds a
# fixed classed point set and answers node_ids_for_mask / render_aabb_ids the
# way binary_protocol.rs does (its pure functions are unit-tested in Rust).


class FakeClassClient:
	extends RefCounted
	signal connection_changed(connected: bool)
	signal topology_updated(edge_count: int)
	## id → [class_code, position]
	var nodes: Dictionary = {}
	var mask_calls: int = 0

	func node_ids_for_mask(mask: int) -> PackedInt32Array:
		mask_calls += 1
		var out := PackedInt32Array()
		var keys: Array = nodes.keys()
		keys.sort()
		for id: int in keys:
			if mask & (1 << int(nodes[id][0])):
				out.append(id)
		return out

	func render_aabb_ids(ids: PackedInt32Array, exclude: int) -> PackedFloat32Array:
		var box := AABB()
		var first := true
		for id: int in ids:
			if id == exclude or not nodes.has(id):
				continue
			var p: Vector3 = nodes[id][1]
			if first:
				box = AABB(p, Vector3.ZERO)
				first = false
			else:
				box = box.expand(p)
		if first:
			return PackedFloat32Array()
		var e: Vector3 = box.end
		return PackedFloat32Array([box.position.x, box.position.y, box.position.z, e.x, e.y, e.z])

	func node_position(id: int) -> Vector3:
		return nodes[id][1] if nodes.has(id) else Vector3.ZERO


var _fake: FakeClassClient
var _conn: Node


func before_each() -> void:
	_fake = FakeClassClient.new()
	_fake.nodes = {
		1: [0, Vector3(0, 0, 0)],
		2: [0, Vector3(4, -2, 1)],
		3: [1, Vector3(10, 10, 10)],
		4: [2, Vector3(-50, 0, 0)],
		5: [3, Vector3(7, 8, 9)],
		6: [0, Vector3(-3, 6, -1)],
	}
	_conn = BackendConnection.make(&"visionclaw", "visionclaw", "ws://h:4000", "", _fake)
	add_child_autofree(_conn)


func test_mask_selects_only_its_class() -> void:
	var know := ConnectionVolumeSource.new(_conn, 0b0001)
	var onto := ConnectionVolumeSource.new(_conn, 0b0010)
	var code := ConnectionVolumeSource.new(_conn, 0b1000)
	assert_eq(know.content_ids(), PackedInt32Array([1, 2, 6]))
	assert_eq(onto.content_ids(), PackedInt32Array([3]))
	assert_eq(code.content_ids(), PackedInt32Array([5]))
	assert_eq(know.content_id(), &"visionclaw:0b0001")
	assert_eq(onto.content_id(), &"visionclaw:0b0010")
	assert_eq(ConnectionVolumeSource.mask_label(0b1000), "0b1000")


func test_aabb_is_full_extent_and_excludes_the_grabbed_node() -> void:
	var know := ConnectionVolumeSource.new(_conn, 0b0001)
	var box: AABB = know.content_aabb()
	assert_eq(box.position, Vector3(-3, -2, -1), "every knowledge node, no percentile trim")
	assert_eq(box.end, Vector3(4, 6, 1))
	know.grabbed_id = 6
	box = know.content_aabb()
	assert_eq(box.position, Vector3(0, -2, 0), "the grabbed node does not refit the volume")
	assert_eq(box.end, Vector3(4, 0, 1))
	var empty := ConnectionVolumeSource.new(_conn, 0b0100 << 4)
	assert_eq(empty.content_aabb(), AABB(), "no ids ⇒ empty AABB")


func test_ids_are_reread_on_a_new_generation() -> void:
	var know := ConnectionVolumeSource.new(_conn, 0b0001)
	assert_eq(know.content_ids().size(), 3)
	_fake.nodes[7] = [0, Vector3(1, 1, 1)]
	assert_eq(know.content_ids().size(), 3, "cached within the refresh window")
	_fake.topology_updated.emit(1)
	assert_eq(know.content_ids().size(), 4, "a topology (new generation) re-reads at once")
	_fake.nodes[8] = [0, Vector3(2, 2, 2)]
	know.invalidate()
	assert_eq(know.content_ids().size(), 5, "invalidate re-reads")


func test_position_of_reads_the_connection() -> void:
	var onto := ConnectionVolumeSource.new(_conn, 0b0010)
	assert_eq(onto.position_of(3), Vector3(10, 10, 10))
	assert_eq(onto.position_of(99), Vector3.ZERO)


func test_mount_and_unmount_leave_no_orphans() -> void:
	var parent := Node3D.new()
	add_child_autofree(parent)
	var tpl := MultiMeshInstance3D.new()
	tpl.multimesh = MultiMesh.new()
	tpl.multimesh.transform_format = MultiMesh.TRANSFORM_3D
	tpl.multimesh.use_colors = true
	tpl.multimesh.use_custom_data = true
	tpl.multimesh.mesh = BoxMesh.new()
	tpl.material_override = StandardMaterial3D.new()
	var know := ConnectionVolumeSource.new(_conn, 0b0001)
	know.set_templates({"nodes": tpl})
	var before: int = Performance.get_monitor(Performance.OBJECT_ORPHAN_NODE_COUNT)
	for i: int in 3:
		know.mount(parent)
		assert_true(know.is_mounted())
		assert_eq(parent.get_child_count(), 1, "one container per source, remounting replaces it")
		assert_eq(know.root.get_child_count(), ConnectionVolumeSource.ROLES.size())
	var nodes_mmi: MultiMeshInstance3D = know.instance("nodes")
	assert_eq(nodes_mmi.multimesh.mesh, tpl.multimesh.mesh, "template mesh shared")
	assert_eq(nodes_mmi.material_override, tpl.material_override, "template material shared")
	assert_ne(nodes_mmi.multimesh, tpl.multimesh, "but never the template's MultiMesh")
	assert_false(know.instance("edges").multimesh.use_colors, "edge stride carries no colour")
	assert_true(know.instance("edges").multimesh.use_custom_data)
	know.unmount()
	assert_false(know.is_mounted())
	assert_eq(parent.get_child_count(), 0)
	assert_eq(know.instances.size(), 0)
	assert_eq(Performance.get_monitor(Performance.OBJECT_ORPHAN_NODE_COUNT), before, "no orphan nodes left")
	know.unmount()
	tpl.free()
