extends "res://addons/gut/test.gd"
const LinkLayer := preload("res://scripts/link_layer.gd")

# ADR-2164 L4 and L5: the cross-volume link layer. One MultiMesh (one draw
# call), at most 512 instances with an overflow count, 16-float stride, off ⇒
# zero instances in the same frame, a reconnect drops the old generation's
# records before anything new is drawn, and a closed connection draws nothing
# without an error. Volumes and connections are duck-typed fakes.


class FakeVolume extends RefCounted:
	var offset := Vector3.ZERO
	var visible_state := true
	var is_closed := false

	func world_from_content(p: Vector3) -> Vector3:
		return p + offset


class FakeConn extends RefCounted:
	var conn_id: StringName
	var generation := 1
	var connected := true
	var positions := {}

	func _init(id: StringName) -> void:
		conn_id = id

	func node_position(id: int) -> Vector3:
		return positions.get(id, Vector3(float(id), 0.0, 0.0))

	func is_connected_now() -> bool:
		return connected


var layer: Node3D
var volumes: Dictionary
var conns: Dictionary


func before_each() -> void:
	layer = LinkLayer.new()
	add_child_autofree(layer)
	var code := FakeVolume.new()
	code.offset = Vector3(0, 0, -2)
	var onto := FakeVolume.new()
	onto.offset = Vector3(2, 0, 0)
	var know := FakeVolume.new()
	know.offset = Vector3(-2, 0, 0)
	volumes = {&"codescape": code, &"ontology": onto, &"knowledge": know}
	conns = {&"codescape": FakeConn.new(&"codescape"), &"visionclaw": FakeConn.new(&"visionclaw")}


func _rec(sid: int, tid: int, kind := "annotates", tvol := "ontology", gens := [1, 1]) -> Dictionary:
	var src_conn := "codescape" if kind == "annotates" else "visionclaw"
	var src_vol := "codescape" if kind == "annotates" else "knowledge"
	return {
		"source": {"volume": src_vol, "conn": src_conn, "key": "s%05d" % sid, "id": sid},
		"target": {"volume": tvol, "conn": "visionclaw", "key": "t%05d" % tid, "id": tid},
		"kind": kind,
		"provenance": {"via": "test", "generations": gens},
	}


# INSTANCE_CUSTOM is stored at reduced precision (0.72 reads back 0.7197).
func _assert_colour(got: Color, want: Color, text := "") -> void:
	for i: int in 4:
		assert_almost_eq(got[i], want[i], 0.005, text)


# Instance `i` decoded from the packed buffer the layer hands the MultiMesh
# (rows of the basis, origin in the 4th column, then the colour). Read from
# `buffer` rather than the per-instance getters: the headless dummy renderer
# keeps the buffer but not per-instance state.
func _xf(i: int) -> Transform3D:
	var b: PackedFloat32Array = _mm().buffer
	var k := i * LinkLayer.STRIDE
	var basis := Basis(Vector3(b[k], b[k + 4], b[k + 8]), Vector3(b[k + 1], b[k + 5], b[k + 9]),
		Vector3(b[k + 2], b[k + 6], b[k + 10]))
	return Transform3D(basis, Vector3(b[k + 3], b[k + 7], b[k + 11]))


func _custom(i: int) -> Color:
	var b: PackedFloat32Array = _mm().buffer
	var k := i * LinkLayer.STRIDE + 12
	return Color(b[k], b[k + 1], b[k + 2], b[k + 3])


func _mm() -> MultiMesh:
	return layer.multimesh_instance.multimesh


func test_off_by_default_costs_nothing() -> void:
	layer.set_records("annotates", [_rec(1, 2)])
	layer.tick(1.0, volumes, conns)
	assert_false(layer.is_enabled())
	assert_false(layer.visible)
	assert_eq(_mm().instance_count, 0)
	assert_eq(layer.draw_calls(), 0)
	assert_eq(layer.triangle_count(), 0)


func test_one_multimesh_with_a_16_float_stride_and_12_triangles() -> void:
	var mm := _mm()
	assert_eq(mm.transform_format, MultiMesh.TRANSFORM_3D)
	assert_true(mm.use_custom_data, "4 custom floats")
	assert_false(mm.use_colors, "no colour block: stride stays 16")
	assert_eq(mm.mesh.surface_get_array_len(0) / 3, LinkLayer.TRIS_PER_LINK)
	assert_true(LinkLayer.TRIS_PER_LINK <= 12)
	var mmis := 0
	for c: Node in layer.get_children():
		if c is MultiMeshInstance3D:
			mmis += 1
	assert_eq(mmis, 1, "one MultiMeshInstance3D, so one draw call")


func test_enabled_layer_draws_each_record_between_its_endpoints() -> void:
	layer.set_enabled(true)
	layer.set_records("annotates", [_rec(1, 5), _rec(2, 6, "annotates", "knowledge")])
	layer.tick(0.0, volumes, conns)
	var mm := _mm()
	assert_eq(mm.instance_count, 2)
	assert_eq(mm.buffer.size(), 2 * LinkLayer.STRIDE)
	assert_eq(layer.draw_calls(), 1)
	assert_eq(layer.triangle_count(), 2 * LinkLayer.TRIS_PER_LINK)
	# Ordered by target key: t00005 then t00006.
	var a := Vector3(1, 0, -2)  # node 1 in the codescape volume
	var b := Vector3(7, 0, 0)   # node 5 in the ontology volume
	var t: Transform3D = _xf(0)
	assert_almost_eq(t.origin.distance_to((a + b) * 0.5), 0.0, 1e-4, "midpoint")
	assert_almost_eq(t.basis.y.length(), a.distance_to(b), 1e-4, "spans the link")
	assert_almost_eq(t.basis.x.length(), LinkLayer.RADIUS_M, 1e-6)
	_assert_colour(_custom(0), LinkLayer.KIND_COLOURS["annotates"])


func test_switching_off_empties_in_the_same_frame() -> void:
	layer.set_enabled(true)
	layer.set_records("annotates", [_rec(1, 5), _rec(2, 6)])
	layer.tick(0.0, volumes, conns)
	assert_eq(_mm().instance_count, 2)
	layer.set_enabled(false)
	assert_eq(_mm().instance_count, 0, "no tick needed")
	assert_false(layer.visible)
	assert_eq(layer.draw_calls(), 0)
	assert_eq(layer.triangle_count(), 0)
	# Back on: drawn again at the next tick, whatever the refresh phase.
	layer.set_enabled(true)
	layer.tick(0.0, volumes, conns)
	assert_eq(_mm().instance_count, 2)


func test_cap_512_with_overflow_count_in_kind_order() -> void:
	watch_signals(layer)
	layer.set_enabled(true)
	var bridges: Array = []
	for i: int in 100:
		bridges.append(_rec(i, 5000 + i, "bridge", "ontology"))
	var notes: Array = []
	for i: int in 500:
		notes.append(_rec(i, i, "annotates"))
	layer.set_records("bridge", bridges)
	layer.set_records("annotates", notes)
	layer.tick(0.0, volumes, conns)
	assert_eq(_mm().instance_count, LinkLayer.MAX_LINKS)
	assert_eq(LinkLayer.MAX_LINKS, 512)
	assert_eq(layer.overflow_count(), 88)
	assert_eq(layer.draw_calls(), 1, "never more than one draw call")
	assert_signal_emitted_with_parameters(layer, "counts_changed", [512, 0, 88])
	# All 500 annotations come first; 12 bridges fill the rest.
	var last: Transform3D = _xf(511)
	_assert_colour(_custom(511), LinkLayer.KIND_COLOURS["bridge"], "last is a bridge")
	_assert_colour(_custom(499), LinkLayer.KIND_COLOURS["annotates"], "500th is an annotation")
	assert_true(last.basis.y.length() > 0.0)


func test_drop_connection_empties_its_records_at_once() -> void:
	layer.set_enabled(true)
	layer.set_records("annotates", [_rec(1, 5), _rec(2, 6)])
	layer.set_records("bridge", [_rec(3, 7, "bridge")])
	layer.tick(0.0, volumes, conns)
	assert_eq(_mm().instance_count, 3)
	layer.drop_connection(&"codescape")
	assert_eq(_mm().instance_count, 1, "only the bridge is left, without a tick")
	assert_eq(layer.record_count(), 1)
	layer.drop_connection(&"visionclaw")
	assert_eq(_mm().instance_count, 0)
	assert_eq(layer.record_count(), 0)
	assert_eq(layer.draw_calls(), 0)


func test_reconnect_drops_the_old_generation_before_drawing() -> void:
	layer.set_enabled(true)
	layer.set_records("annotates", [_rec(1, 5, "annotates", "ontology", [1, 1])])
	layer.tick(0.0, volumes, conns)
	assert_eq(_mm().instance_count, 1)
	# Codescape reconnects: its generation moves on and nobody called drop yet.
	conns[&"codescape"].generation = 2
	layer.tick(LinkLayer.REFRESH_HZ, volumes, conns)
	assert_eq(_mm().instance_count, 0, "stale record never drawn")
	assert_eq(layer.record_count(), 0, "and discarded")
	# The fresh join arrives with the new generation and draws.
	layer.set_records("annotates", [_rec(1, 5, "annotates", "ontology", [2, 1])])
	layer.tick(0.0, volumes, conns)
	assert_eq(_mm().instance_count, 1)
	# A VisionClaw reconnect invalidates the target side the same way.
	conns[&"visionclaw"].generation = 7
	layer.tick(1.0, volumes, conns)
	assert_eq(_mm().instance_count, 0)


func test_closed_codescape_connection_draws_nothing_without_error() -> void:
	layer.set_enabled(true)
	layer.set_records("annotates", [_rec(1, 5), _rec(2, 6)])
	conns[&"codescape"].connected = false
	layer.tick(0.0, volumes, conns)
	assert_eq(_mm().instance_count, 0)
	assert_eq(layer.unresolved_count(), 2, "counted, not drawn")
	assert_eq(layer.draw_calls(), 0)
	# With the connection gone from the table altogether, still nothing and no error.
	conns.erase(&"codescape")
	layer.tick(1.0, volumes, conns)
	assert_eq(_mm().instance_count, 0)


func test_hidden_volume_hides_its_links_without_counting_them() -> void:
	layer.set_enabled(true)
	layer.set_records("annotates", [_rec(1, 5), _rec(2, 6, "annotates", "knowledge")])
	volumes[&"ontology"].visible_state = false
	layer.tick(0.0, volumes, conns)
	assert_eq(_mm().instance_count, 1)
	assert_eq(layer.unresolved_count(), 0)
	volumes[&"knowledge"].is_closed = true
	layer.tick(1.0, volumes, conns)
	assert_eq(_mm().instance_count, 0)


func test_endpoints_refresh_at_10_hz() -> void:
	layer.set_enabled(true)
	layer.set_records("annotates", [_rec(1, 5)])
	layer.tick(0.0, volumes, conns)
	var before: Vector3 = _xf(0).origin
	conns[&"visionclaw"].positions[5] = Vector3(50, 0, 0)
	layer.tick(0.03, volumes, conns)
	assert_eq(_xf(0).origin, before, "not yet")
	layer.tick(0.08, volumes, conns)
	assert_ne(_xf(0).origin, before, "after 1/10 s")


func test_external_unresolved_is_reported() -> void:
	watch_signals(layer)
	layer.set_enabled(true)
	layer.set_records("annotates", [_rec(1, 5)])
	layer.set_unresolved("annotates", 4)
	layer.tick(0.0, volumes, conns)
	assert_signal_emitted_with_parameters(layer, "counts_changed", [1, 4, 0])
	layer.drop_connection(&"codescape")
	assert_eq(layer.unresolved_count(), 0, "dropped with its records")
