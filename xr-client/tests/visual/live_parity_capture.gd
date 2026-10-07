extends SceneTree
## Live desktop-parity capture (xr-parity, 2026-10-07): the real GraphScene
## against a running backend, the memory cloud framed on the live graph, and a
## memoryRoute frame injected through the same `apply_route_json` path the
## relay feeds. Screenshots the flat (or mirror) view while the framing cue
## shows and again once it has faded. Run with a display:
##   XR_BACKEND_WS=ws://<backend>:4000 XR_NOSTR_SECRET=<hex> \
##   godot --path xr-client --rendering-driver opengl3 --xr-mode off \
##         --script tests/visual/live_parity_capture.gd
## Output: user://xr-parity-<view>.png and one XR_PARITY_STATE line per shot.

const SCENE := "res://scenes/GraphScene.tscn"
const WAIT_GRAPH_S := 90.0
const WAIT_CLOUD_S := 90.0

var scene: Node


func _initialize() -> void:
	call_deferred("run")


func run() -> void:
	scene = (load(SCENE) as PackedScene).instantiate()
	root.add_child(scene)
	await process_frame
	# graph: wait for the live stream to fill the store
	var t0 := Time.get_ticks_msec()
	while scene._binary_client == null or int(scene._binary_client.node_count()) < 100:
		if Time.get_ticks_msec() - t0 > WAIT_GRAPH_S * 1000.0:
			push_error("no live graph after %d s" % WAIT_GRAPH_S)
			quit(2)
			return
		await create_timer(0.5).timeout
	print("XR_PARITY graph nodes=", scene._binary_client.node_count())
	# cloud: the HUD's Memory toggle, then the real signed GET
	var mc = scene._memory_cloud
	mc.set_enabled(true)
	t0 = Time.get_ticks_msec()
	while mc.state() != "ready":
		if mc.state() in ["forbidden", "failed"] or Time.get_ticks_msec() - t0 > WAIT_CLOUD_S * 1000.0:
			push_error("memory cloud not ready: %s %s" % [mc.state(), mc.state_detail()])
			quit(3)
			return
		await create_timer(0.5).timeout
	await create_timer(2.5).timeout  # first 1 Hz graph read + glide
	var n: int = mc.point_count()
	# a route through the live sample, as the desktop relay would send it
	var path := []
	for k in 12:
		path.append((k * 7919 + 101) % n)
	var side := [path[-1], (path[3] + 17) % n, (path[6] + 29) % n]
	var frame := {
		"type": "memoryRoute", "snapshotId": mc.snapshot_id(), "seq": 1,
		"sentAt": Time.get_unix_time_from_system() * 1000.0,
		"path": path, "sidecar": side, "query": "xr-parity capture",
		"sidecarTotal": 5, "sidecarAgree": 2,
	}
	var verdict: String = mc.apply_route_json(JSON.stringify(frame))
	print("XR_PARITY route verdict=", verdict)
	if verdict != "apply":
		quit(4)
		return
	await create_timer(1.6).timeout
	await _shot("cue")
	await create_timer(4.0).timeout
	await _shot("converged")
	quit(0)


func _shot(view: String) -> void:
	await RenderingServer.frame_post_draw
	var img := root.get_texture().get_image()
	var path := "user://xr-parity-%s.png" % view
	if img.save_png(path) != OK:
		push_error("capture failed: %s" % path)
		quit(1)
		return
	var mc = scene._memory_cloud
	print("XR_PARITY_STATE ", JSON.stringify({
		"view": view,
		"file": ProjectSettings.globalize_path(path),
		"placement": str(mc.placement()),
		"graph_bounds": Array(mc._frame.graph_bounds()),
		"cloud_bounds": Array(mc._frame.cloud_bounds()),
		"cue_alpha": mc.cue_alpha(),
		"agreement": mc.agreement_line(),
		"budget": mc.budget(),
		"xr": root.use_xr,
	}))
