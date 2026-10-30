extends RefCounted
## Used via `const FramePicker := preload("res://scripts/frame_picker.gd")`
## rather than a class_name, so a headless run with a stale global class cache
## still resolves it (see backend_caps.gd).

## Ray against the volume boxes (spatial-volume UX, features 1 and 4).
##
## Each open, visible, bounded volume is an oriented box of `bounds` metres
## around its origin. A ray that enters a box hits its "frame"; a ray that
## passes within `handle_radius_m` of a corner hits that corner's resize
## handle, as `SpatialVolume.handle_at` decides (so its closed, hidden,
## non-interactable and immersive gating applies). Within one volume a handle
## beats a face; across volumes the nearest hit along the ray wins.
##
## A ray that starts inside a box does not hit that box's frame (the user is
## inside it, pointing at its content), and an immersive volume has no box.
## `distance` is metres along the ray, as in volume_picker and link_picker.

const MAX_DISTANCE_M := 30.0


## Returns {} on a miss, else
## {volume: StringName, part: "handle"|"frame", handle: int (-1 for frame), distance: float, world_pos: Vector3}.
static func pick(origin: Vector3, dir: Vector3, volumes: Dictionary, handle_radius_m: float = 0.04) -> Dictionary:
	if dir.length_squared() < 1.0e-12:
		return {}
	var d := dir.normalized()
	var best := {}
	var best_d := INF
	for vid: Variant in volumes:
		var v: Object = volumes[vid]
		var hit := pick_volume(origin, d, v, handle_radius_m)
		if hit.is_empty():
			continue
		if float(hit["distance"]) < best_d:
			best_d = float(hit["distance"])
			hit["volume"] = StringName(String(vid))
			best = hit
	return best


## One volume: its nearest handle hit, else its face hit, else {}. `d` must be
## unit length. The result has no `volume` key; `pick` adds it.
static func pick_volume(origin: Vector3, d: Vector3, v: Object, handle_radius_m: float) -> Dictionary:
	if v == null or not v.has_method("handle_at"):
		return {}
	if v.get("is_closed") == true or v.get("visible_state") == false:
		return {}
	if int(v.get("bounds_mode")) != 0:  # SpatialVolume.BoundsMode.BOUNDED
		return {}
	var xf := _world(v)
	var bounds: Vector3 = v.get("bounds")
	var half := bounds * 0.5
	var handle := _handle_hit(origin, d, v, xf, half, handle_radius_m)
	if not handle.is_empty():
		return handle
	var t := ray_box(origin, d, xf, half)
	if t < 0.0 or t > MAX_DISTANCE_M:
		return {}
	return {"part": "frame", "handle": -1, "distance": t, "world_pos": origin + d * t}


## Entry distance (metres along unit `d`) of the ray into the box of half
## extents `half` under `xf`, or -1 on a miss or when `origin` is inside.
static func ray_box(origin: Vector3, d: Vector3, xf: Transform3D, half: Vector3) -> float:
	var inv := xf.affine_inverse()
	var o: Vector3 = inv * origin
	var ld: Vector3 = inv.basis * d  # not normalised: local t equals world t
	var t0 := -INF
	var t1 := INF
	for k: int in 3:
		if absf(ld[k]) < 1.0e-12:
			if absf(o[k]) > half[k]:
				return -1.0
			continue
		var a := (-half[k] - o[k]) / ld[k]
		var b := (half[k] - o[k]) / ld[k]
		t0 = maxf(t0, minf(a, b))
		t1 = minf(t1, maxf(a, b))
	if t1 < t0 or t1 < 0.0 or t0 < 0.0:
		return -1.0
	return t0


## Nearest corner whose closest approach to the ray lies within the handle
## radius, as judged by the volume's own `handle_at`.
static func _handle_hit(origin: Vector3, d: Vector3, v: Object, xf: Transform3D, half: Vector3, radius_m: float) -> Dictionary:
	# handle_at measures in the volume's local frame: convert the radius so a
	# scaled arrangement keeps the handle radius in metres.
	var s := maxf(xf.basis.get_scale().x, 1.0e-9)
	var best := {}
	var best_t := INF
	for i: int in 8:
		var sgn := Vector3(1.0 if i & 1 else -1.0, 1.0 if i & 2 else -1.0, 1.0 if i & 4 else -1.0)
		var corner: Vector3 = xf * (sgn * half)
		var t := (corner - origin).dot(d)
		if t < 0.0 or t > MAX_DISTANCE_M:
			continue
		var q := origin + d * t
		if int(v.handle_at(q, radius_m / s)) != i:
			continue
		if t < best_t:
			best_t = t
			best = {"part": "handle", "handle": i, "distance": t, "world_pos": corner}
	return best


static func _world(n: Object) -> Transform3D:
	var node := n as Node3D
	if node == null:
		return Transform3D.IDENTITY
	return node.global_transform if node.is_inside_tree() else node.transform
