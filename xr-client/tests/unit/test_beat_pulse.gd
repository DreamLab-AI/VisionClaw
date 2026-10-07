extends "res://addons/gut/test.gd"

# Beat clock, memory bursts and HUD Beat row wired through GraphScene (WP3/5/8).
# Frames are fed through the real `_on_graph_text` entry point and HUD intents
# through `_on_hud_control`, exactly as the socket and the panel deliver them.

const SESSION := "HudViewport/HudControl/Root/Tabs/SessionPage"


func _make_scene() -> Node3D:
	var packed: PackedScene = load("res://scenes/GraphScene.tscn")
	var scene: Node3D = packed.instantiate()
	add_child(scene)
	await get_tree().process_frame
	return scene


func _make_hud() -> Node3D:
	var hud: Node3D = (load("res://scenes/HUD.tscn") as PackedScene).instantiate()
	add_child(hud)
	await get_tree().process_frame
	await get_tree().process_frame
	return hud


func _beat_frame(bpm: float, source: String) -> String:
	var now_ms: float = Time.get_unix_time_from_system() * 1000.0
	return JSON.stringify({"type": "beatClock", "bpm": bpm, "phaseAt": now_ms, "confidence": 0.8,
		"source": source, "serverTime": now_ms})


func test_scene_creates_the_beat_node_with_bursts_under_the_unit_scale_root() -> void:
	var scene: Node3D = await _make_scene()
	var beat: Node = scene.get_node_or_null("BeatPulse")
	assert_not_null(beat, "GraphScene owns one BeatPulse node")
	var bursts: Node = scene.get_node_or_null("AgentEffectsRoot/MemoryBursts")
	assert_not_null(bursts, "bursts live under the unit-scale AgentEffectsRoot, never GraphRoot")
	assert_null(scene.get_node("GraphRoot").find_child("MemoryBursts", true, false))
	assert_false(beat.mic_on(), "microphone is off by default")
	scene.queue_free()
	await get_tree().process_frame


func test_memory_flash_frames_spawn_bursts_and_the_toggle_stops_them() -> void:
	var scene: Node3D = await _make_scene()
	var bursts: Node = scene.get_node("AgentEffectsRoot/MemoryBursts")
	scene._on_graph_text('{"type":"memory_flash","data":{"key":"pattern-auth","namespace":"patterns","action":"search"}}')
	assert_eq(bursts.slot_count(), 3, "a search burst = three ring slots")
	scene._on_hud_control("memory_bursts:0")
	assert_eq(bursts.slot_count(), 0, "turning bursts off clears them")
	scene._on_graph_text('{"type":"memory_flash","data":{"key":"k","action":"store"}}')
	assert_eq(bursts.slot_count(), 0, "no bursts while off")
	scene._on_hud_control("memory_bursts:1")
	scene._on_graph_text('{"type":"memory_flash","data":{"key":"k","action":"store"}}')
	assert_eq(bursts.slot_count(), 2)
	scene.queue_free()
	await get_tree().process_frame


func test_relayed_beat_clock_drives_the_shared_shader_uniforms() -> void:
	var scene: Node3D = await _make_scene()
	var beat: Node = scene.get_node("BeatPulse")
	var default_reduced: bool = beat.reduced_motion
	scene._on_graph_text(_beat_frame(120.0, "tap"))
	var st: Dictionary = beat.beat().status()
	assert_eq(String(st["active"]), "desktop", "the relayed clock is driving")
	assert_almost_eq(float(st["bpm"]), 120.0, 1e-6)
	# Over a beat the pulse must swing; sample several frames.
	var hi := 0.0
	var lo := 1.0
	for i: int in range(40):
		await get_tree().process_frame
		var p: float = beat.current_pulse()
		hi = maxf(hi, p)
		lo = minf(lo, p)
	assert_eq(hi, lo, "steady under reduced motion: no swing through the beat")
	# The uniform must land on the materials the meshes RENDER with: the scene-
	# local duplicates spatial_environment.gd makes, not the shared .tres files.
	# The node halo is its own camera-facing quad layer (NodesHaloMulti); far edges
	# draw as ribbons (EdgesRibbonMulti) beside the near cylinders (EdgesMulti).
	var live_halo: ShaderMaterial = (scene.get_node("GraphRoot/NodesHaloMulti") as MultiMeshInstance3D).material_override
	var live_edge: ShaderMaterial = (scene.get_node("GraphRoot/EdgesMulti") as MultiMeshInstance3D).material_override
	var live_ribbon: ShaderMaterial = (scene.get_node("GraphRoot/EdgesRibbonMulti") as MultiMeshInstance3D).material_override
	for m: ShaderMaterial in [live_halo, live_edge, live_ribbon]:
		assert_true(m.shader.get_shader_uniform_list().any(func(u: Dictionary) -> bool: return u["name"] == "beat_pulse"),
			"%s declares beat_pulse" % m.shader.resource_path)
	assert_ne(live_edge, load("res://materials/edge_flow.tres"), "edges render with a scene-local duplicate")
	assert_true(beat.pulse_materials().has(live_halo) and beat.pulse_materials().has(live_edge) and beat.pulse_materials().has(live_ribbon))
	assert_eq(float(live_halo.get_shader_parameter("beat_pulse")), 0.0, "reduced motion: halos hold steady")
	assert_eq(float(live_edge.get_shader_parameter("beat_pulse")), 0.0, "reduced motion: edges hold steady")
	assert_eq(float(scene.get_node("AgentEffectsRoot/MemoryBursts").beat_pulse), 0.0, "reduced motion: bursts hold steady")
	assert_string_contains(String(beat.beat().status_line()), "120.0 bpm", "the HUD readout still shows the tempo")
	# Land exactly on a beat with motion allowed, so the pulse is near 1 and a
	# stale 0 on the live materials cannot pass by accident.
	beat.reduced_motion = false
	scene._reduced_motion = false
	scene._on_graph_text(_beat_frame(120.0, "tap"))
	await get_tree().process_frame
	var p_now: float = beat.current_pulse()
	assert_gt(p_now, 0.5, "on the beat the pulse is strong")
	assert_almost_eq(float(live_halo.get_shader_parameter("beat_pulse")), p_now, 0.01, "live halo carries the pulse")
	assert_almost_eq(float(live_edge.get_shader_parameter("beat_pulse")), p_now, 0.01, "live edges carry the pulse")
	assert_almost_eq(float(live_ribbon.get_shader_parameter("beat_pulse")), p_now, 0.01, "far edge ribbons carry the pulse")
	# A comfort toggle swaps the materials again; the next frame must follow.
	scene.get_tree().get_first_node_in_group("xr_visual_environment").set_visual_comfort(true, false)
	await get_tree().process_frame
	for m: ShaderMaterial in beat.pulse_materials():
		assert_almost_eq(float(m.get_shader_parameter("beat_pulse")), beat.current_pulse(), 0.01, "pulse follows the swap")
	# Reduced motion (the default) stops beat pulsing entirely (ADR-2107).
	assert_true(default_reduced, "reduced motion is the comfort default")
	assert_eq(hi, 0.0, "no pulse at all under reduced motion")
	# A source of "off" from the desktop stops the pulse.
	scene._on_graph_text(_beat_frame(120.0, "off"))
	await get_tree().process_frame
	await get_tree().process_frame
	assert_almost_eq(beat.current_pulse(), 0.0, 1e-6, "off clock, no pulse")
	scene.queue_free()
	await get_tree().process_frame


func test_invalid_beat_frames_are_ignored() -> void:
	var scene: Node3D = await _make_scene()
	var beat: Node = scene.get_node("BeatPulse")
	scene._on_graph_text('{"type":"beatClock","bpm":999,"phaseAt":1,"confidence":0.5,"source":"tap"}')
	scene._on_graph_text('{"type":"beatClock","bpm":120,"phaseAt":1,"confidence":0.5,"source":"radio"}')
	assert_eq(String(beat.beat().status()["active"]), "none")
	scene.queue_free()
	await get_tree().process_frame


func test_hud_tap_intent_reaches_tap_tempo() -> void:
	var scene: Node3D = await _make_scene()
	var beat: Node = scene.get_node("BeatPulse")
	for i: int in range(3):
		scene._on_hud_control("beat_tap")
	assert_eq(beat.tap_total(), 3)
	assert_eq(int(beat.beat().tap_count()), 3, "three taps registered with the Rust tap tempo")
	assert_eq(String(beat.beat().status()["active"]), "tap")
	scene.queue_free()
	await get_tree().process_frame


func test_pong_is_consumed_and_the_cloud_reads_the_beat() -> void:
	var scene: Node3D = await _make_scene()
	var now_ms: float = Time.get_unix_time_from_system() * 1000.0
	scene._on_graph_text(JSON.stringify({"type": "pong", "timestamp": now_ms - 20.0, "serverTime": now_ms + 5000.0}))
	var st: Dictionary = scene.get_node("BeatPulse").beat().status()
	assert_true(bool(st["synced"]), "pong with serverTime sets the clock offset")
	assert_almost_eq(float(st["offset_ms"]), 5010.0, 30.0, "offset ≈ serverTime − (sent + rtt/2)")
	# xr-cloud's route layer pulses on this clock through beat_source.
	var beat: Node = scene.get_node("BeatPulse")
	assert_eq(scene._memory_cloud.beat_source, beat, "memory cloud reads the beat from BeatPulse")
	assert_false(beat.is_locked(), "no clock yet")
	assert_lt(beat.beat_phase(), 0.0, "no phase without a clock")
	scene._on_graph_text(_beat_frame(120.0, "tap"))
	assert_true(beat.is_locked())
	var ph: float = beat.beat_phase()
	assert_true(ph >= 0.0 and ph < 1.0, "phase in [0, 1)")
	assert_almost_eq(beat.beat_pulse(), exp(-6.0 * ph), 0.1, "raw envelope, not reduced-motion scaled")
	scene.queue_free()
	await get_tree().process_frame


func test_roster_teleport_and_comfort_actions_are_routed_not_unknown() -> void:
	var scene: Node3D = await _make_scene()
	# A Swarm-roster row emits "teleport:<id>"; before the fix only the radial menu
	# handled it, so the HUD tap fell through to the unknown-action warning.
	# In a session the XRCamera3D is the viewport's current camera; GUT's current
	# scene is not GraphScene, so make it current explicitly.
	(scene.get_node("XROrigin3D/XRCamera3D") as Camera3D).current = true
	scene._teleport_active = false
	scene._teleport_pulse_id = -1
	scene._on_hud_control("teleport:42")
	assert_true(scene._teleport_active, "roster teleport starts the glide")
	assert_eq(scene._teleport_pulse_id, 42, "and highlights the target node")
	# Comfort toggles are owned by spatial_environment; GraphScene must not treat
	# them as unknown (it returns before the match).
	scene._on_hud_control("visual_motion:1")
	scene._on_hud_control("visual_quality:0")
	assert_eq(scene._teleport_pulse_id, 42, "comfort toggles leave scene state alone")
	scene.queue_free()
	await get_tree().process_frame


func test_session_beat_row_fires_on_press_and_fits() -> void:
	var hud: Node3D = await _make_hud()
	hud._show_tab("session")
	await get_tree().process_frame
	var row: Node = hud.get_node_or_null("%s/BeatRow" % SESSION)
	assert_not_null(row, "Beat row on the Session page")
	for n: String in ["BeatTap", "BeatMic", "MemoryBursts"]:
		var b: BaseButton = row.get_node(n)
		assert_eq(b.action_mode, BaseButton.ACTION_MODE_BUTTON_PRESS, "%s fires on press (Invariant 4)" % n)
		assert_true(b.has_meta("hint"), "%s has a hint" % n)
	var page: Control = hud.get_node(SESSION)
	var host: Control = hud.get_node("HudViewport/HudControl/Root/Tabs")
	assert_lte(page.get_combined_minimum_size().y, host.size.y + 1.0, "Session page fits its %.0fpx host" % host.size.y)
	watch_signals(hud)
	(row.get_node("BeatMic") as Button).emit_signal("pressed")
	assert_signal_emitted_with_parameters(hud, "control_pressed", ["beat_mic:1"])
	(row.get_node("BeatTap") as Button).emit_signal("pressed")
	assert_signal_emitted_with_parameters(hud, "control_pressed", ["beat_tap"])
	hud.queue_free()
	await get_tree().process_frame


func test_mic_badge_shows_only_while_listening() -> void:
	var hud: Node3D = await _make_hud()
	var badge: Label = hud.get_node("HudViewport/HudControl/Root/Header/MicBadge")
	assert_false(badge.visible, "no badge while the mic is off")
	hud.set_beat_status("Beat: off · mic listening", true, "listening")
	assert_true(badge.visible, "badge visible while listening")
	assert_string_contains(badge.text, "listening")
	assert_string_contains((hud.get_node("%s/BeatRow/BeatMic" % SESSION) as Button).text, "☑")
	hud.set_beat_status("Beat: off", false, "off")
	assert_false(badge.visible)
	hud.queue_free()
	await get_tree().process_frame

