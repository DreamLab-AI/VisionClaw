extends Node

## Keeps the HUD canvas inside a handful of draw calls when it re-renders.
##
## Measured on HP-Desktop (perf/hud_draw_calls.gd, Compatibility renderer): a
## HUD re-render cost 59-73 draw calls and up to 8 242 primitives. Every
## StyleBoxFlat (rounded, anti-aliased, bordered) is emitted as a triangle
## polygon, which the GLES3 canvas renderer draws on its own and which splits
## the text batch on either side of it. With the boxes stripped the Graph page
## fell to 14 calls / 510 primitives.
##
## So each boxed control (buttons, panels, line edits, separators) keeps its
## layout but draws nothing itself: its flat/line styleboxes are replaced by
## StyleBoxEmpty with the same content margins. A child `HudBg` (z_index -1,
## mouse-transparent) draws the control's current-state box as nine-patches cut
## from ONE shared atlas texture (a fill region and a border-ring region per
## corner radius / border width). z -1 puts every background before every
## piece of text in the canvas sort, so all backgrounds share one batch (same
## texture, same nine-patch command; colour is per-instance modulate) and the
## text batches on its own.
##
## Laid-out ColorRect swatches move to the same background layer (z -1).
##
## Controls added later, and styleboxes overridden later (xr_theme.apply_tab on
## a tab switch), are converted when they appear. `enabled = false` turns the
## pass off (A/B tests of layout).

static var enabled: bool = true

const XRTheme := preload("res://scripts/xr_theme.gd")

const BG_NAME := "HudBg"
const STATES := ["normal", "hover", "pressed", "hover_pressed", "disabled", "focus", "panel", "read_only", "separator"]
## Radius / border-width pairs always in the atlas (xr_theme.gd boxes).
const BASE_COMBOS := [[10, 1], [10, 2], [10, 3], [14, 1], [14, 2]]

var _vp: SubViewport = null
var _atlas: ImageTexture = null
var _regions: Dictionary = {}     # "fill:r" | "ring:r:w" | "solid" -> Rect2
var _converted: Dictionary = {}   # StyleBox (flat/line) -> Array[StyleBoxTexture]
var _unmapped: Array = []
var _count: int = 0
var _swatches: int = 0


## A sized texture that draws nothing: CheckButton keeps its layout, but emits
## no canvas command (PlaceholderTexture2D still draws, with its own texture).
class EmptyIcon extends Texture2D:
	var _size := Vector2i(1, 1)

	func _init(sz: Vector2 = Vector2.ONE) -> void:
		_size = Vector2i(maxi(int(sz.x), 1), maxi(int(sz.y), 1))

	func _get_width() -> int:
		return _size.x

	func _get_height() -> int:
		return _size.y

	func _has_alpha() -> bool:
		return true

	func _draw(_to_canvas_item: RID, _pos: Vector2, _modulate: Color, _transpose: bool) -> void:
		pass

	func _draw_rect(_to_canvas_item: RID, _rect: Rect2, _tile: bool, _modulate: Color, _transpose: bool) -> void:
		pass

	func _draw_rect_region(_to_canvas_item: RID, _rect: Rect2, _src_rect: Rect2, _modulate: Color, _transpose: bool, _clip_uv: bool) -> void:
		pass


## Background drawer for one boxed control.
class HudBg extends Control:
	var target: Control = null
	var styles: Dictionary = {}   # state -> Array[StyleBox] drawn in order
	var line_rects: Dictionary = {}  # state -> true when the box is a StyleBoxLine (separator)
	var switch_size: Vector2 = Vector2.ZERO  # CheckButton icon size, drawn here from the atlas
	var track: StyleBoxTexture = null
	var knob: StyleBoxTexture = null
	var on_colour: Color = Color.WHITE
	var off_colour: Color = Color.WHITE
	var knob_colour: Color = Color.WHITE

	func _init() -> void:
		name = "HudBg"
		mouse_filter = Control.MOUSE_FILTER_IGNORE
		focus_mode = Control.FOCUS_NONE
		z_index = -1
		set_anchors_preset(Control.PRESET_FULL_RECT)

	## The state whose box is drawn now.
	func state_name() -> String:
		if target is BaseButton:
			var b := target as BaseButton
			if b.disabled:
				return "disabled" if styles.has("disabled") else "normal"
			match b.get_draw_mode():
				BaseButton.DRAW_PRESSED:
					return "pressed" if styles.has("pressed") else "normal"
				BaseButton.DRAW_HOVER:
					return "hover" if styles.has("hover") else "normal"
				BaseButton.DRAW_HOVER_PRESSED:
					if styles.has("hover_pressed"):
						return "hover_pressed"
					return "pressed" if styles.has("pressed") else "normal"
			return "normal"
		if target is LineEdit and not (target as LineEdit).editable and styles.has("read_only"):
			return "read_only"
		for st in ["panel", "separator", "normal"]:
			if styles.has(st):
				return st
		return ""

	func has_switch() -> bool:
		return switch_size != Vector2.ZERO

	## Where CheckButton draws its icon: right-aligned inside the right content
	## margin, vertically centred (scene/gui/check_button.cpp).
	func switch_rect() -> Rect2:
		var right: float = target.get_theme_stylebox("normal").get_margin(SIDE_RIGHT)
		var y: float = (size.y - switch_size.y) * 0.5 + float(target.get_theme_constant("check_v_offset"))
		return Rect2(Vector2(size.x - right - switch_size.x, y), switch_size)

	func _draw() -> void:
		var st := state_name()
		_draw_state(st)
		if styles.has("focus") and target != null and target.has_focus():
			_draw_state("focus")
		if has_switch():
			var r := switch_rect()
			var on: bool = (target as BaseButton).button_pressed
			var dim: float = 0.45 if (target as BaseButton).disabled else 1.0
			track.modulate_color = (on_colour if on else off_colour) * Color(1, 1, 1, dim)
			draw_style_box(track, r)
			var d: float = r.size.y - 6.0
			var kx: float = r.end.x - d - 3.0 if on else r.position.x + 3.0
			knob.modulate_color = knob_colour * Color(1, 1, 1, dim)
			draw_style_box(knob, Rect2(kx, r.position.y + 3.0, d, d))

	func _draw_state(st: String) -> void:
		if not styles.has(st):
			return
		var r := Rect2(Vector2.ZERO, size)
		if line_rects.has(st):
			var t: float = float(line_rects[st])
			r = Rect2(-1.0, (size.y - t) * 0.5, size.x + 2.0, t)
		for sb: StyleBox in styles[st]:
			draw_style_box(sb, r)


## Attach to the HUD's SubViewport (hud.gd _ready). Returns the manager node.
static func attach(vp: SubViewport) -> Node:
	var m: Node = (load("res://scripts/hud_batching.gd") as GDScript).new()
	m.name = "HudBatching"
	vp.get_parent().add_child(m)
	if enabled:
		m.call("_bind", vp)
	return m


func _bind(vp: SubViewport) -> void:
	_vp = vp
	_build_atlas(_collect_combos(vp))
	_walk(vp)  # also connects child_entered_tree on every node, the viewport included


## Boxes converted so far (test seam).
func converted() -> int:
	return _count


## Swatches moved to the background layer (test seam).
func swatches() -> int:
	return _swatches


## Flat boxes whose radius/width had no atlas region (test seam; must be empty).
func unmapped() -> Array:
	return _unmapped


# --- conversion ----------------------------------------------------------------

func _walk(n: Node) -> void:
	if n is Control and not (n is HudBg):
		_convert(n as Control)
	# Laid-out colour swatches (Key page) join the background layer: a ColorRect
	# draws with the default white texture, so between two labels it split the
	# text batch twice per row.
	if n is ColorRect and n.get_parent() is Container and (n as ColorRect).z_index == 0:
		(n as ColorRect).z_index = -1
		_swatches += 1
	if not n.child_entered_tree.is_connected(_on_node_added):
		n.child_entered_tree.connect(_on_node_added)
	for c in n.get_children():
		_walk(c)


func _on_node_added(n: Node) -> void:
	if n is HudBg:
		return
	# deferred: builders set text/theme/overrides right after add_child. The
	# node may be freed before the deferred call runs (a list rebuilt twice in
	# one frame): a typed `_walk(n: Node)` then fails to convert the freed
	# object, so the call goes through a Variant check.
	_walk_if_valid.call_deferred(n)


func _walk_if_valid(n: Variant) -> void:
	if is_instance_valid(n):
		_walk(n as Node)


func _convert(c: Control) -> void:
	if not is_instance_valid(c):
		return
	var styles: Dictionary = {}
	var lines: Dictionary = {}
	for st in STATES:
		if not c.has_theme_stylebox(st):
			continue
		var sb := c.get_theme_stylebox(st)
		if not (sb is StyleBoxFlat or sb is StyleBoxLine):
			continue
		var parts: Array = _parts(sb)
		if parts.is_empty() and sb is StyleBoxFlat and not _drawable(sb as StyleBoxFlat):
			# nothing visible to draw; still strip it so the control emits no polygon
			pass
		elif parts.is_empty():
			continue  # unmapped: leave the control drawing its own box
		styles[st] = parts
		if sb is StyleBoxLine:
			lines[st] = maxf(1.0, float((sb as StyleBoxLine).thickness))
		c.add_theme_stylebox_override(st, _empty_like(sb))
		_count += 1
	var switch_size := Vector2.ZERO
	if c is CheckButton:
		var tex := c.get_theme_icon("unchecked")
		if tex != null and not (tex is EmptyIcon):
			switch_size = tex.get_size()
			# same size, draws nothing: layout unchanged, switch drawn by HudBg
			for icon in ["checked", "unchecked", "checked_disabled", "unchecked_disabled", "checked_mirrored", "unchecked_mirrored", "checked_disabled_mirrored", "unchecked_disabled_mirrored"]:
				var sz: Vector2 = c.get_theme_icon(icon).get_size() if c.has_theme_icon(icon) else switch_size
				c.add_theme_icon_override(icon, EmptyIcon.new(sz))
	if styles.is_empty() and switch_size == Vector2.ZERO:
		return
	var bg: HudBg = c.get_node_or_null(BG_NAME) as HudBg
	if bg == null:
		bg = HudBg.new()
		bg.target = c
		c.add_child(bg, false, Node.INTERNAL_MODE_FRONT)
		c.draw.connect(bg.queue_redraw)
		c.focus_entered.connect(bg.queue_redraw)
		c.focus_exited.connect(bg.queue_redraw)
		# a later override (tab switch restyle) brings a flat box back: convert again
		c.theme_changed.connect(_convert.bind(c), CONNECT_DEFERRED)
	for st in styles:
		bg.styles[st] = styles[st]
		if lines.has(st):
			bg.line_rects[st] = lines[st]
	if switch_size != Vector2.ZERO:
		bg.switch_size = switch_size
		bg.track = _patch("fill:10", 10, Color.WHITE)
		bg.knob = _patch("fill:10", 10, Color.WHITE)
		bg.on_colour = XRTheme.CYAN
		bg.off_colour = XRTheme.LINE
		bg.knob_colour = XRTheme.TEXT
		_count += 1
	bg.queue_redraw()


func _drawable(f: StyleBoxFlat) -> bool:
	return (f.draw_center and f.bg_color.a > 0.0) or (_width(f) > 0 and f.border_color.a > 0.0)


func _width(f: StyleBoxFlat) -> int:
	return maxi(maxi(f.border_width_left, f.border_width_right), maxi(f.border_width_top, f.border_width_bottom))


func _radius(f: StyleBoxFlat) -> int:
	return maxi(maxi(f.corner_radius_top_left, f.corner_radius_top_right), maxi(f.corner_radius_bottom_left, f.corner_radius_bottom_right))


## Atlas nine-patches drawing `sb`: [fill?, ring?] for a flat box, [solid] for
## a line. Empty when no region fits (recorded in _unmapped).
func _parts(sb: StyleBox) -> Array:
	if _converted.has(sb):
		return _converted[sb]
	var out: Array = []
	if sb is StyleBoxLine:
		out.append(_patch("solid", 1, (sb as StyleBoxLine).color))
	else:
		var f := sb as StyleBoxFlat
		var r := _radius(f)
		var w := _width(f)
		if f.draw_center and f.bg_color.a > 0.0:
			if not _regions.has("fill:%d" % r):
				_unmapped.append("fill r=%d" % r)
				return []
			out.append(_patch("fill:%d" % r, r, f.bg_color))
		if w > 0 and f.border_color.a > 0.0:
			if not _regions.has("ring:%d:%d" % [r, w]):
				_unmapped.append("ring r=%d w=%d" % [r, w])
				return []
			out.append(_patch("ring:%d:%d" % [r, w], r, f.border_color))
	_converted[sb] = out
	return out


func _patch(region: String, margin: int, colour: Color) -> StyleBoxTexture:
	var t := StyleBoxTexture.new()
	t.texture = _atlas
	t.region_rect = _regions[region]
	var m := float(margin)
	t.texture_margin_left = m
	t.texture_margin_right = m
	t.texture_margin_top = m
	t.texture_margin_bottom = m
	t.modulate_color = colour
	return t


func _empty_like(sb: StyleBox) -> StyleBoxEmpty:
	var e := StyleBoxEmpty.new()
	e.content_margin_left = sb.get_margin(SIDE_LEFT)
	e.content_margin_right = sb.get_margin(SIDE_RIGHT)
	e.content_margin_top = sb.get_margin(SIDE_TOP)
	e.content_margin_bottom = sb.get_margin(SIDE_BOTTOM)
	return e


# --- atlas ---------------------------------------------------------------------

func _collect_combos(n: Node, out: Dictionary = {}) -> Dictionary:
	if out.is_empty():
		for c in BASE_COMBOS:
			out["%d:%d" % [c[0], c[1]]] = c
	if n is Control:
		for st in STATES:
			if (n as Control).has_theme_stylebox(st):
				var sb := (n as Control).get_theme_stylebox(st)
				if sb is StyleBoxFlat:
					var f := sb as StyleBoxFlat
					out["%d:%d" % [_radius(f), _width(f)]] = [_radius(f), _width(f)]
	for c in n.get_children():
		_collect_combos(c, out)
	return out


## One RGBA atlas: white with anti-aliased alpha coverage. A fill region per
## radius (rounded square, 2r+2 px: corners r, a 2 px stretch centre), a ring
## region per (radius, width), and a solid 4 px square for lines.
func _build_atlas(combos: Dictionary) -> void:
	var radii := {}
	for k in combos:
		radii[int(combos[k][0])] = true
	var cells: Array = [["solid", 4, 0, 0]]
	for r in radii:
		cells.append(["fill:%d" % r, 2 * r + 2, r, 0])
	for k in combos:
		var r: int = combos[k][0]
		var w: int = combos[k][1]
		if w > 0:
			cells.append(["ring:%d:%d" % [r, w], 2 * r + 2, r, w])
	var pad := 2
	var width := 0
	var height := 0
	for cell in cells:
		width += int(cell[1]) + pad
		height = maxi(height, int(cell[1]))
	var img := Image.create(maxi(width, 4), maxi(height, 4), false, Image.FORMAT_RGBA8)
	img.fill(Color(1, 1, 1, 0))
	var x := 0
	for cell in cells:
		var sz: int = cell[1]
		var r: float = float(cell[2])
		var w: float = float(cell[3])
		for py in sz:
			for px in sz:
				var a: float
				if cell[0] == "solid":
					a = 1.0
				elif w <= 0.0:
					a = _coverage(px, py, sz, r, 0.0)
				else:
					a = clampf(_coverage(px, py, sz, r, 0.0) - _coverage(px, py, sz, maxf(r - w, 0.0), w), 0.0, 1.0)
				img.set_pixel(x + px, py, Color(1, 1, 1, a))
		_regions[cell[0]] = Rect2(x, 0, sz, sz)
		x += sz + pad
	_atlas = ImageTexture.create_from_image(img)


## Alpha coverage of pixel (px, py) for a rounded square of side `sz`, corner
## radius `r`, inset by `inset` on every side (4x4 supersampled).
static func _coverage(px: int, py: int, sz: int, r: float, inset: float) -> float:
	var lo := inset
	var hi := float(sz) - inset
	var hits := 0
	for sy in 4:
		for sx in 4:
			var x := float(px) + (float(sx) + 0.5) / 4.0
			var y := float(py) + (float(sy) + 0.5) / 4.0
			if x < lo or x > hi or y < lo or y > hi:
				continue
			var cx := clampf(x, lo + r, hi - r)
			var cy := clampf(y, lo + r, hi - r)
			if Vector2(x - cx, y - cy).length() <= r:
				hits += 1
	return float(hits) / 16.0
