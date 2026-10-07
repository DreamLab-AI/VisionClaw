extends SceneTree
## Live memory-search capture (xr-sms, 2026-10-07): the real GraphScene against
## a running backend; the memory cloud loads through its signed GET, then one
## Query-page preset runs through the HUD intent path ("memory_preset:<i>" →
## memory_search.gd → signed POST /api/memory-cloud/query) and its sidecar
## top-k route is drawn. A sampled hit is then pressed ("memory_hit:<i>") to
## send the guide cue there. Run with a display:
##   XR_BACKEND_WS=ws://<backend>:4000 XR_NOSTR_SECRET=<hex> \
##   godot --path xr-client --rendering-driver opengl3 --xr-mode off \
##         --script tests/visual/live_memory_search_capture.gd [-- preset="<text>"]
## Output: user://xr-memory-search-<view>.png and one XR_MEMORY_SEARCH line per
## step. Spends one query of the caller's per-minute budget.

const SCENE := "res://scenes/GraphScene.tscn"
const WAIT_GRAPH_S := 90.0
const WAIT_CLOUD_S := 90.0
const WAIT_QUERY_S := 30.0
## Preferred preset: a curated question scoped to an estate namespace that the
## live sample covers well. Falls back to the first namespace question.
const DEFAULT_PRESET := "lessons and patterns that worked"

var scene: Node


func _initialize() -> void:
	call_deferred("run")


func _arg(name: String, fallback: String) -> String:
	for a in OS.get_cmdline_user_args():
		if a.begins_with(name + "="):
			return a.substr(name.length() + 1)
	return fallback


func run() -> void:
	scene = (load(SCENE) as PackedScene).instantiate()
	root.add_child(scene)
	await process_frame
	var t0 := Time.get_ticks_msec()
	while scene._binary_client == null or int(scene._binary_client.node_count()) < 100:
		if Time.get_ticks_msec() - t0 > WAIT_GRAPH_S * 1000.0:
			push_error("no live graph after %d s" % WAIT_GRAPH_S)
			quit(2)
			return
		await create_timer(0.5).timeout
	var mc = scene._memory_cloud
	var search = scene._memory_search
	mc.set_enabled(true)
	t0 = Time.get_ticks_msec()
	while mc.state() != "ready":
		if mc.state() in ["forbidden", "failed"] or Time.get_ticks_msec() - t0 > WAIT_CLOUD_S * 1000.0:
			push_error("memory cloud not ready: %s %s" % [mc.state(), mc.state_detail()])
			quit(3)
			return
		await create_timer(0.5).timeout
	await create_timer(2.5).timeout
	search.refresh_presets()
	var want := _arg("preset", DEFAULT_PRESET)
	var presets: Array = search.presets()
	var idx := -1
	for i in presets.size():
		if str(presets[i]["text"]) == want:
			idx = i
			break
	if idx < 0:
		for i in presets.size():
			if str(presets[i]["text"]).begins_with("what does "):
				idx = i
				break
	print("XR_MEMORY_SEARCH ", JSON.stringify({"step": "presets", "count": presets.size(),
		"labels": presets.map(func(p: Dictionary) -> String: return str(p["label"])), "chosen": idx}))
	if idx < 0:
		quit(4)
		return
	scene._on_hud_control("memory_preset:%d" % idx)
	t0 = Time.get_ticks_msec()
	while search._pending:
		if Time.get_ticks_msec() - t0 > WAIT_QUERY_S * 1000.0:
			push_error("query did not complete")
			quit(5)
			return
		await create_timer(0.2).timeout
	var hits: Array = search._hits
	var sampled: Array = hits.filter(func(h: Dictionary) -> bool: return int(h["row"]) >= 0)
	print("XR_MEMORY_SEARCH ", JSON.stringify({"step": "answer", "preset": presets[idx],
		"caption": scene.hud._memory_search_status.text if scene.hud != null else "",
		"hits_shown": hits.size(), "sampled_shown": sampled.size(),
		"route_active": mc.route_active(), "route_source": mc.route_source(),
		"hops": int(mc._route.hop_count()) if mc.route_active() else 0,
		"agreement": mc.agreement_line()}))
	await create_timer(1.6).timeout
	await _shot("route")
	if not sampled.is_empty():
		var hi: int = hits.find(sampled[sampled.size() - 1])
		await create_timer(4.0).timeout
		scene._on_hud_control("memory_hit:%d" % hi)
		await create_timer(0.6).timeout
		print("XR_MEMORY_SEARCH ", JSON.stringify({"step": "hit_cue", "hit": hi, "cue_alpha": mc.cue_alpha()}))
		await _shot("hit-cue")
	quit(0 if mc.route_active() else 6)


func _shot(view: String) -> void:
	await RenderingServer.frame_post_draw
	var img := root.get_texture().get_image()
	var path := "user://xr-memory-search-%s.png" % view
	if img.save_png(path) != OK:
		push_error("capture failed: %s" % path)
		return
	print("XR_MEMORY_SEARCH ", JSON.stringify({"step": "shot", "file": ProjectSettings.globalize_path(path)}))
