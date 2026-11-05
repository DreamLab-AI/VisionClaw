extends "res://addons/gut/test.gd"

# ADR-2170: the headset's Gaussian splat volume (scripts/splat_volume.gd, materials/
# splat.gdshader), headless. Nothing here renders a splat: the renderer is the dummy one,
# so these tests pin behaviour (placement, priority, the budget account, the dim, the
# hash refusal, comfort), not pixels. On-device rendering, stereo and multiview
# texelFetch are unverified.

const SplatVolume := preload("res://scripts/splat_volume.gd")
const SpacePortals := preload("res://scripts/space_portals.gd")
const VolumeArrangement := preload("res://scripts/volume_arrangement.gd")
const VolumeScene := preload("res://scenes/EmulatedVolume.tscn")
const FixtureContentSource := preload("res://tests/fixtures/fixture_content_source.gd")
const HudScene := preload("res://scenes/HUD.tscn")
const SHADER_PATH := "res://materials/splat.gdshader"

const DT := 1.0 / 90.0
const IDS: Array[StringName] = [&"knowledge", &"ontology", &"codescape"]


class FakePass:
	extends RefCounted
	var caps: Dictionary = {"splat_tier": 3}
	var demand: Array = []
	var applied: Array = []
	func set_splat_demand(tier: int, volumes: int, entered: bool) -> void:
		demand.append([tier, volumes, entered])
	func set_splat_applied(tris: int, calls: int) -> void:
		applied.append([tris, calls])


var _requests: Array = []      # urls the fake server saw
var _tamper := false           # serve the blob with one bit flipped
var _blobs: Dictionary = {}    # url -> PackedByteArray
var _listing := ""
var _cache_dirs: Array[String] = []


func after_each() -> void:
	for d: String in _cache_dirs:
		for f: String in DirAccess.get_files_at(d):
			DirAccess.remove_absolute(d.path_join(f))
		DirAccess.remove_absolute(d)
	_cache_dirs.clear()


# A throwaway cache directory for this test's loader: the real one, user://splat-cache, is
# shared with the live client and would answer from an earlier test's verified entry.
func _fresh_cache(sv: Node3D) -> String:
	var dir := ProjectSettings.globalize_path("user://splat-test-%d-%d" % [Time.get_ticks_usec(), _cache_dirs.size()])
	DirAccess.make_dir_recursive_absolute(dir)
	_cache_dirs.append(dir)
	sv._loader.configure(dir)
	return dir


# The governor starts at tier 1; feed it calm GPU time while "entered" until it reaches `tier`.
func _climb(sv: Node3D, tier: int) -> void:
	sv._governor.use_gpu()
	var guard := 0
	while int(sv._governor.tier()) < tier and guard < 1000:
		sv._governor.feed(2.0, 1.0, true)
		guard += 1


# --- fixtures ------------------------------------------------------------------

# A real SPZ v3 file of `n` splats: the 16-byte header and `n` x 20 bytes of seeded noise,
# gzipped (every byte pattern decodes: colours, scales and the smallest-three rotation).
func _spz(n: int, seed_: int) -> PackedByteArray:
	var raw := PackedByteArray()
	raw.resize(16)
	raw.encode_u32(0, 0x5053474e)
	raw.encode_u32(4, 3)
	raw.encode_u32(8, n)
	raw[12] = 0 # SH degree
	raw[13] = 12 # fractional bits
	raw[14] = 0
	raw[15] = 0
	var rng := RandomNumberGenerator.new()
	rng.seed = seed_
	var payload := PackedByteArray()
	payload.resize(n * 20)
	for i: int in payload.size():
		payload[i] = rng.randi() & 0xff
	# keep scales small and alpha opaque-ish: bytes n*10 .. n*13 are colour, n*13..n*16 scale
	for i: int in n:
		payload[n * 9 + i] = 200
		for k: int in 3:
			payload[n * 13 + i * 3 + k] = 120 # exp(120/16 - 10) = 0.08
	raw.append_array(payload)
	return raw.compress(FileAccess.COMPRESSION_GZIP)


func _sha(b: PackedByteArray) -> String:
	var ctx := HashingContext.new()
	ctx.start(HashingContext.HASH_SHA256)
	ctx.update(b)
	return ctx.finish().hex_encode()


func _variant(name_: String, blob: PackedByteArray, splats: int) -> Dictionary:
	var sha := _sha(blob)
	var address := "sha256-12-%s" % sha.substr(0, 12)
	return {"name": name_, "splats": splats, "shDegree": 0, "sha256": sha, "address": address,
		"bytes": blob.size(), "url": "/api/splats/blob/%s" % address}


func _serve(tiers: Array = [3, 2, 1]) -> void:
	_requests.clear()
	_blobs.clear()
	_tamper = false
	var variants: Array = []
	for t: int in tiers:
		var n := 12 * t
		var blob := _spz(n, 100 + t)
		var v := _variant("xr-%d" % t, blob, n)
		variants.append(v)
		_blobs["https://srv" + String(v["url"])] = blob
	_listing = JSON.stringify({"assets": [{
		"id": "iss", "title": "ISS", "author": "T. Maté", "sourceUrl": "https://example.org/iss",
		"licence": {"spdx": "CC-BY-4.0", "url": "https://creativecommons.org/licenses/by/4.0/"},
		"attribution": "T. Maté, ISS", "modified": true,
		"bounds": {"min": [-4.0, -2.0, -4.0], "max": [4.0, 2.0, 4.0]},
		"variants": variants}]})


func _fake_http(url: String, done: Callable) -> void:
	_requests.append(url)
	if url.ends_with("/api/splats"):
		done.call(200, _listing.to_utf8_buffer())
		return
	if _blobs.has(url):
		var b: PackedByteArray = _blobs[url].duplicate()
		if _tamper:
			b[b.size() / 2] ^= 0x01
		done.call(200, b)
		return
	done.call(404, PackedByteArray())


class FakeConn:
	extends Node
	func http_base() -> String:
		return "https://srv"


func _camera() -> Camera3D:
	var cam := Camera3D.new()
	add_child_autofree(cam)
	cam.current = true
	return cam


# A splat volume with a fake server, a fake budget pass and a camera, inside an
# arrangement beside the three graph volumes.
func _scene(grant := 3) -> Dictionary:
	_serve()
	var arr: Node3D = VolumeArrangement.new()
	add_child_autofree(arr)
	for id: StringName in IDS:
		var v: Node3D = VolumeScene.instantiate()
		v.volume_id = id
		arr.add_volume(v, VolumeArrangement.DEFAULT_YAWS[id])
	var sv: Node3D = SplatVolume.new()
	arr.add_volume(sv, SplatVolume.RING_YAW)
	var fp := FakePass.new()
	fp.caps = {"splat_tier": grant}
	sv.bind_budget(fp)
	sv.conn = FakeConn.new()
	add_child_autofree(sv.conn)
	sv.http_override = Callable(self, "_fake_http")
	_fresh_cache(sv)
	# eye_candy.gd has probed and found a GPU timer (its reading is 0 in a headless run, so
	# nothing is fed and the tier set below holds).
	var comfort := FakeComfort.new()
	comfort.candy.signal_mode = "gpu"
	add_child_autofree(comfort)
	comfort.add_child(comfort.candy)
	sv._comfort_node = comfort
	sv._signal_mode = "gpu"
	_climb(sv, grant)
	_camera()
	return {"arr": arr, "sv": sv, "pass": fp}


# Tick the volume until `cond` holds (workers decode off-thread), or fail.
func _until(sv: Node3D, cond: Callable, label: String, limit_ms := 20000) -> bool:
	var t0 := Time.get_ticks_msec()
	while Time.get_ticks_msec() - t0 < limit_ms:
		sv.tick(DT)
		sv.update_splat(DT)
		if cond.call():
			return true
		OS.delay_msec(4)
	fail_test("timed out waiting for: %s (phase %d, failure %s)" % [label, sv.phase, sv.failure])
	return false


func _load(sv: Node3D) -> void:
	sv.request_asset("iss")
	_until(sv, func() -> bool: return sv.loaded_tier > 0 and not sv.is_fading(), "first scene")


# --- the shader and the constants ------------------------------------------------

func _code_only(src: String) -> String:
	var out := PackedStringArray()
	for line: String in src.split("\n"):
		var i := line.find("//")
		out.append(line if i < 0 else line.substr(0, i))
	return "\n".join(out)


func test_the_shader_follows_the_adr_and_the_edge_ribbon_pattern() -> void:
	var raw := FileAccess.get_file_as_string(SHADER_PATH)
	var src := _code_only(raw)
	assert_ne(src, "")
	assert_true(src.contains("render_mode blend_mix, unshaded, cull_disabled, depth_draw_never, shadows_disabled, fog_disabled;"))
	assert_true(src.contains("texelFetch(splat_index"), "the sorted index is read by texelFetch")
	assert_true(src.contains("INSTANCE_ID"))
	assert_true(src.contains("const float MAX_RADIUS_PX = 64.0;"), "screen radius capped at 64 px")
	assert_true(src.contains("const float NEAR_FADE_M = 0.3;"), "near fade within 0.3 m")
	assert_true(src.contains("ALPHA = alpha;") and src.contains("exp(power)"), "the fragment evaluates the Gaussian")
	for banned: String in ["SCREEN_TEXTURE", "DEPTH_TEXTURE", "hint_screen_texture", "hint_depth_texture", "NORMAL_ROUGHNESS_TEXTURE", "SubViewport", "compute"]:
		assert_false(src.contains(banned), "splat.gdshader must not use %s" % banned)
	assert_false(src.contains("MODEL_MATRIX"), "the MultiMesh's instance transforms are never read")
	assert_true(raw.contains("UNVERIFIED on"), "the multiview texelFetch caveat is written next to the code")


func test_the_shader_layout_constants_are_the_rust_ones() -> void:
	var src := _code_only(FileAccess.get_file_as_string(SHADER_PATH))
	assert_true(src.contains("const int TEXELS_PER_SPLAT = %d;" % SplatVolume.TEXELS_PER_SPLAT))
	assert_true(src.contains("const int DATA_ROW_SPLATS = %d;" % SplatVolume.DATA_ROW_SPLATS))
	assert_true(src.contains("const int INDEX_W = %d;" % SplatVolume.INDEX_W))
	assert_true(ClassDB.class_exists("SplatLoader") and ClassDB.class_exists("FrameBudget"), "gdext library loaded")
	var layout: Dictionary = ClassDB.instantiate("SplatLoader").layout()
	assert_eq(int(layout["texels_per_splat"]), SplatVolume.TEXELS_PER_SPLAT)
	assert_eq(int(layout["row_splats"]), SplatVolume.DATA_ROW_SPLATS)
	assert_eq(int(layout["index_width"]), SplatVolume.INDEX_W)
	var spec: Dictionary = ClassDB.instantiate("FrameBudget").splat_spec()
	assert_eq(Array(spec["tier_counts"]), Array(SplatVolume.TIER_COUNTS))
	assert_eq(int(spec["max_tier"]), SplatVolume.MAX_TIER)
	assert_eq(int(spec["tris_per_splat"]), SplatVolume.TRIS_PER_SPLAT)


func test_the_material_is_the_splat_shader() -> void:
	var s := _scene()
	var sv: Node3D = s["sv"]
	assert_eq(sv._material.shader, load(SHADER_PATH))
	assert_eq(sv._quad.size, Vector2(2.0, 2.0), "corners at +-1: the shader sizes the ellipse")


# --- placement and priority -----------------------------------------------------

func test_the_volume_is_in_the_arrangement_and_never_under_graph_root_or_a_node() -> void:
	var s := _scene()
	var sv: Node3D = s["sv"]
	var arr: Node3D = s["arr"]
	assert_eq(sv.get_parent(), arr)
	assert_eq(arr.volume(&"splat"), sv)
	assert_eq(sv.volume_id, &"splat")
	var n: Node = sv
	while n != null:
		assert_ne(String(n.name), "GraphRoot", "never under GraphRoot (decision 1, Invariant 8)")
		n = n.get_parent()
	assert_true(sv.is_in_group(&"xr_splat_volumes"))


func test_an_unentered_box_that_meets_a_graph_box_does_not_draw() -> void:
	var s := _scene()
	var sv: Node3D = s["sv"]
	var arr: Node3D = s["arr"]
	_load(sv)
	assert_true(sv.placement_ok(), "the ring slot is clear of the three graph boxes")
	assert_true(sv.wants_draw())
	# A graph box dragged onto the splat box.
	arr.move_volume(&"ontology", sv.transform)
	assert_false(sv.placement_ok())
	assert_false(sv.wants_draw(), "no splat draws while its box meets a graph volume's")
	sv.update_splat(DT)
	assert_false(sv.is_drawing())
	assert_eq((s["pass"] as FakePass).demand.back(), [0, 0, false], "and the budget demand is withdrawn")
	# Moved clear again, it is back.
	arr.move_volume(&"ontology", VolumeArrangement.ring_transform(60.0))
	assert_true(sv.placement_ok())


func test_the_ring_slot_stays_clear_after_every_bring_to_front() -> void:
	var s := _scene()
	var sv: Node3D = s["sv"]
	var arr: Node3D = s["arr"]
	for front: StringName in [&"splat", &"knowledge", &"codescape", &"ontology", &"splat"]:
		arr.bring_to_front(front, 0.0)
		assert_true(sv.placement_ok(), "after bringing %s to the front" % front)
	arr.reset_default()
	assert_true(sv.placement_ok())


func test_the_render_priority_follows_decision_two() -> void:
	var s := _scene()
	var sv: Node3D = s["sv"]
	# Un-entered it shares the graph's transparent priority.
	assert_eq(sv.render_priority(), SplatVolume.PRIORITY_SHARED)
	var graph_priorities: Array[int] = [
		int((load("res://materials/edge_flow.tres") as Material).render_priority),
		int((load("res://materials/node_halo.tres") as Material).render_priority),
		int((load("res://materials/agent_beam.tres") as Material).render_priority),
	]
	assert_eq(SplatVolume.PRIORITY_SHARED, graph_priorities.min(), "the same priority as the lowest graph layer, so distance orders whole volumes")
	# Entered it is below every graph layer and portal.
	sv.set_portal_room(true)
	assert_eq(sv.render_priority(), SplatVolume.PRIORITY_ENTERED)
	for p: int in graph_priorities:
		assert_lt(SplatVolume.PRIORITY_ENTERED, p)
	assert_lt(SplatVolume.PRIORITY_ENTERED, SpacePortals.MEMBRANE_PRIORITY, "below the portal membranes")
	sv.set_portal_room(false)
	assert_eq(sv.render_priority(), SplatVolume.PRIORITY_SHARED)


func test_entering_through_the_existing_portal_code_fits_the_room_and_leaving_restores_the_ring() -> void:
	var s := _scene()
	var sv: Node3D = s["sv"]
	var arr: Node3D = s["arr"]
	for id: StringName in IDS:
		arr.volume(id).attach(FixtureContentSource.new(PackedVector3Array([Vector3(-1, 0, -1), Vector3(1, 1, 1)]), StringName("fx:%s" % id)))
	_load(sv)
	var ring_xf: Transform3D = sv.transform
	var p: Node3D = SpacePortals.new()
	p.bind(arr)
	p.reduced_motion = true
	p.offline_check = func(id: StringName) -> bool: return id == &"splat" and sv.loaded_tier <= 0
	assert_true(p.can_enter(&"splat"), "a loaded splat volume can be entered like any volume")
	assert_true(p.enter(&"splat", Vector3(0.0, 1.6, 0.0)))
	for _i: int in 60:
		arr.tick(DT)
		p.tick(DT)
		sv.tick(DT)
	assert_true(sv.is_entered())
	assert_eq(sv.bounds, SpacePortals.ROOM_BOUNDS, "the ADR-2166 room-scale fit")
	assert_eq(sv.render_priority(), SplatVolume.PRIORITY_ENTERED)
	assert_true(p.portal_ids().has(&"knowledge"), "the graph volumes became portals")
	# The scene is fitted into the room: its largest extent is inside the box.
	var aabb: AABB = sv.content.transform * sv._splat_source.content_aabb()
	assert_lte(aabb.size.x, SpacePortals.ROOM_BOUNDS.x + 0.001)
	assert_lte(aabb.size.y, SpacePortals.ROOM_BOUNDS.y + 0.001)
	assert_true(p.leave())
	for _i: int in 60:
		arr.tick(DT)
		p.tick(DT)
	assert_false(sv.is_entered())
	assert_eq(sv.render_priority(), SplatVolume.PRIORITY_SHARED)
	assert_true(sv.transform.is_equal_approx(ring_xf), "the ring is exactly as it was")


# --- loading, verification, refusal ---------------------------------------------

func test_it_loads_the_granted_tier_and_draws_once_the_first_order_arrives() -> void:
	var s := _scene(2)
	var sv: Node3D = s["sv"]
	watch_signals(sv)
	_load(sv)
	assert_eq(sv.loaded_tier, 2, "the variant for the grant, not the largest")
	assert_eq(sv.splat_count, 24)
	assert_signal_emitted(sv, "loaded")
	assert_eq(_requests.size(), 2, "one listing, one blob")
	assert_true(_requests[0].ends_with("/api/splats"))
	assert_true(String(_requests[1]).contains("/api/splats/blob/sha256-12-"))
	assert_eq(sv._mmi.multimesh.instance_count, 24)
	assert_eq(sv._mmi.multimesh.visible_instance_count, 24)
	_until(sv, func() -> bool: return sv.is_drawing(), "the first sorted order")
	assert_true(sv.is_drawing())
	assert_eq(sv.drawn_splats(), 24)
	assert_eq(sv.splat_draw_calls(), 1)
	var last_applied: Array = (s["pass"] as FakePass).applied.back()
	assert_eq(last_applied[0], 48, "the triangles it drew are reported: 24 quads x 2")
	assert_eq(sv._index_tex.get_width(), SplatVolume.INDEX_W)
	assert_eq(sv._data_tex.get_width(), 4096)
	assert_true(sv._mmi.custom_aabb.size.x >= 8.0, "culled by the scene's bounds, not the empty instance transforms")


func test_it_shows_the_credits_while_drawn_and_hides_them_otherwise() -> void:
	var s := _scene(3)
	var sv: Node3D = s["sv"]
	var seen: Array[String] = []
	sv.credits_changed.connect(func(line: String) -> void: seen.append(line))
	_load(sv)
	_until(sv, func() -> bool: return sv.is_drawing(), "drawn")
	var want := "T. Maté, ISS (CC-BY-4.0 https://creativecommons.org/licenses/by/4.0/), modified (decimated)"
	assert_eq(sv.credits_line(), want)
	assert_true(sv.credits_line().contains("modified (decimated)"))
	assert_eq(seen.back(), want)
	sv.set_comfort(true, true)
	sv.update_splat(DT)
	assert_eq(sv.credits_line(), "", "nothing drawn, nothing credited")
	assert_eq(seen.back(), "")


func test_a_hash_mismatch_is_a_refusal_with_no_retry_and_nothing_drawn() -> void:
	var s := _scene(3)
	var sv: Node3D = s["sv"]
	_tamper = true
	watch_signals(sv)
	sv.request_asset("iss")
	_until(sv, func() -> bool: return sv.phase == SplatVolume.Phase.FAILED, "the refusal")
	assert_true(sv.failure_refused, "a mismatch is a refusal")
	assert_true(sv.failure.contains("does not match"), sv.failure)
	assert_signal_emitted_with_parameters(sv, "load_failed", [sv.failure, true])
	assert_eq(_requests.size(), 2, "the listing and ONE blob request: no retry")
	for _i: int in 30:
		sv.update_splat(DT)
		OS.delay_msec(5)
	assert_eq(_requests.size(), 2, "and none later either")
	assert_eq(sv.loaded_tier, 0)
	assert_false(sv.is_drawing())
	assert_eq(sv.credits_line(), "")


func test_an_http_failure_is_not_a_refusal() -> void:
	var s := _scene(3)
	var sv: Node3D = s["sv"]
	_listing = "not json"
	sv.request_asset("iss")
	_until(sv, func() -> bool: return sv.phase == SplatVolume.Phase.FAILED, "the failure")
	assert_false(sv.failure_refused)


func test_an_entered_scene_cannot_be_entered_before_it_is_loaded() -> void:
	var s := _scene()
	var arr: Node3D = s["arr"]
	var sv: Node3D = s["sv"]
	var p: Node3D = SpacePortals.new()
	p.bind(arr)
	p.offline_check = func(id: StringName) -> bool: return id == &"splat" and sv.loaded_tier <= 0
	assert_false(p.can_enter(&"splat"), "nothing loaded yet")


# --- the budget account ---------------------------------------------------------

func test_it_states_its_demand_and_never_draws_above_the_grant() -> void:
	var s := _scene(1)
	var sv: Node3D = s["sv"]
	var fp: FakePass = s["pass"]
	_load(sv)
	assert_eq(sv.loaded_tier, 1, "granted tier 1: the xr-1 variant loads, however high the governor climbs")
	var d: Array = fp.demand.back()
	assert_eq(d[1], 1, "one volume asks for a call")
	assert_false(bool(d[2]), "not entered")
	# The grant rises (entered, the account lets it climb): the larger variant loads, and the
	# swap lands through the dim.
	fp.caps = {"splat_tier": 3}
	_climb(sv, 3)
	_until(sv, func() -> bool: return sv.loaded_tier == 3 and not sv.is_fading(), "the tier-3 swap")
	assert_eq(sv.splat_count, 36)
	# The grant falls: nothing above it draws, from the next frame.
	fp.caps = {"splat_tier": 1}
	sv.update_splat(DT)
	assert_false(sv.is_drawing(), "fail closed: hidden the same frame the grant drops")
	_until(sv, func() -> bool: return sv.loaded_tier == 1 and not sv.is_fading(), "the swap down")
	assert_lte(sv.splat_count, 12)


func test_a_late_variant_larger_than_the_grant_is_dropped() -> void:
	var s := _scene(3)
	var sv: Node3D = s["sv"]
	var fp: FakePass = s["pass"]
	sv.request_asset("iss")
	_until(sv, func() -> bool: return sv.phase == SplatVolume.Phase.LOADING or sv.phase == SplatVolume.Phase.DOWNLOADING or sv.loaded_tier > 0, "load started")
	fp.caps = {"splat_tier": 1}
	_until(sv, func() -> bool: return sv.loaded_tier > 0 and not sv.is_fading() and sv.phase == SplatVolume.Phase.READY, "settled")
	assert_lte(sv.loaded_tier, 1, "never above the grant, even for a decode already in flight")


func test_the_volume_asks_for_one_call_only_while_it_wants_to_draw() -> void:
	var s := _scene(3)
	var sv: Node3D = s["sv"]
	var fp: FakePass = s["pass"]
	sv.update_splat(DT)
	assert_eq(fp.demand.back(), [0, 0, false], "no asset requested: no demand")
	_load(sv)
	sv.update_splat(DT)
	assert_eq(fp.demand.back()[1], 1)
	sv.request_visible(false)
	sv.update_splat(DT)
	assert_eq(fp.demand.back(), [0, 0, false], "a hidden volume withdraws its demand")


# --- comfort ------------------------------------------------------------------

func test_low_cost_hides_the_splats_and_withdraws_the_demand_in_the_same_call() -> void:
	var s := _scene(3)
	var sv: Node3D = s["sv"]
	var fp: FakePass = s["pass"]
	_load(sv)
	_until(sv, func() -> bool: return sv.is_drawing(), "drawn")
	assert_gt(sv.wanted_tier(), 0)
	sv.set_comfort(true, true) # no tick in between
	assert_false(sv.is_drawing(), "hidden in the same call")
	assert_eq(sv.wanted_tier(), 0, "the governor reads tier 0")
	assert_eq(fp.demand.back(), [0, 0, false], "the demand is withdrawn now, not at the next tick")
	assert_eq(fp.applied.back(), [0, 0])
	for _i: int in 20:
		sv.update_splat(DT)
	assert_false(sv.is_drawing())
	sv.set_comfort(true, false)
	assert_gt(sv.wanted_tier(), 0, "and it resumes when low cost ends")


func test_reduced_motion_is_a_dim_and_snap_otherwise_a_half_second_ease() -> void:
	var s := _scene(3)
	var sv: Node3D = s["sv"]
	sv.reduced_motion = true
	assert_almost_eq(sv._fade_half() * 2.0, 2.0 * SplatVolume.FADE_SEC, 1.0e-6)
	assert_lt(sv._fade_half() * 2.0, 0.3, "a dim, not an animation")
	sv.reduced_motion = false
	assert_almost_eq(sv._fade_half() * 2.0, SplatVolume.EASE_SEC, 1.0e-6, "the eased swap is the 0.5 s of ADR-2107")


func test_a_tier_change_lands_only_while_the_view_is_dark() -> void:
	var s := _scene(1)
	var sv: Node3D = s["sv"]
	var fp: FakePass = s["pass"]
	_load(sv)
	_until(sv, func() -> bool: return sv.is_drawing(), "drawn")
	fp.caps = {"splat_tier": 3}
	_climb(sv, 3)
	# Run until the fade starts, recording the dim at the frame the new scene lands.
	var landed_alpha := -1.0
	var before: int = sv.loaded_tier
	var t0 := Time.get_ticks_msec()
	while Time.get_ticks_msec() - t0 < 20000 and landed_alpha < 0.0:
		sv.tick(DT)
		sv.update_splat(DT)
		if sv.loaded_tier != before:
			landed_alpha = sv.fade_alpha
		OS.delay_msec(4)
	assert_gte(landed_alpha, 0.0, "the swap happened")
	assert_lte(landed_alpha, 0.001, "and it landed in full dark (dim-and-snap)")
	assert_eq(sv.loaded_tier, 3)
	_until(sv, func() -> bool: return not sv.is_fading(), "the dim back in")
	assert_almost_eq(sv.fade_alpha, 1.0, 1.0e-6)


func test_the_volume_is_static_and_world_locked() -> void:
	var s := _scene(3)
	var sv: Node3D = s["sv"]
	_load(sv)
	for _i: int in 30:
		sv.update_splat(DT)
	var xf: Transform3D = sv.transform
	var content_xf: Transform3D = sv.content.transform
	for _i: int in 200:
		sv.tick(DT)
		sv.update_splat(DT)
	assert_eq(sv.transform, xf, "nothing animates the volume's own transform")
	assert_true(sv.content.transform.is_equal_approx(content_xf), "no drift, no auto-rotation")
	assert_true("rotate" not in _code_only(FileAccess.get_file_as_string("res://scripts/splat_volume.gd")), "no spin in the script")


# --- the governor follows eye_candy.gd's signal ---------------------------------

class FakeCandy:
	extends Node3D
	var signal_mode := "pending"


class FakeComfort:
	extends Node3D
	var candy := FakeCandy.new()
	func get_eye_candy() -> Node3D:
		return candy


func test_the_governor_takes_its_signal_from_eye_candy_and_runs_no_probe_of_its_own() -> void:
	var s := _scene(3)
	var sv: Node3D = s["sv"]
	var comfort: Node3D = sv._comfort_node
	comfort.candy.signal_mode = "pending"
	sv._signal_mode = "pending"
	sv._feed_governor(DT)
	assert_eq(sv._signal_mode, "pending", "eye_candy is still probing: nothing is fed")
	comfort.candy.signal_mode = "gpu"
	sv._feed_governor(DT)
	assert_eq(String(sv._governor.signal()), "gpu")
	comfort.candy.signal_mode = "frame"
	sv._feed_governor(DT)
	assert_eq(String(sv._governor.signal()), "frame", "falls back when eye_candy does")
	assert_eq(int(sv._governor.max_tier()), 2, "with the ceiling at tier 2")
	var src := _code_only(FileAccess.get_file_as_string("res://scripts/splat_volume.gd")).replace("# ", "#")
	assert_false(src.contains("viewport_set_measure_render_time"), "eye_candy.gd owns the measure flag")


# --- the HUD ------------------------------------------------------------------

func _hud() -> Node3D:
	var hud: Node3D = HudScene.instantiate()
	add_child_autofree(hud)
	await get_tree().process_frame
	await get_tree().process_frame
	return hud


func test_the_hud_resting_line_carries_the_credits_and_hover_hints_win() -> void:
	var hud := await _hud()
	var line := "T. Maté, ISS (CC-BY-4.0 https://creativecommons.org/licenses/by/4.0/), modified (decimated)"
	hud.set_credits(line)
	hud._process(0.016)
	assert_eq(hud._hint_bar.text, line, "the credits are the resting line")
	assert_true(hud._hint_bar.text.contains("modified (decimated)"))
	hud.flash_notice("hello", 4.0)
	hud._process(0.016)
	assert_true(hud._hint_bar.text.contains("hello"), "a notice outranks the credits")
	hud.set_credits("")
	hud._notice_until_ms = 0
	hud._process(0.016)
	assert_true(hud._hint_bar.text.begins_with("ⓘ "), "no splat, the ordinary hint")


func test_the_hud_splat_button_appears_with_a_loaded_scene_and_emits_enter_and_leave() -> void:
	var hud := await _hud()
	assert_false(hud._splat_row.visible, "hidden without a scene: the Layout page costs nothing")
	watch_signals(hud)
	hud.set_splat_state({"enabled": true, "available": true, "entered": false})
	assert_true(hud._splat_row.visible)
	assert_eq(hud._splat_button.text, "Enter Splat Scene")
	hud._splat_button.pressed.emit()
	assert_signal_emitted_with_parameters(hud, "control_pressed", ["splat_enter"])
	hud.set_splat_state({"enabled": true, "available": true, "entered": true})
	assert_eq(hud._splat_button.text, "Leave Splat Scene")
	hud._splat_button.pressed.emit()
	assert_signal_emitted_with_parameters(hud, "control_pressed", ["splat_leave"])
	hud._show_tab("layout")
	await get_tree().process_frame
	var page: Control = hud._pages["layout"]
	assert_lte(page.get_combined_minimum_size().y, hud._tabs_host.size.y + 1.0, "the Layout page still fits its host with the button")


func test_the_session_row_has_a_splat_toggle_that_reads_the_scene_state() -> void:
	var hud := await _hud()
	assert_not_null(hud._splat_toggle, "beside KG, Onto and Code in the volume row")
	assert_eq(hud._splat_toggle.get_parent(), (hud._volume_buttons[&"codescape"] as Button).get_parent())
	assert_eq(hud._splat_toggle.text, "Splat off", "off until GraphScene says otherwise")
	watch_signals(hud)
	hud._splat_toggle.pressed.emit()
	assert_signal_emitted_with_parameters(hud, "control_pressed", ["splat_toggle"])
	hud.set_splat_state({"enabled": true, "loading": true})
	assert_eq(hud._splat_toggle.text, "Splat …")
	hud.set_splat_state({"enabled": true, "available": true})
	assert_eq(hud._splat_toggle.text, "Splat")
	hud.set_splat_state({"enabled": true, "failure": "splat listing: HTTP 502"})
	assert_eq(hud._splat_toggle.text, "Splat ×")
	hud.set_splat_state({"enabled": false, "available": true})
	assert_eq(hud._splat_toggle.text, "Splat off", "hidden wins over loaded")


func test_the_layout_group_shows_while_on_with_enter_live_only_once_loaded() -> void:
	var hud := await _hud()
	hud.set_splat_state({"enabled": true, "loading": true})
	assert_true(hud._splat_header.visible, "on: the group is there even before a variant lands")
	assert_true(hud._splat_row.visible)
	assert_true(hud._splat_button.disabled, "nothing to enter yet")
	assert_eq(hud._splat_button.text, "Loading splat scene…")
	hud.set_splat_state({"enabled": true, "failure": "splat listing: HTTP 502"})
	assert_true(hud._splat_button.disabled)
	assert_true(hud._splat_button.text.contains("HTTP 502"), "the failure is said, not hidden")
	hud.set_splat_state({"enabled": true, "available": true})
	assert_false(hud._splat_button.disabled)
	assert_eq(hud._splat_button.text, "Enter Splat Scene")
	hud.set_splat_state({"enabled": false})
	assert_false(hud._splat_row.visible, "off: the Layout page costs nothing")


func test_the_session_volume_row_still_fits_the_panel_with_the_splat_toggle() -> void:
	var hud := await _hud()
	hud.set_splat_state({"enabled": true, "failure": "x"})
	hud.set_volume_state(&"knowledge", {&"knowledge": {"shown": false, "online": false, "name": "Knowledge Graph"}})
	var row: Control = hud._splat_toggle.get_parent()
	assert_lte(row.get_combined_minimum_size().x, hud._tabs_host.size.x + 1.0,
		"eight buttons, longest faces, inside the page width")


func test_the_splat_scene_is_off_by_default() -> void:
	var XrPrefs = load("res://scripts/xr_prefs.gd")
	assert_false(XrPrefs.splat_scene(), "off until measured on the headset (unless XR_SPLATS or the stored choice says so)")


# --- the governor's feed: render frames only, only while drawing -------------------

class FakeEnv:
	extends Node3D
	signal visual_comfort_changed(reduced_motion: bool, low_cost: bool)
	var state := {"reduced_motion": true, "low_cost": false}
	var candy := FakeCandy.new()
	func get_visual_comfort() -> Dictionary:
		return state
	func get_eye_candy() -> Node3D:
		return candy


func test_the_governor_is_fed_from_the_render_frame_not_the_physics_tick() -> void:
	var src := _code_only(FileAccess.get_file_as_string("res://scripts/splat_volume.gd"))
	var update := src.substr(src.find("func update_splat("), src.find("func _manage_tier(") - src.find("func update_splat("))
	assert_false(update.contains("_feed_governor"), "update_splat runs on the physics tick: a fixed delta would read as a constant frame time")
	assert_true(src.substr(src.find("func _process("), 600).contains("_feed_governor(delta)"), "fed from _process")


func test_no_sample_is_fed_while_no_splat_is_drawn_so_the_volume_cannot_deadlock_at_the_floor() -> void:
	var s := _scene(1)
	var sv: Node3D = s["sv"]
	var fp: FakePass = s["pass"]
	var env := FakeEnv.new()
	add_child_autofree(env)
	env.candy.signal_mode = "frame"
	env.add_child(env.candy)
	sv._comfort_node = env
	sv._signal_mode = "pending"
	# Slow frames (50 ms) from the start, before the listing has even returned: nothing is
	# drawn, so nothing is fed, and the tier cannot fall.
	for _i: int in 600:
		sv._process(0.05)
	assert_eq(sv.wanted_tier(), 1, "the floor holds and the volume still asks for a grant")
	_load(sv)
	assert_gt(sv.loaded_tier, 0, "the scene loads however slow the graph is")
	_until(sv, func() -> bool: return sv.is_drawing(), "drawn")
	# It can be entered, and once entered it climbs on calm frames (frame mode: tier 2 at most).
	var p: Node3D = SpacePortals.new()
	p.bind(s["arr"])
	p.offline_check = func(id: StringName) -> bool: return id == &"splat" and sv.loaded_tier <= 0
	assert_true(p.can_enter(&"splat"))
	sv.set_portal_room(true)
	fp.caps = {"splat_tier": 2}
	for _i: int in 2000:
		sv.update_splat(DT)
		sv._process(0.011) # 11 ms: under 1.03 x the 90 Hz budget
	assert_eq(sv.wanted_tier(), 2, "entered and calm: it climbs")
	assert_eq(int(sv._governor.max_tier()), 2, "to the frame-time ceiling")


func test_slow_frames_while_drawing_drop_the_tier_to_the_floor_not_to_nothing() -> void:
	var s := _scene(1)
	var sv: Node3D = s["sv"]
	_load(sv)
	_until(sv, func() -> bool: return sv.is_drawing(), "drawn")
	_climb(sv, 3)
	assert_eq(sv.wanted_tier(), 3)
	for _i: int in 3000:
		sv._governor.feed(12.0, 0.05, true) # GPU ms, well over 8
	assert_eq(sv.wanted_tier(), 1, "down to the smallest scene")
	sv.update_splat(DT)
	assert_gt(sv.loaded_tier, 0, "and never to nothing")


# --- the comfort node arrives after the volume ----------------------------------

func test_a_comfort_node_that_joins_late_is_still_bound_and_low_cost_hides_in_the_same_call() -> void:
	var s := _scene(3)
	var sv: Node3D = s["sv"]
	sv._comfort_node = null # as if no environment existed when the volume was built
	_load(sv)
	_until(sv, func() -> bool: return sv.is_drawing(), "drawn")
	var env := FakeEnv.new()
	add_child_autofree(env)
	env.candy.signal_mode = "gpu"
	env.add_child(env.candy)
	env.add_to_group(&"xr_visual_environment")
	sv._comfort_tries = 0
	sv._process(DT) # the retry finds it
	assert_eq(sv._comfort_node, env, "bound late")
	env.visual_comfort_changed.emit(false, true)
	assert_false(sv.is_drawing(), "low cost hides the splats in the same call")
	assert_false(sv.reduced_motion, "and reduced motion arrived with it")
	assert_eq((s["pass"] as FakePass).demand.back(), [0, 0, false])
	env.remove_from_group(&"xr_visual_environment")


func test_a_late_comfort_node_with_low_cost_already_set_is_taken_at_once() -> void:
	var s := _scene(3)
	var sv: Node3D = s["sv"]
	sv._comfort_node = null
	var env := FakeEnv.new()
	env.state = {"reduced_motion": false, "low_cost": true}
	add_child_autofree(env)
	env.candy.signal_mode = "gpu"
	env.add_child(env.candy)
	env.add_to_group(&"xr_visual_environment")
	sv._bind_comfort()
	assert_true(sv.is_low_cost())
	assert_eq(sv.wanted_tier(), 0)
	env.remove_from_group(&"xr_visual_environment")


# --- the scene wiring -----------------------------------------------------------

func test_graph_scene_builds_the_splat_volume_into_the_arrangement_when_the_pref_is_on() -> void:
	var XrPrefs = load("res://scripts/xr_prefs.gd")
	var tmp := "user://xr_prefs.splat_test.cfg"
	DirAccess.remove_absolute(ProjectSettings.globalize_path(tmp))
	XrPrefs.path_override = tmp
	XrPrefs.set_splat_scene(true)
	assert_true(XrPrefs.splat_scene())
	var scene: Node3D = load("res://scenes/GraphScene.tscn").instantiate()
	add_child(scene)
	await get_tree().process_frame
	await get_tree().process_frame
	var sv: Node3D = scene._splat
	assert_not_null(sv, "the pref builds the volume")
	if sv != null:
		assert_eq(sv.get_parent(), scene._arrangement, "in the arrangement, a sibling of GraphRoot")
		assert_false(scene._volumes.has(&"splat"), "not a graph volume: no connection, no packing")
		assert_false(&"splat" in scene.VOLUME_ORDER)
		assert_true(scene._volume_offline(&"splat"), "not enterable before a scene has loaded")
		scene._on_hud_control("splat_enter")
		assert_false(scene._portals.is_entered(), "Enter is refused until loaded, without error")
		# The physics tick runs the splat volume and the budget pass with it present.
		scene._physics_process(DT)
		scene._tick_frame_budget(0.3)
		assert_true(scene._budget.caps.has("splat_tier"))
		assert_true(scene._budget.splat_demand.has("entered"))
		# The hud follows the volume.
		scene._sync_hud_splat()
	XrPrefs.path_override = ""
	DirAccess.remove_absolute(ProjectSettings.globalize_path(tmp))
	scene.queue_free()
	await get_tree().process_frame


func test_graph_scene_has_no_splat_volume_by_default() -> void:
	var XrPrefs = load("res://scripts/xr_prefs.gd")
	var tmp := "user://xr_prefs.splat_default.cfg"
	DirAccess.remove_absolute(ProjectSettings.globalize_path(tmp))
	XrPrefs.path_override = tmp
	var scene: Node3D = load("res://scenes/GraphScene.tscn").instantiate()
	add_child(scene)
	await get_tree().process_frame
	if not XrPrefs.splat_scene(): # XR_SPLATS in the environment would turn it on
		assert_null(scene._splat, "off by default: the live client is unchanged")
		assert_true(scene._volume_offline(&"splat"))
	XrPrefs.path_override = ""
	scene.queue_free()
	await get_tree().process_frame


func test_the_hud_toggle_builds_hides_and_reshows_the_splat_volume_and_is_remembered() -> void:
	var XrPrefs = load("res://scripts/xr_prefs.gd")
	var tmp := "user://xr_prefs.splat_toggle.cfg"
	DirAccess.remove_absolute(ProjectSettings.globalize_path(tmp))
	XrPrefs.path_override = tmp
	XrPrefs.set_splat_scene(false)
	var scene: Node3D = load("res://scenes/GraphScene.tscn").instantiate()
	add_child(scene)
	await get_tree().process_frame
	assert_null(scene._splat, "stored off: not built")
	assert_eq(scene._splat_hud_state(), {"enabled": false})
	scene._on_hud_control("splat_toggle")
	var sv: Node3D = scene._splat
	assert_not_null(sv, "the first press builds it live")
	assert_true(XrPrefs.splat_scene(), "and remembers on")
	if sv != null:
		assert_eq(sv.get_parent(), scene._arrangement)
		assert_true(scene._splat_hud_state()["enabled"])
		assert_true(scene._splat_hud_state()["loading"] or scene._splat_hud_state()["failure"] != "",
			"loading, or failed with a reason (no backend in the test)")
		scene._on_hud_control("splat_toggle")
		assert_false(sv.visible_state, "the second press hides it")
		assert_false(sv.wants_draw(), "hidden: no splat demand")
		assert_false(XrPrefs.splat_scene(), "and remembers off over XR_SPLATS")
		assert_false(scene._splat_hud_state()["enabled"])
		scene._on_hud_control("splat_toggle")
		assert_true(sv.visible_state, "the third press shows the same volume again")
		assert_eq(scene._splat, sv, "never built twice")
	XrPrefs.path_override = ""
	DirAccess.remove_absolute(ProjectSettings.globalize_path(tmp))
	scene.queue_free()
	await get_tree().process_frame


# ADR-2178: the stage reaches the volume from the plan, and only a usable one does.
func test_a_staged_plan_gives_the_volume_its_stage() -> void:
	var content := AABB(Vector3(-16.4, -0.3, -24.0), Vector3(30.2, 24.6, 32.9))
	var plan := {"staged": true, "metres_per_unit": 2.25, "focus": Vector3(0.06, 4.76, -12.02)}
	var st: Dictionary = SplatVolume.stage_from_plan(plan, content)
	assert_almost_eq(float(st["metres_per_unit"]), 2.25, 1.0e-6)
	assert_eq(st["focus"], Vector3(0.06, 4.76, -12.02))
	assert_eq(st["content"], content)
	assert_eq(SplatVolume.stage_from_plan({"staged": false}, content), {}, "unstaged")
	assert_eq(SplatVolume.stage_from_plan({}, content), {}, "a server without stages")
	assert_eq(SplatVolume.stage_from_plan({"staged": true, "metres_per_unit": 0.0, "focus": Vector3.ZERO}, content), {}, "no scale")
	assert_eq(SplatVolume.stage_from_plan({"staged": true, "metres_per_unit": 1.0, "focus": Vector3(0.0, 99.0, 0.0)}, content), {}, "focus outside")

