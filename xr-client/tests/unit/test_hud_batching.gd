extends "res://addons/gut/test.gd"

# HUD canvas batching (scripts/hud_batching.gd). Measured on HP before the fix:
# a HUD re-render cost 59-73 draw calls and up to 8 242 primitives, because
# every StyleBoxFlat (rounded, anti-aliased) is a polygon the Compatibility
# renderer draws on its own and that splits the text batch around it. Boxes
# are now nine-patches from one shared atlas, drawn by a background layer
# (z -1) under all text, so backgrounds batch together and text batches
# together. perf/hud_draw_calls.gd measures the result in a GL window.

const Batching := preload("res://scripts/hud_batching.gd")
const STATES := ["normal", "hover", "pressed", "disabled", "focus", "panel", "read_only", "separator", "hover_pressed"]


func _hud() -> Node3D:
	var hud: Node3D = (load("res://scenes/HUD.tscn") as PackedScene).instantiate()
	add_child(hud)
	await get_tree().process_frame
	await get_tree().process_frame
	return hud


func _controls(n: Node, out: Array = []) -> Array:
	if n is Control:
		out.append(n)
	for c in n.get_children():
		_controls(c, out)
	return out


func test_no_control_draws_a_polygon_stylebox() -> void:
	var hud := await _hud()
	var bad: Array = []
	for c: Control in _controls(hud.get_node("HudViewport")):
		for st in STATES:
			if c.has_theme_stylebox(st):
				var sb := c.get_theme_stylebox(st)
				if sb is StyleBoxFlat or sb is StyleBoxLine:
					bad.append("%s:%s" % [c.get_path(), st])
	assert_eq(bad, [], "every flat/line box goes through the atlas background layer")
	hud.queue_free()


func test_layout_is_unchanged() -> void:
	Batching.enabled = false
	var plain := await _hud()
	Batching.enabled = true
	var batched := await _hud()
	await get_tree().process_frame
	var a: Array = _controls(plain.get_node("HudViewport"))
	var b: Array = _controls(batched.get_node("HudViewport")).filter(func(c: Control) -> bool: return not (c is Batching.HudBg))
	assert_eq(a.size(), b.size(), "same control tree apart from the background nodes")
	var moved: Array = []
	for i in mini(a.size(), b.size()):
		var ca: Control = a[i]
		var cb: Control = b[i]
		# hidden containers do not lay out their children: their sizes are stale
		if not ca.is_visible_in_tree():
			continue
		# live readouts (FPS, room) may differ between the two HUDs' frames
		if "text" in ca and str(ca.get("text")) != str(cb.get("text")):
			continue
		if not ca.get_global_rect().is_equal_approx(cb.get_global_rect()):
			moved.append("%s %s -> %s" % [ca.name, ca.get_global_rect(), cb.get_global_rect()])
	assert_eq(moved, [], "content margins kept: nothing moves")
	plain.queue_free()
	batched.queue_free()


func test_backgrounds_follow_control_state() -> void:
	var hud := await _hud()
	var btn: Button = null
	for c in _controls(hud.get_node("HudViewport")):
		if c is Button and c.is_visible_in_tree() and not (c as Button).disabled:
			btn = c
			break
	assert_not_null(btn)
	var bg: Control = btn.get_node("HudBg")
	assert_not_null(bg, "a background node per boxed control")
	assert_eq(bg.z_index, -1, "drawn in the background layer")
	assert_eq(bg.mouse_filter, Control.MOUSE_FILTER_IGNORE, "never steals the wand pointer")
	assert_eq(bg.state_name(), "normal")
	btn.disabled = true
	await get_tree().process_frame
	assert_eq(bg.state_name(), "disabled")
	btn.disabled = false
	btn.toggle_mode = true
	btn.button_pressed = true
	await get_tree().process_frame
	assert_eq(bg.state_name(), "pressed")
	hud.queue_free()


func test_atlas_covers_every_theme_box() -> void:
	var hud := await _hud()
	var od: Node = hud.get_node("HudBatching")
	assert_eq(od.unmapped(), [], "every StyleBoxFlat radius/width has an atlas region")
	assert_gt(od.converted(), 20, "the HUD's boxes were converted")
	hud.queue_free()


func test_fps_header_updates_at_most_every_two_seconds() -> void:
	var hud := await _hud()
	var label: Label = hud._fps_header
	var changes := 0
	var last := label.text
	var t0 := Time.get_ticks_msec()
	while Time.get_ticks_msec() - t0 < 1500:
		await get_tree().process_frame
		if label.text != last:
			changes += 1
			last = label.text
	assert_lte(changes, 1, "one update at most in 1.5 s")
	hud.queue_free()


func test_every_hud_glyph_is_in_the_hud_font() -> void:
	# A glyph missing from the theme font is drawn from a fallback font with
	# its own texture: one extra draw call per symbol run.
	var hud := await _hud()
	var font: Font = (hud.get_node("HudViewport/HudControl") as Control).get_theme_font("font", "Label")
	assert_eq(font.get_font_name(), "VisionClaw HUD Sans")
	var missing := {}
	var texts: Array = []
	for c in _controls(hud.get_node("HudViewport")):
		for prop in ["text", "placeholder_text"]:
			if prop in c and typeof(c.get(prop)) == TYPE_STRING:
				texts.append(c.get(prop))
		if c is RichTextLabel:
			texts.append((c as RichTextLabel).get_parsed_text())
	# strings hud.gd writes at runtime (hint bar, notices, toggles)
	texts.append_array(["ⓘ ", "⚠ ", "☑", "☐", "▲", "▼", "●", "→", "—", "…"])
	# memory search captions and the keyboard entry line (ADR-2136)
	texts.append_array(["route: query point → sidecar top-k (not a search path)", "hud layout|"])
	for t: String in texts:
		for i in t.length():
			var cp := t.unicode_at(i)
			if cp >= 32 and cp != 0xA0 and not font.has_char(cp):
				missing[t[i]] = true
	assert_eq(missing.keys(), [], "glyphs missing from the HUD font")
	hud.queue_free()


func test_check_buttons_draw_their_switch_from_the_atlas() -> void:
	var hud := await _hud()
	var cbs: Array = _controls(hud.get_node("HudViewport")).filter(func(c: Control) -> bool: return c is CheckButton)
	assert_gt(cbs.size(), 0)
	for cb: CheckButton in cbs:
		for icon in ["checked", "unchecked", "checked_disabled", "unchecked_disabled"]:
			assert_true(cb.get_theme_icon(icon) is Batching.EmptyIcon, "%s %s icon blanked" % [cb.name, icon])
		var bg: Control = cb.get_node("HudBg")
		assert_not_null(bg)
		assert_true(bg.has_switch(), "background draws the switch")
		var r: Rect2 = bg.switch_rect()
		assert_gt(r.size.x, 0.0)
		assert_almost_eq(r.end.x, cb.size.x - cb.get_theme_stylebox("normal").get_margin(SIDE_RIGHT), 0.5, "where Godot draws the icon")
	hud.queue_free()
