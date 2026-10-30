extends RefCounted
## Used via `const LinkPicker := preload("res://scripts/link_picker.gd")`
## rather than a class_name, so a headless run with a stale global class cache
## still resolves it (see backend_caps.gd).

## Ray against the drawn cross-volume links (spatial-volume UX, feature 3).
##
## Reads `layer.drawn_segments()` (world space, as of the layer's last
## refresh) and takes the closest approach of the ray to each segment. A
## segment counts when that gap is at most `max_dist_m`; among those the
## nearest along the ray wins (ties: the smaller gap).
##
## `t` is the segment parameter of the closest point, 0 at `from` (the
## record's source end) and 1 at `to`. The far end, for "follow a link", is
## `to` when t < 0.5 and `from` otherwise (see `far_end`). `distance` is
## metres along the ray, as in volume_picker and frame_picker.

const MAX_DISTANCE_M := 30.0


## Returns {} on a miss, else
## {record: Dictionary, from: {volume, conn, node_id}, to: {…}, t: float, distance: float, world_pos: Vector3}.
static func pick(origin: Vector3, dir: Vector3, layer: Node, max_dist_m: float = 0.02) -> Dictionary:
	if layer == null or not layer.has_method("drawn_segments") or dir.length_squared() < 1.0e-12:
		return {}
	var d := dir.normalized()
	var best := {}
	var best_along := INF
	var best_gap := INF
	for seg: Dictionary in layer.drawn_segments():
		var hit := closest(origin, d, seg["a"], seg["b"])
		var along: float = hit[0]
		var t: float = hit[1]
		var gap: float = hit[2]
		if gap > max_dist_m or along < 0.0 or along > MAX_DISTANCE_M:
			continue
		if along < best_along - 1.0e-6 or (absf(along - best_along) <= 1.0e-6 and gap < best_gap):
			best_along = along
			best_gap = gap
			var a: Vector3 = seg["a"]
			var b: Vector3 = seg["b"]
			best = {
				"record": seg.get("record", {}),
				"from": seg.get("from", {}),
				"to": seg.get("to", {}),
				"t": t,
				"distance": along,
				"world_pos": a.lerp(b, t),
			}
	return best


## The end a "follow" should travel to: the one farther from the hit point.
static func far_end(hit: Dictionary) -> Dictionary:
	if hit.is_empty():
		return {}
	return hit["to"] if float(hit["t"]) < 0.5 else hit["from"]


## Closest approach of the ray (`origin`, unit `d`, s ≥ 0) to segment a→b:
## [s along the ray, t on the segment in 0..1, gap in metres].
static func closest(origin: Vector3, d: Vector3, a: Vector3, b: Vector3) -> Array:
	var u := b - a
	var w := origin - a
	var uu := u.dot(u)
	var du := d.dot(u)
	var dw := d.dot(w)
	var uw := u.dot(w)
	var t := 0.0
	if uu > 1.0e-12:
		var den := uu - du * du  # |d| = 1
		if den > 1.0e-12:
			t = clampf((uw - du * dw) / den, 0.0, 1.0)
		else:
			t = clampf(uw / uu, 0.0, 1.0)  # parallel: any t; take the one nearest the origin
	var s := maxf((a + u * t - origin).dot(d), 0.0)
	# Re-project t for the clamped s, then s for that t (one round settles a segment).
	if uu > 1.0e-12:
		t = clampf((origin + d * s - a).dot(u) / uu, 0.0, 1.0)
		s = maxf((a + u * t - origin).dot(d), 0.0)
	var gap := (origin + d * s).distance_to(a + u * t)
	return [s, t, gap]
