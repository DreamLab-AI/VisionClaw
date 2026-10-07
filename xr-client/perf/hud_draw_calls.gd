extends SceneTree
## HUD re-render cost probe (GL window; headless counts read 0).
##
##   godot --path xr-client --rendering-method gl_compatibility --xr-mode off \
##     -s res://perf/hud_draw_calls.gd [-- variant=<name>]
##
## Instantiates HUD.tscn alone in front of a camera, forces its SubViewport to
## re-render every frame, and prints the renderer's global worst-frame draw
## calls and primitives for every page, as HUD_DRAW_CALLS={json}. Variants strip
## one suspected batch breaker at a time (diagnosis only):
##   baseline, no_styleboxes, no_text, no_clip, msdf, swatch_bg, msdf_swatch_bg

func _init() -> void:
	var variant := "baseline"
	for a in OS.get_cmdline_user_args():
		if a.begins_with("variant="):
			variant = a.trim_prefix("variant=")
	var cam := Camera3D.new()
	root.add_child(cam)
	var hud: Node3D = (load("res://scenes/HUD.tscn") as PackedScene).instantiate()
	hud.position = Vector3(0, 0, -0.9)
	root.add_child(hud)
	await process_frame
	await process_frame
	var vp: SubViewport = hud.get_node("HudViewport")
	_apply_variant(vp, variant)
	var out := {"variant": variant, "pages": {}}
	if variant.begins_with("bisect:"):
		# bisect:<Page>: hide each direct child of the page in turn
		var page_name := variant.trim_prefix("bisect:")
		var page: Node = vp.get_node("HudControl/Root/Tabs/" + page_name)
		for c in vp.get_node("HudControl/Root/Tabs").get_children():
			if c is CanvasItem:
				(c as CanvasItem).visible = c == page
		var res := {"all": await _measure(vp)}
		var kids: Array = page.get_children()
		for i in kids.size():
			var k: Node = kids[i]
			if not (k is CanvasItem) or not (k as CanvasItem).visible:
				continue
			(k as CanvasItem).visible = false
			res["%d:%s:%s" % [i, k.name, k.get_class()]] = await _measure(vp)
			(k as CanvasItem).visible = true
		print("HUD_BISECT=%s" % JSON.stringify(res))
		quit(0)
		return
	var tabs: Node = vp.get_node_or_null("HudControl/Root/Tabs")
	var pages: Array = []
	for c in tabs.get_children():
		if c is CanvasItem:
			pages.append(c.name)
	for page in pages:
		if tabs != null:
			for c in tabs.get_children():
				if c is CanvasItem:
					(c as CanvasItem).visible = c.name == page
		var dc := 0
		var pr := 0
		for i in 12:
			vp.render_target_update_mode = SubViewport.UPDATE_ONCE
			await process_frame
			if i >= 4:
				dc = maxi(dc, int(RenderingServer.get_rendering_info(RenderingServer.RENDERING_INFO_TOTAL_DRAW_CALLS_IN_FRAME)))
				pr = maxi(pr, int(RenderingServer.get_rendering_info(RenderingServer.RENDERING_INFO_TOTAL_PRIMITIVES_IN_FRAME)))
		out["pages"][page] = {"draw_calls": dc, "primitives": pr}
	var nonascii := {}
	for n in _all(vp):
		for prop in ["text"]:
			if n is Control and prop in n and typeof(n.get(prop)) == TYPE_STRING:
				var t: String = n.get(prop)
				for i in t.length():
					if t.unicode_at(i) >= 128:
						nonascii[t[i]] = int(nonascii.get(t[i], 0)) + 1
	out["non_ascii"] = nonascii
	var lab: Label = null
	for n in _all(vp):
		if n is Label:
			lab = n
			break
	if lab != null:
		var fnt: Font = lab.get_theme_font("font")
		var info := {"font": fnt.get_class(), "label_font_size": lab.get_theme_font_size("font_size")}
		var ff: FontFile = fnt as FontFile
		if ff == null and fnt is FontVariation:
			ff = (fnt as FontVariation).base_font as FontFile
		if ff != null:
			info["msdf"] = ff.multichannel_signed_distance_field
			var sizes := {}
			for sz: Vector2i in ff.get_size_cache_list(0):
				sizes[str(sz)] = {"textures": ff.get_texture_count(0, sz), "tex_px": ff.get_texture_image(0, sz, 0).get_size() if ff.get_texture_count(0, sz) > 0 else Vector2i.ZERO}
			info["caches"] = sizes
		out["font"] = info
	print("HUD_DRAW_CALLS=%s" % JSON.stringify(out))
	quit(0)


func _measure(vp: SubViewport) -> int:
	var dc := 0
	for i in 10:
		vp.render_target_update_mode = SubViewport.UPDATE_ONCE
		await process_frame
		if i >= 4:
			dc = maxi(dc, int(RenderingServer.get_rendering_info(RenderingServer.RENDERING_INFO_TOTAL_DRAW_CALLS_IN_FRAME)))
	return dc


func _apply_variant(vp: SubViewport, variant: String) -> void:
	if variant.contains("msdf"):
		var base: Font = ThemeDB.fallback_font
		print("fallback font class: ", base.get_class())
		if base is FontFile:
			var f := (base as FontFile).duplicate() as FontFile
			f.multichannel_signed_distance_field = true
			f.msdf_pixel_range = 8
			f.msdf_size = int(OS.get_environment("HUD_MSDF_SIZE")) if OS.has_environment("HUD_MSDF_SIZE") else 24
			var hc: Control = vp.get_node("HudControl")
			hc.theme.default_font = f
			var bold := FontVariation.new()
			bold.base_font = f
			bold.variation_embolden = 0.6
			hc.theme.set_font("bold_font", "RichTextLabel", bold)
			hc.theme.set_font("normal_font", "RichTextLabel", f)
	for n in _all(vp):
		if not (n is Control):
			continue
		var c := n as Control
		match variant:
			"no_styleboxes":
				for sb in ["normal", "hover", "pressed", "disabled", "focus", "panel", "read_only", "separator", "background", "fill", "tab_selected", "tab_unselected"]:
					c.add_theme_stylebox_override(sb, StyleBoxEmpty.new())
			"no_text":
				if c is Label:
					(c as Label).text = ""
				elif c is Button:
					(c as Button).text = ""
				elif c is RichTextLabel:
					(c as RichTextLabel).text = ""
				elif c is LineEdit:
					(c as LineEdit).text = ""
					(c as LineEdit).placeholder_text = ""
			"no_clip":
				c.clip_contents = false
		if variant.contains("swatch_bg") and c is ColorRect and c.get_parent() is Container:
			c.z_index = -1
		if variant.contains("same_glyphs"):
			if c is Label:
				(c as Label).text = "AB"
			elif c is Button:
				(c as Button).text = "AB"
			elif c is RichTextLabel:
				(c as RichTextLabel).text = "AB"
		if variant.contains("ascii"):
			for prop in ["text", "placeholder_text"]:
				if prop in c and typeof(c.get(prop)) == TYPE_STRING:
					var t: String = c.get(prop)
					var o := ""
					for i in t.length():
						o += t[i] if t.unicode_at(i) < 128 else "-"
					c.set(prop, o)
		if variant.contains("rtl_plain") and c is RichTextLabel:
			(c as RichTextLabel).text = (c as RichTextLabel).get_parsed_text()
		if variant.contains("no_scrollbars") and c is ScrollBar:
			c.modulate.a = 0.0
			c.visible = false
		if variant.contains("no_sliders") and (c is Slider or c is Range and not (c is ScrollBar)):
			c.visible = false
		if variant.contains("unclip_tabs") and c.name == "Tabs":
			c.clip_contents = false
		elif variant.contains("unclip") and not variant.contains("unclip_tabs"):
			c.clip_contents = false
		if variant.contains("no_rtl") and c is RichTextLabel:
			c.visible = false
		if variant.contains("labels_only") and (c is Button or c is LineEdit):
			c.visible = false
		if variant.contains("buttons_only") and c is Label:
			c.visible = false


func _all(n: Node) -> Array:
	var out: Array = [n]
	for ch in n.get_children():
		out.append_array(_all(ch))
	return out
