extends "res://addons/gut/test.gd"
const VolumeArrangement := preload("res://scripts/volume_arrangement.gd")
const SpatialVolume := preload("res://scripts/spatial_volume.gd")
const VolumeScene := preload("res://scenes/EmulatedVolume.tscn")

# ADR-2158 D7 and D9, phase-1 contract §1, §3.4 and §8 (agent B).


func _arrangement() -> Node3D:
	var arr: Node3D = VolumeArrangement.new()
	add_child_autofree(arr)
	for id: StringName in [&"knowledge", &"ontology", &"codescape"]:
		var v: Node3D = VolumeScene.instantiate()
		v.volume_id = id
		v.bounds = VolumeArrangement.DEFAULT_BOUNDS
		arr.add_volume(v, VolumeArrangement.DEFAULT_YAWS[id])
	return arr


func test_ring_placement() -> void:
	var arr := _arrangement()
	var r := VolumeArrangement.RING_RADIUS
	var h := VolumeArrangement.RING_HEIGHT
	var k: Node3D = arr.volume(&"knowledge")
	var o: Node3D = arr.volume(&"ontology")
	var c: Node3D = arr.volume(&"codescape")
	assert_eq(arr.volumes().size(), 3)
	assert_true(k.position.is_equal_approx(Vector3(-r * sin(PI / 3.0), h, -r * 0.5)), "knowledge −60° front-left: %s" % k.position)
	assert_true(o.position.is_equal_approx(Vector3(r * sin(PI / 3.0), h, -r * 0.5)), "ontology +60° front-right: %s" % o.position)
	assert_true(c.position.is_equal_approx(Vector3(0.0, h, -r)), "codescape 0° ahead: %s" % c.position)
	for v: Node3D in [k, o, c]:
		var flat := Vector3(v.position.x, 0.0, v.position.z)
		assert_almost_eq(flat.length(), r, 1.0e-5, "%s on the ring" % v.volume_id)
		# Local +Z looks back at the ring centre.
		assert_true(v.transform.basis.z.is_equal_approx(-flat.normalized()), "%s faces the user" % v.volume_id)
	assert_null(arr.volume(&"nope"))


func test_user_move_overrides_until_reset() -> void:
	var arr := _arrangement()
	var o: Node3D = arr.volume(&"ontology")
	var moved := Transform3D(Basis(Vector3.UP, 1.0), Vector3(3.0, 0.5, 2.0))
	o.transform = moved
	var extra: Node3D = VolumeScene.instantiate()
	extra.volume_id = &"df:ab:space"
	arr.add_volume(extra, 180.0)
	arr.set_arrangement_scale(0.5)
	assert_eq(o.transform, moved, "a user move holds through add_volume and rescale")
	assert_true(extra.position.is_equal_approx(Vector3(0.0, VolumeArrangement.RING_HEIGHT, VolumeArrangement.RING_RADIUS)), "180° behind")
	arr.reset_default()
	assert_true(o.transform.is_equal_approx(arr.default_transform(&"ontology")), "reset restores the ring")
	assert_almost_eq(arr.arrangement_scale(), 0.5, 1.0e-9, "reset leaves the scale to the shim")


func test_arrangement_aabb() -> void:
	var arr := _arrangement()
	var expected := AABB()
	var first := true
	for v: Node3D in arr.volumes():
		for i: int in 8:
			var p: Vector3 = v.transform * (SpatialVolume.corner_sign(i) * 0.5)
			if first:
				expected = AABB(p, Vector3.ZERO)
				first = false
			else:
				expected = expected.expand(p)
	var got: AABB = arr.arrangement_aabb()
	assert_true(got.position.is_equal_approx(expected.position) and got.size.is_equal_approx(expected.size), "%s vs %s" % [got, expected])
	# Bounds count: a bigger codescape box widens the AABB.
	arr.volume(&"codescape").request_bounds(Vector3(1.0, 3.0, 1.0))
	assert_almost_eq(arr.arrangement_aabb().size.y, 3.0, 1.0e-5)
	# Hidden and closed volumes drop out; none left gives an empty AABB.
	arr.volume(&"codescape").request_visible(false)
	assert_almost_eq(arr.arrangement_aabb().size.y, 1.0, 1.0e-5)
	arr.volume(&"knowledge").close()
	arr.volume(&"ontology").request_visible(false)
	assert_eq(arr.arrangement_aabb(), AABB())


func test_native_fit_is_idempotent() -> void:
	var arr := _arrangement()
	var container := Vector3(1.5, 1.5, 1.5)
	var s1 := SpatialVolume.fit_scale(container, arr.arrangement_aabb().size)
	arr.set_arrangement_scale(s1)
	var s2 := SpatialVolume.fit_scale(container, arr.arrangement_aabb().size)
	assert_almost_eq(s2, s1, 1.0e-9, "the aabb is measured before the arrangement scale")
	assert_almost_eq(arr.scale.x, s1, 1.0e-6)
	arr.set_arrangement_scale(0.0)
	arr.set_arrangement_scale(NAN)
	assert_almost_eq(arr.arrangement_scale(), s1, 1.0e-6, "bad scales ignored")
	arr.set_arrangement_scale(1.0)
	arr.reset_default()
	assert_eq(arr.scale, Vector3.ONE, "fallback restores 1:1")


func test_world_points_follow_the_arrangement_scale() -> void:
	var arr := _arrangement()
	var k: Node3D = arr.volume(&"knowledge")
	var p0: Vector3 = k.global_position
	arr.set_arrangement_scale(0.5)
	assert_true(k.global_position.is_equal_approx(p0 * 0.5), "scaled about the user's start position")
