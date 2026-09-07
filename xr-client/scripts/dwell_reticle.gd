extends Control

## Gaze-dwell charging reticle (PRD-023 WP-9 M2, copresence brief §4). A filled
## radial that charges over the dwell window so a gaze selection is confirmable
## and cancellable — the Midas-touch mitigation. The charge value (0..1) is the
## Rust dwell resolver's `charge_ratio()`; this Control only draws it.

const TRACK_COLOR: Color = Color(1, 1, 1, 0.18)
const FILL_COLOR: Color = Color(0.35, 0.85, 1.0, 0.9)
const RADIUS: float = 22.0
const THICKNESS: float = 5.0

var _charge: float = 0.0


func set_charge(ratio: float) -> void:
	var clamped: float = clampf(ratio, 0.0, 1.0)
	if is_equal_approx(clamped, _charge):
		return
	_charge = clamped
	visible = _charge > 0.0
	queue_redraw()


func _draw() -> void:
	if _charge <= 0.0:
		return
	var centre := size * 0.5
	# An opaque backing keeps charge legible over bright graph nodes.
	draw_circle(centre, RADIUS + 6.0, Color(0.025, 0.05, 0.09, 0.94))
	draw_circle(centre, 2.5, FILL_COLOR)
	# Full track, then the charged arc from 12 o'clock clockwise.
	draw_arc(centre, RADIUS, 0.0, TAU, 48, TRACK_COLOR, THICKNESS, true)
	var start := -PI * 0.5
	var end := start + TAU * _charge
	draw_arc(centre, RADIUS, start, end, 48, FILL_COLOR, THICKNESS, true)
