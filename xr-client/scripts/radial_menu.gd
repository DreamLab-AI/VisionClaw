class_name RadialMenu
extends Node3D

## Reusable wand-operated radial menu for the Godot 4 XR client.
##
## Structure mirrors HUD.tscn: a world-space QuadMesh ("MenuPanel") whose
## material_override samples a ViewportTexture of a SubViewport ("MenuViewport").
## The SubViewport hosts a Control tree; Button nodes are laid out on a circle at
## runtime. The integrator feeds a wand ray via `pointer_input()` exactly the way
## graph_scene.gd::_update_hud_pointer feeds the HUD SubViewport.
##
## Overflow (Graph2VR CircleMenu port): when more than VISIBLE_LIMIT items are
## present, an HSlider rotates the whole ring through a 180 degree window; items
## whose angular position falls outside the window collapse to Vector2.ZERO.

## Emitted when a button is chosen. `action` is the item dict's "action" value.
signal item_selected(action: String)

## Items beyond this count trigger the rotating overflow window.
const VISIBLE_LIMIT: int = 10

# Cached nodes.
@onready var _viewport: SubViewport = $MenuViewport
@onready var _items_root: Control = $MenuViewport/MenuControl/Items
@onready var _slider: HSlider = $MenuViewport/MenuControl/OverflowSlider

# Items at or below this count are laid out AROUND THE CENTRE (a small cluster the
# natural centre-aim can hit) instead of on the outer ring; a lone item lands dead
# centre. Above it, the full ring + overflow window applies.
const CENTRE_CLUSTER_MAX: int = 3

# Runtime state.
var _items: Array = []               # Array[Dictionary]: label, action, count?
var _buttons: Array[Button] = []
var _rotation_offset: float = 0.0    # radians added to every item's base angle
var _click_was_down: bool = false    # edge-detect for pointer_input clicks
var _last_pointer_pos: Vector2 = Vector2.ZERO  # last viewport-space sample, for a clean release on close()
var _backdrop: Control
var _debug: bool = false             # set_debug() — mirrors graph_scene QB_DEBUG


## Enable/disable the radial→mark chain instrumentation (graph_scene sets this from
## its QB_DEBUG flag at wire-up). Silenced by default.
func set_debug(on: bool) -> void:
	_debug = on


func _ready() -> void:
	($MenuPanel.material_override as StandardMaterial3D).albedo_texture = _viewport.get_texture()
	$MenuViewport/MenuControl.theme = preload("res://scripts/xr_theme.gd").create()
	_backdrop = Control.new()
	_backdrop.set_script(preload("res://scripts/radial_backdrop.gd"))
	_backdrop.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	_backdrop.mouse_filter = Control.MOUSE_FILTER_IGNORE
	$MenuViewport/MenuControl.add_child(_backdrop)
	$MenuViewport/MenuControl.move_child(_backdrop, 0)
	visible = false
	if _slider != null and not _slider.value_changed.is_connected(_on_slider_changed):
		_slider.value_changed.connect(_on_slider_changed)


## Open the menu: position it at `world_pos`, build buttons for `items`
## (Array[Dictionary] with keys `label:String`, `action:String`, optional
## `count:int`), lay them out in a circle, and show the panel.
func open(items: Array, world_pos: Vector3) -> void:
	_items = items
	if _backdrop != null:
		_backdrop.item_count = items.size()
		_backdrop.queue_redraw()
	global_position = world_pos
	_rotation_offset = 0.0
	_click_was_down = false
	_build_buttons()
	var overflow := _items.size() > VISIBLE_LIMIT
	if _slider != null:
		_slider.visible = overflow
		_slider.set_block_signals(true)
		_slider.value = 0.0
		_slider.set_block_signals(false)
	_relayout()
	visible = true


## Hide the menu. Safe to call when already closed. If a synthetic press is still
## held (e.g. close() fires mid-click, or from the selected button's handler),
## push a release first so the SubViewport doesn't latch that button pressed into
## the next open().
func close() -> void:
	if _click_was_down and _viewport != null:
		_push_mouse(_last_pointer_pos, false)
		_click_was_down = false
	visible = false


## Feed a wand-ray pointer sample into the SubViewport.
##
## `px` is a NORMALISED coordinate in [0,1] x [0,1] (u right, v down) taken from
## the integrator's existing ray->quad UV hit; it is multiplied by the viewport
## size here. `click` is the current trigger-down state; a press+release pair is
## synthesised on the rising edge (was-up -> now-down). Always emits motion.
func pointer_input(px: Vector2, click: bool) -> void:
	if _viewport == null or not visible:
		return
	var pos := Vector2(px.x * float(_viewport.size.x), px.y * float(_viewport.size.y))
	_last_pointer_pos = pos
	var motion := InputEventMouseMotion.new()
	motion.position = pos
	motion.global_position = pos
	_viewport.push_input(motion)
	if click and not _click_was_down:
		_push_mouse(pos, true)
	elif not click and _click_was_down:
		_push_mouse(pos, false)
	_click_was_down = click


func _push_mouse(pos: Vector2, pressed: bool) -> void:
	var ev := InputEventMouseButton.new()
	ev.button_index = MOUSE_BUTTON_LEFT
	ev.pressed = pressed
	ev.position = pos
	ev.global_position = pos
	_viewport.push_input(ev)


func _build_buttons() -> void:
	for b in _buttons:
		b.queue_free()
	_buttons.clear()
	for i in _items.size():
		var item: Dictionary = _items[i]
		var btn := Button.new()
		var label: String = str(item.get("label", ""))
		if item.has("count"):
			label += " (%d)" % int(item["count"])
		btn.text = label
		btn.tooltip_text = label
		btn.text_overrun_behavior = TextServer.OVERRUN_TRIM_ELLIPSIS
		btn.add_theme_font_size_override("font_size", 24)
		btn.custom_minimum_size = Vector2(200, 72)
		# Pivot at centre so scale-to-zero collapses symmetrically.
		btn.pivot_offset = Vector2(100, 36)
		var action: String = str(item.get("action", ""))
		btn.pressed.connect(_on_button_pressed.bind(action))
		_items_root.add_child(btn)
		_buttons.append(btn)


## Recompute every button's position and scale from the current rotation offset.
## Items within the visible 180 degree window render at full scale; others
## collapse to Vector2.ZERO (hidden but still parented).
func _relayout() -> void:
	if _buttons.is_empty():
		return
	var vp_size := Vector2(_viewport.size)
	var centre := vp_size * 0.5
	var count := _buttons.size()
	# Small menus cluster near the centre with widened hit targets so the natural
	# centre-aim lands on an item — a lone item sits dead centre. (Fixes the "single
	# item parked at 3 o'clock, centre-aim misses it" ergonomics.)
	if count <= CENTRE_CLUSTER_MAX:
		var big := Vector2(300, 90)
		if count == 1:
			var only := _buttons[0]
			only.custom_minimum_size = big
			only.size = big
			only.pivot_offset = big * 0.5
			only.position = centre - big * 0.5
			only.scale = Vector2.ONE
			return
		var cluster_radius := vp_size.x * 0.20
		for i in count:
			var b := _buttons[i]
			b.custom_minimum_size = big
			b.size = big
			b.pivot_offset = big * 0.5
			# Start at the top (-PI/2) so 2 items read top/bottom, 3 read triangular.
			var a := -PI * 0.5 + TAU * float(i) / float(count)
			var d := Vector2(cos(a), sin(a))
			b.position = centre + d * cluster_radius - big * 0.5
			b.scale = Vector2.ONE
		return
	var radius := vp_size.x * 0.35
	var overflow := count > VISIBLE_LIMIT
	# The visible window is FIXED at [-PI/2, PI/2]; the slider rotates the item ring
	# THROUGH it (only the item angle carries _rotation_offset). If the offset were
	# also added to window_start the two terms would cancel and the slider would
	# reveal nothing — the bug this fixes.
	var window_start := -PI * 0.5
	for i in count:
		var btn := _buttons[i]
		var base_angle := TAU * float(i) / float(count)
		var angle := base_angle + _rotation_offset
		var dir := Vector2(cos(angle), sin(angle))
		var half := btn.custom_minimum_size * 0.5
		btn.position = centre + dir * radius - half
		var shown := true
		if overflow:
			shown = _angle_in_window(angle, window_start)
		btn.scale = Vector2.ONE if shown else Vector2.ZERO


# True when `angle` falls inside the 180 degree window starting at `start`.
func _angle_in_window(angle: float, start: float) -> bool:
	var d := fposmod(angle - start, TAU)
	return d <= PI


func _on_slider_changed(value: float) -> void:
	# Slider 0..1 maps to a full 360 degree rotation of the ring.
	_rotation_offset = value * TAU
	_relayout()


func _on_button_pressed(action: String) -> void:
	# Instrumentation (mirrors graph_scene QB_DEBUG): confirms a wand click actually
	# reached a menu Button and is emitting its action.
	if _debug:
		print("[QB] RadialMenu button pressed action='%s'" % action)
	item_selected.emit(action)
	close()
