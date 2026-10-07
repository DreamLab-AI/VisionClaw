extends "res://addons/gut/test.gd"

# Graph Separation in the headset (ADR-2135; desktop Motion › Layout Forces ›
# "Separate Knowledge · Ontology · Memory", graphSeparationX 0–400 step 5).
#   * separation_control.gd — the write coalescer: ≤ 4 Hz while dragging, a final
#     write on release, one write at a time behind the physics gate;
#   * hud.gd — the Layout-page row: press-fire −/+ and a wand-draggable slider
#     inside the 532 px host (Invariants 4 and 5);
#   * graph_scene.gd — the PUT ?graph=knowledge path (ADR-2041) and the
#     settingsUpdated read-back that moves the slider when a peer changes it.

const SeparationControl := preload("res://scripts/separation_control.gd")
const ParityScript := preload("res://scripts/graph_parity.gd")
const TABS := "HudViewport/HudControl/Root/Tabs"


# GraphScene with the HTTP door replaced by a recorder; everything else (the
# control dispatch, the gate, the staged commit, the HUD refresh) is the real code.
class RecordingScene:
	extends "res://scripts/graph_scene.gd"
	var puts: Array = []
	var accept: bool = true
	func _put_physics_body(body: Dictionary) -> bool:
		if _physics_pending or not accept:
			return false
		puts.append(body)
		_physics_pending = true
		return true


func _scene() -> RecordingScene:
	var gs := RecordingScene.new()
	gs._graph_separation = 100.0
	return gs


func _ok(gs: RecordingScene) -> void:
	gs._on_physics_completed(HTTPRequest.RESULT_SUCCESS, 200, PackedStringArray(), PackedByteArray())


func _make_hud() -> Node3D:
	var hud: Node3D = (load("res://scenes/HUD.tscn") as PackedScene).instantiate()
	add_child(hud)
	await get_tree().process_frame
	await get_tree().process_frame
	return hud


# --- separation_control.gd ---------------------------------------------------

func test_snap_matches_the_desktop_slider_range_and_step() -> void:
	assert_eq(SeparationControl.snap(0.0), 0.0)
	assert_eq(SeparationControl.snap(2.4), 0.0, "rounds to the 5-unit step")
	assert_eq(SeparationControl.snap(2.6), 5.0)
	assert_eq(SeparationControl.snap(251.0), 250.0)
	assert_eq(SeparationControl.snap(-30.0), 0.0, "clamped at 0")
	assert_eq(SeparationControl.snap(999.0), 400.0, "clamped at 400")
	assert_eq(SeparationControl.MIN, 0.0)
	assert_eq(SeparationControl.MAX, 400.0)
	assert_eq(SeparationControl.STEP, 5.0)


func test_a_drag_writes_at_most_four_times_a_second() -> void:
	var c := SeparationControl.new()
	var sends: Array = []
	# Two seconds of a 90 Hz drag sweeping 0 → 400, every write landing at once.
	for frame in range(180):
		var now: int = frame * 11
		c.drag(400.0 * float(frame) / 179.0)
		var v: float = c.take(now, false, 100.0)
		if not is_nan(v):
			sends.append([now, v])
			c.settled()
	assert_gt(sends.size(), 1, "the drag writes while it moves")
	assert_lte(sends.size(), 9, "≤ 4 Hz over 2 s (+ the first)")
	for i in range(1, sends.size()):
		assert_gte(int(sends[i][0]) - int(sends[i - 1][0]), SeparationControl.DRAG_INTERVAL_MS,
			"writes are ≥ 250 ms apart while dragging")


func test_release_writes_the_final_value_at_once() -> void:
	var c := SeparationControl.new()
	c.drag(150.0)
	assert_eq(c.take(1000, false, 100.0), 150.0, "first drag value goes out")
	c.settled()
	c.drag(180.0)
	assert_true(is_nan(c.take(1050, false, 100.0)), "held by the 4 Hz throttle")
	c.release(203.0)
	assert_eq(c.take(1060, false, 100.0), 205.0, "release writes the snapped final value without waiting")
	c.settled()
	assert_true(is_nan(c.take(2000, false, 205.0)), "nothing further once the final value is out")
	assert_false(c.holding(), "idle after the final write settles")


func test_a_busy_gate_defers_and_the_latest_value_wins() -> void:
	var c := SeparationControl.new()
	c.release(120.0)
	assert_true(is_nan(c.take(0, true, 100.0)), "another physics write is in flight")
	c.release(300.0)
	assert_eq(c.take(10, false, 100.0), 300.0, "the newest intent is sent when the gate frees")


func test_releasing_at_the_server_value_sends_nothing() -> void:
	var c := SeparationControl.new()
	c.release(100.0)
	assert_true(is_nan(c.take(0, false, 100.0)), "no redundant write")
	assert_false(c.holding())


func test_step_builds_on_an_unsent_intent() -> void:
	var c := SeparationControl.new()
	c.step(5.0, 100.0)
	c.step(5.0, 100.0)
	assert_eq(c.display_value(100.0), 110.0, "two quick presses queue +10")
	assert_eq(c.take(0, false, 100.0), 110.0)
	assert_eq(c.display_value(100.0), 110.0, "the in-flight value shows until it settles")
	c.settled()
	assert_eq(c.display_value(110.0), 110.0)
	c.step(-5.0, 0.0)
	assert_true(is_nan(c.take(0, false, 0.0)), "no-op at the rail")
	assert_false(c.holding(), "−5 at 0 queues nothing")


# --- hud.gd ------------------------------------------------------------------

func test_layout_page_hosts_the_separation_row_inside_532px() -> void:
	var hud: Node3D = await _make_hud()
	var layout: Control = hud.get_node("%s/LayoutPage" % TABS)
	var slider: HSlider = hud._separation_slider
	assert_not_null(slider, "slider built")
	assert_true(layout.is_ancestor_of(slider), "on the Layout page")
	assert_eq(slider.min_value, 0.0)
	assert_eq(slider.max_value, 400.0)
	assert_eq(slider.step, 5.0)
	assert_gte(slider.custom_minimum_size.y, float(hud.BTN_H), "a full wand hit target")
	assert_gte(slider.get_theme_icon("grabber").get_width(), 32, "a grabber legible at arm's length (XR theme)")
	assert_gte(slider.get_theme_stylebox("slider").get_minimum_size().y, 14.0, "a thick track")
	for b: Button in [hud._separation_minus_button, hud._separation_plus_button]:
		assert_true(layout.is_ancestor_of(b))
		assert_eq(b.action_mode, BaseButton.ACTION_MODE_BUTTON_PRESS, "press-fire (Invariant 4)")
	assert_true(slider.has_meta(hud.HINT_META), "slider carries a hover hint")
	assert_lte(layout.get_combined_minimum_size().y, 532.0, "Layout page stays within 532 px (Invariant 5)")
	watch_signals(hud)
	hud._separation_minus_button.pressed.emit()
	assert_signal_emitted_with_parameters(hud, "control_pressed", ["separation_minus"])
	hud._separation_plus_button.pressed.emit()
	assert_signal_emitted_with_parameters(hud, "control_pressed", ["separation_plus"])
	hud.queue_free()
	await get_tree().process_frame


func test_wand_drag_on_the_slider_emits_drag_then_release() -> void:
	var hud: Node3D = await _make_hud()
	hud._show_tab("layout")
	await get_tree().process_frame
	await get_tree().process_frame
	var slider: HSlider = hud._separation_slider
	var vp: SubViewport = hud.get_node("HudViewport")
	var r: Rect2 = slider.get_global_rect()
	assert_gt(r.size.x, 100.0, "slider has a usable width")
	var y: float = r.get_center().y
	watch_signals(hud)
	# The wand path in graph_scene.gd: motion every frame, a press when the
	# trigger crosses 0.6, motion while held, a release when it lets go.
	_mouse(vp, Vector2(r.position.x + r.size.x * 0.25, y), MOUSE_BUTTON_LEFT, true)
	assert_true(hud._separation_dragging, "a press on the track starts a drag")
	_motion(vp, Vector2(r.position.x + r.size.x * 0.75, y))
	var p: Array = get_signal_parameters(hud, "control_pressed")
	assert_true(str(p[0]).begins_with("separation_drag:"), "dragging reports the live value: %s" % str(p))
	_mouse(vp, Vector2(r.position.x + r.size.x * 0.75, y), MOUSE_BUTTON_LEFT, false)
	p = get_signal_parameters(hud, "control_pressed")
	assert_true(str(p[0]).begins_with("separation_release:"), "letting go reports the final value: %s" % str(p))
	var v: float = float(str(p[0]).get_slice(":", 1))
	assert_almost_eq(v, 300.0, 15.0, "three quarters along a 0–400 track")
	assert_eq(fmod(v, 5.0), 0.0, "on the 5-unit step")
	assert_false(hud._separation_dragging)
	hud.queue_free()
	await get_tree().process_frame


func test_a_peer_change_moves_the_slider_but_never_mid_drag() -> void:
	var hud: Node3D = await _make_hud()
	hud.set_graph_separation(250.0)
	assert_eq(hud._separation_slider.value, 250.0, "read-back moves the slider")
	assert_true(hud._separation_value_label.text.contains("250"), "value readout follows")
	watch_signals(hud)
	hud.set_graph_separation(260.0)
	assert_signal_not_emitted(hud, "control_pressed", "a read-back never echoes a write")
	hud._separation_dragging = true
	hud.set_graph_separation(0.0)
	assert_eq(hud._separation_slider.value, 260.0, "the operator's drag is not yanked")
	hud.queue_free()
	await get_tree().process_frame


# --- graph_scene.gd ----------------------------------------------------------

func test_stepper_puts_graph_separation_and_commits_on_2xx() -> void:
	var gs := _scene()
	gs._on_hud_control("separation_plus")
	gs._pump_separation(0)
	assert_eq(gs.puts.size(), 1, "one PUT")
	assert_eq(gs.puts[0], {"graphSeparationX": 105.0}, "the camelCase field the desktop writes")
	assert_eq(gs._graph_separation, 100.0, "not committed before the server answers")
	_ok(gs)
	assert_eq(gs._graph_separation, 105.0, "committed on 2xx")
	gs._on_hud_control("separation_minus")
	gs._pump_separation(10)
	assert_eq(gs.puts[1], {"graphSeparationX": 100.0})
	gs._on_physics_completed(HTTPRequest.RESULT_SUCCESS, 403, PackedStringArray(), PackedByteArray())
	assert_eq(gs._graph_separation, 105.0, "a refused write is discarded")
	gs.free()


func test_scene_drag_is_throttled_and_the_release_lands_after_the_gate() -> void:
	var gs := _scene()
	gs._on_hud_control("separation_drag:150")
	gs._pump_separation(1000)
	assert_eq(gs.puts.size(), 1, "first drag value goes out")
	gs._on_hud_control("separation_drag:200")
	gs._pump_separation(1100)
	assert_eq(gs.puts.size(), 1, "gate busy and inside 250 ms")
	_ok(gs)
	gs._pump_separation(1150)
	assert_eq(gs.puts.size(), 1, "still inside 250 ms")
	gs._pump_separation(1260)
	assert_eq(gs.puts.size(), 2, "next write at the 4 Hz tick")
	assert_eq(gs.puts[1], {"graphSeparationX": 200.0})
	gs._on_hud_control("separation_release:240")
	gs._pump_separation(1270)
	assert_eq(gs.puts.size(), 2, "final value waits for the in-flight write")
	_ok(gs)
	gs._pump_separation(1280)
	assert_eq(gs.puts.size(), 3, "final value written once the gate frees")
	assert_eq(gs.puts[2], {"graphSeparationX": 240.0})
	_ok(gs)
	assert_eq(gs._graph_separation, 240.0)
	gs._pump_separation(5000)
	assert_eq(gs.puts.size(), 3, "nothing more")
	gs.free()


func test_settings_updated_read_back_moves_the_hud_slider() -> void:
	assert_true(ClassDB.class_exists("BinaryProtocolClient"), "gdext library loaded")
	var hud: Node3D = await _make_hud()
	var gs := _scene()
	gs.hud = hud
	var parity: Node = ParityScript.new()
	add_child(parity)
	parity.setup(gs, BinaryProtocolClient.create(), null, hud)
	parity.apply_physics_body("{\"graphSeparationX\":250.0}")
	assert_eq(gs._graph_separation, 250.0, "read back into the scene")
	assert_eq(hud._separation_slider.value, 250.0, "and onto the slider")
	parity.queue_free()
	gs.free()
	hud.queue_free()
	await get_tree().process_frame


func _mouse(vp: SubViewport, at: Vector2, button: MouseButton, pressed: bool) -> void:
	var e := InputEventMouseButton.new()
	e.button_index = button
	e.pressed = pressed
	e.position = at
	e.global_position = at
	vp.push_input(e)


func _motion(vp: SubViewport, at: Vector2) -> void:
	var e := InputEventMouseMotion.new()
	e.position = at
	e.global_position = at
	vp.push_input(e)
