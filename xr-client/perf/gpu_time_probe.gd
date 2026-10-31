extends SceneTree

# Does the renderer report GPU time at all? Run windowed, XR off, never against
# the live client:
#   bash tools/capture.sh res://perf/gpu_time_probe.gd
# Prints one line, "[GPU_TIME_PROBE]={...}". `measured` is true when
# RenderingServer.viewport_get_measured_render_time_gpu returns a nonzero value
# at any point, and `follows_load` when the heavy phase reads clearly above the
# idle one. EyeCandy runs the same check at startup (EyeCandy.probe_gpu_timer).
#
# --headless uses the dummy rasteriser and reports 0 whatever the driver
# supports, so a headless zero proves nothing; the result carries the rendering
# driver and method so the log says what was measured.

const IDLE_FRAMES := 240
const LOAD_FRAMES := 240
const SKIP := 30
const QUADS := 40

var _frame := 0
var _idle_gpu: Array[float] = []
var _idle_cpu: Array[float] = []
var _load_gpu: Array[float] = []
var _load_cpu: Array[float] = []
var _load_root: Node3D
var _rid: RID


func _initialize() -> void:
	var cam := Camera3D.new()
	cam.position = Vector3(0.0, 0.0, 2.0)
	root.add_child(cam)
	_rid = root.get_viewport_rid()
	RenderingServer.viewport_set_measure_render_time(_rid, OS.get_environment("PROBE_NOMEASURE") == "")
	_load_root = Node3D.new()
	_load_root.visible = false
	root.add_child(_load_root)
	# Full-screen blended quads: fill-rate load, the cost EyeCandy's extras add.
	var shader := Shader.new()
	shader.code = "shader_type spatial;\nrender_mode unshaded, blend_add, depth_draw_never, cull_disabled;\nvoid fragment(){ float n = 0.0; for(int i=0;i<24;i++){ n += sin(FRAGCOORD.x*0.01*float(i+1)) * cos(FRAGCOORD.y*0.013*float(i+1)); } ALBEDO = vec3(0.01 + 0.0001*n); ALPHA = 1.0; }\n"
	var mat := ShaderMaterial.new()
	mat.shader = shader
	var quad := QuadMesh.new()
	quad.size = Vector2(8.0, 8.0)
	for i: int in QUADS:
		var mi := MeshInstance3D.new()
		mi.mesh = quad
		mi.material_override = mat
		mi.position.z = -0.01 * float(i)
		_load_root.add_child(mi)


func _process(_delta: float) -> bool:
	_frame += 1
	if _frame % 120 == 1:
		print("[GPU_TIME_PROBE] frame ", _frame, " t=", Time.get_ticks_msec())
	var gpu: float = RenderingServer.viewport_get_measured_render_time_gpu(_rid)
	var cpu: float = RenderingServer.viewport_get_measured_render_time_cpu(_rid)
	if _frame > SKIP and _frame <= IDLE_FRAMES:
		_idle_gpu.append(gpu)
		_idle_cpu.append(cpu)
	elif _frame == IDLE_FRAMES + 1:
		_load_root.visible = true
	elif _frame > IDLE_FRAMES + SKIP + 1 and _frame <= IDLE_FRAMES + LOAD_FRAMES:
		_load_gpu.append(gpu)
		_load_cpu.append(cpu)
	elif _frame > IDLE_FRAMES + LOAD_FRAMES:
		_report()
		return true
	return false


static func _stat(a: Array[float]) -> Dictionary:
	if a.is_empty():
		return {"n": 0, "min": 0.0, "mean": 0.0, "p95": 0.0, "max": 0.0, "nonzero": 0}
	var s: Array[float] = a.duplicate()
	s.sort()
	var sum := 0.0
	var nz := 0
	for v: float in s:
		sum += v
		if v > 0.0:
			nz += 1
	return {"n": s.size(), "min": s[0], "mean": sum / s.size(), "p95": s[int(s.size() * 0.95)], "max": s[-1], "nonzero": nz}


func _report() -> void:
	var idle := _stat(_idle_gpu)
	var load_ := _stat(_load_gpu)
	var measured: bool = int(idle["nonzero"]) > 0 or int(load_["nonzero"]) > 0
	var follows: bool = measured and float(load_["mean"]) > 1.5 * float(idle["mean"]) + 0.05
	var out := {
		"engine": Engine.get_version_info().get("string", ""),
		"rendering_method": str(ProjectSettings.get_setting("rendering/renderer/rendering_method")),
		"rendering_driver": RenderingServer.get_current_rendering_driver_name(),
		"adapter": RenderingServer.get_video_adapter_name(),
		"display_driver": DisplayServer.get_name(),
		"measured": measured,
		"follows_load": follows,
		"gpu_idle_ms": idle, "gpu_load_ms": load_,
		"cpu_idle_ms": _stat(_idle_cpu), "cpu_load_ms": _stat(_load_cpu),
	}
	print("[GPU_TIME_PROBE]=" + JSON.stringify(out))
	quit(0)
