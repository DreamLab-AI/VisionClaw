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

var duration_s: float = DEFAULT_DURATION_S
var fixture_path: String = DEFAULT_FIXTURE

var _frame_times_ms: PackedFloat32Array = PackedFloat32Array()
var _draw_calls: PackedInt32Array = PackedInt32Array()
var _tri_counts: PackedInt32Array = PackedInt32Array()
var _static_mem_kb: PackedInt32Array = PackedInt32Array()
var _started_at_us: int = 0
var _fixture: Dictionary = {}
# WP4: the cluster-hull layer at its maximum (32 hulls), rendered for the whole
# run so the measured draw-call/triangle maxima include it.
const HULL_MAX := 32
const ParityScript := preload("res://scripts/graph_parity.gd")
var _hull_report: Dictionary = {"enabled": false}

func _ready() -> void:
	if has_meta("duration_seconds"):
		duration_s = float(get_meta("duration_seconds"))
	if has_meta("fixture_path"):
		fixture_path = String(get_meta("fixture_path"))
	_fixture = _load_fixture(fixture_path)
	_populate_scene_from_fixture(_fixture)
	_add_hull_layer(_fixture)
	_started_at_us = Time.get_ticks_usec()

func _process(delta: float) -> void:
	_frame_times_ms.append(delta * 1000.0)
	_draw_calls.append(int(RenderingServer.get_rendering_info(RenderingServer.RENDERING_INFO_TOTAL_DRAW_CALLS_IN_FRAME)))
	_tri_counts.append(int(RenderingServer.get_rendering_info(RenderingServer.RENDERING_INFO_TOTAL_PRIMITIVES_IN_FRAME)))
	_static_mem_kb.append(int(Performance.get_monitor(Performance.MEMORY_STATIC) / 1024))

	var elapsed_s := (Time.get_ticks_usec() - _started_at_us) / 1_000_000.0
	if elapsed_s >= duration_s:
		_emit_and_quit(elapsed_s)

func _emit_and_quit(elapsed_s: float) -> void:
	set_process(false)
	var report := _build_report(elapsed_s)
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
	var p99 := _percentile(fts, 0.99)
	var p95 := _percentile(fts, 0.95)
	var p50 := _percentile(fts, 0.50)

	var pass_p99 := p99 <= FRAME_BUDGET_MS_P99
	var pass_dc := draw_max <= MAX_DRAW_CALLS
	var pass_tri := tri_max <= MAX_TRIANGLES

	# Frame-time -> CPU/GPU split is unavailable from a pure GDScript scene; the
	# self-hosted runner can supplement via `adb shell dumpsys gfxinfo`. For now
	# both are reported as the frame time, which is enough to gate PRD-008 §6.
	return {
		"schema_version": 1,
		"fixture": fixture_path,
		"node_count": int(_fixture.get("node_count", 0)),
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
		"static_mem_kb_max": _max_int(_static_mem_kb),
		"hull_layer": _hull_report,
		"pass": pass_p99 and pass_dc and pass_tri,
		"pass_breakdown": {
			"p99_frame_time": pass_p99,
			"draw_calls": pass_dc,
			"triangles": pass_tri,
		},
		"budgets": {
			"p99_frame_ms": FRAME_BUDGET_MS_P99,
			"max_draw_calls": MAX_DRAW_CALLS,
			"max_triangles": MAX_TRIANGLES,
		},
	}

# Build the hull layer from the fixture's node positions, split round-robin into
# HULL_MAX groups — overlapping, worst-case hulls, so the measurement is an upper
# bound. Estimate: one surface = 1 draw call; ≤ 124 triangles per hull (hulls.rs
# MAX_TRIS_PER_HULL) → ≤ 3 968 triangles at the cap.
func _add_hull_layer(fixture: Dictionary) -> void:
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
	var d: Dictionary = client.hull_mesh_from_points(pts, groups, 0.15, HULL_MAX)
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
		"triangles_est_max": HULL_MAX * 124,
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

func _mean(arr: PackedFloat32Array) -> float:
	if arr.is_empty():
		return 0.0
	var s := 0.0
	for v in arr:
		s += v
	return s / arr.size()

func _percentile(sorted: PackedFloat32Array, q: float) -> float:
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
