extends "res://addons/gut/test.gd"
const LinkLayer := preload("res://scripts/link_layer.gd")

# Spatial-volume UX features 2 and 3: the link layer's picking and highlight
# API. drawn_segments() exposes the last refresh in world space; set_highlight
# draws touching links brighter and thicker and rings their ends, all in the
# one MultiMesh; triangle_count() charges the rings; highlighted_counterparts
# lists the far ends from the records.


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

	func _init(id: StringName) -> void:
		conn_id = id

	func node_position(id: int) -> Vector3:
		return Vector3(float(id), 0.0, 0.0)

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
	layer.set_enabled(true)
	# code 1 → onto 5, code 1 → know 6, code 2 → onto 5.
	layer.set_records("annotates", [_rec(1, 5), _rec(1, 6, "knowledge"), _rec(2, 5)])
	layer.tick(0.0, volumes, conns)


func _rec(sid: int, tid: int, tvol := "ontology", gens := [1, 1]) -> Dictionary:
	return {
		"source": {"volume": "codescape", "conn": "codescape", "key": "s%05d" % sid, "id": sid},
		"target": {"volume": tvol, "conn": "visionclaw", "key": "t%05d" % tid, "id": tid},
		"kind": "annotates",
		"provenance": {"via": "test", "generations": gens},
	}


func _mm() -> MultiMesh:
	return layer.multimesh_instance.multimesh


func _custom(i: int) -> Color:
	var b: PackedFloat32Array = _mm().buffer
	var k := i * LinkLayer.STRIDE + 12
	return Color(b[k], b[k + 1], b[k + 2], b[k + 3])


func _radius(i: int) -> float:
	var b: PackedFloat32Array = _mm().buffer
	var k := i * LinkLayer.STRIDE
	return Vector3(b[k], b[k + 4], b[k + 8]).length()


func _origin(i: int) -> Vector3:
	var b: PackedFloat32Array = _mm().buffer
	var k := i * LinkLayer.STRIDE
	return Vector3(b[k + 3], b[k + 7], b[k + 11])


func test_drawn_segments_are_world_space_in_draw_order() -> void:
	var segs: Array = layer.drawn_segments()
	assert_eq(segs.size(), 3)
	# Order: target key t00005 (sources s00001, s00002), then t00006.
	var s0: Dictionary = segs[0]
	assert_true((s0["a"] as Vector3).is_equal_approx(Vector3(1, 0, -2)))
	assert_true((s0["b"] as Vector3).is_equal_approx(Vector3(7, 0, 0)))
	assert_eq(s0["from"], {"volume": &"codescape", "conn": &"codescape", "node_id": 1})
	assert_eq(s0["to"], {"volume": &"ontology", "conn": &"visionclaw", "node_id": 5})
	assert_eq((s0["record"] as Dictionary)["kind"], "annotates")
	assert_false(s0["highlighted"])
	assert_eq((segs[2] as Dictionary)["to"]["volume"], &"knowledge")


func test_drawn_segments_follow_the_layer_transform_and_empty_when_off() -> void:
	layer.position = Vector3(0, 1, 0)
	layer.set_highlight([{"conn": &"codescape", "node_id": 1}])
	var s0: Dictionary = layer.drawn_segments()[0]
	assert_true((s0["a"] as Vector3).is_equal_approx(Vector3(1, 0, -2)), "segments stay world space")
	assert_true(_origin(0).is_equal_approx((Vector3(1, -1, -2) + Vector3(7, -1, 0)) * 0.5), "instances are layer-local")
	layer.set_enabled(false)
	assert_eq(layer.drawn_segments(), [])


func test_hidden_volume_link_is_not_a_segment() -> void:
	(volumes[&"knowledge"] as FakeVolume).visible_state = false
	layer.set_highlight([{"conn": &"codescape", "node_id": 2}])  # forces a refresh
	assert_eq(layer.drawn_segments().size(), 2)


func test_highlight_draws_brighter_thicker_and_rings_the_ends() -> void:
	assert_eq(layer.triangle_count(), 3 * LinkLayer.TRIS_PER_LINK)
	layer.set_highlight([{"conn": &"visionclaw", "node_id": 5}])
	# Links 0 and 1 touch onto 5; link 2 (to know 6) does not.
	var base: Color = LinkLayer.KIND_COLOURS["annotates"]
	var lit := base.lerp(Color.WHITE, LinkLayer.HIGHLIGHT_LIFT)
	for i: int in 2:
		assert_almost_eq(_radius(i), LinkLayer.RADIUS_M * LinkLayer.HIGHLIGHT_RADIUS_SCALE, 1e-6)
		assert_almost_eq(_custom(i).g, lit.g, 0.005, "brighter")
		assert_true((layer.drawn_segments()[i] as Dictionary)["highlighted"])
	assert_almost_eq(_radius(2), LinkLayer.RADIUS_M, 1e-6)
	assert_almost_eq(_custom(2).g, base.g, 0.005)
	# Rings: code 1, onto 5, code 2 (onto 5 shared): 3 markers after the 3 links.
	assert_eq(layer.drawn_count(), 3, "drawn_count stays links only")
	assert_eq(layer.marker_count(), 3)
	assert_eq(_mm().instance_count, 6)
	assert_eq(layer.triangle_count(), 6 * LinkLayer.TRIS_PER_LINK, "rings are charged")
	assert_eq(layer.draw_calls(), 1)
	var rings := []
	for i: int in range(3, 6):
		assert_almost_eq(_radius(i), LinkLayer.MARKER_RADIUS_M, 1e-6)
		rings.append(_origin(i))
	for p: Vector3 in [Vector3(1, 0, -2), Vector3(7, 0, 0), Vector3(2, 0, -2)]:
		var found := false
		for r: Vector3 in rings:
			found = found or r.is_equal_approx(p)
		assert_true(found, "ring at %s" % p)


func test_clearing_the_highlight_restores_the_plain_draw() -> void:
	layer.set_highlight([{"conn": &"codescape", "node_id": 1}])
	assert_eq(layer.marker_count(), 3)
	layer.set_highlight([])
	assert_eq(layer.marker_count(), 0)
	assert_eq(_mm().instance_count, 3)
	assert_eq(layer.triangle_count(), 3 * LinkLayer.TRIS_PER_LINK)
	assert_almost_eq(_radius(0), LinkLayer.RADIUS_M, 1e-6)
	assert_eq(layer.highlight_keys(), [])


func test_highlight_keys_round_trip_and_string_conns_match() -> void:
	layer.set_highlight([{"conn": "codescape", "node_id": 2}, {"conn": &"visionclaw", "node_id": 6}, "junk"])
	var keys: Array = layer.highlight_keys()
	assert_eq(keys.size(), 2)
	assert_true(keys.has({"conn": &"codescape", "node_id": 2}))
	assert_true(keys.has({"conn": &"visionclaw", "node_id": 6}))
	assert_eq(layer.marker_count(), 4, "code 2, onto 5, code 1, know 6")


func test_highlight_while_off_draws_nothing_and_applies_on_enable() -> void:
	layer.set_enabled(false)
	layer.set_highlight([{"conn": &"codescape", "node_id": 1}])
	assert_eq(layer.triangle_count(), 0)
	assert_eq(layer.marker_count(), 0)
	layer.set_enabled(true)
	layer.tick(0.0, volumes, conns)
	assert_eq(layer.marker_count(), 3)


func test_counterparts_are_the_far_ends_deduplicated() -> void:
	var from_code: Array = layer.highlighted_counterparts({"conn": &"codescape", "node_id": 1})
	assert_eq(from_code, [
		{"volume": &"ontology", "conn": &"visionclaw", "node_id": 5},
		{"volume": &"knowledge", "conn": &"visionclaw", "node_id": 6},
	])
	var from_onto: Array = layer.highlighted_counterparts({"conn": "visionclaw", "node_id": 5})
	assert_eq(from_onto, [
		{"volume": &"codescape", "conn": &"codescape", "node_id": 1},
		{"volume": &"codescape", "conn": &"codescape", "node_id": 2},
	])
	assert_eq(layer.highlighted_counterparts({"conn": &"codescape", "node_id": 99}), [])


func test_counterparts_include_hidden_volumes_but_not_stale_generations() -> void:
	(volumes[&"knowledge"] as FakeVolume).visible_state = false
	layer.tick(1.0, volumes, conns)
	assert_eq(layer.highlighted_counterparts({"conn": &"codescape", "node_id": 1}).size(), 2, "hidden still listed")
	(conns[&"visionclaw"] as FakeConn).generation = 2
	assert_eq(layer.highlighted_counterparts({"conn": &"codescape", "node_id": 1}), [], "old generation")


func test_drop_connection_clears_segments_and_rings() -> void:
	layer.set_highlight([{"conn": &"codescape", "node_id": 1}])
	layer.drop_connection(&"codescape")
	assert_eq(layer.drawn_segments(), [])
	assert_eq(layer.marker_count(), 0)
	assert_eq(layer.triangle_count(), 0)
	assert_eq(layer.draw_calls(), 0)
