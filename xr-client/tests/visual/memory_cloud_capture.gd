extends SceneTree
## Deterministic screenshots of the memory cloud + route (XR WP6/WP7) without
## a network session: a synthetic snapshot through the real parse path and a
## route through the real memoryRoute gate. Run with a display:
##   godot --path xr-client --rendering-driver opengl3 --script tests/visual/memory_cloud_capture.gd
## Output: user://xr-memory-<view>.png (trace mid-way, converged, age colours,
## reduced motion).

const Layer := preload("res://scripts/memory_cloud_layer.gd")
const Bench := preload("res://perf/benchmark.gd")

var layer: Node3D
var cam: Camera3D


func _initialize() -> void:
	call_deferred("capture")


func capture() -> void:
	var env := WorldEnvironment.new()
	env.environment = Environment.new()
	env.environment.background_mode = Environment.BG_COLOR
	env.environment.background_color = Color(0.02, 0.024, 0.04)
	root.add_child(env)
	cam = Camera3D.new()
	cam.position = Vector3(0, 0.2, 6.5)
	cam.fov = 70
	root.add_child(cam)
	cam.current = true
	# GraphRoot-like holder: ±500 server units → ±5 m
	var holder := Node3D.new()
	holder.scale = Vector3.ONE * 0.01
	root.add_child(holder)
	layer = Layer.new()
	layer.reduced_motion = false
	holder.add_child(layer)
	await process_frame
	if not layer.ingest_snapshot(Bench.synthetic_snapshot("vis", 6000).to_utf8_buffer()):
		push_error("snapshot rejected: %s" % layer.state_detail())
		quit(1)
		return
	layer.set_enabled(true)
	var verdict: String = layer.apply_route_json('{"type":"memoryRoute","snapshotId":"vis","seq":1,"sentAt":1,"path":[0,41,1802,3603,5404,7,2448],"sidecar":[2448,49,90]}')
	if verdict != "apply":
		push_error("route not applied: %s" % verdict)
		quit(1)
		return
	await _settle(1.2)
	await _shot("trace")
	await _settle(3.0)
	await _shot("converged")
	layer.cycle_colour_mode()
	layer.cycle_colour_mode()
	await _settle(0.2)
	await _shot("age")
	layer.reduced_motion = true
	await _settle(0.2)
	await _shot("reduced")
	quit(0)


# Real time, not frames: uncapped the scene renders thousands of frames a second.
func _settle(seconds: float) -> void:
	await create_timer(seconds).timeout
	await process_frame


func _shot(view: String) -> void:
	await RenderingServer.frame_post_draw
	var img := root.get_texture().get_image()
	var path := "user://xr-memory-%s.png" % view
	if img.save_png(path) != OK:
		push_error("capture failed: %s" % path)
		quit(1)
		return
	print("XR_MEMORY_CAPTURE ", view, " ", ProjectSettings.globalize_path(path))
