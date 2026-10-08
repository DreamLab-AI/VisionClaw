extends "res://addons/gut/test.gd"

## visionclaw_tri_layout::SEPARATION (crate constant; the fixture holds it)
const SEPARATION := 190.0

# Memory cloud + route layer (scripts/memory_cloud_layer.gd, XR WP6/WP7).
# Drives the real load path (_on_http_completed with a JSON body), the real
# memoryRoute gate, the flash lookup and the hover pick. No network: the
# layer is configured with an empty base, so reload() resolves at once.

const Layer := preload("res://scripts/memory_cloud_layer.gd")
const OK_RESULT := HTTPRequest.RESULT_SUCCESS


static func snapshot_json(sid: String, n: int) -> String:
	var pos := PackedStringArray()
	var meta := PackedStringArray()
	for i in n:
		pos.append("%d,%d,%d" % [i * 10 - 50, (i % 3) * 5, -20])
		var ns: String = ["project-state", "patterns", "dream-cycle"][i % 3]
		meta.append('{"id":"id%d","key":"k%d","namespace":"%s","sourceType":"agent","updatedAt":%d}' % [i, i, ns, 1000 + i])
	return '{"version":1,"snapshotId":"%s","generatedAt":1,"dim":384,"count":%d,"positions":[%s],"metadata":[%s],"namespaces":["dream-cycle","patterns","project-state"],"sourceTypes":["agent"],"strata":[],"excludedNamespaces":[],"vectorsUrl":"vectors?snapshot=%s"}' % [sid, n, ",".join(pos), ",".join(meta), sid]


func _make() -> Node3D:
	var l: Node3D = Layer.new()
	add_child(l)
	await get_tree().process_frame
	return l


func _load(l: Node3D, sid: String, n: int) -> void:
	l._on_http_completed(OK_RESULT, 200, PackedStringArray(), snapshot_json(sid, n).to_utf8_buffer())


func _points(l: Node3D) -> MultiMesh:
	return (l.get_node("CloudRoot/CloudCore/Points") as MultiMeshInstance3D).multimesh


# 2026-10-08: the server samples 30 000, the headset draws at most the frame
# budget's cap (8 000). The Memory button states drawn of sampled when they
# differ, never the sample size alone.
func test_memory_button_states_drawn_of_sampled() -> void:
	var l: Node3D = await _make()
	l._enabled = true
	_load(l, "s1", 9)
	l.flush()
	assert_eq(l.status_label(), "Memory: 9", "all drawn: one number")
	l._cloud.set_sprite_cap(4)
	l.flush()
	assert_eq(int(l.drawn_count()), 4)
	assert_eq(l.status_label(), "Memory: 4 of 9", "drawn of sampled")
	# a frame-budget cap change re-announces the label to the HUD
	watch_signals(l)
	l.apply_frame_caps({"cloud_sprites": 6})
	assert_signal_emitted(l, "status_changed")
	assert_eq(l.status_label(), "Memory: 6 of 9")
	l.queue_free()


func test_snapshot_loads_into_one_stride_16_multimesh() -> void:
	var l: Node3D = await _make()
	l._enabled = true
	_load(l, "s1", 9)
	l.flush()
	assert_eq(l.state(), "ready")
	assert_eq(l.point_count(), 9)
	var mm := _points(l)
	assert_eq(mm.instance_count, 9)
	assert_true(mm.use_colors, "palette colour per instance")
	assert_true(mm.use_custom_data, "flash emphasis per instance")
	assert_eq(mm.buffer.size(), 9 * 20, "12 transform + 4 colour + 4 custom floats per sprite")
	var root: Node3D = l.get_node("CloudRoot")
	assert_almost_eq(root.scale.x, 50.0, 0.0001, "no graph: cloudScale × the memory body scale (10)")
	l.queue_free()


func test_malformed_body_fails_and_keeps_the_previous_cloud() -> void:
	var l: Node3D = await _make()
	l._enabled = true
	_load(l, "s1", 4)
	l._on_http_completed(OK_RESULT, 200, PackedStringArray(), "{\"version\":1,\"snapshotId\":\"s2\",\"count\":3,\"positions\":[1,2],\"metadata\":[]}".to_utf8_buffer())
	assert_eq(l.state(), "failed")
	assert_eq(l.snapshot_id(), "s1", "short payload never replaces a good snapshot")
	l.queue_free()


func test_forbidden_hides_quietly_without_retry() -> void:
	var l: Node3D = await _make()
	l._enabled = true
	_load(l, "s1", 4)
	l._on_http_completed(OK_RESULT, 403, PackedStringArray(), PackedByteArray())
	assert_eq(l.state(), "forbidden")
	assert_false((l.get_node("CloudRoot/CloudCore/Points") as Node3D).visible, "points hidden")
	assert_eq(l.retry_in_ms(), -1, "no polling of a locked endpoint")
	assert_eq(l.status_label(), "Memory: Locked")
	l.queue_free()


func test_unavailable_honours_retry_after_then_doubles() -> void:
	var l: Node3D = await _make()
	l._enabled = true
	l._on_http_completed(OK_RESULT, 503, PackedStringArray(["Retry-After: 3"]), PackedByteArray())
	assert_eq(l.state(), "unavailable")
	assert_almost_eq(l.retry_in_ms(), 3000, 100)
	l._on_http_completed(OK_RESULT, 503, PackedStringArray(), PackedByteArray())
	assert_almost_eq(l.retry_in_ms(), 5000, 100, "no Retry-After: 5 s")
	l._on_http_completed(OK_RESULT, 503, PackedStringArray(), PackedByteArray())
	assert_almost_eq(l.retry_in_ms(), 10000, 100, "then doubles")
	l.queue_free()


func test_stale_reloads_once_then_fails() -> void:
	var l: Node3D = await _make()
	l._enabled = true
	var before: int = l.requests_sent()
	l._on_http_completed(OK_RESULT, 409, PackedStringArray(), PackedByteArray())
	# reload() with an empty base resolves as unreachable at once
	assert_eq(l.requests_sent(), before + 1, "409 reloads at once")
	l._stale_retry_used = true
	l._on_http_completed(OK_RESULT, 409, PackedStringArray(), PackedByteArray())
	assert_eq(l.state(), "failed")
	assert_eq(l.requests_sent(), before + 1, "but only once")
	l.queue_free()


func test_flash_lookup_lands_on_real_rows() -> void:
	var l: Node3D = await _make()
	_load(l, "s1", 6)
	var rows: PackedInt32Array = l.resolve_flash("k4", "patterns")
	assert_eq(Array(rows), [4])
	assert_eq(l.last_flash_match(), "key")
	assert_eq(l.point_local(4), Vector3(-10, 5, -20))
	rows = l.resolve_flash("not-sampled", "dream-cycle")
	assert_eq(l.last_flash_match(), "namespace")
	assert_eq(rows.size(), 2, "both dream-cycle rows stand in (cap 3)")
	assert_eq(l.resolve_flash("x", "nowhere").size(), 0)
	assert_eq(l.last_flash_match(), "none")
	var w: Vector3 = l.world_point(4)
	assert_eq(w, l.cloud_root().global_transform * Vector3(-10, 5, -20))
	l.queue_free()


func test_hover_labels_key_and_namespace() -> void:
	var l: Node3D = await _make()
	l._enabled = true
	l.visible = true
	_load(l, "s1", 6)
	var target: Vector3 = l.world_point(2)
	var origin := target + Vector3(0, 0, 3)
	var row: int = l.update_hover(origin, (target - origin).normalized())
	assert_eq(row, 2)
	assert_string_contains(l.hover_text(), "k2")
	assert_string_contains(l.hover_text(), "dream-cycle / agent")
	assert_eq(l.update_hover(origin, Vector3.UP), -1)
	assert_eq(l.hover_text(), "", "label hidden off target")
	l.queue_free()


func test_route_applies_draws_and_dims_off_route() -> void:
	var l: Node3D = await _make()
	l._enabled = true
	l.visible = true
	_load(l, "s1", 6)
	var v: String = l.apply_route_json('{"type":"memoryRoute","snapshotId":"s1","seq":1,"sentAt":10,"path":[0,3,5],"sidecar":[5]}')
	assert_eq(v, "apply")
	assert_true(l.route_active())
	var tube: MeshInstance3D = l.get_node("CloudRoot/CloudCore/Route/Tube")
	assert_not_null(tube.mesh)
	assert_eq(tube.mesh.get_surface_count(), 1, "one surface, one draw call")
	var beads: MultiMesh = (l.get_node("CloudRoot/CloudCore/Route/Beads") as MultiMeshInstance3D).multimesh
	assert_eq(beads.instance_count, 3 * 2 + 2 + 12, "bead + halo per knot, comet + glow, 12 cue dots")
	var rings: MultiMesh = (l.get_node("CloudRoot/CloudCore/Route/Rings") as MultiMeshInstance3D).multimesh
	assert_eq(rings.instance_count, 3 + 1, "root, answer, pulse + one sidecar mark")
	assert_eq(l.apply_route_json('{"type":"memoryRoute","snapshotId":"s1","seq":1,"sentAt":10,"path":[1,2]}'), "stale")
	assert_eq(l.apply_route_json('{"type":"memoryRoute","snapshotId":"s1","seq":2,"sentAt":11,"path":[]}'), "clear")
	assert_false(l.route_active())
	assert_eq(l.apply_route_json("{oops"), "error")
	l.queue_free()


# Operator decision 2026-10-08: the headset draws the route's lines (tube,
# beads, comet, guide dots) at a tenth of the desktop thickness and brightness.
func test_route_lines_draw_at_the_xr_factor() -> void:
	assert_almost_eq(MemoryRoute.xr_route_thickness_scale(), 0.1, 1e-6)
	assert_almost_eq(MemoryRoute.xr_route_glow_scale(), 0.1, 1e-6)
	var l: Node3D = await _make()
	l._enabled = true
	l.visible = true
	_load(l, "s1", 6)
	assert_eq(l.apply_route_json('{"type":"memoryRoute","snapshotId":"s1","seq":1,"sentAt":10,"path":[0,3,5]}'), "apply")
	var tube: MeshInstance3D = l.get_node("CloudRoot/CloudCore/Route/Tube")
	var arrays: Array = tube.mesh.surface_get_arrays(0)
	var widest := 0.0
	for uv in arrays[Mesh.ARRAY_TEX_UV2]:
		widest = maxf(widest, uv.x)
	# the outer glow sheath: desktop TUBE_R 0.45 × 7, at the factor
	assert_almost_eq(widest, 0.45 * 7.0 * 0.1, 1e-4, "tube a tenth as thick")
	var brightest := 0.0
	for c in arrays[Mesh.ARRAY_COLOR]:
		brightest = maxf(brightest, maxf(c.r, maxf(c.g, c.b)))
	assert_lt(brightest, 0.25, "tube a tenth as bright (desktop core ~1.5)")
	l.queue_free()


func test_route_for_another_snapshot_waits_for_the_reload_then_applies() -> void:
	var l: Node3D = await _make()
	_load(l, "old", 6)
	# cloud hidden: the frame waits for the next load instead of fetching now
	assert_eq(l.apply_route_json('{"type":"memoryRoute","snapshotId":"new","seq":1,"sentAt":1,"path":[0,1]}'), "reload")
	assert_false(l.route_active(), "nothing drawn against the wrong cloud")
	_load(l, "new", 6)
	assert_true(l.route_active(), "pending route applied after the reload")
	l.queue_free()


func test_route_mismatch_with_cloud_on_fetches_and_drops_on_failure() -> void:
	var l: Node3D = await _make()
	l._enabled = true
	_load(l, "old", 6)
	var before: int = l.requests_sent()
	assert_eq(l.apply_route_json('{"type":"memoryRoute","snapshotId":"new","seq":1,"sentAt":1,"path":[0,1]}'), "reload")
	assert_eq(l.requests_sent(), before + 1, "one reload")
	# the empty base makes that reload fail at once: the route is dropped, the old cloud kept
	assert_false(l.route_active())
	assert_eq(l.snapshot_id(), "old")
	assert_eq(l.apply_route_json('{"type":"memoryRoute","snapshotId":"new","seq":2,"sentAt":2,"path":[0,1]}'), "drop", "never a second reload for the same snapshot")
	l.queue_free()


func test_reduced_motion_freezes_the_route() -> void:
	var l: Node3D = await _make()
	l._enabled = true
	l.reduced_motion = true
	_load(l, "s1", 6)
	l.apply_route_json('{"type":"memoryRoute","snapshotId":"s1","seq":1,"sentAt":1,"path":[0,3,5]}')
	var mat: ShaderMaterial = (l.get_node("CloudRoot/CloudCore/Route/Tube") as MeshInstance3D).material_override
	assert_almost_eq(float(mat.get_shader_parameter("head_u")), 1.0, 0.0001, "shown converged at once")
	assert_almost_eq(float(mat.get_shader_parameter("comet_u")), -1.0, 0.0001, "no comet")
	l.queue_free()


func test_budget_reports_layer_costs() -> void:
	var l: Node3D = await _make()
	l._enabled = true
	_load(l, "s1", 6)
	l.apply_route_json('{"type":"memoryRoute","snapshotId":"s1","seq":1,"sentAt":1,"path":[0,3,5]}')
	var b: Dictionary = l.budget()
	assert_eq(int(b["cloud_draw_calls"]), 1)
	assert_eq(int(b["cloud_triangles"]), 6, "one triangle per sprite")
	assert_eq(int(b["route_draw_calls"]), 3)
	assert_gt(int(b["route_triangles"]), 0)
	assert_lte(int(b["route_triangles"]), 7500, "route stays inside ROUTE_TRIANGLE_BUDGET")
	l.queue_free()


func test_shaders_compile_in_this_renderer() -> void:
	for path in ["res://materials/memory_point.gdshader", "res://materials/memory_route.gdshader", "res://materials/memory_ring.gdshader", "res://materials/memory_bead.gdshader"]:
		var sh: Shader = load(path)
		assert_not_null(sh, path)
		assert_eq(sh.get_mode(), Shader.MODE_SPATIAL, path)
		assert_gt(sh.get_shader_uniform_list().size() + 1, 0, path)


func test_meshes_match_the_rust_triangle_budget() -> void:
	var layer = await _make()
	var t: Dictionary = layer.mesh_triangles()
	assert_eq(t["sprite"], 1, "one triangle per sprite")
	assert_eq(t["bead"], t["bead_expected"], "bead disc matches BEAD_TRIANGLES")
	assert_eq(t["bead"], 1, "beads are one-triangle discs")



func test_frame_budget_demand_and_caps() -> void:
	var l: Node3D = await _make()
	l._enabled = true
	_load(l, "s1", 6)
	l.apply_route_json('{"type":"memoryRoute","snapshotId":"s1","seq":1,"sentAt":1,"path":[0,3,5],"sidecar":[1]}')
	var d: Dictionary = l.frame_demand()
	assert_eq(int(d["cloud_rows"]), 6)
	assert_eq(int(d["route_rows"]), 3)
	assert_eq(int(d["route_sidecar"]), 1)
	var full: int = int(l._route.sample_count())
	assert_eq(full, 2 * 16 + 1, "short route at full detail")
	var caps: Dictionary = FrameBudget.new().allocate(1000, 1500, 0, 0, 0, 0, 6, int(d["cloud_rows"]), int(d["route_rows"]), int(d["route_sidecar"]), 0)
	assert_false(bool(caps["over_budget"]))
	assert_eq(int(caps["cloud_sprites"]), 6)
	# a starved budget: fewest sprites the keep rows allow, one sample per hop
	l.apply_frame_caps({"cloud_sprites": 2, "route_ring_cap": 3})
	l.flush()
	assert_eq(l.drawn_count(), 4, "route + sidecar rows stay drawn under any cap")
	assert_eq(int(l._route.sample_count()), 3)
	assert_true(l.route_active(), "route still shown")
	l.apply_frame_caps({"cloud_sprites": 8000, "route_ring_cap": 121})
	l.flush()
	assert_eq(l.drawn_count(), 6)
	assert_eq(int(l._route.sample_count()), full)
	l.set_enabled(false)
	assert_eq(int(l.frame_demand()["cloud_rows"]), 0, "hidden cloud demands nothing")
	l.queue_free()


# --- memory_flash row emphasis (xr-pulse set_row_emphasis contract) ----------

# Read back the uploaded MultiMesh buffer (works under the headless dummy
# renderer, unlike the per-instance getters).
func _custom(l: Node3D, row: int) -> Color:
	var b: PackedFloat32Array = _points(l).buffer
	var o: int = int(l._cloud.instance_of_row(row)) * 20
	return Color(b[o + 16], b[o + 17], b[o + 18], b[o + 19])


func _size(l: Node3D, row: int) -> float:
	var b: PackedFloat32Array = _points(l).buffer
	var o: int = int(l._cloud.instance_of_row(row)) * 20
	return Vector3(b[o], b[o + 4], b[o + 8]).length()


func test_row_emphasis_restyles_sprites_without_geometry() -> void:
	var l: Node3D = await _make()
	l._enabled = true
	_load(l, "s1", 6)
	l.flush()
	var count_before: int = _points(l).instance_count
	var tris_before: int = int(l.budget()["cloud_triangles"])
	var base: float = _size(l, 1)
	assert_eq(_custom(l, 1), Color(0, 0, 0, 1), "neutral before any flash")
	var tint := Color.html("39ff14")
	l.set_row_emphasis(PackedInt32Array([1, 4]), PackedColorArray([tint, tint]), PackedFloat32Array([2.5, 1.5]), PackedFloat32Array([2.0, 1.0]))
	# Instance data round-trips through the renderer's MultiMesh storage, which
	# under GL Compatibility keeps less than float32 precision (0x39/255 reads back
	# 0.2235); compare within 1/255 per channel. Exact Color equality passed only
	# under GUT 9.7's comparator and the headless dummy renderer.
	_assert_colour_near(_custom(l, 1), Color(tint.r, tint.g, tint.b, 2.5), "emphasis tint and gain")
	assert_almost_eq(_size(l, 1), base * 2.0, 1e-4, "scale multiplies the sprite")
	assert_almost_eq(_size(l, 4), base, 1e-4)
	assert_eq(_points(l).instance_count, count_before, "no instances added")
	assert_eq(int(l.budget()["cloud_triangles"]), tris_before, "no triangles added")
	# replace-all: row 1 left out returns to normal
	l.set_row_emphasis(PackedInt32Array([4]), PackedColorArray([tint]), PackedFloat32Array([2.0]), PackedFloat32Array([1.5]))
	assert_eq(_custom(l, 1), Color(0, 0, 0, 1), "dropped row back to neutral")
	assert_almost_eq(_size(l, 1), base, 1e-4, "and back to its size")
	assert_eq(l.emphasised_count(), 1)
	# survives a full buffer upload
	l.flush()
	_assert_colour_near(_custom(l, 4), Color(tint.r, tint.g, tint.b, 2.0), "re-applied after upload")
	assert_almost_eq(_size(l, 4), base * 1.5, 1e-4)
	# empty clears
	l.set_row_emphasis(PackedInt32Array(), PackedColorArray(), PackedFloat32Array(), PackedFloat32Array())
	assert_eq(l.emphasised_count(), 0)
	assert_eq(_custom(l, 4), Color(0, 0, 0, 1))
	assert_almost_eq(_size(l, 4), base, 1e-4)
	l.queue_free()


func test_row_emphasis_clamps_caps_and_ignores_undrawn_rows() -> void:
	var l: Node3D = await _make()
	l._enabled = true
	_load(l, "s1", 120)
	l.flush()
	var rows := PackedInt32Array()
	var tints := PackedColorArray()
	var gains := PackedFloat32Array()
	var scales := PackedFloat32Array()
	for r in 100:
		rows.append(r)
		tints.append(Color.RED)
		gains.append(9.0)
		scales.append(7.0)
	rows.append(5000)  # not a row: ignored
	tints.append(Color.RED)
	gains.append(2.0)
	scales.append(1.0)
	var base: float = _size(l, 0)
	l.set_row_emphasis(rows, tints, gains, scales)
	assert_eq(l.emphasised_count(), 64, "at most 64 rows")
	assert_eq(_custom(l, 0).a, 2.5, "gain clamped")
	assert_almost_eq(_size(l, 0), base * 2.0, 1e-4, "scale clamped")
	assert_eq(_custom(l, 64), Color(0, 0, 0, 1), "row past the cap untouched")
	# mismatched lengths use the common prefix
	l.set_row_emphasis(PackedInt32Array([2, 3]), PackedColorArray([Color.BLUE]), PackedFloat32Array([2.0, 2.0]), PackedFloat32Array([1.0, 1.0]))
	assert_eq(l.emphasised_count(), 1)
	l.queue_free()


# xr-pulse's test_beat_pulse flash test, against the real layer instead of its
# FakeCloud: a memory_flash on a loaded cloud restyles the matching sprite, adds
# no ring geometry, and clears when the burst ends.
class RealScene extends Node3D:
	var _memory_cloud = null


func test_beat_pulse_flash_lands_on_the_real_cloud() -> void:
	var root := Node3D.new()
	add_child(root)
	var scene := RealScene.new()
	root.add_child(scene)
	var l: Node3D = Layer.new()
	scene.add_child(l)
	scene._memory_cloud = l
	await get_tree().process_frame
	l._enabled = true
	_load(l, "s1", 6)
	l.flush()
	var effects := Node3D.new()
	root.add_child(effects)
	var beat: Node = (load("res://scripts/beat_pulse.gd") as GDScript).new()
	root.add_child(beat)
	beat.setup(scene, null, null, null, null, effects, func() -> Vector3: return Vector3.ZERO)
	var flash := '{"type":"memory_flash","data":{"key":"k3","namespace":"project-state","action":"store"}}'
	beat.on_text(flash, "memory_flash")
	var bursts: Node = effects.get_node("MemoryBursts")
	assert_eq(bursts.slot_count(), 0, "no ring geometry while the cloud is shown")
	await get_tree().process_frame
	await get_tree().process_frame
	assert_eq(l.emphasised_count(), 1, "the flashed row is restyled")
	var c: Color = _custom(l, 3)
	assert_gt(c.a, 1.0, "brightened")
	# store base #39ff14, hue-nudged by namespace as the desktop does (semantic.rs)
	var want: Color = MemoryFlashCodec.parse(flash)[0]["color"]
	assert_eq(Color(c.r, c.g, c.b).to_html(false), want.to_html(false), "desktop burst colour for (store, project-state)")
	await get_tree().create_timer(2.2).timeout
	await get_tree().process_frame
	await get_tree().process_frame
	assert_eq(l.emphasised_count(), 0, "cleared when the burst ends")
	assert_eq(_custom(l, 3), Color(0, 0, 0, 1))
	root.queue_free()
	await get_tree().process_frame


func test_burst_pool_matches_the_allocator() -> void:
	var b: Node3D = (load("res://scripts/memory_bursts.gd") as GDScript).new()
	add_child(b)
	await get_tree().process_frame
	var spec: PackedInt32Array = FrameBudget.new().burst_pool_spec()
	var mm: MultiMesh = (b.get_node("BurstMulti") as MultiMeshInstance3D).multimesh
	assert_eq(spec[0], b.POOL_SIZE, "slots")
	assert_eq(mm.mesh.get_faces().size() / 3, spec[1], "triangles per ring")
	b.queue_free()


func _assert_colour_near(got: Color, want: Color, label: String) -> void:
	for k in range(4):
		assert_almost_eq(got[k], want[k], 1.0 / 255.0, "%s channel %d" % [label, k])


# --- xr-parity: placement, route on top, framing cue, agreement line ---------

func _graph_bounds(c: Vector3, r: float) -> Callable:
	return func() -> PackedFloat32Array: return PackedFloat32Array([c.x, c.y, c.z, r])


func test_cloud_is_framed_on_the_graph_centre_and_radius() -> void:
	var l: Node3D = await _make()
	l._enabled = true
	l.reduced_motion = true  # snap, so one frame places it
	l.graph_bounds_source = _graph_bounds(Vector3(90, -3, -14), 300.0)
	_load(l, "s1", 9)
	l._process(0.016)
	var p: Dictionary = l.placement()
	var cb: PackedFloat32Array = l._frame.cloud_bounds()
	assert_almost_eq(float(p["scale"]) * cb[3], 3000.0, 0.1, "cloudScale 5: cloud radius = 10 graph radii")
	assert_eq(p["offset"], -Vector3(cb[0], cb[1], cb[2]), "inner node recentres the cloud on its core")
	# on the memory vertex's ray (−Z) at half the distance that would clear both
	# graph bodies (operator decision 2026-10-08, ADR-2135), so it overlaps them
	var pos: Vector3 = p["position"]
	assert_almost_eq(pos.x, 90.0, 0.01, "on the memory ray through the graph centre")
	assert_almost_eq(pos.y, -3.0, 0.01)
	var R: float = SEPARATION * 2.0 / sqrt(3.0)
	var need := 1.25 * (3000.0 + 300.0)  # CLEARANCE × (cloud + graph)
	var b := R * cos(deg_to_rad(120.0))
	var clear_d := b + sqrt(b * b - R * R + need * need)
	assert_almost_eq(pos.z, -14.0 - 0.5 * clear_d, 0.5, "half the clear distance")
	assert_lt(pos.z, -14.0 - R, "still further out than the memory vertex")
	for sx in [-1.0, 1.0]:
		var g := Vector3(90.0 + sx * R * sin(deg_to_rad(60.0)), -3.0, -14.0 + R * cos(deg_to_rad(60.0)))
		assert_lt(pos.distance_to(g) + 300.0, 3000.0, "the closer ×10 cloud encloses the graph at %s" % g)
	# the cloud's own centre lands on the placement position
	var core_world: Vector3 = l.cloud_root().global_transform * Vector3(cb[0], cb[1], cb[2])
	assert_true(core_world.is_equal_approx(l.global_transform * pos), "%s" % core_world)
	# the user's scale setting keeps its meaning
	l.cloud_scale = 2.5
	l._process(0.016)
	assert_almost_eq(float(l.placement()["scale"]) * cb[3], 1500.0, 0.1, "2.5 = five graph radii")
	l.queue_free()


func test_there_is_no_separation_control_and_world_helpers_scale_with_the_body() -> void:
	# ADR-2135 (2026-10-08): always separated, the cloud ten graphs wide; the
	# hover label and its reach grow with it so a hit stays readable.
	var l: Node3D = await _make()
	assert_false("graph_separation" in l, "no separation property")
	assert_almost_eq(l.body_scale(), 10.0, 1e-6)
	var label: Label3D = l.get_node("HoverLabel")
	assert_almost_eq(label.pixel_size, l.LABEL_PIXEL * 10.0, 1e-7, "label ×10")
	l.queue_free()


func test_placement_glides_when_physics_moves_the_graph() -> void:
	var l: Node3D = await _make()
	l._enabled = true
	l.reduced_motion = false
	var centre := [Vector3.ZERO]
	l.graph_bounds_source = func() -> PackedFloat32Array: return PackedFloat32Array([centre[0].x, centre[0].y, centre[0].z, 200.0])
	_load(l, "s1", 9)
	l._process(0.011)
	var p0: Vector3 = l.placement()["position"]
	assert_almost_eq(p0.x, 0.0, 0.01, "first placement snaps")
	centre[0] = Vector3(100, 0, 0)
	l._process(0.011)  # not yet due: 1 Hz reads
	assert_eq(l.placement()["position"], p0, "graph re-read at 1 Hz, not every frame")
	# ~1 s of 90 Hz frames: the 1 Hz read lands, then each frame moves dt/0.8 of the way
	for i in 92:
		l._process(1.0 / 90.0)
	var x: float = l.placement()["position"].x
	assert_gt(x, 0.0, "moving towards the new centre")
	assert_lt(x, 50.0, "glides rather than jumps")
	l.queue_free()


func test_route_draws_on_top_and_hud_label_above_it() -> void:
	var l: Node3D = await _make()
	var tube: MeshInstance3D = l.get_node("CloudRoot/CloudCore/Route/Tube")
	var beads: MultiMeshInstance3D = l.get_node("CloudRoot/CloudCore/Route/Beads")
	var rings: MultiMeshInstance3D = l.get_node("CloudRoot/CloudCore/Route/Rings")
	for m in [tube.material_override, beads.material_override, rings.material_override]:
		assert_eq((m as Material).render_priority, 10, "ROUTE_RENDER_PRIORITY")
		assert_string_contains((m as ShaderMaterial).shader.code, "depth_test_disabled")
	var label: Label3D = l.get_node("HoverLabel")
	assert_eq(label.render_priority, 20, "hover label above the route")
	l.queue_free()


func test_new_route_shows_the_framing_cue_and_a_repeat_does_not() -> void:
	var l: Node3D = await _make()
	l._enabled = true
	l.visible = true
	l.reduced_motion = false
	var cam := Camera3D.new()
	add_child(cam)
	cam.make_current()
	cam.global_position = Vector3(0, 0, 400)
	_load(l, "s1", 6)
	assert_eq(l.apply_route_json('{"type":"memoryRoute","snapshotId":"s1","seq":1,"sentAt":10,"path":[0,3,5]}'), "apply")
	for i in 60:
		l._process(1.0 / 60.0)
	assert_gt(l.cue_alpha(), 0.9, "cue shows after a new route")
	var beads: MultiMesh = (l.get_node("CloudRoot/CloudCore/Route/Beads") as MultiMeshInstance3D).multimesh
	var buf: PackedFloat32Array = beads.buffer
	var first_dot: int = (3 * 2 + 2) * 16
	assert_gt(buf[first_dot], 0.0, "guide dots drawn (no extra draw call: same MultiMesh)")
	for i in 300:
		l._process(1.0 / 60.0)
	assert_eq(l.cue_alpha(), 0.0, "cue over after a few seconds")
	assert_eq(l.apply_route_json('{"type":"memoryRoute","snapshotId":"s1","seq":2,"sentAt":20,"path":[0,3,5]}'), "refresh", "10 s repeat")
	l._process(0.016)
	assert_eq(l.cue_alpha(), 0.0, "the repeat neither replays the trace nor re-fires the cue")
	var mat: ShaderMaterial = (l.get_node("CloudRoot/CloudCore/Route/Tube") as MeshInstance3D).material_override
	assert_almost_eq(float(mat.get_shader_parameter("head_u")), 1.0, 0.0001, "trace stays complete")
	cam.queue_free()
	l.queue_free()


func test_reduced_motion_cue_is_steady() -> void:
	var l: Node3D = await _make()
	l._enabled = true
	l.visible = true
	l.reduced_motion = true
	var cam := Camera3D.new()
	add_child(cam)
	cam.make_current()
	cam.global_position = Vector3(0, 0, 400)
	_load(l, "s1", 6)
	l.apply_route_json('{"type":"memoryRoute","snapshotId":"s1","seq":1,"sentAt":10,"path":[0,3,5]}')
	for i in 60:
		l._process(1.0 / 60.0)
	var beads: MultiMesh = (l.get_node("CloudRoot/CloudCore/Route/Beads") as MultiMeshInstance3D).multimesh
	var a: PackedFloat32Array = beads.buffer.slice((3 * 2 + 2) * 16)
	l._process(1.0 / 60.0)
	var b: PackedFloat32Array = beads.buffer.slice((3 * 2 + 2) * 16)
	assert_eq(a, b, "no travelling wave under reduced motion")
	assert_gt(l.cue_alpha(), 0.9, "but the cue still shows")
	cam.queue_free()
	l.queue_free()


func test_agreement_line_uses_the_desktop_accounting() -> void:
	var l: Node3D = await _make()
	l._enabled = true
	_load(l, "s1", 6)
	watch_signals(l)
	l.apply_route_json('{"type":"memoryRoute","snapshotId":"s1","seq":1,"sentAt":1,"path":[0,3,5],"sidecar":[5,1],"sidecarTotal":4,"sidecarAgree":1}')
	assert_eq(l.agreement_line(), "Route: 2 of 4 sidecar hits are in the sample · 1 of 2 sampled agree with the local top-k")
	assert_signal_emitted(l, "route_stats_changed")
	l.apply_route_json('{"type":"memoryRoute","snapshotId":"s1","seq":2,"sentAt":2,"path":[]}')
	assert_eq(l.agreement_line(), "", "cleared")
	l.queue_free()


func test_held_aim_ray_draws_above_the_route_and_keeps_its_depth() -> void:
	var GraphScene: GDScript = load("res://scripts/graph_scene.gd")
	var ray: MeshInstance3D = GraphScene.make_aim_ray(5.0)
	var mat := ray.material_override as StandardMaterial3D
	var anim = MemoryRoute.create()
	assert_eq(mat.render_priority, int(anim.held_render_priority()), "HELD_RENDER_PRIORITY")
	assert_gt(mat.render_priority, int(anim.route_render_priority()), "ray above the route")
	assert_lt(mat.render_priority, int(anim.overlay_render_priority()), "HUD above the ray")
	# priority only orders the transparent pass, which runs after the opaque one
	assert_ne(mat.transparency, BaseMaterial3D.TRANSPARENCY_DISABLED, "in the transparent pass")
	# the route writes no depth, so the ray wins on order alone; keeping the
	# depth test stops its far end showing through nodes in front of it
	assert_false(mat.no_depth_test, "ray still occluded by nearer nodes")
	ray.free()


func test_radial_menu_draws_with_the_overlay_above_the_route() -> void:
	var menu: Node3D = load("res://scenes/RadialMenu.tscn").instantiate()
	add_child_autofree(menu)
	await get_tree().process_frame
	var mat := (menu.get_node("MenuPanel") as MeshInstance3D).material_override as Material
	assert_eq(mat.render_priority, int(MemoryRoute.create().overlay_render_priority()))
