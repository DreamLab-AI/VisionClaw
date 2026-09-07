extends RefCounted
## Shared, motion-free instrument palette for world-space XR controls.
## Contrast comes from opaque surfaces and focus outlines, independent of bloom.
const INK := Color("0b1424")
const SURFACE := Color("13243a")
const RAISED := Color("1d3550")
const LINE := Color("375774")
const TEXT := Color("edf5ff")
const MUTED := Color("adbed2")
const CYAN := Color("79dfef")

static func box(fill: Color, border: Color, radius: int = 10, width: int = 1) -> StyleBoxFlat:
	var s := StyleBoxFlat.new()
	s.bg_color = fill
	s.border_color = border
	s.set_border_width_all(width)
	s.set_corner_radius_all(radius)
	s.content_margin_left = 14
	s.content_margin_right = 14
	s.content_margin_top = 6
	s.content_margin_bottom = 6
	return s

static func create() -> Theme:
	var t := Theme.new()
	t.default_font_size = 28
	for kind in ["Label", "Button", "CheckButton", "LineEdit", "RichTextLabel"]:
		t.set_color("font_color", kind, TEXT)
		t.set_color("font_hover_color", kind, TEXT)
		t.set_color("font_focus_color", kind, TEXT)
		t.set_color("font_pressed_color", kind, TEXT)
		t.set_color("font_disabled_color", kind, MUTED)
	for kind in ["Button", "CheckButton"]:
		t.set_stylebox("normal", kind, box(SURFACE, LINE))
		t.set_stylebox("hover", kind, box(RAISED, CYAN, 10, 2))
		t.set_stylebox("pressed", kind, box(Color("24495d"), CYAN, 10, 2))
		t.set_stylebox("disabled", kind, box(INK, Color("27374b")))
		var focus := box(Color.TRANSPARENT, CYAN, 10, 3)
		focus.draw_center = false
		t.set_stylebox("focus", kind, focus)
	t.set_stylebox("panel", "Panel", box(INK, LINE, 14))
	t.set_stylebox("panel", "PanelContainer", box(INK, LINE, 14))
	t.set_stylebox("normal", "LineEdit", box(INK, LINE))
	t.set_stylebox("focus", "LineEdit", box(INK, CYAN, 10, 2))
	t.set_color("caret_color", "LineEdit", CYAN)
	t.set_color("font_placeholder_color", "LineEdit", MUTED)
	t.set_color("selection_color", "LineEdit", Color("305b73"))
	t.set_color("default_color", "RichTextLabel", TEXT)
	var separator := StyleBoxLine.new()
	separator.color = LINE
	separator.thickness = 1
	t.set_stylebox("separator", "HSeparator", separator)
	return t

static func apply_tab(button: Button, selected: bool) -> void:
	button.add_theme_stylebox_override("normal", box(
		Color("24495d") if selected else INK, CYAN if selected else LINE, 10, 2 if selected else 1))
	button.add_theme_color_override("font_color", TEXT if selected else MUTED)
