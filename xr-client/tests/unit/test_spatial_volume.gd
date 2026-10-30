extends "res://addons/gut/test.gd"
const SpatialVolume := preload("res://scripts/spatial_volume.gd")

# ADR-2158 D5, phase-1 contract §3.2 and §8 (agent B): the shared fit maths.


func test_fit_scale_isotropic() -> void:
	assert_almost_eq(SpatialVolume.fit_scale(Vector3.ONE, Vector3(4.0, 4.0, 4.0)), 0.25, 1.0e-9)
	assert_almost_eq(SpatialVolume.fit_scale(Vector3(2.0, 2.0, 2.0), Vector3.ONE), 2.0, 1.0e-9)


func test_fit_scale_anisotropic_checks_each_axis() -> void:
	# The binding axis moves across x, y and z; the result is always the minimum ratio.
	assert_almost_eq(SpatialVolume.fit_scale(Vector3.ONE, Vector3(10.0, 2.0, 1.0)), 0.1, 1.0e-9, "x binds")
	assert_almost_eq(SpatialVolume.fit_scale(Vector3.ONE, Vector3(1.0, 10.0, 2.0)), 0.1, 1.0e-9, "y binds")
	assert_almost_eq(SpatialVolume.fit_scale(Vector3.ONE, Vector3(2.0, 1.0, 10.0)), 0.1, 1.0e-9, "z binds")
	# Anisotropic bounds: 4/8 = 0.5 on x beats 1/1 on y and 2/1 on z.
	assert_almost_eq(SpatialVolume.fit_scale(Vector3(4.0, 1.0, 2.0), Vector3(8.0, 1.0, 1.0)), 0.5, 1.0e-9)
	var b := Vector3(1.0, 1.5, 0.5)
	var e := Vector3(3.0, 7.0, 0.25)
	var s := SpatialVolume.fit_scale(b, e)
	for i: int in 3:
		assert_true(e[i] * s <= b[i] + 1.0e-9, "axis %d fits" % i)


func test_fit_scale_ignores_axes_below_min_extent() -> void:
	# A flat graph (z extent 0) fits on x and y alone.
	assert_almost_eq(SpatialVolume.fit_scale(Vector3.ONE, Vector3(2.0, 4.0, 0.0)), 0.25, 1.0e-9)
	assert_almost_eq(SpatialVolume.fit_scale(Vector3.ONE, Vector3(2.0, 4.0, 1.0e-7)), 0.25, 1.0e-9)
	# Just above MIN_EXTENT qualifies. (Vector3 is f32: 1e-6 itself rounds to
	# 9.99999997e-7, below the double MIN_EXTENT, so it does not.)
	assert_almost_eq(SpatialVolume.fit_scale(Vector3.ONE, Vector3(2.0e-6, 0.0, 0.0)), 5.0e5, 1.0)


func test_fit_scale_all_axes_below_min_extent_is_one() -> void:
	assert_eq(SpatialVolume.fit_scale(Vector3.ONE, Vector3.ZERO), 1.0, "empty")
	assert_eq(SpatialVolume.fit_scale(Vector3(3.0, 2.0, 1.0), Vector3(1.0e-7, 0.0, 9.0e-7)), 1.0, "a single point")


# --- the volume (EmulatedVolume.tscn over a fixture source) -----------------

const VolumeScene := preload("res://scenes/EmulatedVolume.tscn")
const EmulatedVolume := preload("res://scripts/emulated_volume.gd")
const FixtureContentSource := preload("res://tests/fixtures/fixture_content_source.gd")

const DT := 1.0 / 90.0


func _volume(b := Vector3.ONE) -> Node3D:
	var v: Node3D = VolumeScene.instantiate()
	v.volume_id = &"knowledge"
	v.bounds = b
	add_child_autofree(v)
	# Somewhere off the origin, turned, so world ≠ local in every check.
	v.transform = Transform3D(Basis(Vector3.UP, 0.7), Vector3(1.2, 1.3, -0.9))
	return v


# Anisotropic graph-unit cloud: 400 × 20 × 100, off-centre, seeded.
func _points() -> PackedVector3Array:
	var rng := RandomNumberGenerator.new()
	rng.seed = 2158
	var pts := PackedVector3Array()
	for i: int in 300:
		pts.append(Vector3(rng.randf_range(-100.0, 300.0), rng.randf_range(40.0, 60.0), rng.randf_range(-50.0, 50.0)))
	# Pin the extremes so the extent is exact.
	pts.append(Vector3(-100.0, 40.0, -50.0))
	pts.append(Vector3(300.0, 60.0, 50.0))
	return pts


func _outside_count(v: Node3D, src: RefCounted) -> int:
	var n := 0
	for p: Vector3 in src.points:
		if not v.contains_world(v.world_from_content(p)):
			n += 1
	return n


func test_scene_builds_its_children_before_add_child() -> void:
	var v: Node3D = VolumeScene.instantiate()
	assert_not_null(v.content, "Content resolved at instantiate")
	assert_eq(v.content.name, &"Content")
	v.free()
	var w: Node3D = _volume()
	assert_not_null(w.frame.mesh, "wire box built")
	assert_eq(w.frame.mesh.get_surface_count(), 1, "one surface, one draw call")
	assert_eq(w.frame.mesh.surface_get_array_len(0), 24, "12 edges")
	assert_eq(w.handles.get_child_count(), 9, "8 handle areas + 1 marker MultiMesh")
	for i: int in 8:
		var a: Area3D = w.handles.get_node("Handle%d" % i)
		assert_false(a.monitoring, "handle %d never monitors" % i)
		assert_eq(a.get_child_count(), 1, "handle %d has its CollisionShape3D" % i)
	assert_eq(w.overhead_draw_calls(), 1, "frame only while unfocused")
	w.set_focused(true)
	assert_eq(w.overhead_draw_calls(), 2, "frame + markers while focused")


func test_new_without_scene_builds_children_too() -> void:
	var v: Node3D = EmulatedVolume.new()
	add_child_autofree(v)
	assert_not_null(v.content)
	assert_not_null(v.get_node_or_null("Frame"))
	assert_eq(v.handles.get_child_count(), 9)


func test_e7_every_point_inside_after_fit() -> void:
	var v: Node3D = _volume(Vector3(2.0, 1.0, 0.5))
	var src := FixtureContentSource.new(_points())
	v.attach(src)
	v.tick(DT)
	assert_eq(_outside_count(v, src), 0, "drawn transform keeps every point inside")
	# The same check on the target transform, in volume-local metres.
	var t: Transform3D = v.target_content_transform()
	var half := Vector3(1.0, 0.5, 0.25)
	for p: Vector3 in src.points:
		var q: Vector3 = t * p
		for k: int in 3:
			assert_true(absf(q[k]) <= half[k] + 1.0e-4, "target keeps axis %d inside" % k)
	# Tight: z binds (0.5 / 100 = 0.005 against 2/400 and 1/20), so z touches the faces.
	assert_almost_eq(v.target_scale(), 0.005, 1.0e-7)
	assert_almost_eq((t * Vector3(0.0, 0.0, 50.0)).z, 0.25, 1.0e-5)
	# Centred on the volume origin.
	assert_true((t * Vector3(100.0, 50.0, 0.0)).is_equal_approx(Vector3.ZERO), "centre maps to the origin")


func test_e7_resize_every_frame_never_outside_more_than_one_frame() -> void:
	var v: Node3D = _volume()
	var src := FixtureContentSource.new(_points())
	v.attach(src)
	v.tick(DT)
	var run := 0
	var worst := 0
	for f: int in 240:
		# Shrink and grow every axis independently, every frame.
		var b := Vector3(
			1.2 + 0.9 * sin(f * 0.21),
			0.8 + 0.6 * sin(f * 0.37 + 1.0),
			0.6 + 0.5 * cos(f * 0.29))
		v.set_runtime_bounds(b, EmulatedVolume.BoundsMode.BOUNDED)
		v.tick(DT)
		if _outside_count(v, src) > 0:
			run += 1
			worst = maxi(worst, run)
		else:
			run = 0
	assert_true(worst <= 1, "longest excursion %d frame(s)" % worst)


func test_growth_eases_and_converges() -> void:
	var v: Node3D = _volume()
	var src := FixtureContentSource.new(_points())
	v.attach(src)
	v.tick(DT)
	var s0: float = v.current_scale()
	watch_signals(v)
	v.set_runtime_bounds(Vector3(2.0, 2.0, 2.0), EmulatedVolume.BoundsMode.BOUNDED)
	assert_signal_emitted(v, "fit_changed")
	assert_almost_eq(v.target_scale(), 2.0 * s0, 1.0e-7)
	v.tick(DT)
	var s1: float = v.current_scale()
	assert_true(s1 > s0 and s1 < 2.0 * s0, "one eased step, not a jump")
	assert_almost_eq(s1, s0 + (2.0 * s0 - s0) * EmulatedVolume.FIT_EASE, 1.0e-6, "FIT_EASE at 90 Hz")
	for i: int in 200:
		v.tick(DT)
	assert_almost_eq(v.current_scale(), 2.0 * s0, 1.0e-6, "converged")


func test_shrink_snaps_inside() -> void:
	var v: Node3D = _volume(Vector3(2.0, 2.0, 2.0))
	var src := FixtureContentSource.new(_points())
	v.attach(src)
	v.tick(DT)
	v.request_bounds(Vector3(0.5, 0.5, 0.5))
	v.tick(DT)
	assert_almost_eq(v.current_scale(), v.target_scale(), 1.0e-9, "a shrink is immediate")
	assert_eq(_outside_count(v, src), 0)


func test_immersive_is_one_to_one() -> void:
	var v: Node3D = _volume()
	var src := FixtureContentSource.new(_points())
	v.attach(src)
	v.tick(DT)
	watch_signals(v)
	v.request_bounds_mode(EmulatedVolume.BoundsMode.IMMERSIVE)
	assert_signal_emitted_with_parameters(v, "bounds_changed", [&"knowledge", Vector3.ONE, EmulatedVolume.BoundsMode.IMMERSIVE])
	assert_eq(v.target_scale(), 1.0)
	for i: int in 300:
		v.tick(DT)
	assert_almost_eq(v.current_scale(), 1.0, 1.0e-5, "1:1")
	assert_false(v.frame.visible, "no frame in immersive mode")
	assert_true(v.contains_world(Vector3(1.0e4, 0.0, 0.0)), "immersive has no box")
	v.request_bounds_mode(EmulatedVolume.BoundsMode.BOUNDED)
	v.tick(DT)
	assert_almost_eq(v.current_scale(), 0.0025, 1.0e-7, "bounded again: refits, shrink snaps")
	assert_eq(_outside_count(v, src), 0)


func test_request_bounds_granted_clamped_denied() -> void:
	var v: Node3D = _volume()
	watch_signals(v)
	v.request_bounds(Vector3(0.5, 1.5, 3.0))
	assert_signal_emitted_with_parameters(v, "bounds_changed", [&"knowledge", Vector3(0.5, 1.5, 3.0), 0])
	assert_eq(v.bounds, Vector3(0.5, 1.5, 3.0), "granted as asked")
	assert_eq(v.frame.scale, Vector3(0.5, 1.5, 3.0), "frame follows")
	v.request_bounds(Vector3(10.0, 0.05, 1.0))
	assert_eq(v.bounds, Vector3(4.0, 0.2, 1.0), "clamped to [0.2, 4.0]")
	assert_signal_emit_count(v, "bounds_changed", 2)
	v.request_bounds(Vector3(-1.0, 1.0, 1.0))
	assert_signal_emitted_with_parameters(v, "bounds_request_denied", [&"knowledge", Vector3(-1.0, 1.0, 1.0)])
	v.request_bounds(Vector3(NAN, 1.0, 1.0))
	v.request_bounds(Vector3(1.0, INF, 1.0))
	v.request_bounds(Vector3(1.0, 1.0, 0.0))
	assert_signal_emit_count(v, "bounds_request_denied", 4)
	assert_eq(v.bounds, Vector3(4.0, 0.2, 1.0), "denials change nothing")
	v.request_bounds_mode(7)
	assert_signal_emitted_with_parameters(v, "bounds_mode_request_denied", [&"knowledge", 7])
	assert_eq(v.bounds_mode, EmulatedVolume.BoundsMode.BOUNDED)


func test_closed_stops_ticking_and_denies() -> void:
	var v: Node3D = _volume()
	var src := FixtureContentSource.new(_points())
	v.attach(src)
	v.tick(DT)
	watch_signals(v)
	v.close()
	assert_signal_emitted_with_parameters(v, "closed", [&"knowledge"])
	assert_true(v.is_closed)
	assert_null(src.instance, "source unmounted")
	assert_null(v.source())
	var before: Transform3D = v.content.transform
	v.undo_yaw_deg = 45.0
	for i: int in 30:
		v.tick(DT)
	assert_eq(v.content.transform, before, "a closed volume does not tick")
	v.request_bounds(Vector3(2.0, 2.0, 2.0))
	assert_signal_emitted(v, "bounds_request_denied")
	v.request_bounds_mode(EmulatedVolume.BoundsMode.IMMERSIVE)
	assert_signal_emitted(v, "bounds_mode_request_denied")
	v.attach(src)
	assert_null(v.source(), "nothing attaches to a closed volume")
	v.close()
	assert_signal_emit_count(v, "closed", 1, "closes once")


func test_undo_yaw_is_applied_before_measuring() -> void:
	var v: Node3D = _volume(Vector3(2.0, 1.0, 0.5))
	var src := FixtureContentSource.new(_points())
	v.attach(src)
	v.tick(DT)
	assert_almost_eq(v.target_scale(), 0.005, 1.0e-7, "no yaw: z (100) binds")
	v.undo_yaw_deg = 90.0
	v.tick(DT)
	# Rotated 90° about +Y: the 400-unit x extent now lies along z (0.5 / 400).
	assert_almost_eq(v.target_scale(), 0.00125, 1.0e-8, "yawed extent measured")
	var r := Basis(Vector3.UP, deg_to_rad(90.0))
	var t: Transform3D = v.target_content_transform()
	assert_true(t.basis.is_equal_approx(r.scaled(Vector3.ONE * 0.00125)), "R.scaled(s) on Content")
	assert_true(t.origin.is_equal_approx(-(r * Vector3(100.0, 50.0, 0.0)) * 0.00125), "-(R c) s")
	assert_true(v.content.transform.basis.is_equal_approx(t.basis), "shrink snapped to the target")
	assert_eq(_outside_count(v, src), 0)


func test_grabbed_node_is_excluded_from_the_fit() -> void:
	var pts := PackedVector3Array([Vector3(-1, -1, -1), Vector3(1, 1, 1), Vector3(99, 0, 0)])
	var v: Node3D = _volume()
	var src := FixtureContentSource.new(pts)
	v.attach(src)
	v.tick(DT)
	assert_almost_eq(v.target_scale(), 1.0 / 100.0, 1.0e-7)
	src.grabbed_id = 2
	v.set_runtime_bounds(v.bounds, v.bounds_mode)
	assert_almost_eq(v.target_scale(), 0.5, 1.0e-7, "outlier held: fits the rest")


func test_source_recomputed_on_cadence() -> void:
	var v: Node3D = _volume()
	var src := FixtureContentSource.new(PackedVector3Array([Vector3(-1, -1, -1), Vector3(1, 1, 1)]))
	v.attach(src)
	v.tick(DT)
	assert_almost_eq(v.target_scale(), 0.5, 1.0e-7)
	src.set_points(PackedVector3Array([Vector3(-2, -2, -2), Vector3(2, 2, 2)]))
	var ticks := 0
	while v.target_scale() > 0.3 and ticks < 50:
		v.tick(DT)
		ticks += 1
	assert_almost_eq(v.target_scale(), 0.25, 1.0e-7, "re-read the source")
	assert_true(ticks <= EmulatedVolume.FIT_RECOMPUTE, "within FIT_RECOMPUTE ticks (%d)" % ticks)


func test_manual_latch_holds_until_bounds_change() -> void:
	var v: Node3D = _volume()
	var src := FixtureContentSource.new(_points())
	v.attach(src)
	v.tick(DT)
	v.manual_latched = true
	var held: Transform3D = Transform3D(Basis.IDENTITY, Vector3(0.1, 0.2, 0.3))
	v.content.transform = held
	for i: int in 20:
		v.tick(DT)
	assert_eq(v.content.transform, held, "latched: no refit, no ease")
	v.set_runtime_bounds(Vector3(0.5, 0.5, 0.5), EmulatedVolume.BoundsMode.BOUNDED)
	assert_false(v.manual_latched, "runtime bounds un-latch")
	v.tick(DT)
	assert_eq(_outside_count(v, src), 0)


func test_resize_handle_drives_runtime_bounds() -> void:
	var v: Node3D = _volume()
	var ctrl := Node3D.new()
	add_child_autofree(ctrl)
	var corner_world: Vector3 = v.global_transform * Vector3(0.5, 0.5, 0.5)
	assert_eq(v.handle_at(corner_world + Vector3(0.01, 0.0, 0.0), 0.05), 7, "+x+y+z corner")
	assert_eq(v.handle_at(corner_world + Vector3(0.3, 0.0, 0.0), 0.05), -1, "too far")
	ctrl.global_position = corner_world
	watch_signals(v)
	v.begin_resize(7, ctrl)
	assert_true(v.is_resizing())
	ctrl.global_position = v.global_transform * Vector3(0.75, 0.5, 0.5)
	v.update_resize(ctrl)
	assert_true(v.bounds.is_equal_approx(Vector3(1.5, 1.0, 1.0)), "x grows by twice the travel: %s" % v.bounds)
	assert_signal_emitted(v, "bounds_changed")
	ctrl.global_position = v.global_transform * Vector3(9.0, -9.0, 0.5)
	v.update_resize(ctrl)
	assert_true(v.bounds.is_equal_approx(Vector3(4.0, 0.2, 1.0)), "clamped: %s" % v.bounds)
	var h: Area3D = v.handles.get_node("Handle7")
	assert_true(h.position.is_equal_approx(Vector3(2.0, 0.1, 0.5)), "handles follow the corners")
	v.end_resize()
	assert_false(v.is_resizing())
	v.request_interactable(false)
	assert_eq(v.handle_at(v.global_transform * Vector3(2.0, 0.1, 0.5), 0.05), -1, "not interactable")
	v.begin_resize(7, ctrl)
	assert_false(v.is_resizing(), "no resize while not interactable")


func test_visible_focus_selection_signals() -> void:
	var v: Node3D = _volume()
	var src := FixtureContentSource.new(_points())
	v.attach(src)
	v.tick(DT)
	watch_signals(v)
	v.request_visible(false)
	assert_signal_emitted_with_parameters(v, "visible_changed", [&"knowledge", false])
	assert_false(v.visible)
	assert_false(v.frame.visible)
	assert_eq(v.overhead_draw_calls(), 0)
	v.request_visible(false)
	assert_signal_emit_count(v, "visible_changed", 1, "no change, no signal")
	v.request_visible(true)
	assert_true(v.frame.visible)
	v.request_interactable(false)
	assert_signal_emitted_with_parameters(v, "interactable_changed", [&"knowledge", false])
	v.set_focused(true)
	assert_signal_emitted_with_parameters(v, "focus_changed", [&"knowledge", true])
	v.select(&"visionclaw", 42)
	assert_signal_emitted_with_parameters(v, "selection_changed", [&"knowledge", &"visionclaw", 42])
	assert_eq(v.selected_node, 42)
	v.select(&"visionclaw", -1)
	assert_eq(v.selected_node, -1)
	assert_eq(v.selected_conn, &"")


func test_fixture_mount_unmount_leaves_no_orphans() -> void:
	var v: Node3D = _volume()
	var a := FixtureContentSource.new(_points())
	var b := FixtureContentSource.new(PackedVector3Array([Vector3.ZERO, Vector3.ONE]), &"fixture:0b0010")
	v.attach(a)
	assert_eq(v.content.get_child_count(), 1)
	assert_eq(a.instance.multimesh.instance_count, a.points.size())
	v.attach(b)
	assert_null(a.instance, "replaced source unmounted")
	assert_eq(v.content.get_child_count(), 1, "at most one source")
	v.detach()
	assert_eq(v.content.get_child_count(), 0)
	assert_no_new_orphans()
