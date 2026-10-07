extends "res://addons/gut/test.gd"

# HUD control-centre redesign (task #20): tab switching, hover-hint resolution,
# per-tab fit (no below-the-fold overflow), and preserved public API/signals.
# No godot binary in this container → these run in CI / a live session.

const TABS := "HudViewport/HudControl/Root/Tabs"
const TABBAR := "HudViewport/HudControl/Root/TabBar"


func _make_hud() -> Node3D:
	var packed: PackedScene = load("res://scenes/HUD.tscn")
	var hud: Node3D = packed.instantiate()
	add_child(hud)
	# Two frames so the SubViewport lays out the built Control tree.
	await get_tree().process_frame
	await get_tree().process_frame
	return hud


func test_all_pages_built_and_default_graph_visible() -> void:
	var hud: Node3D = await _make_hud()
	for page_name in ["GraphPage", "LayoutPage", "QueryPage", "PinsPage", "SwarmPage", "KeyPage", "SessionPage", "HelpPage"]:
		assert_not_null(hud.get_node_or_null("%s/%s" % [TABS, page_name]), "%s exists" % page_name)
	var graph: Control = hud.get_node("%s/GraphPage" % TABS)
	assert_true(graph.visible, "Graph is the default visible tab")
	var session: Control = hud.get_node("%s/SessionPage" % TABS)
	assert_false(session.visible, "other tabs start hidden")
	hud.queue_free()
	await get_tree().process_frame


func test_show_tab_switches_exactly_one_page() -> void:
	var hud: Node3D = await _make_hud()
	hud._show_tab("session")
	assert_true((hud.get_node("%s/SessionPage" % TABS) as Control).visible, "session shown")
	assert_false((hud.get_node("%s/GraphPage" % TABS) as Control).visible, "graph hidden")
	var visible_count := 0
	for page_name in ["GraphPage", "LayoutPage", "QueryPage", "PinsPage", "SwarmPage", "KeyPage", "SessionPage", "HelpPage"]:
		if (hud.get_node("%s/%s" % [TABS, page_name]) as Control).visible:
			visible_count += 1
	assert_eq(visible_count, 1, "exactly one page visible after a tab switch")
	hud.queue_free()
	await get_tree().process_frame


func test_every_button_and_tab_has_a_hint() -> void:
	var hud: Node3D = await _make_hud()
	var missing: Array[String] = []
	# Tabs + tab bar AND the overlay panels (Document / Intervention) — every
	# interactive control anywhere in the panel must carry a hint.
	for path in [TABS, TABBAR,
			"HudViewport/HudControl/DocumentPanel",
			"HudViewport/HudControl/InterventionPanel"]:
		var root: Node = hud.get_node_or_null(path)
		if root != null:
			_collect_hintless(root, missing)
	assert_eq(missing.size(), 0, "every interactive control carries a 'hint' meta; missing: %s" % str(missing))
	hud.queue_free()
	await get_tree().process_frame


# Recurse the WHOLE subtree (recursion is outside the interactive-check, so it
# descends plain containers too), flagging any Button/CheckButton/LineEdit that
# lacks a "hint" meta.
func _collect_hintless(root: Node, out: Array) -> void:
	for n in root.get_children():
		if (n is Button or n is CheckButton or n is LineEdit) and not n.has_meta("hint"):
			out.append(String(n.name))
		_collect_hintless(n, out)


func test_hint_resolution_defaults_when_nothing_hovered() -> void:
	var hud: Node3D = await _make_hud()
	# No synthetic pointer pushed → nothing hovered → default hint.
	assert_eq(hud._resolve_hint(), "Point at a control for help", "falls back to the default hint")
	hud.queue_free()
	await get_tree().process_frame


func test_no_page_overflows_its_host() -> void:
	var hud: Node3D = await _make_hud()
	var host: Control = hud.get_node(TABS)
	assert_gt(host.size.y, 1.0, "tab host has a real height")
	for page_name in ["GraphPage", "LayoutPage", "QueryPage", "PinsPage", "SwarmPage", "KeyPage", "SessionPage", "HelpPage"]:
		var page: Control = hud.get_node("%s/%s" % [TABS, page_name])
		var need: float = page.get_combined_minimum_size().y
		assert_lte(need, host.size.y + 1.0,
			"%s min-height %.0f must fit host %.0f (no below-the-fold)" % [page_name, need, host.size.y])
	hud.queue_free()
	await get_tree().process_frame


func test_public_api_and_signals_preserved() -> void:
	var hud: Node3D = await _make_hud()
	for m in ["set_controls_status", "set_control_states", "set_fold_state", "set_query_preview",
			"hide_query_preview", "show_document", "hide_document", "show_case", "clear_case",
			"set_case_count", "set_dwell_charge", "set_avatar_count", "set_mtp_ms", "flash_notice",
			"configure_intervention", "approve_selected_case", "deny_selected_case", "_on_connection_status"]:
		assert_true(hud.has_method(m), "public method preserved: %s" % m)
	for s in ["join_requested", "mute_toggled", "decision_submitted", "case_decided",
			"control_pressed", "query_execute_pressed", "query_clear_pressed"]:
		assert_true(hud.has_signal(s), "signal preserved: %s" % s)
	hud.queue_free()
	await get_tree().process_frame


func test_control_button_emits_named_action() -> void:
	var hud: Node3D = await _make_hud()
	watch_signals(hud)
	# The Reset Layout button lives on the Graph page; press it programmatically.
	var reset := _find_button(hud.get_node("%s/GraphPage" % TABS), "Reset Layout")
	assert_not_null(reset, "Reset Layout button exists on the Graph page")
	if reset != null:
		reset.pressed.emit()
		assert_signal_emitted_with_parameters(hud, "control_pressed", ["reset_layout"])
	hud.queue_free()
	await get_tree().process_frame


func _find_button(root: Node, text: String) -> Button:
	for n in root.get_children():
		if n is Button and (n as Button).text == text:
			return n
		var found := _find_button(n, text)
		if found != null:
			return found
	return null


# Wave 2, Feature 3 — type show/hide toggles live on the Graph page and, when
# pressed, flip their visible state and emit control_pressed
# "type_toggle:<class>:<1|0>".
func test_type_toggles_present_and_emit_on_press() -> void:
	var hud: Node3D = await _make_hud()
	var graph: Control = hud.get_node("%s/GraphPage" % TABS)
	# Starts visible → label shows "☑".
	var knowledge: Button = _find_button(graph, "Knowledge ☑")
	assert_not_null(knowledge, "Knowledge type toggle present, visible by default")
	assert_not_null(_find_button(graph, "Ontology ☑"), "Ontology toggle present")
	assert_not_null(_find_button(graph, "Agents ☑"), "Agents toggle present")
	# Pressing emits control_pressed with the now-hidden (0) state and relabels.
	watch_signals(hud)
	knowledge.pressed.emit()
	assert_signal_emitted_with_parameters(hud, "control_pressed", ["type_toggle:knowledge:0"])
	assert_eq(knowledge.text, "Knowledge ☐", "label flips to hidden marker")
	# Pressing again re-shows it.
	knowledge.pressed.emit()
	assert_signal_emitted_with_parameters(hud, "control_pressed", ["type_toggle:knowledge:1"])
	hud.queue_free()
	await get_tree().process_frame


# Colour key tab: a legend of the live palette. Every row carries a "key" meta
# (its label) + a hover hint and at least one swatch; the agent-status rows reuse
# the Swarm roster LUT so the two can never drift apart.
func test_key_tab_lists_swatch_rows() -> void:
	var hud: Node3D = await _make_hud()
	var page: Control = hud.get_node("%s/KeyPage" % TABS)
	var rows: Array = []
	_collect_key_rows(page, rows)
	assert_gte(rows.size(), 18, "one key row per legend entry")
	var labels: Array = []
	for r: Node in rows:
		labels.append(String(r.get_meta("key")))
		assert_true(r.has_meta("hint"), "key row '%s' carries a hover hint" % r.get_meta("key"))
		assert_gte(_swatch_count(r), 1, "key row '%s' has a swatch" % r.get_meta("key"))
	for expected in ["Community hue", "Query mark ?v1…?v8", "Working", "Create (widens in)", "Delete (implodes)", "Subclass-of", "Ray firing"]:
		assert_has(labels, expected, "key lists '%s'" % expected)
	var blocked: Node = _key_row(rows, "Blocked / error")
	assert_not_null(blocked, "blocked row present")
	if blocked != null:
		assert_eq(_first_swatch(blocked).color, hud.SWARM_STATUS_COLORS[2], "blocked swatch = roster LUT")
	# Palette helpers are deterministic and opaque, like the Rust originals.
	assert_eq(hud.community_swatch(3), hud.community_swatch(3), "community swatch deterministic")
	assert_ne(hud.community_swatch(1), hud.community_swatch(2), "communities differ")
	assert_eq(hud.query_swatch(0), hud.query_swatch(8), "query palette cycles at 8")
	assert_eq(hud.query_swatch(1).a, 1.0, "query swatch opaque")
	hud.queue_free()
	await get_tree().process_frame


func _collect_key_rows(root: Node, out: Array) -> void:
	for n in root.get_children():
		if n.has_meta("key"):
			out.append(n)
		_collect_key_rows(n, out)


func _swatch_count(row: Node) -> int:
	var c := 0
	for n in row.get_children():
		if n is ColorRect:
			c += 1
	return c


func _first_swatch(row: Node) -> ColorRect:
	for n in row.get_children():
		if n is ColorRect:
			return n
	return null


func _key_row(rows: Array, label: String) -> Node:
	for r: Node in rows:
		if String(r.get_meta("key")) == label:
			return r
	return null


# A flashed notice takes over the bottom strip (every tab) and then hands it back
# to the hover hints once it expires.
func test_flash_notice_overrides_hint_then_expires() -> void:
	var hud: Node3D = await _make_hud()
	var bar: Label = hud.get_node("HudViewport/HudControl/Root/HintBar")
	hud.flash_notice("Layout write denied (HTTP 401)", 0.2)
	assert_true(hud._notice_active(), "notice active right after flash")
	await get_tree().process_frame
	assert_true(bar.text.begins_with("⚠ Layout write denied"), "hint bar shows the notice")
	await get_tree().create_timer(0.35).timeout
	assert_false(hud._notice_active(), "notice expires")
	await get_tree().process_frame
	assert_true(bar.text.begins_with("ⓘ "), "hint bar returns to hover hints")
	hud.queue_free()
	await get_tree().process_frame


func test_memory_cloud_buttons_fire_on_press_and_fit_the_graph_page() -> void:
	var hud: Node3D = await _make_hud()
	var graph: Control = hud.get_node("%s/GraphPage" % TABS)
	var b: Button = hud._memory_cloud_button
	assert_not_null(b, "Memory button built")
	assert_true(graph.is_ancestor_of(b), "on the Graph page")
	assert_eq(b.action_mode, BaseButton.ACTION_MODE_BUTTON_PRESS, "press-fire (Invariant 4)")
	assert_eq(hud._memory_colour_button.action_mode, BaseButton.ACTION_MODE_BUTTON_PRESS)
	watch_signals(hud)
	b.pressed.emit()
	assert_signal_emitted_with_parameters(hud, "control_pressed", ["memory_cloud_toggle"])
	hud.set_memory_cloud_state(true, "Memory: 6000", "Age")
	assert_eq(b.text, "Memory: 6000")
	assert_eq(hud._memory_colour_button.text, "Cloud: Age")
	var host: Control = hud.get_node(TABS)
	assert_lte(graph.get_combined_minimum_size().y, host.size.y + 1.0, "Graph page fits its host")
	# the route's sidecar agreement: one line in the Memory row, still inside 532 px
	var line: Label = hud._memory_route_label
	assert_not_null(line)
	assert_false(line.visible, "hidden without a route")
	hud.set_memory_route_line("Route: 50 of 50 sidecar hits are in the sample · 50 of 50 sampled agree with the local top-k")
	assert_true(line.visible)
	assert_true(graph.is_ancestor_of(line), "on the Graph page, under the Memory buttons")
	assert_eq(line.get_line_count(), 1, "a single line")
	assert_lte(graph.get_combined_minimum_size().y, host.size.y + 1.0, "Graph page with the line fits its host")
	assert_lte(graph.get_combined_minimum_size().y, 532.0, "and stays within 532 px")
	hud.set_memory_route_line("")
	assert_false(line.visible)
	assert_eq((hud.get_node("HudPanel").material_override as Material).render_priority, 20, "HUD panel above the route")
	hud.queue_free()
	await get_tree().process_frame
