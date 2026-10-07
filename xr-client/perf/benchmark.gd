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
var _memory: Node3D = null

var _frame_times_ms: PackedFloat32Array = PackedFloat32Array()
var _draw_calls: PackedInt32Array = PackedInt32Array()
var _tri_counts: PackedInt32Array = PackedInt32Array()
var _static_mem_kb: PackedInt32Array = PackedInt32Array()
var _started_at_us: int = 0
var _fixture: Dictionary = {}

func _ready() -> void:
	if has_meta("duration_seconds"):
		duration_s = float(get_meta("duration_seconds"))
	if has_meta("fixture_path"):
		fixture_path = String(get_meta("fixture_path"))
	if has_meta("memory_cloud_rows"):
		memory_cloud_rows = int(get_meta("memory_cloud_rows"))
	_fixture = _load_fixture(fixture_path)
	_populate_scene_from_fixture(_fixture)
	_populate_memory_layers(memory_cloud_rows)
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
		"memory_cloud_rows": memory_cloud_rows,
		"memory_layers": _memory.budget() if _memory != null else {},
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
	holder.add_child(_memory)
	var sid := "bench-%d" % rows
	if not _memory.ingest_snapshot(synthetic_snapshot(sid, rows).to_utf8_buffer()):
		push_warning("benchmark: synthetic memory snapshot rejected: %s" % _memory.state_detail())
		return
	_memory.set_enabled(true)
	var path := PackedStringArray()
	for h in MEMORY_ROUTE_HOPS + 1:
		path.append(str((h * 7919) % rows))
	_memory.apply_route_json('{"type":"memoryRoute","snapshotId":"%s","seq":1,"sentAt":1,"path":[%s],"sidecar":[%s]}' % [sid, ",".join(path), ",".join(path.slice(0, 5))])
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
