extends "res://addons/gut/test.gd"

func _hud() -> Node3D:
	var hud: Node3D = load("res://scenes/HUD.tscn").instantiate()
	add_child_autofree(hud)
	await get_tree().process_frame
	await get_tree().process_frame
	return hud

func test_header_status_does_not_intersect_approval_badge() -> void:
	var hud := await _hud()
	var badge: Control = hud.get_node("HudViewport/HudControl/AcspIndicator")
	assert_false(hud._fps_header.get_global_rect().intersects(badge.get_global_rect()),
		"FPS remains visible beside the approval badge")

func test_active_tab_uses_shape_and_surface_as_well_as_text_colour() -> void:
	var hud := await _hud()
	for tab in hud.TAB_ORDER:
		hud._show_tab(tab)
		var active: Button = hud._tab_buttons[tab]
		var active_box: StyleBoxFlat = active.get_theme_stylebox("normal")
		assert_eq(active_box.border_width_bottom, 2, "selected tab has an explicit outline")
		for other in hud.TAB_ORDER:
			if other != tab:
				var idle: Button = hud._tab_buttons[other]
				assert_ne(active_box.bg_color, idle.get_theme_stylebox("normal").bg_color,
					"selection remains visible without relying on text tint")

func test_document_overlay_remains_above_the_instrument_surface() -> void:
	var hud := await _hud()
	var background: Control = hud.get_node("HudViewport/HudControl/InstrumentSurface")
	assert_eq(background.mouse_filter, Control.MOUSE_FILTER_IGNORE)
	assert_gt(hud.document_panel.z_index, background.z_index)
	assert_gt(hud.intervention_panel.z_index, background.z_index)

func test_comfort_controls_emit_intent_and_external_sync_is_silent() -> void:
	var hud := await _hud()
	watch_signals(hud)
	hud.set_visual_comfort(true, false)
	assert_signal_not_emitted(hud, "control_pressed")
	hud._motion_toggle.button_pressed = false
	assert_signal_emitted_with_parameters(hud, "control_pressed", ["visual_motion:0"])
	hud._quality_toggle.button_pressed = true
	assert_signal_emitted_with_parameters(hud, "control_pressed", ["visual_quality:1"])
