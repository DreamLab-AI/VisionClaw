extends "res://addons/gut/test.gd"

# ADR-2189: the HUD's seven sections come from data/ia.json (generated on the
# desktop from the settings registry and committed byte-identical), each XR page
# sits in exactly one section, at most two levels deep (section tab → rail), and
# every XR subsection name is the desktop's or an explicit headset-only one.

const HudIa := preload("res://scripts/hud_ia.gd")
const LabelBudget := preload("res://scripts/label_budget.gd")
const XrPrefs := preload("res://scripts/xr_prefs.gd")
const TABS := "HudViewport/HudControl/Root/Tabs"
const TABBAR := "HudViewport/HudControl/Root/TabBar"


func _make_hud() -> Node3D:
	var hud: Node3D = (load("res://scenes/HUD.tscn") as PackedScene).instantiate()
	add_child_autofree(hud)
	await get_tree().process_frame
	await get_tree().process_frame
	return hud


func _raw_ia() -> Dictionary:
	return JSON.parse_string(FileAccess.get_file_as_string(HudIa.IA_PATH))


# --- the document --------------------------------------------------------------

func test_the_committed_ia_json_parses_as_the_seven_sections() -> void:
	var doc: Dictionary = HudIa.load_doc()
	assert_true(bool(doc["ok"]), str(doc["error"]))
	var ids: Array[String] = []
	for s: Dictionary in doc["sections"]:
		ids.append(s["id"])
	assert_eq(ids, HudIa.SECTION_IDS)


func test_the_fallback_matches_the_committed_document() -> void:
	var raw: Dictionary = _raw_ia()
	for i: int in range(HudIa.FALLBACK_SECTIONS.size()):
		var f: Dictionary = HudIa.FALLBACK_SECTIONS[i]
		var r: Dictionary = raw["sections"][i]
		assert_eq(f["id"], r["id"])
		assert_eq(f["label"], r["label"])
		assert_eq(f["hotkey"], r["hotkey"])
	for k: String in HudIa.FALLBACK_SCOPES:
		assert_eq(HudIa.FALLBACK_SCOPES[k], raw["scopes"][k], "scope %s" % k)


func test_a_missing_or_malformed_document_falls_back_to_the_seven() -> void:
	var missing: Dictionary = HudIa.load_doc("res://tests/fixtures/no_such_ia.json")
	assert_false(bool(missing["ok"]))
	assert_eq((missing["sections"] as Array).size(), 7)
	var six: Dictionary = HudIa.load_doc("res://tests/fixtures/ia_six_sections.json")
	assert_false(bool(six["ok"]), "six sections are not ADR-2189's seven")
	assert_eq((six["sections"] as Array)[5]["label"], "Agents & Governance", "the built-in seven instead")


func test_the_parser_reads_labels_subsections_and_scopes() -> void:
	var doc: Dictionary = HudIa.load_doc("res://tests/fixtures/ia_min.json")
	assert_true(bool(doc["ok"]))
	assert_eq((doc["sections"] as Array)[2]["subsections"], ["Sub encode"] as Array[String])
	assert_eq(doc["scopes"]["shared"], "Fixture shared text")


# --- the XR placement map ------------------------------------------------------

func test_every_xr_subsection_name_is_the_desktops_or_headset_only() -> void:
	var doc: Dictionary = HudIa.load_doc()
	for s: Dictionary in doc["sections"]:
		var unknown: Array[String] = HudIa.unknown_names(s["id"], s["subsections"])
		assert_eq(unknown, [] as Array[String], "%s: %s not in ia.json and not XR-only" % [s["id"], str(unknown)])


func test_every_page_sits_in_exactly_one_section() -> void:
	var hud := await _make_hud()
	for pid: String in hud.TAB_ORDER:
		var n := 0
		for sid: String in HudIa.SECTION_IDS:
			if HudIa.pages_of(sid).has(pid):
				n += 1
		assert_eq(n, 1, "%s is in one section" % pid)
	for sid: String in HudIa.SECTION_IDS:
		assert_lte(HudIa.pages_of(sid).size(), hud.RAIL_MAX, "%s fits the rail" % sid)


# --- the HUD -------------------------------------------------------------------

func test_the_tab_bar_is_ia_json_s_sections_then_fit_legend_explore() -> void:
	var hud := await _make_hud()
	var raw: Dictionary = _raw_ia()
	var want: Array[String] = []
	for s: Dictionary in raw["sections"]:
		want.append(s["id"])
	assert_eq(hud.section_ids(), want, "the HUD tab list equals ia.json's sections")
	var bar: Control = hud.get_node(TABBAR)
	var names: Array[String] = []
	for b: Node in bar.get_children():
		names.append(b.name)
	assert_eq(names.slice(7), ["FitButton", "LegendButton", "ExploreButton"] as Array[String])
	assert_eq(bar.get_child_count(), 10, "as many buttons as the ten tabs before")
	for i: int in range(7):
		var label: String = raw["sections"][i]["label"]
		var shown: String = (bar.get_child(i) as Button).text
		assert_true(label.begins_with(shown), "%s shows as %s" % [label, shown])


func test_a_section_opens_its_last_page_and_the_rail_switches_within_it() -> void:
	var hud := await _make_hud()
	(hud.get_node("%s/Section_explore" % TABBAR) as Button).pressed.emit()
	assert_eq(hud.active_page(), "query")
	var rail: Control = hud.get_node("%s/Rail" % TABS)
	var shown: Array = []
	for b: Button in rail.get_children():
		if b.visible:
			shown.append(b.text)
	assert_eq(shown, ["Query builder", "Pins"])
	(rail.get_child(1) as Button).pressed.emit()
	assert_eq(hud.active_page(), "pins")
	assert_eq(hud.active_section(), "explore", "two levels: still the same section")
	(hud.get_node("%s/Section_layers" % TABBAR) as Button).pressed.emit()
	assert_false((rail.get_child(0) as Button).visible, "a one-page section shows no rail")
	(hud.get_node("%s/Section_explore" % TABBAR) as Button).pressed.emit()
	assert_eq(hud.active_page(), "pins", "the section remembers its page")


func test_fit_legend_and_explore() -> void:
	var hud := await _make_hud()
	watch_signals(hud)
	var bar: Control = hud.get_node(TABBAR)
	(bar.get_node("FitButton") as Button).pressed.emit()
	assert_signal_emitted_with_parameters(hud, "control_pressed", ["fit_view"])
	(bar.get_node("LegendButton") as Button).pressed.emit()
	assert_eq([hud.active_section(), hud.active_page()], ["encode", "key"])
	(bar.get_node("ExploreButton") as Button).pressed.emit()
	assert_eq([hud.active_section(), hud.active_page()], ["explore", "query"])
	for b: Button in bar.get_children():
		assert_eq(b.action_mode, BaseButton.ACTION_MODE_BUTTON_PRESS, "Invariant 4")


func _assert_fits(hud: Node3D, page_name: String, label: String) -> void:
	var host: Control = hud.get_node(TABS)
	var page: Control = hud.get_node("%s/%s" % [TABS, page_name])
	var need: Vector2 = page.get_combined_minimum_size()
	assert_lte(need.y, host.size.y + 1.0, "%s height %.0f ≤ %.0f" % [label, need.y, host.size.y])
	assert_lte(need.x, host.size.x - hud.RAIL_W - hud.RAIL_GAP + 1.0, "%s width %.0f fits beside the rail" % [label, need.x])


func test_every_page_fits_its_host_in_height_and_width_beside_the_rail() -> void:
	var hud := await _make_hud()
	for pid: String in hud.TAB_ORDER:
		_assert_fits(hud, "%sPage" % hud.TAB_LABELS[pid], pid)
	# The tightest page: Query with the on-screen keyboard open (four rows of ten keys).
	hud._show_tab("query")
	hud._show_query_mode("memory")
	hud.open_memory_keyboard()
	await get_tree().process_frame
	_assert_fits(hud, "QueryPage", "query with the keyboard open")


func test_physics_pages_carry_the_shared_scope_text_from_ia_json() -> void:
	var hud := await _make_hud()
	var shared: String = _raw_ia()["scopes"]["shared"]
	for page_name in ["LayoutPage", "ShapesPage"]:
		var l: Label = hud.get_node("%s/%s" % [TABS, page_name]).find_child("SharedScope", true, false)
		assert_not_null(l, "%s says it is shared" % page_name)
		if l != null:
			assert_true(l.text.ends_with(shared))
	assert_null(hud.get_node("%s/EncodePage" % TABS).find_child("SharedScope", true, false), "headset-local controls do not")


func test_encode_and_headset_steppers_emit_and_show_their_values() -> void:
	var hud := await _make_hud()
	watch_signals(hud)
	var enc: Control = hud.get_node("%s/EncodePage" % TABS)
	var row: Control = enc.find_child("LabelBudgetRow", true, false)
	(row.get_child(3) as Button).pressed.emit()
	assert_signal_emitted_with_parameters(hud, "control_pressed", ["label_budget_plus"])
	(enc.find_child("TextSizeRow", true, false).get_child(1) as Button).pressed.emit()
	assert_signal_emitted_with_parameters(hud, "control_pressed", ["label_text_minus"])
	hud.set_label_controls(6, 1.25)
	assert_eq((row.get_node("Value") as Label).text, "6")
	var head: Control = hud.get_node("%s/HeadsetPage" % TABS)
	(head.find_child("DwellRow", true, false).get_child(3) as Button).pressed.emit()
	assert_signal_emitted_with_parameters(hud, "control_pressed", ["dwell_plus"])
	hud.set_headset_controls(700, false)
	assert_eq((head.find_child("DwellRow", true, false).get_node("Value") as Label).text, "700 ms")
	assert_eq(hud._haptics_button.text, "Haptics: Off")


# --- steps and persistence -----------------------------------------------------

func test_label_steps_stay_inside_the_pool_and_text_scales_snap() -> void:
	assert_eq(LabelBudget.step_budget(8, true), 8, "never past the pool's ranked budget")
	assert_eq(LabelBudget.step_budget(8, false), 6)
	assert_eq(LabelBudget.step_budget(0, false), 0)
	assert_eq(LabelBudget.step_text_scale(1.0, true), 1.25)
	assert_eq(LabelBudget.step_text_scale(0.75, false), 0.75)
	assert_eq(LabelBudget.snap_text_scale(1.3), 1.25)


func test_view_prefs_round_trip_in_the_test_file() -> void:
	XrPrefs.path_override = "user://xr_prefs.ia_test.cfg"
	DirAccess.remove_absolute(ProjectSettings.globalize_path(XrPrefs.path()))
	assert_eq(XrPrefs.get_number(XrPrefs.KEY_DWELL_MS, 600), 600.0, "default when unset")
	XrPrefs.set_number(XrPrefs.KEY_DWELL_MS, 700)
	XrPrefs.set_flag(XrPrefs.KEY_HAPTICS, false)
	assert_eq(XrPrefs.get_number(XrPrefs.KEY_DWELL_MS, 600), 700.0)
	assert_false(XrPrefs.get_flag(XrPrefs.KEY_HAPTICS, true))
	DirAccess.remove_absolute(ProjectSettings.globalize_path(XrPrefs.path()))
	XrPrefs.path_override = ""


# --- GraphScene owns the values -------------------------------------------------

func test_the_scene_steps_persists_and_applies_the_headset_controls() -> void:
	XrPrefs.path_override = "user://xr_prefs.ia_scene_test.cfg"
	DirAccess.remove_absolute(ProjectSettings.globalize_path(XrPrefs.path()))
	var scene: Node3D = (load("res://scenes/GraphScene.tscn") as PackedScene).instantiate()
	add_child(scene)
	await get_tree().process_frame
	assert_eq(scene._label_budget, LabelBudget.DEFAULT_BUDGET)
	scene._on_hud_control("label_budget_minus")
	assert_eq(scene._label_budget, LabelBudget.DEFAULT_BUDGET - LabelBudget.BUDGET_STEP)
	assert_eq(int(XrPrefs.get_number(XrPrefs.KEY_LABEL_BUDGET, -1)), scene._label_budget, "persisted")
	scene._on_hud_control("label_text_plus")
	var title: Label3D = scene._label_pool[0].get_node("Title")
	assert_almost_eq(title.pixel_size, scene.LABEL_TITLE_PX * 1.25, 1e-7, "label text grows")
	scene._on_hud_control("dwell_plus")
	assert_eq(scene._dwell_ms, 700)
	assert_eq(int(scene._selection.dwell_ms()), 700, "the Rust arbiter uses it")
	for _i in range(5):
		scene._on_hud_control("dwell_plus")
	assert_eq(int(scene._selection.dwell_ms()), 800, "clamped to the arbiter's band")
	scene._on_hud_control("haptics_toggle")
	assert_false(scene._haptics_on)
	assert_false(XrPrefs.get_flag(XrPrefs.KEY_HAPTICS, true))
	scene._manual_transform = true
	scene._on_hud_control("fit_view")
	assert_false(scene._manual_transform, "Fit drops the two-hand latch")
	scene.queue_free()
	await get_tree().process_frame
	# A new scene restores the stored values.
	var again: Node3D = (load("res://scenes/GraphScene.tscn") as PackedScene).instantiate()
	add_child(again)
	await get_tree().process_frame
	assert_eq(again._label_budget, LabelBudget.DEFAULT_BUDGET - LabelBudget.BUDGET_STEP)
	assert_eq(again._label_text_scale, 1.25)
	assert_eq(int(again._selection.dwell_ms()), 800)
	assert_false(again._haptics_on)
	again.queue_free()
	await get_tree().process_frame
	DirAccess.remove_absolute(ProjectSettings.globalize_path(XrPrefs.path()))
	XrPrefs.path_override = ""


func test_fit_undoes_the_two_hand_turn_and_scale_and_leaves_physics_alone() -> void:
	var scene: Node3D = (load("res://scenes/GraphScene.tscn") as PackedScene).instantiate()
	add_child(scene)
	await get_tree().process_frame
	var home: Transform3D = scene._arrangement.transform
	# What _apply_two_hand_manip writes: the whole arrangement, turned and scaled.
	scene._arrangement.global_transform = Transform3D(Basis(Vector3.UP, 0.7).scaled(Vector3.ONE * 3.0), Vector3(1, 0.5, -2))
	scene._manual_transform = true
	var physics_before: float = scene._repel_k
	scene._on_hud_control("fit_view")
	assert_true(scene._arrangement.transform.is_equal_approx(home), "Fit restores the arrangement")
	assert_false(scene._manual_transform)
	assert_eq(scene._repel_k, physics_before, "no physics write")
	scene.queue_free()
	await get_tree().process_frame


func test_edges_plus_minus_change_what_each_volume_asks_for() -> void:
	var scene: Node3D = (load("res://scenes/GraphScene.tscn") as PackedScene).instantiate()
	add_child(scene)
	await get_tree().process_frame
	scene._on_hud_control("edges_minus")
	assert_almost_eq(scene._edge_share, 0.7, 1e-6)
	for vid: StringName in scene._sources:
		assert_almost_eq(float(scene._sources[vid].edge_share), 0.7, 1e-6, "%s asks for 70 %%" % vid)
	for _i in range(20):
		scene._on_hud_control("edges_minus")
	assert_almost_eq(scene._edge_share, scene.EDGE_SHARE_MIN, 1e-6, "floored")
	for _i in range(20):
		scene._on_hud_control("edges_plus")
	assert_eq(scene._edge_share, 1.0, "capped at all of them")
	scene.queue_free()
	await get_tree().process_frame


func test_comfort_and_colour_mode_are_restored_from_the_stored_choice() -> void:
	if not OS.get_environment("XR_REDUCED_MOTION").is_empty() or not OS.get_environment("XR_VISUAL_QUALITY").is_empty():
		pending("an XR_REDUCED_MOTION / XR_VISUAL_QUALITY launch value outranks the stored choice")
		return
	XrPrefs.path_override = "user://xr_prefs.ia_comfort_test.cfg"
	DirAccess.remove_absolute(ProjectSettings.globalize_path(XrPrefs.path()))
	XrPrefs.set_flag(XrPrefs.KEY_REDUCED_MOTION, false)
	XrPrefs.set_flag(XrPrefs.KEY_LOW_COST, true)
	XrPrefs.set_number(XrPrefs.KEY_COLOUR_MODE, 1)
	var scene: Node3D = (load("res://scenes/GraphScene.tscn") as PackedScene).instantiate()
	add_child(scene)
	await get_tree().process_frame
	await get_tree().process_frame
	var env: Node = scene.find_child("SpatialEnvironment", true, false)
	if env != null and env.has_method("get_visual_comfort"):
		var st: Dictionary = env.get_visual_comfort()
		assert_false(bool(st["reduced_motion"]), "Reduce motion off, as stored")
		assert_true(bool(st["low_cost"]), "Low-cost visuals on, as stored")
	else:
		fail_test("GraphScene has a SpatialEnvironment with get_visual_comfort")
	assert_eq(int(scene._binary_client.get_color_mode()), 1, "community colouring, as stored")
	# Pressing the toggles stores the new choice.
	scene._on_hud_control("color_mode_toggle")
	assert_eq(int(XrPrefs.get_number(XrPrefs.KEY_COLOUR_MODE, -1)), int(scene._binary_client.get_color_mode()))
	scene.queue_free()
	await get_tree().process_frame
	DirAccess.remove_absolute(ProjectSettings.globalize_path(XrPrefs.path()))
	XrPrefs.path_override = ""
