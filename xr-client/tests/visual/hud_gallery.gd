extends SceneTree
## Deterministic screenshots of actual HUD controls, without a network session.
## Run with a display and --rendering-method gl_compatibility; output: user://.
var hud: Node3D
func _initialize() -> void:
	call_deferred("capture")

func capture() -> void:
	hud = load("res://scenes/HUD.tscn").instantiate()
	root.add_child(hud)
	for tab in hud.TAB_ORDER:
		hud._show_tab(tab)
		await process_frame
		await process_frame
		await RenderingServer.frame_post_draw
		var viewport: SubViewport = hud.get_node("HudViewport")
		var error := viewport.get_texture().get_image().save_png("user://xr-hud-%s.png" % tab)
		if error != OK:
			push_error("HUD screenshot failed: %s" % error)
			quit(1)
			return
		print("XR_HUD_CAPTURE ", tab)
	var radial: Node3D = load("res://scenes/RadialMenu.tscn").instantiate()
	root.add_child(radial)
	var items := []
	for label in ["Inspect", "Pin node", "Neighbours", "Open document", "Focus", "Add to query"]:
		items.append({"label": label, "action": label})
	radial.open(items, Vector3.ZERO)
	await process_frame
	await process_frame
	await RenderingServer.frame_post_draw
	var menu_viewport: SubViewport = radial.get_node("MenuViewport")
	if menu_viewport.get_texture().get_image().save_png("user://xr-radial.png") != OK:
		quit(1)
		return
	radial.queue_free()
	hud.queue_free()
	await process_frame
	quit(0)
