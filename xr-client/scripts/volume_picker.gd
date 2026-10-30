extends RefCounted
## Used via `const VolumePicker := preload("res://scripts/volume_picker.gd")`
## rather than a class_name, so a headless run with a stale global class cache
## still resolves it (see backend_caps.gd).

## Node picking across every spatial volume (spatial-volume UX, feature 2).
##
## The single-graph pick (`graph_scene._pick_node_under_ray`) reads the
## connection client's last pack and maps it through `graph_root`. With volumes,
## one client packs once per volume into that volume's own render-store slot
## (ADR-2159 D4), so the client's "last pack" is whichever volume packed last.
## This picker reads each volume's own slot with
## `get_render_ids_for_slot(source.slot)` / `get_render_positions_for_slot`,
## maps graph units to world through that volume's drawn Content transform
## (`SpatialVolume.world_from_content`), and keeps the nearest node inside a
## narrow cone around the ray.
##
## Hidden, closed or non-interactable volumes, volumes with no source, and
## volumes whose connection is offline are skipped: a hidden volume does not
## repack, so its slot would be stale.
##
## `distance` is metres along the ray (the same measure the link and frame
## pickers return), so callers can compare hits from all three.

## Farthest pickable node along the ray, metres (interaction.rs MAX_RAY_DISTANCE_M).
const MAX_DISTANCE_M := 30.0
## Every candidate within this many metres of the origin is ignored (the hand itself).
const MIN_DISTANCE_M := 1.0e-3


## Nearest node under the ray across visible, online volumes.
## `volumes`: {StringName id → SpatialVolume}; `sources`: {id → ConnectionVolumeSource}.
## A node counts when the angle between the ray and the eye→node direction is
## at most `max_angle_deg`; among those the nearest along the ray wins (ties:
## the smaller angle). Returns {} on a miss, else
## {volume: StringName, conn: StringName, node_id: int, world_pos: Vector3, distance: float}.
static func pick(origin: Vector3, dir: Vector3, volumes: Dictionary, sources: Dictionary, max_angle_deg: float = 1.5) -> Dictionary:
	if dir.length_squared() < 1.0e-12:
		return {}
	var d := dir.normalized()
	var cos_max := cos(deg_to_rad(clampf(max_angle_deg, 0.0, 89.0)))
	var best := {}
	var best_along := INF
	var best_cos := -INF
	for vid: Variant in volumes:
		var v: Object = volumes[vid]
		var src: Object = sources.get(vid)
		if not pickable(v, src):
			continue
		var cand := candidates(src)
		var ids: PackedInt32Array = cand[0]
		var pos: PackedVector3Array = cand[1]
		var n := mini(ids.size(), pos.size())
		for i: int in n:
			var w: Vector3 = v.world_from_content(pos[i])
			var to := w - origin
			var along := to.dot(d)
			if along < MIN_DISTANCE_M or along > MAX_DISTANCE_M:
				continue
			var c := along / to.length()
			if c < cos_max:
				continue
			if along < best_along - 1.0e-6 or (absf(along - best_along) <= 1.0e-6 and c > best_cos):
				best_along = along
				best_cos = c
				best = {
					"volume": StringName(String(vid)),
					"conn": StringName(String(src.conn.conn_id)),
					"node_id": ids[i],
					"world_pos": w,
					"distance": along,
				}
	return best


## Whether volume `v` with source `src` takes part in picking: open, visible,
## interactable, with a source whose connection is present and online.
static func pickable(v: Object, src: Object) -> bool:
	if v == null or src == null or not v.has_method("world_from_content"):
		return false
	if v.get("is_closed") == true or v.get("visible_state") == false or v.get("interactable") == false:
		return false
	var c: Variant = src.get("conn")
	if c == null or not (c is Object):
		return false
	var conn := c as Object
	if conn.has_method("is_connected_now") and not conn.is_connected_now():
		return false
	return true


## [ids, graph-unit positions] drawn by `src`'s volume in its last pack. Reads
## the client's per-slot accessors; a client without them (an older gdext
## build, or a test fake) falls back to the source's content ids and positions.
static func candidates(src: Object) -> Array:
	var conn: Object = src.get("conn")
	var client: Variant = conn.get("client") if conn != null else null
	if client is Object and (client as Object).has_method("get_render_ids_for_slot") \
			and (client as Object).has_method("get_render_positions_for_slot"):
		var slot := int(src.get("slot"))
		var cl := client as Object
		return [cl.get_render_ids_for_slot(slot), cl.get_render_positions_for_slot(slot)]
	var ids: PackedInt32Array = src.content_ids() if src.has_method("content_ids") else PackedInt32Array()
	var pos := PackedVector3Array()
	pos.resize(ids.size())
	if src.has_method("position_of"):
		for i: int in ids.size():
			pos[i] = src.position_of(ids[i])
	return [ids, pos]
