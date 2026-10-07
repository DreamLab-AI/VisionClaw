extends "res://addons/gut/test.gd"

# Memory search from the headset (scripts/memory_search.gd + the Query page's
# Memory mode in hud.gd). Presets → POST /api/memory-cloud/query (signed with
# GraphScene._auth_headers) → hit list on the HUD and a "sidecar top-k" route
# through the real MemoryRoute gate on the real memory cloud layer. No
# network: the POST door is replaced by a recorder and responses are fed to
# the completion handler.

const Layer := preload("res://scripts/memory_cloud_layer.gd")
const SearchScript := preload("res://scripts/memory_search.gd")
const LayerTests := preload("res://tests/unit/test_memory_cloud_layer.gd")
const TABS := "HudViewport/HudControl/Root/Tabs"
const OK_RESULT := HTTPRequest.RESULT_SUCCESS


class RecordingSearch:
	extends "res://scripts/memory_search.gd"
	var posts: Array = []
	var accept: bool = true
	func _post(url: String, headers: PackedStringArray, body: String) -> bool:
		if not accept:
			return false
		posts.append({"url": url, "headers": headers, "body": body})
		return true


func _fixture_text() -> String:
	return FileAccess.get_file_as_string("res://rust/tests/fixtures/memory_query_response.json")


func _make_hud() -> Node3D:
	var hud: Node3D = (load("res://scenes/HUD.tscn") as PackedScene).instantiate()
	add_child(hud)
	await get_tree().process_frame
	await get_tree().process_frame
	return hud


func _make_layer(sid: String, n: int) -> Node3D:
	var l: Node3D = Layer.new()
	add_child(l)
	await get_tree().process_frame
	l._enabled = true
	l._on_http_completed(OK_RESULT, 200, PackedStringArray(), LayerTests.snapshot_json(sid, n).to_utf8_buffer())
	return l


func _run_route(layer: Node3D, seconds: float) -> void:
	for i in int(seconds / 0.1):
		layer._route.tick(0.1, 1.2, -1.0, 0.0, true)


func _auth(url: String, method: String) -> PackedStringArray:
	return PackedStringArray(["Authorization: Nostr test %s %s" % [method, url], "Content-Type: application/json"])


func _search(layer: Node3D, hud: Node3D) -> RecordingSearch:
	var s := RecordingSearch.new()
	s.recent_path = "user://test_memory_search_recent_%d.json" % randi()
	add_child(s)
	s.setup("http://backend:4000", Callable(self, "_auth"), layer, hud)
	return s


# --- presets -----------------------------------------------------------------

func test_presets_are_curated_plus_one_per_snapshot_namespace() -> void:
	assert_true(ClassDB.class_exists("MemoryQuery"), "gdext library loaded")
	var layer: Node3D = await _make_layer("snap-7f3a", 40)
	var s := _search(layer, null)
	var labels: Array = s.presets().map(func(p: Dictionary) -> String: return str(p["label"]))
	assert_true(labels.has("recent architecture decisions"), "curated set: %s" % str(labels))
	for ns in ["dream-cycle", "patterns", "project-state"]:
		assert_true(labels.has("what does %s hold" % ns), "a question per namespace (%s)" % ns)
	var ns_preset: Dictionary = s.presets().filter(func(p: Dictionary) -> bool: return str(p["text"]) == "what does patterns hold")[0]
	assert_eq(str(ns_preset["namespace"]), "patterns", "searched within that namespace")
	s.queue_free()
	layer.queue_free()


func test_a_preset_posts_the_signed_query_and_becomes_recent() -> void:
	var layer: Node3D = await _make_layer("snap-7f3a", 40)
	var s := _search(layer, null)
	var i: int = s.presets().map(func(p: Dictionary) -> String: return str(p["text"])).find("what does patterns hold")
	assert_true(s.run_preset(i), "dispatched")
	assert_eq(s.posts.size(), 1)
	assert_eq(str(s.posts[0]["url"]), "http://backend:4000/api/memory-cloud/query", "the desktop explorer's endpoint")
	var body: Dictionary = JSON.parse_string(str(s.posts[0]["body"]))
	assert_eq(body, {"text": "what does patterns hold", "k": 10.0, "namespace": "patterns"})
	assert_true(str((s.posts[0]["headers"] as PackedStringArray)[0]).begins_with("Authorization: Nostr test POST http://backend:4000/api/memory-cloud/query"),
		"signed for the exact POST URL (ADR-2076, Invariant 6)")
	assert_false(s.run_preset(0), "one query in flight at a time")
	assert_eq(s.posts.size(), 1)
	s.on_query_completed(OK_RESULT, 200, PackedStringArray(), _fixture_text().to_utf8_buffer())
	assert_eq(str(s.presets()[0]["text"]), "what does patterns hold", "the query is now the first preset")
	# persisted: a fresh search object on the same file sees it
	var again := _search(layer, null)
	again.recent_path = s.recent_path
	again._load_recent()
	assert_eq(str(again.presets()[0]["text"]), "what does patterns hold", "recent queries survive a relaunch")
	DirAccess.remove_absolute(s.recent_path)
	again.queue_free()
	s.queue_free()
	layer.queue_free()


# --- response → HUD list + route ----------------------------------------------

func test_hits_list_and_sidecar_route_from_the_response() -> void:
	var hud: Node3D = await _make_hud()
	var layer: Node3D = await _make_layer("snap-7f3a", 40)
	var s := _search(layer, hud)
	s.run_preset(0)
	s.on_query_completed(OK_RESULT, 200, PackedStringArray(), _fixture_text().to_utf8_buffer())
	# the HUD list
	var rows: Array = hud._memory_hit_buttons
	assert_eq(rows.size(), 6, "one row per hit (≤ 8)")
	assert_eq((rows[0] as Button).text, "1. adr-2135-separated-layout · project-state · 0.81 ●")
	assert_eq((rows[1] as Button).text, "2. xr-hud-532px-host · patterns · 0.77 —", "unsampled hit marked")
	assert_true(hud._memory_search_status.text.contains("4 of 6 in the sample"), hud._memory_search_status.text)
	assert_true(hud._memory_search_status.text.contains("sidecar top-k in rank order"), "honest about what the route is")
	# the route: real gate, real geometry
	assert_true(layer.route_active(), "route drawn")
	assert_eq(layer.route_source(), "sidecar_top_k")
	assert_eq(int(layer._route.hop_count()), 3, "four sampled hits, three hops")
	assert_gt(layer.cue_alpha(), -0.001)
	assert_eq(layer.agreement_line(), "Route: sidecar top-k in rank order · 4 of 6 sidecar hits are in the sample",
		"the Memory row line works without a desktop relay")
	s.queue_free()
	layer.queue_free()
	hud.queue_free()
	await get_tree().process_frame


func test_pressing_a_hit_sends_the_guide_cue_there() -> void:
	var hud: Node3D = await _make_hud()
	var layer: Node3D = await _make_layer("snap-7f3a", 40)
	var s := _search(layer, hud)
	s.run_preset(0)
	s.on_query_completed(OK_RESULT, 200, PackedStringArray(), _fixture_text().to_utf8_buffer())
	_run_route(layer, 6.0)   # let the first cue finish (tick clamps dt to 0.1 s)
	assert_eq(layer.cue_alpha(), 0.0, "first cue over")
	assert_true(s.focus_hit(2), "sampled hit (row 4)")
	layer._route.tick(0.5, 1.2, -1.0, 0.0, true)
	assert_gt(layer.cue_alpha(), 0.0, "the cue replays towards the picked point")
	assert_false(s.focus_hit(1), "an unsampled hit has no point to guide to")
	assert_true(hud._notice_active(), "and the HUD says so")
	assert_false(s.focus_hit(99), "out of range")
	s.queue_free()
	layer.queue_free()
	hud.queue_free()
	await get_tree().process_frame


func test_a_query_for_another_snapshot_reloads_the_cloud_first() -> void:
	var layer: Node3D = await _make_layer("old-snap", 40)
	layer._enabled = false   # cloud hidden: the route waits for the next load (no fetch from an empty base)
	var s := _search(layer, null)
	s.on_query_completed(OK_RESULT, 200, PackedStringArray(), _fixture_text().to_utf8_buffer())
	assert_false(layer.route_active(), "held until the cloud matches")
	assert_true(layer._route.has_pending())
	layer._on_http_completed(OK_RESULT, 200, PackedStringArray(), LayerTests.snapshot_json("snap-7f3a", 40).to_utf8_buffer())
	assert_true(layer.route_active(), "drawn once the snapshot it names is loaded")
	s.queue_free()
	layer.queue_free()


func test_failures_are_explained_and_release_the_gate() -> void:
	var hud: Node3D = await _make_hud()
	var layer: Node3D = await _make_layer("snap-7f3a", 40)
	var s := _search(layer, hud)
	for c in [[401, "power-user"], [403, "power-user"], [429, "budget"], [503, "unavailable"], [400, "rejected"]]:
		assert_true(s.run_preset(0), "gate open for %d" % c[0])
		s.on_query_completed(OK_RESULT, int(c[0]), PackedStringArray(), "{\"error\":\"nope\"}".to_utf8_buffer())
		assert_true(hud._memory_search_status.text.to_lower().contains(str(c[1])), "%d: %s" % [c[0], hud._memory_search_status.text])
	assert_true(s.run_preset(0))
	s.on_query_completed(HTTPRequest.RESULT_CANT_CONNECT, 0, PackedStringArray(), PackedByteArray())
	assert_true(hud._memory_search_status.text.to_lower().contains("unreachable"))
	assert_true(s.run_preset(0))
	s.on_query_completed(OK_RESULT, 200, PackedStringArray(), "{not json".to_utf8_buffer())
	assert_true(hud._memory_search_status.text.to_lower().contains("could not be read"))
	assert_false(layer.route_active(), "no route from a failure")
	s.queue_free()
	layer.queue_free()
	hud.queue_free()
	await get_tree().process_frame


# --- the Query page -----------------------------------------------------------

func test_query_page_has_graph_and_memory_modes_inside_532px() -> void:
	var hud: Node3D = await _make_hud()
	hud._show_tab("query")
	var page: Control = hud.get_node("%s/QueryPage" % TABS)
	for b: Button in [hud._query_mode_graph_button, hud._query_mode_memory_button]:
		assert_true(page.is_ancestor_of(b))
		assert_eq(b.action_mode, BaseButton.ACTION_MODE_BUTTON_PRESS, "press-fire (Invariant 4)")
	assert_true(hud._query_graph_box.visible, "graph query by default")
	assert_false(hud._query_memory_box.visible)
	await get_tree().process_frame
	assert_lte(page.get_combined_minimum_size().y, 532.0, "graph mode fits")
	hud._query_mode_memory_button.pressed.emit()
	await get_tree().process_frame
	assert_false(hud._query_graph_box.visible)
	assert_true(hud._query_memory_box.visible, "memory search shown")
	# fill both lists past what they show at once
	var labels: Array = []
	for i in 14:
		labels.append("preset %d with a fairly long label to wrap the grid" % i)
	hud.set_memory_presets(labels)
	var hits: Array = []
	for i in 8:
		hits.append({"line": "%d. some-key · patterns · 0.80 ●" % (i + 1), "row": i})
	hud.set_memory_hits("8 hits · hnsw 9 ms · 8 of 8 in the sample · route: sidecar top-k in rank order (not a search path)", hits)
	await get_tree().process_frame
	await get_tree().process_frame
	assert_lte(page.get_combined_minimum_size().y, 532.0, "memory mode fits with full lists (Invariant 5)")
	var host: Control = hud.get_node(TABS)
	assert_lte(page.get_combined_minimum_size().y, host.size.y + 1.0)
	watch_signals(hud)
	for b: Button in hud._memory_preset_buttons + hud._memory_hit_buttons:
		assert_eq(b.action_mode, BaseButton.ACTION_MODE_BUTTON_PRESS, "press-fire: %s" % b.text)
		assert_true(b.has_meta(hud.HINT_META), "hint: %s" % b.text)
	(hud._memory_preset_buttons[3] as Button).pressed.emit()
	assert_signal_emitted_with_parameters(hud, "control_pressed", ["memory_preset:3"])
	(hud._memory_hit_buttons[5] as Button).pressed.emit()
	assert_signal_emitted_with_parameters(hud, "control_pressed", ["memory_hit:5"])
	hud._query_mode_graph_button.pressed.emit()
	assert_true(hud._query_graph_box.visible, "back to the graph query")
	hud.queue_free()
	await get_tree().process_frame


func test_scene_routes_memory_intents_to_the_search() -> void:
	var gs: Node3D = (load("res://scenes/GraphScene.tscn") as PackedScene).instantiate()
	var layer: Node3D = await _make_layer("snap-7f3a", 40)
	var s := _search(layer, null)
	gs._memory_search = s
	gs._on_hud_control("memory_preset:0")
	assert_eq(s.posts.size(), 1, "a preset press runs the query")
	s.on_query_completed(OK_RESULT, 200, PackedStringArray(), _fixture_text().to_utf8_buffer())
	_run_route(layer, 6.0)
	gs._on_hud_control("memory_hit:0")
	layer._route.tick(0.2, 1.2, -1.0, 0.0, true)
	assert_gt(layer.cue_alpha(), 0.0, "a hit press cues its point")
	gs.free()
	s.queue_free()
	layer.queue_free()
