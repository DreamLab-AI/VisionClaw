extends "res://addons/gut/test.gd"

# HUD SubViewport renders only when one of its controls redraws
# (scripts/hud_render_on_demand.gd). UPDATE_ALWAYS re-rendered the whole 2D
# canvas every frame: 59 draw calls and ~8.2k triangles measured on HP for a
# panel that changes a few times a second at most.

const OnDemand := preload("res://scripts/hud_render_on_demand.gd")


func _hud() -> Node3D:
	var hud: Node3D = (load("res://scenes/HUD.tscn") as PackedScene).instantiate()
	add_child(hud)
	await get_tree().process_frame
	await get_tree().process_frame
	return hud


func test_hud_viewport_is_not_rendered_every_frame() -> void:
	var hud := await _hud()
	var vp: SubViewport = hud.get_node("HudViewport")
	assert_ne(vp.render_target_update_mode, SubViewport.UPDATE_ALWAYS, "not every frame")
	var od: Node = hud.get_node("HudRenderOnDemand")
	assert_not_null(od, "attached by hud.gd")
	# settle: no control changes, no render requests
	for i in 3:
		await get_tree().process_frame
	var before: int = od.requests()
	for i in 5:
		await get_tree().process_frame
	assert_eq(od.requests(), before, "idle HUD asks for no renders")
	hud.queue_free()


func test_a_control_change_requests_exactly_one_render() -> void:
	var hud := await _hud()
	var vp: SubViewport = hud.get_node("HudViewport")
	var od: Node = hud.get_node("HudRenderOnDemand")
	for i in 3:
		await get_tree().process_frame
	var before: int = od.requests()
	var label := Label.new()
	label.text = "status"
	vp.get_node("HudControl").add_child(label)  # a control added later is tracked too
	await get_tree().process_frame
	assert_gt(od.requests(), before, "new control drew: render requested")
	var mid: int = od.requests()
	label.text = "status changed"
	await get_tree().process_frame
	assert_gt(od.requests(), mid, "text change drew: render requested")
	assert_eq(vp.render_target_update_mode, SubViewport.UPDATE_ONCE, "one render, then it stops")
	hud.queue_free()


func test_request_render_is_public_for_texture_only_changes() -> void:
	var hud := await _hud()
	var od: Node = hud.get_node("HudRenderOnDemand")
	var before: int = od.requests()
	od.request_render()
	assert_eq(od.requests(), before + 1)
	assert_eq((hud.get_node("HudViewport") as SubViewport).render_target_update_mode, SubViewport.UPDATE_ONCE)
	hud.queue_free()
