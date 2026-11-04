extends RefCounted

## The live scene's FrameBudget pass (PRD-008: 100k triangles, 50 draw calls a
## frame). xr-cloud's `FrameBudget.allocate` (rust frame_budget.rs) divides the
## frame between the graph's near tiers, the hulls, the memory cloud and the
## query route; this script only gathers the demand it needs from what the scene
## actually drew and keeps the resulting caps for GraphScene to apply:
##   caps["gem_nodes"]      → build_node_buffer_lod near cap
##   caps["cylinder_edges"] → build_edge_buffer_lod near cap
##   caps["max_hulls"]      → GraphParity hull rebuild
##   cloud/route caps       → memory layer apply_frame_caps()
## Triangles outside the budgeted layers (HUD, avatars, controllers, labels) are
## measured, not guessed: the renderer's frame total minus the graph layers
## (`graph_layer_triangles`) and the memory layers (`budget()`), one frame late,
## sampled every frame and held at its peak over OTHER_PEAK_SEC. The HUD canvas
## re-renders about once a second (~8k triangles on that frame); the budget
## reserves that dirty-frame cost rather than the idle one, and the peak-hold
## keeps the caps from oscillating between samples.
##
## Per volume (ADR-2159 D4): the budget stays one budget. The scene passes the
## layer counts summed over every visible volume, `allocate` runs once on that
## sum, and the resulting near caps are shared out by `FrameBudget.split_near_caps`
## in proportion to each visible volume's drawn nodes / edges, with a GEM_MIN
## floor so no visible volume loses its near field. A hidden volume demands 0,
## gets 0 and does not pack. Read the shares from `volume_caps[volume_id]`.
## `set_volume_weights` (portals: called on entering and leaving a space) turns
## that proportional share into drawn x weight; with no weights, or weights all
## equal, the split is exactly the unweighted one.
##
## Visual extras (eye_candy.gd): the controller states what it wants with
## `set_extras_demand(tier, frames)` and what it actually drew with
## `set_extras_applied(tris, draw_calls)`. `allocate_with_extras` holds back
## the requested tier's cost (an allowance, `caps["extras_allowance"]`) ahead of
## the graph's gem and cylinder share, so the graph loses at most
## ceil(allowance / 288) gems and the cloud, route and burst caps do not move; with
## the minimums overrun or a memory layer short the allowance is 0 and the extras
## are the last claim. The granted tier is `caps["extras_tier"]`. What the extras drew is part of the renderer's
## measured primitives, so it is taken out of `other_tris` here, or the pass
## would charge it twice and revoke it two seconds later.

## Splat account (ADR-2170 decision 7, accepted by the owner). `splat_volume.gd` states
## what it wants with `set_splat_demand(tier, volumes, entered)` and what it drew with
## `set_splat_applied(tris, draw_calls)`. `allocate_with_splats` reserves the splat draw
## calls (one a visible volume, at most two) before the extras allowance, and reports the
## splat triangles in `caps["splat_tris"]` without charging them to the graph. The
## measured primitives include every splat quad, so `tick` takes the applied splat
## triangles out of `other_tris` exactly as it does the extras': otherwise a 100k-splat
## room (200k triangles) would read as "everything outside the budgeted layers" and the
## graph's caps would collapse. The splat volume's own draw calls are NOT in
## `layers.draw_calls` (the allocator adds them); only its frame is, via
## `overhead_draw_calls`.

const NodeLod := preload("res://scripts/node_lod.gd")
## Low rate: demand changes with the drawn set, not per frame (caps only move
## the tier split, never invalidate the pack plans).
const BUDGET_SEC: float = 0.25
const DEFAULT_MAX_HULLS: int = 32
const OTHER_PEAK_SEC: float = 2.0
## Highest extras tier; mirrors rust/src/frame_budget.rs EXTRAS_MAX_TIER (checked in test_frame_budget_pass.gd).
const EXTRAS_MAX_TIER: int = 4
## Highest splat tier; mirrors rust/src/frame_budget.rs SPLAT_MAX_TIER (checked in test_splat_volume.gd).
const SPLAT_MAX_TIER: int = 3
## Burst pool slots reserved while memory_flash bursts are on and the cloud is
## hidden (memory_bursts.gd POOL_SIZE; with the cloud shown bursts restyle rows).
const BURST_POOL: int = 64

var caps: Dictionary = {
	"gem_nodes": NodeLod.NEAR_CAP,
	"cylinder_edges": NodeLod.NEAR_EDGE_CAP,
	"max_hulls": DEFAULT_MAX_HULLS,
	"burst_slots": 0,
	"extras_tier": 0,
	"extras_tris": 0,
	"extras_draw_calls": 0,
	"extras_allowance": 0,
	"splat_volumes": 0,
	"splat_tier": 0,
	"splat_portal_tier": 0,
	"splat_count": 0,
	"splat_tris": 0,
	"splat_draw_calls": 0,
	"over_budget": false,
}
var last_report: Dictionary = {}
## volume_id → {"gem_nodes": int, "cylinder_edges": int}, from the last pass.
var volume_caps: Dictionary = {}
## volume_id → float ≥ 0 (see `set_volume_weights`); empty = every volume 1.0.
var volume_weights: Dictionary = {}
## What the visual extras want: {"tier": 0..4, "frames": visible volume frames}.
var extras_demand: Dictionary = {"tier": 0, "frames": 0}
## What the splat volumes want: {"tier": 0..3, "volumes": visible loaded splat volumes,
## "entered": a splat volume is the entered room}.
var splat_demand: Dictionary = {"tier": 0, "volumes": 0, "entered": false}

var _accum: float = BUDGET_SEC
var _fb: Object = null
var _bucket_max: int = 0                 # peak other_tris since the last pass
var _peaks: Array[int] = []              # per-pass peaks over OTHER_PEAK_SEC
var _last_volumes: Dictionary = {}       # the volumes of the last tick, for an immediate re-split
var _extras_tris: int = 0                # triangles the extras drew last frame (reported by the controller)
var _splat_tris: int = 0                 # triangles the splat volumes drew last frame (reported by them)


## Call every frame. `layers`: instance counts drawn last frame — gems, faded,
## halos, impostors, cylinders, ribbons, hulls, hull_tris, draw_calls. `memory`:
## the memory-cloud layer (frame_demand/budget/apply_frame_caps) or null.
## `measured_tris`: the renderer's total primitives last frame (0 = unknown).
## `bursts_on`: memory_flash burst rings enabled. `volumes`: volume_id →
## {"nodes", "edges"} drawn by that volume last frame (0/0 or absent when
## hidden); empty = the single-graph scene. Returns true when a pass ran.
func tick(delta: float, client: RefCounted, layers: Dictionary, memory: Object, measured_tris: int, bursts_on: bool = false, volumes: Dictionary = {}) -> bool:
	if client == null or not client.has_method("graph_layer_triangles"):
		return false
	var gems: int = int(layers.get("gems", 0))
	var faded: int = int(layers.get("faded", 0))
	var impostors: int = int(layers.get("impostors", 0))
	var cylinders: int = int(layers.get("cylinders", 0))
	var ribbons: int = int(layers.get("ribbons", 0))
	var hull_tris: int = int(layers.get("hull_tris", 0))
	var graph_tris: int = int(client.graph_layer_triangles(gems, faded, int(layers.get("halos", 0)), impostors,
		cylinders, ribbons, hull_tris))
	var memory_tris: int = 0
	if memory != null and memory.has_method("budget"):
		var b: Dictionary = memory.budget()
		memory_tris = int(b.get("cloud_triangles", 0)) + int(b.get("route_triangles", 0))
	if measured_tris > 0:
		_bucket_max = maxi(_bucket_max, maxi(0, measured_tris - graph_tris - memory_tris - _extras_tris - _splat_tris))
	_accum += delta
	if _accum < BUDGET_SEC:
		return false
	_accum = 0.0
	_peaks.append(_bucket_max)
	_bucket_max = 0
	while _peaks.size() > int(ceil(OTHER_PEAK_SEC / BUDGET_SEC)):
		_peaks.pop_front()
	var other_tris: int = 0
	for v: int in _peaks:
		other_tris = maxi(other_tris, v)
	if not ClassDB.class_exists("FrameBudget"):
		return false
	if _fb == null:
		_fb = ClassDB.instantiate("FrameBudget")
	var demand: Dictionary = {"cloud_rows": 0, "route_rows": 0, "route_sidecar": 0}
	if memory != null and memory.has_method("frame_demand"):
		demand = memory.frame_demand()
	var cloud_shown: bool = int(demand.get("cloud_rows", 0)) > 0
	var burst_demand: int = BURST_POOL if bursts_on and not cloud_shown else 0
	var result: Dictionary
	if _fb.has_method("allocate_with_splats"):
		result = _fb.allocate_with_splats(gems + impostors + faded, cylinders + ribbons, int(layers.get("hulls", 0)),
			hull_tris, faded, other_tris, int(layers.get("draw_calls", 0)), int(demand.get("cloud_rows", 0)),
			int(demand.get("route_rows", 0)), int(demand.get("route_sidecar", 0)), burst_demand,
			int(extras_demand.get("tier", 0)), int(extras_demand.get("frames", 0)), splat_demand)
	elif _fb.has_method("allocate_with_extras"):
		result = _fb.allocate_with_extras(gems + impostors + faded, cylinders + ribbons, int(layers.get("hulls", 0)),
			hull_tris, faded, other_tris, int(layers.get("draw_calls", 0)), int(demand.get("cloud_rows", 0)),
			int(demand.get("route_rows", 0)), int(demand.get("route_sidecar", 0)), burst_demand,
			int(extras_demand.get("tier", 0)), int(extras_demand.get("frames", 0)))
	else:
		result = _fb.allocate(gems + impostors + faded, cylinders + ribbons, int(layers.get("hulls", 0)),
			hull_tris, faded, other_tris, int(layers.get("draw_calls", 0)), int(demand.get("cloud_rows", 0)),
			int(demand.get("route_rows", 0)), int(demand.get("route_sidecar", 0)), burst_demand)
	var changed: bool = result != caps
	caps = result
	_last_volumes = volumes
	volume_caps = split_for_volumes(volumes)
	# apply_frame_caps already no-ops on identical caps; skip the call too.
	if changed and memory != null and memory.has_method("apply_frame_caps"):
		memory.apply_frame_caps(result)
	last_report = {"graph_tris": graph_tris, "memory_tris": memory_tris, "other_tris": other_tris, "measured_tris": measured_tris}
	return true


## Weight the per-volume split (portals call this on entering and leaving a
## space). `weights`: volume_id → float ≥ 0; the share of the proportional step
## is drawn × weight. A volume missing from the dictionary weighs 1.0; NaN,
## negative and non-numeric values weigh 0, which still keeps the GEM_MIN floor.
## 1.0 is the neutral weight; all-1.0 is exactly `{}`. An empty dictionary, or
## weights all equal, gives exactly the unweighted split. Takes effect on the last seen volumes at once.
func set_volume_weights(weights: Dictionary) -> void:
	var clean: Dictionary = {}
	for id: Variant in weights:
		var w: Variant = weights[id]
		var f: float = 0.0
		if typeof(w) == TYPE_FLOAT or typeof(w) == TYPE_INT:
			f = float(w)
			if is_nan(f) or f < 0.0:
				f = 0.0
		clean[StringName(str(id))] = f   # volume ids are StringNames; a String key still matches
	volume_weights = clean
	if not _last_volumes.is_empty():
		volume_caps = split_for_volumes(_last_volumes)


## The visual extras' request for the next pass (see the file header).
func set_extras_demand(tier: int, frames: int) -> void:
	extras_demand = {"tier": clampi(tier, 0, EXTRAS_MAX_TIER), "frames": maxi(0, frames)}


## What the extras drew last frame, so the measured total is not charged twice.
func set_extras_applied(tris: int, _draw_calls: int) -> void:
	# `draw_calls` is informational: the scene already counts every call the extras
	# make (the frames' via overhead_draw_calls, the motes' as the extras' own,
	# which the allocator prices), so none is taken out of `caps.draw_calls`.
	_extras_tris = maxi(0, tris)


## The splat volumes' request for the next pass (see the file header). `volumes` is the
## visible, loaded splat volumes; `entered` is true while one of them is the entered room.
func set_splat_demand(tier: int, volumes: int, entered: bool) -> void:
	splat_demand = {"tier": clampi(tier, 0, SPLAT_MAX_TIER), "volumes": maxi(0, volumes), "entered": entered}


## What the splat volumes drew last frame, so the measured total is not charged twice.
## `draw_calls` is informational: the allocator already counts them.
func set_splat_applied(tris: int, _draw_calls: int) -> void:
	_splat_tris = maxi(0, tris)


## Share the current near caps between `volumes` (volume_id → {"nodes",
## "edges"}), in the dictionary's order. Needs the gdext FrameBudget; without it
## every volume keeps the whole cap (the allocator never ran either).
func split_for_volumes(volumes: Dictionary) -> Dictionary:
	var out: Dictionary = {}
	if volumes.is_empty():
		return out
	var ids: Array = volumes.keys()
	if _fb == null and ClassDB.class_exists("FrameBudget"):
		_fb = ClassDB.instantiate("FrameBudget")
	if _fb == null:
		for id: Variant in ids:
			out[id] = {"gem_nodes": int(caps["gem_nodes"]), "cylinder_edges": int(caps["cylinder_edges"])}
		return out
	var node_d := PackedInt64Array()
	var edge_d := PackedInt64Array()
	for id: Variant in ids:
		var d: Dictionary = volumes[id]
		node_d.append(maxi(0, int(d.get("nodes", 0))))
		edge_d.append(maxi(0, int(d.get("edges", 0))))
	var floor_n: int = int(_fb.gem_min())
	var gems: PackedInt64Array
	var cyls: PackedInt64Array
	if volume_weights.is_empty() or not _fb.has_method("split_near_caps_weighted"):
		gems = _fb.split_near_caps(int(caps["gem_nodes"]), node_d, floor_n)
		cyls = _fb.split_near_caps(int(caps["cylinder_edges"]), edge_d, floor_n)
	else:
		var w := PackedFloat64Array()
		for id: Variant in ids:
			w.append(float(volume_weights.get(StringName(str(id)), 1.0)))
		gems = _fb.split_near_caps_weighted(int(caps["gem_nodes"]), node_d, w, floor_n)
		cyls = _fb.split_near_caps_weighted(int(caps["cylinder_edges"]), edge_d, w, floor_n)
	for i: int in ids.size():
		out[ids[i]] = {"gem_nodes": int(gems[i]), "cylinder_edges": int(cyls[i])}
	return out
