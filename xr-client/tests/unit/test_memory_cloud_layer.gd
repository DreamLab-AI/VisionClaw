extends "res://addons/gut/test.gd"

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
	return (l.get_node("CloudRoot/Points") as MultiMeshInstance3D).multimesh


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
	assert_eq(mm.buffer.size(), 9 * 16, "12 transform + 4 colour floats per sprite")
	var root: Node3D = l.get_node("CloudRoot")
	assert_almost_eq(root.scale.x, 5.0, 0.0001, "desktop cloudScale")
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
	assert_false((l.get_node("CloudRoot/Points") as Node3D).visible, "points hidden")
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
	var tube: MeshInstance3D = l.get_node("CloudRoot/Route/Tube")
	assert_not_null(tube.mesh)
	assert_eq(tube.mesh.get_surface_count(), 1, "one surface, one draw call")
	var beads: MultiMesh = (l.get_node("CloudRoot/Route/Beads") as MultiMeshInstance3D).multimesh
	assert_eq(beads.instance_count, 3 * 2 + 2, "bead + halo per knot, comet + glow")
	var rings: MultiMesh = (l.get_node("CloudRoot/Route/Rings") as MultiMeshInstance3D).multimesh
	assert_eq(rings.instance_count, 3 + 1, "root, answer, pulse + one sidecar mark")
	assert_eq(l.apply_route_json('{"type":"memoryRoute","snapshotId":"s1","seq":1,"sentAt":10,"path":[1,2]}'), "stale")
	assert_eq(l.apply_route_json('{"type":"memoryRoute","snapshotId":"s1","seq":2,"sentAt":11,"path":[]}'), "clear")
	assert_false(l.route_active())
	assert_eq(l.apply_route_json("{oops"), "error")
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
	var mat: ShaderMaterial = (l.get_node("CloudRoot/Route/Tube") as MeshInstance3D).material_override
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
	var caps: Dictionary = FrameBudget.new().allocate(1000, 1500, 0, 0, 0, 0, 6, int(d["cloud_rows"]), int(d["route_rows"]), int(d["route_sidecar"]))
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
