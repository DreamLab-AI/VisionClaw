extends Node3D

# On-device perf benchmark scene. Runs for 30 seconds, samples per-frame stats,
# emits a single line "[XR_PERF_RESULT]={...}" to logcat (also "BENCHMARK_RESULT=…"
# for backwards-compat with the existing CI scraper), then quits.
#
# Pass criteria (PRD-008 §6):
#   - p99 frame time <= 11.1ms (90fps)
#   - max draw calls  <= 50
#   - max triangles   <= 100K
# Exit code 0 on pass, 1 on fail. Consumed by xr-godot-ci.yml on-device-perf job
# and by the xr-perf-regression bin (xr-client/rust/perf-regression) for trend
# comparison against baseline.json.

const DEFAULT_DURATION_S := 30.0
const DEFAULT_FIXTURE := "res://perf/fixtures/perf_graph_1k.json"
const RESULT_MARKER := "[XR_PERF_RESULT]"
const LEGACY_MARKER := "BENCHMARK_RESULT"
const FRAME_BUDGET_MS_P99 := 11.1
const MAX_DRAW_CALLS := 50
const MAX_TRIANGLES := 100000
# CPU budget for the per-frame graph pack (Quest XR2 Gen 2 is ~3-4x slower per
# core than HP's desktop CPU against an 8 ms CPU budget): the Rust pack
# (`last_pack_cpu_ms`: node + edge LOD build and the near-tier hand-off) and the
# whole GDScript-side rebuild (`lod_build`: pack + far-tier getters + MultiMesh
# uploads), both p99. Asserted on THREAD CPU time (CLOCK_THREAD_CPUTIME_ID), so a
# loaded host's preemption cannot fail the gate; wall time is reported beside it.
const PACK_BUDGET_MS_P99 := 2.0
const LOD_BUILD_BUDGET_MS_P99 := 3.0
# Those are steady-state budgets: the warm-up window (_tail's skip, which holds
# the one-time first plan build) is gated separately against these, ~2x the
# worst first build measured on HP (2026-10-07, 25 cold runs: pack 4.9-5.9 ms,
# LOD 10.7-16.2 ms). 33 ms = three 90 Hz frames: a one-off load hitch, no more.
const WARMUP_PACK_LIMIT_MS := 12.0
const WARMUP_LOD_BUILD_LIMIT_MS := 33.0
# The gate runs with the main thread pinned to its L3 cache domain
# (BinaryProtocolClient.pin_thread_to_l3; thread_cpu.rs has the measurement):
# cross-domain migrations on a multi-L3 host made p99 intermittent.
var _cpu_affinity := PackedInt32Array()
# Memory cloud + route layers (XR WP6/WP7): a synthetic snapshot fed through the
# real parse path and a relayed route through the real memoryRoute gate.
# metadata/memory_cloud_rows = 0 turns them off (graph-only baseline).
const MemoryCloudLayer := preload("res://scripts/memory_cloud_layer.gd")
const DEFAULT_MEMORY_ROWS := 6000
const MEMORY_NAMESPACES := 40
const MEMORY_ROUTE_HOPS := 12

var duration_s: float = DEFAULT_DURATION_S
var fixture_path: String = DEFAULT_FIXTURE
var memory_cloud_rows: int = DEFAULT_MEMORY_ROWS
var memory_route_hops: int = MEMORY_ROUTE_HOPS
var memory_route_sidecar: int = 5
# FrameBudget (rust frame_budget.rs) caps for the graph's near tiers; the
# lod.rs defaults until the allocator runs.
const GRAPH_DRAW_CALLS := 6  # graph-only benchmark, measured (gems, halos, impostors, cylinders, ribbons, hulls)
var _gem_cap: int = NodeLod.NEAR_CAP
var _edge_cap: int = NodeLod.NEAR_EDGE_CAP
var _budget_report: Dictionary = {"enabled": false}
# Everything a headset frame carries outside the budgeted layers: the HUD,
# both controllers' aim rays (as graph_scene._ensure_controller_rays builds
# them) and two remote avatars. A calibration phase renders them alone, with the
# HUD forced to re-render every frame, and the renderer's global worst-frame
# count becomes FrameBudget's other_tris (and its draw calls are added to the
# graph's).
var with_extras: bool = true
var bursts_on: bool = true
const CALIBRATE_SKIP := 5
const CALIBRATE_FRAMES := 30  # 3 per HUD page (8 pages) + settle
var _phase: int = 0          # 0 calibrating extras, 1 measuring
var _calib_frames: int = 0
var _other_tris: int = 0
var _other_dc: int = 0
var _calib_page: String = ""
var _prev_page: String = ""
var _page_cost: Dictionary = {}  # HUD page -> [draw calls, primitives] of its dirty frame (extras included)
var _bursts: Node3D = null
var _burst_slots: int = 0
var _emph_rows := PackedInt32Array()
var _hud_od: Node = null   # HudRenderOnDemand; XR_BENCH_HUD_ACTIVE=1 re-renders it every frame (wand on the panel)
const MemoryBurstsScript := preload("res://scripts/memory_bursts.gd")
const BURST_TINT := Color(0.2235, 1.0, 0.0784)  # desktop "store" #39ff14
var _memory: Node3D = null

var _frame_times_ms: PackedFloat32Array = PackedFloat32Array()
var _draw_calls: PackedInt32Array = PackedInt32Array()
var _tri_counts: PackedInt32Array = PackedInt32Array()
# The eye-buffer scene alone (root viewport, 3D + its own canvas), without the
# HUD SubViewport's offscreen 2D renders: what FrameBudget governs.
var _scene_draw_calls: PackedInt32Array = PackedInt32Array()
var _scene_tri_counts: PackedInt32Array = PackedInt32Array()
var _hud_render_frames: int = 0
var _static_mem_kb: PackedInt32Array = PackedInt32Array()
var _started_at_us: int = 0
var _fixture: Dictionary = {}
# WP4: the cluster-hull layer at its maximum (32 hulls), rendered for the whole
# run so the measured draw-call/triangle maxima include it.
const HULL_MAX := 32
const ParityScript := preload("res://scripts/graph_parity.gd")
var _hull_report: Dictionary = {"enabled": false}
# Node-mesh LOD on the production path: fixture (or synthetic) nodes go through
# the Rust render store and lod::split_node_tiers every frame, exactly as in
# GraphScene._update_multimesh. Worst case by default: the near radius is
# unbounded, so the gem cap is always full.
#   XR_BENCH_NODES=<n>          synthesise n nodes (13164 = production density)
#   XR_BENCH_NEAR_RADIUS=<m>    gem-tier radius in metres (default: unbounded)
const NodeLod := preload("res://scripts/node_lod.gd")
var _client: RefCounted = null
var _ids := PackedInt32Array()
var _impostors: MultiMeshInstance3D = null
var _near_radius: float = INF
var _lod_report: Dictionary = {"enabled": false}
var _node_source: String = "fixture"
var _lod_build_ms := PackedFloat32Array()   # CPU cost of the per-frame LOD pack
var _pack_ms := PackedFloat32Array()        # Rust-side pack only, wall (last_pack_ms)
var _pack_cpu_ms := PackedFloat32Array()    # Rust-side pack only, thread CPU (the gate)
var _lod_build_cpu_ms := PackedFloat32Array()  # whole rebuild, thread CPU (the gate)
# Edges at production density: the fixture's edges for the 1k fixture, else
# XR_BENCH_EDGES (default 20 000 = GraphScene EDGE_SAFETY_CEILING) random pairs.
# Drawn exactly as GraphScene does: Rust build_edge_buffer → EdgesMulti with the
# production cylinder and edge_flow material (edge LOD when available).
const EDGE_SAFETY_CEILING := 20000
var _edge_pairs := PackedInt32Array()
var _edges: MultiMeshInstance3D = null
var _ribbons: MultiMeshInstance3D = null
var _halos: MultiMeshInstance3D = null       # node halo quads, as GraphScene NodesHaloMulti
var _edge_report: Dictionary = {"enabled": false}

func _ready() -> void:
	if has_meta("duration_seconds"):
		duration_s = float(get_meta("duration_seconds"))
	if has_meta("fixture_path"):
		fixture_path = String(get_meta("fixture_path"))
	if has_meta("memory_cloud_rows"):
		memory_cloud_rows = int(get_meta("memory_cloud_rows"))
	if has_meta("memory_route_hops"):
		memory_route_hops = int(get_meta("memory_route_hops"))
	if has_meta("memory_route_sidecar"):
		memory_route_sidecar = int(get_meta("memory_route_sidecar"))
	if has_meta("extras"):
		with_extras = bool(get_meta("extras"))
	if has_meta("bursts"):
		bursts_on = bool(get_meta("bursts"))
	_fixture = _load_fixture(fixture_path)
	var synth: int = int(OS.get_environment("XR_BENCH_NODES")) if OS.has_environment("XR_BENCH_NODES") else 0
	if synth > 0:
		_fixture = _synthetic_fixture(synth)
		_node_source = "synthetic"
	if OS.has_environment("XR_BENCH_NEAR_RADIUS"):
		_near_radius = float(OS.get_environment("XR_BENCH_NEAR_RADIUS"))
	if with_extras:
		_add_extras()
		_phase = 0
	else:
		_begin_measurement()


func _begin_measurement() -> void:
	# keep the costliest HUD page open: its re-renders are the run's worst frames
	if _hud_od != null and not _page_cost.is_empty():
		var worst := ""
		for p in _page_cost:
			if worst == "" or int(_page_cost[p][0]) > int(_page_cost[worst][0]):
				worst = p
		_hud_od.get_parent()._show_tab(worst)
	if not _populate_lod_path(_fixture):
		_populate_scene_from_fixture(_fixture)
	_add_hull_layer(_fixture)
	_populate_memory_layers(memory_cloud_rows)
	_add_bursts()
	_apply_frame_budget()
	_phase = 1
	_started_at_us = Time.get_ticks_usec()

func _process(delta: float) -> void:
	if _hud_od != null and OS.get_environment("XR_BENCH_HUD_ACTIVE") == "1":
		_hud_od.request_render()
	if _phase == 0:
		# Worst case, not idle: the HUD re-renders on every calibration frame and
		# the renderer's global count (offscreen HUD pass included) is reserved.
		if _hud_od != null:
			# every page in turn: the reserve is the costliest page's dirty frame
			var hud: Node = _hud_od.get_parent()
			var order: Array = hud.TAB_ORDER
			var k: int = maxi(_calib_frames - CALIBRATE_SKIP, 0) / 3
			if k < order.size() and (_calib_frames - CALIBRATE_SKIP) % 3 == 0:
				hud._show_tab(order[k])
				_calib_page = order[k]
			_hud_od.request_render()
		_calib_frames += 1
		if _calib_frames > CALIBRATE_SKIP:
			# the counters report the frame just rendered: the page shown last frame
			var dc := int(RenderingServer.get_rendering_info(RenderingServer.RENDERING_INFO_TOTAL_DRAW_CALLS_IN_FRAME))
			var tr := int(RenderingServer.get_rendering_info(RenderingServer.RENDERING_INFO_TOTAL_PRIMITIVES_IN_FRAME))
			if _prev_page != "" and (not _page_cost.has(_prev_page) or dc > int(_page_cost[_prev_page][0])):
				_page_cost[_prev_page] = [dc, tr]
			_prev_page = _calib_page
			_other_dc = maxi(_other_dc, dc)
			_other_tris = maxi(_other_tris, tr)
		if _calib_frames >= CALIBRATE_SKIP + CALIBRATE_FRAMES:
			_begin_measurement()
		return
	_drive_bursts()
	_lod_rebuild()
	_frame_times_ms.append(delta * 1000.0)
	_draw_calls.append(int(RenderingServer.get_rendering_info(RenderingServer.RENDERING_INFO_TOTAL_DRAW_CALLS_IN_FRAME)))
	_tri_counts.append(int(RenderingServer.get_rendering_info(RenderingServer.RENDERING_INFO_TOTAL_PRIMITIVES_IN_FRAME)))
	var sc: Vector2i = _scene_info()
	_scene_draw_calls.append(sc.x)
	_scene_tri_counts.append(sc.y)
	if _tri_counts[-1] > sc.y:
		_hud_render_frames += 1
	_static_mem_kb.append(int(Performance.get_monitor(Performance.MEMORY_STATIC) / 1024))

	var elapsed_s := (Time.get_ticks_usec() - _started_at_us) / 1_000_000.0
	if elapsed_s >= duration_s:
		_emit_and_quit(elapsed_s)

func _emit_and_quit(elapsed_s: float) -> void:
	set_process(false)
	var report := _build_report(elapsed_s)
	_dump_samples()
	var json := JSON.stringify(report)
	print("%s=%s" % [RESULT_MARKER, json])
	print("%s=%s" % [LEGACY_MARKER, json])
	var pass_ok := bool(report.get("pass", false))
	get_tree().quit(0 if pass_ok else 1)

func _build_report(elapsed_s: float) -> Dictionary:
	var fts := _frame_times_ms.duplicate()
	fts.sort()
	var n := fts.size()
	var fps_samples := PackedFloat32Array()
	for ms in _frame_times_ms:
		if ms > 0.0:
			fps_samples.append(1000.0 / ms)
	var fps_sorted := fps_samples.duplicate()
	fps_sorted.sort()

	var draw_max := _max_int(_draw_calls)
	var tri_max := _max_int(_tri_counts)
	var scene_draw_max := _max_int(_scene_draw_calls)
	var scene_tri_max := _max_int(_scene_tri_counts)
	var p99 := _percentile(fts, 0.99)
	var p95 := _percentile(fts, 0.95)
	var p50 := _percentile(fts, 0.50)

	var pass_p99 := p99 <= FRAME_BUDGET_MS_P99
	var pass_dc := draw_max <= MAX_DRAW_CALLS
	var pass_tri := tri_max <= MAX_TRIANGLES
	# Skip the first second of samples: buffers grow and plans are built once.
	var lod_p99 := _percentile(_sorted(_tail(_lod_build_ms)), 0.99)
	var pack_p99 := _percentile(_sorted(_tail(_pack_ms)), 0.99)
	var gate: Dictionary = cpu_gate(_pack_cpu_ms, _lod_build_cpu_ms)
	var lod_cpu_p99: float = gate["lod_cpu_p99"]
	var pack_cpu_p99: float = gate["pack_cpu_p99"]
	var pass_pack: bool = _client == null or bool(gate["pass_pack"])
	var pass_lod: bool = _client == null or bool(gate["pass_lod"])
	var pass_warmup: bool = _client == null or bool(gate["pass_warmup"])

	# Frame-time -> CPU/GPU split is unavailable from a pure GDScript scene; the
	# self-hosted runner can supplement via `adb shell dumpsys gfxinfo`. For now
	# both are reported as the frame time, which is enough to gate PRD-008 §6.
	return {
		"schema_version": 1,
		"fixture": fixture_path,
		"node_count": int(_fixture.get("node_count", 0)),
		"node_source": _node_source,
		"node_lod": _lod_report,
		"edges": _edge_report,
		"lod_build_ms_p50": _percentile(_sorted(_tail(_lod_build_ms)), 0.50),
		"lod_build_ms_p99": lod_p99,
		"pack_ms_p50": _percentile(_sorted(_tail(_pack_ms)), 0.50),
		"pack_ms_p99": pack_p99,
		"pack_cpu_ms_p50": _percentile(_sorted(_tail(_pack_cpu_ms)), 0.50),
		"pack_cpu_ms_p99": pack_cpu_p99,
		"lod_build_cpu_ms_p50": _percentile(_sorted(_tail(_lod_build_cpu_ms)), 0.50),
		"lod_build_cpu_ms_p99": lod_cpu_p99,
		"warmup_pack_cpu_ms_max": gate["warmup_pack_cpu_max"],
		"warmup_lod_build_cpu_ms_max": gate["warmup_lod_cpu_max"],
		"cpu_affinity": _cpu_affinity,
		"edge_count": int(_fixture.get("edge_count", 0)),
		"avatar_count": int(_fixture.get("avatar_count", 0)),
		"duration_s": elapsed_s,
		"frame_count": n,
		"fps_mean": _mean(fps_samples),
		"fps_p99": _percentile(fps_sorted, 0.01),
		"cpu_ms_p50": p50,
		"cpu_ms_p95": p95,
		"cpu_ms_p99": p99,
		"gpu_ms_p50": p50,
		"gpu_ms_p95": p95,
		"gpu_ms_p99": p99,
		"frame_ms_p50": p50,
		"frame_ms_p95": p95,
		"frame_ms_p99": p99,
		"draw_calls_max": draw_max,
		"tri_count_max": tri_max,
		"scene": {"draw_calls_max": scene_draw_max, "tri_count_max": scene_tri_max,
			"pass": scene_draw_max <= MAX_DRAW_CALLS and scene_tri_max <= MAX_TRIANGLES,
			"hud_render_frames": _hud_render_frames, "frames": _scene_tri_counts.size()},
		"static_mem_kb_max": _max_int(_static_mem_kb),
		"hull_layer": _hull_report,
		"memory_cloud_rows": memory_cloud_rows,
		"memory_layers": _memory.budget() if _memory != null else {},
		"cloud_placement": str(_memory.placement()) if _memory != null else "",
		"frame_budget": _budget_report,
		"extras": {"enabled": with_extras, "other_tris": _other_tris, "other_draw_calls": _other_dc, "hud_pages": _page_cost},
		"bursts": {"enabled": bursts_on, "ring_slots": _burst_slots, "emphasised_rows": _emph_rows.size(),
			"ring_instances": _bursts.slot_count() if _bursts != null else 0},
		"pass": pass_p99 and pass_dc and pass_tri and pass_pack and pass_lod and pass_warmup,
		"pass_breakdown": {
			"p99_frame_time": pass_p99,
			"draw_calls": pass_dc,
			"triangles": pass_tri,
			"pack_cpu_ms": pass_pack,
			"lod_build_cpu_ms": pass_lod,
			"warmup_cpu_ms": pass_warmup,
		},
		"budgets": {
			"p99_frame_ms": FRAME_BUDGET_MS_P99,
			"max_draw_calls": MAX_DRAW_CALLS,
			"max_triangles": MAX_TRIANGLES,
			"pack_cpu_ms_p99": PACK_BUDGET_MS_P99,
			"lod_build_cpu_ms_p99": LOD_BUILD_BUDGET_MS_P99,
			"warmup_pack_cpu_ms_max": WARMUP_PACK_LIMIT_MS,
			"warmup_lod_build_cpu_ms_max": WARMUP_LOD_BUILD_LIMIT_MS,
		},
	}

# Build the hull layer from the fixture's node positions, split round-robin into
# HULL_MAX groups — overlapping, worst-case hulls, so the measurement is an upper
# bound. Estimate: one surface = 1 draw call; ≤ 124 triangles per hull (hulls.rs
# MAX_TRIS_PER_HULL) → ≤ 3 968 triangles at the cap.
func _add_hull_layer(fixture: Dictionary, max_hulls: int = HULL_MAX) -> void:
	# XR_BENCH_HULLS=0 runs the same scene without the layer (A/B baseline).
	if OS.get_environment("XR_BENCH_HULLS") == "0":
		_hull_report = {"enabled": false, "reason": "XR_BENCH_HULLS=0"}
		return
	if not ClassDB.class_exists("BinaryProtocolClient"):
		_hull_report = {"enabled": false, "reason": "gdext library not loaded"}
		return
	var pts := PackedVector3Array()
	var groups := PackedInt32Array()
	var i := 0
	for n in fixture.get("nodes", []):
		var p: Array = n.get("position", [0.0, 0.0, 0.0])
		pts.append(Vector3(float(p[0]), float(p[1]), float(p[2])))
		groups.append(1 + i % HULL_MAX)
		i += 1
	var client: RefCounted = BinaryProtocolClient.create()
	var d: Dictionary = client.hull_mesh_from_points(pts, groups, 0.15, max_hulls)
	var mesh: ArrayMesh = ParityScript.make_hull_mesh(d)
	if mesh != null:
		var inst := MeshInstance3D.new()
		inst.name = "ClusterHulls"
		inst.mesh = mesh
		var mat := ShaderMaterial.new()
		mat.shader = preload("res://materials/cluster_hull.gdshader")
		inst.material_override = mat
		add_child(inst)
	_hull_report = {
		"enabled": mesh != null,
		"hulls": int(d.get("hulls", 0)),
		"triangles": int(d.get("triangles", 0)),
		"draw_calls_est": 1 if mesh != null else 0,
		"triangles_est_max": max_hulls * 124,
	}

# (draw calls, primitives) of the root viewport's last frame: the eye-buffer
# scene, without offscreen SubViewport renders (the HUD canvas).
func _scene_info() -> Vector2i:
	var rid: RID = get_viewport().get_viewport_rid()
	var t := RenderingServer.VIEWPORT_RENDER_INFO_TYPE_VISIBLE
	return Vector2i(
		RenderingServer.viewport_get_render_info(rid, t, RenderingServer.VIEWPORT_RENDER_INFO_DRAW_CALLS_IN_FRAME),
		RenderingServer.viewport_get_render_info(rid, t, RenderingServer.VIEWPORT_RENDER_INFO_PRIMITIVES_IN_FRAME))


# HUD panel 0.6 m ahead of the camera (GraphScene parents it to XRCamera3D),
# both controllers' aim rays, two remote avatars at conversational distance.
# XR_BENCH_EXTRAS=hud,controllers,avatars (default all) picks a subset, to
# attribute the cost.
func _add_extras() -> void:
	var cam := get_node_or_null("Camera3D") as Camera3D
	var anchor: Node3D = cam if cam != null else self
	var pick: String = OS.get_environment("XR_BENCH_EXTRAS") if OS.has_environment("XR_BENCH_EXTRAS") else "hud,controllers,avatars"
	if pick.contains("hud"):
		var hud: Node3D = (load("res://scenes/HUD.tscn") as PackedScene).instantiate()
		hud.position = Vector3(0.0, -0.1, -0.6)
		anchor.add_child(hud)
		_hud_od = hud.get_node_or_null("HudRenderOnDemand")
	for side: float in ([-1.0, 1.0] if pick.contains("controllers") else []):
		var wand := Node3D.new()
		wand.name = "Controller%s" % ("L" if side < 0.0 else "R")
		wand.position = Vector3(0.18 * side, -0.3, -0.35)
		anchor.add_child(wand)
		# The real held ray (graph_scene.make_aim_ray): transparent pass at
		# HELD_RENDER_PRIORITY, above the depth-ignoring route.
		var ray: MeshInstance3D = (load("res://scripts/graph_scene.gd") as GDScript).make_aim_ray(5.0)
		wand.add_child(ray)
	var avatar_scene := load("res://scenes/Avatar.tscn") as PackedScene
	for i in (2 if pick.contains("avatars") else 0):
		var av: Node3D = avatar_scene.instantiate()
		av.position = Vector3(-0.6 + 1.2 * i, -0.2, -1.6)
		anchor.add_child(av)
		if av.has_method("set_display_name"):
			av.set_display_name("Peer %d" % (i + 1))


# memory_flash load: with the cloud shown, 64 rows emphasised every frame (the
# set_row_emphasis path, no geometry); with it hidden, the ring pool kept at the
# allocator's slot count.
func _add_bursts() -> void:
	if not bursts_on:
		return
	_bursts = MemoryBurstsScript.new()
	_bursts.name = "MemoryBursts"
	_bursts.reduced_motion = false
	add_child(_bursts)
	if _memory != null and _memory.has_snapshot():
		var n: int = mini(64, _memory.point_count())
		for k in n:
			_emph_rows.append((k * 7919 + 101) % _memory.point_count())


func _drive_bursts() -> void:
	if _bursts == null:
		return
	if not _emph_rows.is_empty():
		var t: float = Time.get_ticks_msec() / 1000.0
		var tints := PackedColorArray()
		var gains := PackedFloat32Array()
		var scales := PackedFloat32Array()
		for k in _emph_rows.size():
			var u: float = 0.5 + 0.5 * sin(t * 3.0 + k)
			tints.append(BURST_TINT)
			gains.append(1.0 + 1.5 * u)
			scales.append(1.0 + u)
		_memory.set_row_emphasis(_emph_rows, tints, gains, scales)
		return
	var cam := get_node_or_null("Camera3D") as Camera3D
	var centre: Vector3 = (cam.global_position + cam.global_transform.basis.z * -3.0) if cam != null else Vector3(0, 1.5, -3)
	var k := 0
	while _bursts.slot_count() < _burst_slots:
		var a: float = TAU * float(_bursts.slot_count() + k) / 64.0
		_bursts.spawn(centre + Vector3(cos(a), sin(a) * 0.6, 0.0) * 1.2, {"rings": 1, "duration": 1.6, "color": BURST_TINT})
		k += 1


# One FrameBudget pass for the whole scene, as GraphScene runs it: the graph's
# far tiers, the route, the cloud, then hulls, gems and cylinders.
func _apply_frame_budget() -> void:
	if not ClassDB.class_exists("FrameBudget"):
		return
	var demand: Dictionary = _memory.frame_demand() if _memory != null else {"cloud_rows": 0, "route_rows": 0, "route_sidecar": 0}
	var hulls: int = int(_hull_report.get("hulls", 0))
	var nodes: int = _ids.size()
	var edges: int = _edge_pairs.size() / 2 if _edges != null else 0
	# faded 0 (no labels); other_tris / other draw calls measured in the calibration phase
	var cloud_shown: bool = int(demand["cloud_rows"]) > 0
	var ring_slots: int = MemoryBurstsScript.POOL_SIZE if bursts_on and not cloud_shown else 0
	var caps: Dictionary = FrameBudget.new().allocate(nodes, edges, hulls, int(_hull_report.get("triangles", 0)), 0, _other_tris,
		GRAPH_DRAW_CALLS + _other_dc, int(demand["cloud_rows"]), int(demand["route_rows"]), int(demand["route_sidecar"]), ring_slots)
	_burst_slots = int(caps["burst_slots"])
	_gem_cap = int(caps["gem_nodes"])
	_edge_cap = int(caps["cylinder_edges"])
	if int(caps["max_hulls"]) < hulls:
		var old := get_node_or_null("ClusterHulls")
		if old != null:
			remove_child(old)
			old.free()
		_add_hull_layer(_fixture, int(caps["max_hulls"]))
	if _memory != null:
		_memory.apply_frame_caps(caps)
		_memory.flush()
	_lod_rebuild()
	_budget_report = caps.duplicate()
	_budget_report["enabled"] = true
	_budget_report["demand"] = {"nodes": nodes, "edges": edges, "hulls": hulls, "hull_tris": int(_hull_report.get("triangles", 0)),
		"other_tris": _other_tris, "other_draw_calls": _other_dc, "ring_slots": ring_slots}


# Deterministic production-density stand-in: n nodes in the fixture's ±10 m
# volume (same seed every run).
func _synthetic_fixture(n: int) -> Dictionary:
	var rng := RandomNumberGenerator.new()
	rng.seed = 0xC0FFEE
	var nodes: Array = []
	for i in range(n):
		var p := Vector3(rng.randf_range(-1, 1), rng.randf_range(-1, 1), rng.randf_range(-1, 1)).normalized() * 10.0 * pow(rng.randf(), 1.0 / 3.0)
		nodes.append({"id": i + 1, "kind": "knowledge", "position": [p.x, p.y, p.z]})
	return {"node_count": n, "edge_count": 0, "avatar_count": 0, "nodes": nodes}


# Feed every node through the real decode door (a V3 frame into ingest) and set
# up the gem + impostor MultiMeshes. False when the gdext library is missing, in
# which case the legacy direct-transform population runs instead.
func _populate_lod_path(fixture: Dictionary) -> bool:
	if not ClassDB.class_exists("BinaryProtocolClient"):
		_lod_report = {"enabled": false, "reason": "gdext library not loaded"}
		return false
	var nodes_multi := get_node_or_null("NodesMulti") as MultiMeshInstance3D
	if nodes_multi == null or nodes_multi.multimesh == null:
		return false
	_client = BinaryProtocolClient.create()
	if _client.has_method("pin_thread_to_l3"):
		_cpu_affinity = _client.pin_thread_to_l3()
	var b := StreamPeerBuffer.new()
	b.big_endian = false
	b.put_u8(3)
	for n in fixture.get("nodes", []):
		var p: Array = n.get("position", [0.0, 0.0, 0.0])
		var id: int = int(n.get("id", 0))
		b.put_u32(id)
		b.put_float(float(p[0])); b.put_float(float(p[1])); b.put_float(float(p[2]))
		for _k in range(4):
			b.put_float(0.0)
		b.put_32(-1)
		b.put_u32(0)
		b.put_float(0.0)
		b.put_u32(0)
		b.put_float(0.5)
		_ids.append(id)
	_client.ingest(b.data_array)
	_edge_pairs = _make_edge_pairs(fixture)
	_edges = _make_edge_instance()
	add_child(_edges)
	# XR_BENCH_EDGE_LOD=0 draws every edge as a cylinder (pre-LOD A/B baseline).
	if OS.get_environment("XR_BENCH_EDGE_LOD") != "0":
		_ribbons = NodeLod.make_ribbon_instance()
		add_child(_ribbons)
	var mm: MultiMesh = nodes_multi.multimesh
	mm.instance_count = 0
	mm.use_colors = true
	mm.use_custom_data = true
	_impostors = NodeLod.make_impostor_instance()
	add_child(_impostors)
	var hmm := MultiMesh.new()
	hmm.transform_format = MultiMesh.TRANSFORM_3D
	hmm.use_colors = true
	hmm.use_custom_data = true
	hmm.mesh = QuadMesh.new()
	_halos = MultiMeshInstance3D.new()
	_halos.name = "NodesHaloMulti"
	_halos.multimesh = hmm
	_halos.material_override = load("res://materials/node_halo_quad.tres")
	add_child(_halos)
	_lod_rebuild()
	return true


func _make_edge_pairs(fixture: Dictionary) -> PackedInt32Array:
	var pairs := PackedInt32Array()
	var edges: Array = fixture.get("edges", [])
	if not edges.is_empty() and not OS.has_environment("XR_BENCH_EDGES"):
		for e in edges:
			pairs.append(int(e.get("source", 0)))
			pairs.append(int(e.get("target", 0)))
		return pairs
	var n: int = _ids.size()
	var want: int = int(OS.get_environment("XR_BENCH_EDGES")) if OS.has_environment("XR_BENCH_EDGES") else EDGE_SAFETY_CEILING
	if n < 2:
		return pairs
	var rng := RandomNumberGenerator.new()
	rng.seed = 0xED6E
	for _i in range(want):
		var a: int = rng.randi_range(0, n - 1)
		var b: int = (a + 1 + rng.randi_range(0, n - 2)) % n
		pairs.append(_ids[a])
		pairs.append(_ids[b])
	return pairs


# The production edge cylinder (GraphScene.tscn CylinderMesh_edge) and material.
func _make_edge_instance() -> MultiMeshInstance3D:
	var scene_mesh: CylinderMesh = null
	var gs: PackedScene = load("res://scenes/GraphScene.tscn")
	if gs != null:
		var st := gs.get_state()
		for i in range(st.get_node_count()):
			if st.get_node_name(i) == "EdgesMulti":
				for p in range(st.get_node_property_count(i)):
					if st.get_node_property_name(i, p) == "multimesh":
						scene_mesh = (st.get_node_property_value(i, p) as MultiMesh).mesh as CylinderMesh
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.use_custom_data = true
	mm.mesh = scene_mesh if scene_mesh != null else CylinderMesh.new()
	var inst := MultiMeshInstance3D.new()
	inst.name = "EdgesMulti"
	inst.multimesh = mm
	inst.material_override = load("res://materials/edge_flow.tres")
	return inst


func _lod_rebuild() -> void:
	if _client == null:
		return
	var cam := get_node_or_null("Camera3D") as Camera3D
	var eye: Vector3 = cam.global_position if cam != null else Vector3.ZERO
	var t0 := Time.get_ticks_usec()
	var c0: float = float(_client.thread_cpu_ms()) if _client.has_method("thread_cpu_ms") else 0.0
	var near: PackedFloat32Array = _client.build_node_buffer_lod(_ids, 1.0, 0.7, 1.9, eye, _gem_cap, _near_radius)
	var near_count: int = NodeLod.assign(get_node("NodesMulti") as MultiMeshInstance3D, near)
	if _halos != null:
		NodeLod.assign_halo(_halos, near, _client.faded_node_buffer())
	var far_count: int = NodeLod.assign(_impostors, _client.impostor_node_buffer())
	var edge_count: int = 0
	var ribbon_count: int = 0
	if _edges != null:
		var eb: PackedFloat32Array
		if _ribbons != null:
			eb = _client.build_edge_buffer_lod(_edge_pairs, 1.0, eye, _edge_cap, _near_radius)
			NodeLod.sync_edge_params(_edges.material_override, _ribbons.material_override)
			ribbon_count = NodeLod.assign_stride(_ribbons, _client.ribbon_edge_buffer(), 16)
		else:
			eb = _client.build_edge_buffer(_edge_pairs, 1.0)
		edge_count = NodeLod.assign_stride(_edges, eb, 16)
	_lod_build_ms.append((Time.get_ticks_usec() - t0) / 1000.0)
	if _client.has_method("thread_cpu_ms"):
		_lod_build_cpu_ms.append(float(_client.thread_cpu_ms()) - c0)
	if _client.has_method("last_pack_ms"):
		_pack_ms.append(float(_client.last_pack_ms()))
	if _client.has_method("last_pack_cpu_ms"):
		_pack_cpu_ms.append(float(_client.last_pack_cpu_ms()))
	_edge_report = {"enabled": _edges != null, "pairs": _edge_pairs.size() / 2, "cylinders": edge_count,
		"ribbons": ribbon_count, "near_edge_cap": _edge_cap}
	_lod_report = {
		"enabled": true,
		"near": near_count,
		"impostors": far_count,
		"near_cap": _gem_cap,
		"near_radius_m": _near_radius if is_finite(_near_radius) else -1.0,
		"triangles_est": near_count * 290 + far_count * 2,
	}


func _load_fixture(path: String) -> Dictionary:
	if not FileAccess.file_exists(path):
		push_warning("perf fixture missing: %s" % path)
		return {}
	var f := FileAccess.open(path, FileAccess.READ)
	if f == null:
		push_warning("could not open perf fixture: %s" % path)
		return {}
	var text := f.get_as_text()
	f.close()
	var parsed: Variant = JSON.parse_string(text)
	if typeof(parsed) != TYPE_DICTIONARY:
		push_warning("perf fixture not a JSON object: %s" % path)
		return {}
	return parsed

func _populate_scene_from_fixture(fixture: Dictionary) -> void:
	if fixture.is_empty():
		return
	var nodes_multi := get_node_or_null("NodesMulti") as MultiMeshInstance3D
	var ont_multi := get_node_or_null("OntologyMulti") as MultiMeshInstance3D
	var agent_multi := get_node_or_null("AgentMulti") as MultiMeshInstance3D
	var nodes_arr: Array = fixture.get("nodes", [])

	var by_kind := {"knowledge": [], "ontology": [], "agent": []}
	for n in nodes_arr:
		var kind := String(n.get("kind", "knowledge"))
		if not by_kind.has(kind):
			kind = "knowledge"
		by_kind[kind].append(n)

	_apply_to_multimesh(nodes_multi, by_kind["knowledge"])
	_apply_to_multimesh(ont_multi, by_kind["ontology"])
	_apply_to_multimesh(agent_multi, by_kind["agent"])

func _apply_to_multimesh(mm_inst: MultiMeshInstance3D, nodes: Array) -> void:
	if mm_inst == null or mm_inst.multimesh == null or nodes.is_empty():
		return
	var mm := mm_inst.multimesh
	mm.instance_count = nodes.size()
	for i in nodes.size():
		var p: Array = nodes[i].get("position", [0.0, 0.0, 0.0])
		var t := Transform3D(Basis(), Vector3(float(p[0]), float(p[1]), float(p[2])))
		mm.set_instance_transform(i, t)

## XR_BENCH_SAMPLES=<path>: write the per-frame series (diagnosis only).
func _dump_samples() -> void:
	var path := OS.get_environment("XR_BENCH_SAMPLES")
	if path == "":
		return
	var f := FileAccess.open(path, FileAccess.WRITE)
	if f == null:
		push_warning("cannot write %s" % path)
		return
	f.store_string(JSON.stringify({"frame_ms": _frame_times_ms, "pack_cpu_ms": _pack_cpu_ms,
		"lod_build_cpu_ms": _lod_build_cpu_ms, "pack_ms": _pack_ms, "lod_build_ms": _lod_build_ms}))
	f.close()


## Samples skipped before steady state: buffers grow and plans are built once.
static func _warmup_len(n: int) -> int:
	return mini(90, n / 10)


func _tail(arr: PackedFloat32Array) -> PackedFloat32Array:
	return arr.slice(_warmup_len(arr.size()))


static func _max_of(arr: PackedFloat32Array) -> float:
	var m := 0.0
	for v in arr:
		m = maxf(m, v)
	return m


static func _p99_of(arr: PackedFloat32Array) -> float:
	var c := arr.duplicate()
	c.sort()
	return _percentile(c, 0.99)


## Thread-CPU gate: steady-state p99 (after the warm-up window) against the
## per-frame budgets, and the warm-up window's worst sample (the first plan
## build) against its own limits.
static func cpu_gate(pack_cpu: PackedFloat32Array, lod_cpu: PackedFloat32Array) -> Dictionary:
	var kp := _warmup_len(pack_cpu.size())
	var kl := _warmup_len(lod_cpu.size())
	var pack_p99 := _p99_of(pack_cpu.slice(kp))
	var lod_p99 := _p99_of(lod_cpu.slice(kl))
	var wp := _max_of(pack_cpu.slice(0, kp))
	var wl := _max_of(lod_cpu.slice(0, kl))
	return {
		"pack_cpu_p99": pack_p99, "lod_cpu_p99": lod_p99,
		"warmup_pack_cpu_max": wp, "warmup_lod_cpu_max": wl,
		"pass_pack": pack_p99 <= PACK_BUDGET_MS_P99,
		"pass_lod": lod_p99 <= LOD_BUILD_BUDGET_MS_P99,
		"pass_warmup": wp <= WARMUP_PACK_LIMIT_MS and wl <= WARMUP_LOD_BUILD_LIMIT_MS,
	}

func _sorted(arr: PackedFloat32Array) -> PackedFloat32Array:
	var c := arr.duplicate()
	c.sort()
	return c


# The cloud sits under a holder scaled like a fitted GraphRoot (±500 server units
# → ±5 m) in front of the camera; reduced motion off so the route animates.
func _populate_memory_layers(rows: int) -> void:
	if rows <= 0:
		return
	var holder := Node3D.new()
	holder.name = "MemoryHolder"
	holder.scale = Vector3.ONE * 0.01
	holder.position = Vector3(0.0, 1.7, 0.0)
	add_child(holder)
	_memory = MemoryCloudLayer.new()
	_memory.reduced_motion = false
	# Frame the cloud on the benchmark graph as GraphScene does (desktop
	# cloudFrame.ts): the graph is drawn unscaled at the scene root, so its
	# robust bounds are converted into the holder's space before the layer
	# places the cloud at the graph's centre and radius (the 1 Hz read + glide
	# run every frame of the measurement).
	if _client != null and _client.has_method("graph_robust_bounds"):
		var client: RefCounted = _client
		_memory.graph_bounds_source = func() -> PackedFloat32Array:
			# the benchmark measures the merged layout (separation 0)
			var b: PackedFloat32Array = client.graph_robust_bounds(0.0)
			if b.size() != 4:
				return PackedFloat32Array()
			var c: Vector3 = holder.global_transform.affine_inverse() * Vector3(b[0], b[1], b[2])
			return PackedFloat32Array([c.x, c.y, c.z, b[3] / holder.global_transform.basis.get_scale().x])
	holder.add_child(_memory)
	var sid := "bench-%d" % rows
	if not _memory.ingest_snapshot(synthetic_snapshot(sid, rows).to_utf8_buffer()):
		push_warning("benchmark: synthetic memory snapshot rejected: %s" % _memory.state_detail())
		return
	_memory.set_enabled(true)
	var path := PackedStringArray()
	for h in memory_route_hops + 1:
		path.append(str((h * 7919) % rows))
	var side := PackedStringArray()
	for k in memory_route_sidecar:
		side.append(str((k * 104729 + 13) % rows))
	_memory.apply_route_json('{"type":"memoryRoute","snapshotId":"%s","seq":1,"sentAt":1,"path":[%s],"sidecar":[%s]}' % [sid, ",".join(path), ",".join(side)])
	_memory.flush()


## Deterministic snapshot shaped like the server's: rows clustered per
## namespace within ±100, 40 namespaces, three source types.
static func synthetic_snapshot(sid: String, rows: int) -> String:
	var rng := RandomNumberGenerator.new()
	rng.seed = 2133
	var centres: Array = []
	for i in MEMORY_NAMESPACES:
		centres.append(Vector3(rng.randf_range(-70, 70), rng.randf_range(-70, 70), rng.randf_range(-70, 70)))
	var pos := PackedStringArray()
	var meta := PackedStringArray()
	var names := PackedStringArray()
	for i in MEMORY_NAMESPACES:
		names.append('"ns-%02d"' % i)
	for r in rows:
		var ns: int = r % MEMORY_NAMESPACES
		var p: Vector3 = centres[ns] + Vector3(rng.randfn(0, 12), rng.randfn(0, 12), rng.randfn(0, 12))
		pos.append("%.2f,%.2f,%.2f" % [p.x, p.y, p.z])
		meta.append('{"id":"r%d","key":"key-%d","namespace":"ns-%02d","sourceType":"%s","updatedAt":%d}' % [r, r, ns, ["agent", "hook", "ingest"][r % 3], 1_700_000_000_000 + r * 1000])
	return '{"version":1,"snapshotId":"%s","generatedAt":1,"dim":384,"count":%d,"positions":[%s],"metadata":[%s],"namespaces":[%s],"sourceTypes":["agent","hook","ingest"],"strata":[],"excludedNamespaces":[],"vectorsUrl":""}' % [sid, rows, ",".join(pos), ",".join(meta), ",".join(names)]


func _mean(arr: PackedFloat32Array) -> float:
	if arr.is_empty():
		return 0.0
	var s := 0.0
	for v in arr:
		s += v
	return s / arr.size()

static func _percentile(sorted: PackedFloat32Array, q: float) -> float:
	if sorted.is_empty():
		return 0.0
	var idx := int(clamp(floor(q * (sorted.size() - 1)), 0, sorted.size() - 1))
	return sorted[idx]

func _max_int(arr: PackedInt32Array) -> int:
	var m := 0
	for v in arr:
		if v > m:
			m = v
	return m
