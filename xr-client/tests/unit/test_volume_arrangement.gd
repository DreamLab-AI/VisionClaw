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


# --- UX features 4 and 5: slots, moves, bring-to-front -------------------------

func _settle(arr: Node3D, seconds: float) -> void:
	var t := 0.0
	while t < seconds:
		arr.tick(1.0 / 90.0)
		t += 1.0 / 90.0


func test_bring_to_front_swaps_slots_and_eases() -> void:
	var arr := _arrangement()
	var k: Node3D = arr.volume(&"knowledge")
	var c: Node3D = arr.volume(&"codescape")
	var k0: Transform3D = k.transform
	assert_eq(arr.occupant(0.0), &"codescape")
	assert_true(arr.bring_to_front(&"knowledge"), "knowledge goes to the front")
	assert_almost_eq(arr.slot_yaw(&"knowledge"), 0.0, 1.0e-6)
	assert_almost_eq(arr.slot_yaw(&"codescape"), -60.0, 1.0e-6, "the occupant takes the vacated slot")
	assert_eq(arr.occupant(0.0), &"knowledge")
	assert_true(arr.is_easing(&"knowledge") and arr.is_easing(&"codescape") and not arr.is_easing(&"ontology"))
	assert_eq(k.transform, k0, "nothing moves before the first tick")
	arr.tick(0.35 * 0.5)
	var mid: Vector3 = k.position
	assert_false(mid.is_equal_approx(k0.origin), "moving at the half-way tick")
	assert_false(mid.is_equal_approx(VolumeArrangement.ring_transform(0.0).origin), "not there yet at half time")
	assert_almost_eq(mid.distance_to(k0.origin), mid.distance_to(VolumeArrangement.ring_transform(0.0).origin), 1.0e-4,
		"smoothstep is half-way at half time")
	_settle(arr, 0.4)
	assert_false(arr.is_easing(), "the ease ends within its duration")
	assert_true(k.transform.is_equal_approx(VolumeArrangement.ring_transform(0.0)), "knowledge at 0°")
	assert_true(c.transform.is_equal_approx(VolumeArrangement.ring_transform(-60.0)), "codescape at −60°")


func test_bring_to_front_never_compounds_scale() -> void:
	var arr := _arrangement()
	arr.set_arrangement_scale(0.5)
	for i: int in 12:
		var id: StringName = [&"knowledge", &"ontology", &"codescape"][i % 3]
		arr.bring_to_front(id, 0.35)
		arr.tick(0.1)  # interrupt half-way through: a new ease starts from where it stands
	_settle(arr, 0.5)
	for v: Node3D in arr.volumes():
		assert_almost_eq(v.transform.basis.get_scale().x, 1.0, 1.0e-5, "%s stays unit scale" % v.volume_id)
		assert_almost_eq(v.transform.basis.get_scale().y, 1.0, 1.0e-5)
	assert_almost_eq(arr.arrangement_scale(), 0.5, 1.0e-9, "the arrangement scale is untouched")
	# The three slots are still a permutation of the defaults.
	var yaws: Array = []
	for v: Node3D in arr.volumes():
		yaws.append(roundi(arr.slot_yaw(v.volume_id)))
	yaws.sort()
	assert_eq(yaws, [-60, 0, 60])


func test_bring_to_front_noop_and_snap() -> void:
	var arr := _arrangement()
	assert_false(arr.bring_to_front(&"codescape"), "already in front and at rest")
	assert_false(arr.bring_to_front(&"nope"), "unknown id")
	assert_true(arr.bring_to_front(&"ontology", 0.0), "zero duration snaps")
	assert_false(arr.is_easing())
	assert_true(arr.volume(&"ontology").transform.is_equal_approx(VolumeArrangement.ring_transform(0.0)))
	assert_true(arr.volume(&"codescape").transform.is_equal_approx(VolumeArrangement.ring_transform(60.0)))


func test_moved_box_and_front() -> void:
	var arr := _arrangement()
	arr.set_arrangement_scale(2.0)
	var k: Node3D = arr.volume(&"knowledge")
	# A move carrying a scaled basis lands at unit scale.
	arr.move_volume(&"knowledge", Transform3D(Basis.IDENTITY.scaled(Vector3.ONE * 2.0), Vector3(0.2, 1.0, -0.4)))
	assert_true(arr.is_moved(&"knowledge"))
	assert_almost_eq(k.transform.basis.get_scale().x, 1.0, 1.0e-6, "orthonormalised")
	assert_true(k.position.is_equal_approx(Vector3(0.2, 1.0, -0.4)))
	arr.tick(1.0)
	assert_true(k.position.is_equal_approx(Vector3(0.2, 1.0, -0.4)), "tick leaves a user move alone")
	# A moved box still holds its slot, so the swap hands codescape −60°.
	assert_true(arr.bring_to_front(&"knowledge"))
	assert_false(arr.is_moved(&"knowledge"))
	_settle(arr, 0.4)
	assert_true(k.transform.is_equal_approx(VolumeArrangement.ring_transform(0.0)))
	assert_true(arr.volume(&"codescape").transform.is_equal_approx(VolumeArrangement.ring_transform(-60.0)))
	# A moved front box comes back to the front.
	arr.move_volume(&"knowledge", Transform3D(Basis.IDENTITY, Vector3(1.0, 1.0, 1.0)))
	assert_true(arr.bring_to_front(&"knowledge", 0.0), "moved front box returns")
	assert_true(k.transform.is_equal_approx(VolumeArrangement.ring_transform(0.0)))


func test_move_cancels_ease() -> void:
	var arr := _arrangement()
	arr.bring_to_front(&"ontology")
	arr.tick(0.05)
	arr.move_volume(&"ontology", Transform3D(Basis.IDENTITY, Vector3(0.0, 2.0, 0.0)))
	assert_false(arr.is_easing(&"ontology"), "a grab takes the box out of its ease")
	_settle(arr, 0.5)
	assert_true(arr.volume(&"ontology").position.is_equal_approx(Vector3(0.0, 2.0, 0.0)))
	arr.mark_moved(&"nope")  # unknown ids are ignored


func test_reset_restores_slots_and_bounds() -> void:
	var arr := _arrangement()
	arr.bring_to_front(&"knowledge")
	arr.tick(0.1)
	arr.move_volume(&"ontology", Transform3D(Basis.IDENTITY, Vector3(5.0, 0.0, 0.0)))
	arr.volume(&"codescape").request_bounds(Vector3(2.0, 2.0, 2.0))
	arr.reset_default()
	assert_false(arr.is_easing())
	assert_false(arr.is_moved(&"ontology"))
	for v: Node3D in arr.volumes():
		assert_true(v.transform.is_equal_approx(arr.default_transform(v.volume_id)), "%s back on its default slot" % v.volume_id)
		assert_almost_eq(arr.slot_yaw(v.volume_id), VolumeArrangement.DEFAULT_YAWS[v.volume_id], 1.0e-6)
	assert_eq(arr.volume(&"codescape").bounds, Vector3(2.0, 2.0, 2.0), "a plain reset keeps bounds (native shim fallback)")
	arr.reset_default(true)
	assert_eq(arr.volume(&"codescape").bounds, VolumeArrangement.DEFAULT_BOUNDS, "the HUD reset restores bounds")
	assert_true(is_nan(arr.slot_yaw(&"nope")))
	assert_eq(arr.occupant(180.0), &"")


func test_ease_transform_endpoints() -> void:
	var a := VolumeArrangement.ring_transform(-60.0)
	var b := VolumeArrangement.ring_transform(60.0)
	assert_true(VolumeArrangement.ease_transform(a, b, 0.0).is_equal_approx(a))
	assert_true(VolumeArrangement.ease_transform(a, b, 1.0).is_equal_approx(b))
	assert_true(VolumeArrangement.ease_transform(a, b, 2.0).is_equal_approx(b), "clamped")
	var m := VolumeArrangement.ease_transform(a, b, 0.5)
	assert_true(m.basis.is_equal_approx(Basis.IDENTITY), "half-way between ±60° faces 0°: %s" % m.basis)
