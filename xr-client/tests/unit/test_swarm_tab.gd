extends "res://addons/gut/test.gd"

# Swarm tab roster (ADR-140, Pillar 4 / P5). The HUD Swarm page lists live agents —
# a status dot, name → target-node label, and the current task line — each row tap
# emitting control_pressed("teleport:<agent_id>") to reuse GraphScene's node-teleport
# glide (agent wire ids ARE node ids). set_swarm_roster() is the push API GraphScene
# calls; these drive it directly. No godot binary in-container → runs in CI / device.

const TABS := "HudViewport/HudControl/Root/Tabs"


func _make_hud() -> Node3D:
	var packed: PackedScene = load("res://scenes/HUD.tscn")
	var hud: Node3D = packed.instantiate()
	add_child(hud)
	await get_tree().process_frame
	await get_tree().process_frame
	return hud


func test_swarm_page_exists_and_starts_empty() -> void:
	var hud: Node3D = await _make_hud()
	assert_not_null(hud.get_node_or_null("%s/SwarmPage" % TABS), "SwarmPage built")
	assert_true(hud.has_method("set_swarm_roster"), "roster push API present")
	hud.set_swarm_roster([])
	await get_tree().process_frame
	# Empty roster shows the placeholder, no rows.
	assert_eq(_count_row_buttons(hud), 0, "no agent rows when the roster is empty")
	hud.queue_free()
	await get_tree().process_frame


func test_roster_rows_render_and_teleport_on_tap() -> void:
	var hud: Node3D = await _make_hud()
	watch_signals(hud)
	hud.set_swarm_roster([
		{"id": 7, "name": "Planner", "status": 1, "target": "Auth Module", "task": "reading spec"},
		{"id": 9, "name": "", "status": 2, "target": "", "task": ""},
	])
	await get_tree().process_frame
	assert_eq(_count_row_buttons(hud), 2, "one teleport button per agent")
	# The first row's button carries the name → target and teleports to the wire id.
	var btn := _find_button_containing(hud.get_node("%s/SwarmPage" % TABS), "Planner")
	assert_not_null(btn, "named agent row present")
	assert_true(btn.text.contains("Auth Module"), "row shows name → target label")
	btn.pressed.emit()
	assert_signal_emitted_with_parameters(hud, "control_pressed", ["teleport:7"])
	# The nameless agent falls back to "agent <id>".
	assert_not_null(_find_button_containing(hud.get_node("%s/SwarmPage" % TABS), "agent 9"),
		"nameless agent falls back to 'agent <id>'")
	hud.queue_free()
	await get_tree().process_frame


func _count_row_buttons(hud: Node3D) -> int:
	var n := [0]
	_walk_buttons(hud._swarm_list, n)
	return n[0]


func _walk_buttons(root: Node, n: Array) -> void:
	for c in root.get_children():
		if c is Button:
			n[0] += 1
		_walk_buttons(c, n)


func _find_button_containing(root: Node, text: String) -> Button:
	for c in root.get_children():
		if c is Button and (c as Button).text.contains(text):
			return c
		var found := _find_button_containing(c, text)
		if found != null:
			return found
	return null
