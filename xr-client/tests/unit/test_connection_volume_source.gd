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

	# Pack surface: records the slot each build ran under and returns fixed
	# buffers (2 gems, 3 impostors, 1 faded; 4 cylinders, 5 ribbons).
	var slot_now: int = 0
	var builds: Array = []
	var dropped: Array = []

	func set_pack_slot(s: int) -> void:
		slot_now = s

	func drop_pack_slot(s: int) -> void:
		dropped.append(s)

	func edges_for_mask(mask: int, _max: int) -> PackedInt32Array:
		return PackedInt32Array([1, 2]) if mask & 1 else PackedInt32Array()

	func _buf(n: int, stride: int) -> PackedFloat32Array:
		var b := PackedFloat32Array()
		b.resize(n * stride)
		return b

	func build_node_buffer_lod(ids: PackedInt32Array, comp: float, _lo: float, _hi: float, cam: Vector3, cap: int, near: float) -> PackedFloat32Array:
		builds.append(["nodes", slot_now, ids, comp, cam, cap, near])
		return _buf(2, 20)

	func impostor_node_buffer() -> PackedFloat32Array:
		return _buf(3, 20)

	func faded_node_buffer() -> PackedFloat32Array:
		return _buf(1, 20)

	func build_edge_buffer_lod(pairs: PackedInt32Array, comp: float, _cam: Vector3, cap: int, _near: float) -> PackedFloat32Array:
		builds.append(["edges", slot_now, pairs, comp, cap])
		return _buf(4, 16)

	func ribbon_edge_buffer() -> PackedFloat32Array:
		return _buf(5, 16)


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


func test_pack_uses_its_own_slot_and_fills_every_layer() -> void:
	var parent := Node3D.new()
	add_child_autofree(parent)
	parent.scale = Vector3.ONE * 0.5          # 0.5 world metres per graph unit
	var know := ConnectionVolumeSource.new(_conn, 0b0001)
	var onto := ConnectionVolumeSource.new(_conn, 0b0010)
	assert_ne(know.slot, onto.slot, "two volumes on one connection never share a pack slot")
	assert_ne(know.slot, 0, "slot 0 stays the default for other callers")
	know.mount(parent)
	know.pack(Vector3(0, 0, 2), 0.06, 0.05, 40, 48, 1.0, 1.0)
	var nb: Array = _fake.builds[0]
	assert_eq(nb[0], "nodes")
	assert_eq(nb[1], know.slot, "built under the volume's slot")
	assert_eq(nb[2], PackedInt32Array([1, 2, 6]), "only the masked ids")
	assert_almost_eq(float(nb[3]), 0.12, 1e-5, "node ratio / world-per-graph-unit")
	assert_eq(nb[4], Vector3(0, 0, 4), "camera in graph units")
	assert_eq(nb[5], 40, "the volume's gem share")
	assert_almost_eq(float(nb[6]), 2.0, 1e-5, "near radius in graph units")
	var eb: Array = _fake.builds[1]
	assert_eq(eb[1], know.slot)
	assert_eq(eb[2], PackedInt32Array([1, 2]), "only this volume's edges")
	assert_eq(eb[4], 48)
	assert_eq(know.counts, {"nodes": 2, "impostors": 3, "faded": 1, "halos": 3, "edges": 4, "ribbons": 5})
	assert_eq(know.instance("halos").multimesh.instance_count, 3, "halo per gem + faded node")
	assert_eq(know.demand(), {"nodes": 6, "edges": 9})
	know.clear_instances()
	for role: String in ConnectionVolumeSource.ROLES:
		assert_eq(know.instance(role).multimesh.instance_count, 0, "%s cleared" % role)
	assert_eq(know.demand(), {"nodes": 0, "edges": 0})
	know.unmount()
	assert_true(_fake.dropped.has(know.slot), "unmount frees the pack slot")


func test_pack_does_nothing_unmounted() -> void:
	var know := ConnectionVolumeSource.new(_conn, 0b0001)
	know.pack(Vector3.ZERO, 0.06, 0.05, 40, 48, 1.0, 1.0)
	assert_eq(_fake.builds.size(), 0)
