extends SceneTree
## Live memory-search capture (xr-sms, 2026-10-07): the real GraphScene against
## a running backend; the memory cloud loads through its signed GET, then one
## Query-page preset runs through the HUD intent path ("memory_preset:<i>" →
## memory_search.gd → signed POST /api/memory-cloud/query) and its sidecar
## top-k route is drawn. A sampled hit is then pressed ("memory_hit:<i>") to
## send the guide cue there. With `typed="<text>"` (ADR-2136) the query is
## typed key by key on the HUD's on-screen keyboard instead, and Search sends
## it through the same intent path ("memory_typed:<scope>|<text>", the
## keyboard's default scope, or `scope=<ns>`, "all" for every namespace); the route then
## starts at the server's query point. Run with a display:
##   XR_BACKEND_WS=ws://<backend>:4000 XR_NOSTR_SECRET=<hex> \
##   godot --path xr-client --rendering-driver opengl3 --xr-mode off \
##         --script tests/visual/live_memory_search_capture.gd [-- preset="<text>" | typed="<text>"]
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
	var typed := _arg("typed", "")
	if not typed.is_empty():
		await _run_typed(typed, mc, search)
		return
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


## Type `text` on the HUD keyboard (each key a real button press), press
## Search, and report the route. Exit 0 only when the route starts at the
## query point.
func _run_typed(text: String, mc, search) -> void:
	var hud = scene.hud
	hud._show_tab("query")
	hud._query_mode_memory_button.pressed.emit()
	hud._memory_type_button.pressed.emit()
	var want_scope := _arg("scope", "")
	if not want_scope.is_empty():
		var target: String = "" if want_scope == "all" else want_scope
		for n in hud._memory_scopes.size():
			if str(hud._memory_scopes[hud._memory_scope_index]) == target:
				break
			(hud._keyboard_buttons["scope"] as Button).pressed.emit()
	for i in text.length():
		var c: String = text[i].to_lower()
		var key: String = "space" if c == " " else c
		if hud._keyboard_buttons.has(key):
			(hud._keyboard_buttons[key] as Button).pressed.emit()
	var entry: String = hud._memory_keyboard_entry.text
	await create_timer(0.5).timeout
	await _shot("keyboard")
	(hud._keyboard_buttons["enter"] as Button).pressed.emit()
	print("XR_MEMORY_SEARCH ", JSON.stringify({"step": "typed", "entry": entry, "sent": search.queries_sent,
		"scope": (hud._keyboard_buttons["scope"] as Button).text}))
	var t0 := Time.get_ticks_msec()
	while search._pending:
		if Time.get_ticks_msec() - t0 > WAIT_QUERY_S * 1000.0:
			push_error("query did not complete")
			quit(5)
			return
		await create_timer(0.2).timeout
	var hits: Array = search._hits
	var sampled: Array = hits.filter(func(h: Dictionary) -> bool: return int(h["row"]) >= 0)
	var line: String = mc.agreement_line()
	var place: Dictionary = mc.placement()
	print("XR_MEMORY_SEARCH ", JSON.stringify({"step": "answer", "typed": text,
		"caption": hud._memory_search_status.text,
		"hits_shown": hits.size(), "sampled_shown": sampled.size(),
		"route_active": mc.route_active(), "route_source": mc.route_source(),
		"hops": int(mc._route.hop_count()) if mc.route_active() else 0,
		"agreement": line, "from_query_point": line.contains("query point"),
		"cloud_scale": place.get("scale", 0.0), "cloud_position": str(place.get("position", ""))}))
	print("XR_MEMORY_SEARCH ", JSON.stringify(_cloud_in_metres(mc)))
	await create_timer(1.6).timeout
	await _shot("typed-route")
	quit(0 if mc.route_active() and line.contains("query point") else 6)


## Where the memory cloud sits for a user at the XR origin (head at the
## camera, 1.6 m): its world centre and robust radius in metres, the gap from
## the head to its near side, and the GraphRoot fit scale (m per server unit).
func _cloud_in_metres(mc) -> Dictionary:
	var cb: PackedFloat32Array = mc._frame.cloud_bounds()
	var world_per_local: float = mc._cloud_core.global_transform.basis.get_scale().x
	var centre: Vector3 = mc._cloud_core.global_transform * Vector3(cb[0], cb[1], cb[2]) if cb.size() == 4 else Vector3.ZERO
	var radius_m: float = cb[3] * world_per_local if cb.size() == 4 else 0.0
	var head := Vector3(0.0, 1.6, 0.0)
	var cam: Camera3D = scene.get_viewport().get_camera_3d()
	if cam != null:
		head = cam.global_position
	var graph_centre: Vector3 = scene._graph_centre_world()
	return {"step": "cloud_metres", "graph_scale_m_per_unit": scene._graph_scale,
		"cloud_centre_m": [snappedf(centre.x, 0.01), snappedf(centre.y, 0.01), snappedf(centre.z, 0.01)],
		"cloud_radius_m": snappedf(radius_m, 0.01),
		"head_m": [snappedf(head.x, 0.01), snappedf(head.y, 0.01), snappedf(head.z, 0.01)],
		"head_to_cloud_centre_m": snappedf(head.distance_to(centre), 0.01),
		"head_to_near_side_m": snappedf(head.distance_to(centre) - radius_m, 0.01),
		"graph_centre_m": [snappedf(graph_centre.x, 0.01), snappedf(graph_centre.y, 0.01), snappedf(graph_centre.z, 0.01)]}


func _shot(view: String) -> void:
	await RenderingServer.frame_post_draw
	var img := root.get_texture().get_image()
	var path := "user://xr-memory-search-%s.png" % view
	if img.save_png(path) != OK:
		push_error("capture failed: %s" % path)
		return
	print("XR_MEMORY_SEARCH ", JSON.stringify({"step": "shot", "file": ProjectSettings.globalize_path(path)}))
