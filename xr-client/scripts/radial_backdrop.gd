extends Control
## A stationary visual anchor; all interaction still belongs to the menu buttons.
var item_count: int = 0
func _draw() -> void:
	var centre := size * 0.5
	if item_count <= 3:
		return
	var radius := size.x * 0.35
	draw_circle(centre, radius + 66.0, Color(0.035, 0.065, 0.11, 0.92))
	draw_arc(centre, radius + 64.0, 0, TAU, 128, Color(0.28, 0.47, 0.60, 0.7), 2, true)
	draw_arc(centre, radius - 64.0, 0, TAU, 128, Color(0.28, 0.47, 0.60, 0.4), 1, true)
	for i in 40:
		var direction := Vector2.from_angle(TAU * float(i) / 40.0)
		draw_line(centre + direction * (radius + 48.0), centre + direction * (radius + 57.0),
			Color(0.48, 0.79, 0.88, 0.55), 2, true)
	draw_circle(centre, 5, Color(0.48, 0.87, 0.94, 0.9))
