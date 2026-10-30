extends "res://addons/gut/test.gd"
const LinkPicker := preload("res://scripts/link_picker.gd")

# Spatial-volume UX feature 3: the ray against the drawn link segments. A fake
# layer hands out segments; the picker takes the closest approach, keeps those
# within max_dist_m, and returns the nearest along the ray with its segment
# parameter t (0 at `from`).


class FakeLayer extends Node:
	var segs: Array = []

	func drawn_segments() -> Array:
		return segs


var layer: FakeLayer


func _seg(a: Vector3, b: Vector3, sid: int, tid: int) -> Dictionary:
	return {
		"a": a, "b": b,
		"record": {"kind": "annotates", "source": {"id": sid}, "target": {"id": tid}},
		"from": {"volume": &"codescape", "conn": &"codescape", "node_id": sid},
		"to": {"volume": &"ontology", "conn": &"visionclaw", "node_id": tid},
		"highlighted": false,
	}


func before_each() -> void:
	layer = FakeLayer.new()
	add_child_autofree(layer)
	# A horizontal link 2 m ahead, from x = -1 to x = 1, and one 4 m ahead.
	layer.segs = [_seg(Vector3(-1, 0, -2), Vector3(1, 0, -2), 1, 5), _seg(Vector3(-1, 0, -4), Vector3(1, 0, -4), 2, 6)]


func test_hit_returns_record_ends_t_and_distance() -> void:
	var hit := LinkPicker.pick(Vector3.ZERO, Vector3(0.5, 0, -2).normalized(), layer)
	assert_eq((hit["from"] as Dictionary)["node_id"], 1)
	assert_eq((hit["to"] as Dictionary)["node_id"], 5)
	assert_eq((hit["record"] as Dictionary)["kind"], "annotates")
	assert_almost_eq(float(hit["t"]), 0.75, 1e-4)
	assert_almost_eq(float(hit["distance"]), Vector3(0.5, 0, -2).length(), 1e-4)
	assert_true((hit["world_pos"] as Vector3).is_equal_approx(Vector3(0.5, 0, -2)))


func test_nearest_along_the_ray_wins() -> void:
	# Straight ahead crosses both links; the 2 m one is nearer.
	var hit := LinkPicker.pick(Vector3.ZERO, Vector3(0, 0, -1), layer)
	assert_eq((hit["from"] as Dictionary)["node_id"], 1)
	layer.segs.remove_at(0)
	hit = LinkPicker.pick(Vector3.ZERO, Vector3(0, 0, -1), layer)
	assert_eq((hit["from"] as Dictionary)["node_id"], 2)


func test_tolerance_and_misses() -> void:
	# 1.5 cm above the 2 m link passes within 2 cm; 3 cm does not.
	var near := Vector3(0, 0.015, -2).normalized()
	var far := Vector3(0, 0.03, -2).normalized()
	assert_false(LinkPicker.pick(Vector3.ZERO, near, layer).is_empty())
	assert_eq(LinkPicker.pick(Vector3.ZERO, far, layer), {}, "3 cm at 2 m, 6 cm at 4 m")
	assert_eq(LinkPicker.pick(Vector3.ZERO, far, layer, 0.04).size(), 6, "a wider tolerance catches it")
	assert_eq(LinkPicker.pick(Vector3.ZERO, near, layer, 0.005), {}, "a narrower one does not")
	assert_eq(LinkPicker.pick(Vector3.ZERO, Vector3(0, 1, 0), layer), {})
	assert_eq(LinkPicker.pick(Vector3.ZERO, Vector3(0, 0, 1), layer), {}, "behind")
	assert_eq(LinkPicker.pick(Vector3.ZERO, Vector3.ZERO, layer), {}, "zero direction")
	assert_eq(LinkPicker.pick(Vector3.ZERO, Vector3(0, 0, -1), null), {}, "no layer")


func test_past_the_end_of_a_segment_misses() -> void:
	assert_eq(LinkPicker.pick(Vector3.ZERO, Vector3(1.5, 0, -2).normalized(), layer, 0.02).size(), 0)


func test_far_end_is_the_end_away_from_the_hit() -> void:
	var near_from := LinkPicker.pick(Vector3.ZERO, Vector3(-0.8, 0, -2).normalized(), layer)
	assert_lt(float(near_from["t"]), 0.5)
	assert_eq(LinkPicker.far_end(near_from)["node_id"], 5)
	var near_to := LinkPicker.pick(Vector3.ZERO, Vector3(0.8, 0, -2).normalized(), layer)
	assert_eq(LinkPicker.far_end(near_to)["node_id"], 1)
	assert_eq(LinkPicker.far_end({}), {})


func test_closest_handles_crossing_parallel_and_degenerate_segments() -> void:
	var c := LinkPicker.closest(Vector3.ZERO, Vector3(0, 0, -1), Vector3(-1, 0.01, -2), Vector3(1, 0.01, -2))
	assert_almost_eq(float(c[0]), 2.0, 1e-5)
	assert_almost_eq(float(c[1]), 0.5, 1e-5)
	assert_almost_eq(float(c[2]), 0.01, 1e-5)
	# Parallel to the ray, 1 cm to the side.
	c = LinkPicker.closest(Vector3.ZERO, Vector3(0, 0, -1), Vector3(0.01, 0, -2), Vector3(0.01, 0, -3))
	assert_almost_eq(float(c[2]), 0.01, 1e-5)
	assert_almost_eq(float(c[0]), 2.0, 1e-5)
	# Zero-length segment: a point.
	c = LinkPicker.closest(Vector3.ZERO, Vector3(0, 0, -1), Vector3(0, 0.02, -1), Vector3(0, 0.02, -1))
	assert_almost_eq(float(c[2]), 0.02, 1e-5)
	assert_almost_eq(float(c[1]), 0.0, 1e-5)


func test_works_against_a_real_link_layer() -> void:
	var real: Node3D = preload("res://scripts/link_layer.gd").new()
	add_child_autofree(real)
	assert_eq(LinkPicker.pick(Vector3.ZERO, Vector3(0, 0, -1), real), {}, "off layer has no segments")
