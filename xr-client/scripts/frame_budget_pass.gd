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
## (`graph_layer_triangles`) and the memory layers (`budget()`), one frame late.

const NodeLod := preload("res://scripts/node_lod.gd")
## Low rate: demand changes with the drawn set, not per frame (caps only move
## the tier split, never invalidate the pack plans).
const BUDGET_SEC: float = 0.25
const DEFAULT_MAX_HULLS: int = 32

var caps: Dictionary = {
	"gem_nodes": NodeLod.NEAR_CAP,
	"cylinder_edges": NodeLod.NEAR_EDGE_CAP,
	"max_hulls": DEFAULT_MAX_HULLS,
	"over_budget": false,
}
var last_report: Dictionary = {}

var _accum: float = BUDGET_SEC
var _fb: Object = null


## Run a pass when due. `layers`: instance counts drawn last frame — gems, faded,
## halos, impostors, cylinders, ribbons, hulls, hull_tris, draw_calls. `memory`:
## the memory-cloud layer (frame_demand/budget/apply_frame_caps) or null.
## `measured_tris`: the renderer's total primitives last frame (0 = unknown).
## Returns true when a pass ran.
func tick(delta: float, client: RefCounted, layers: Dictionary, memory: Object, measured_tris: int) -> bool:
	_accum += delta
	if _accum < BUDGET_SEC:
		return false
	_accum = 0.0
	if not ClassDB.class_exists("FrameBudget") or client == null or not client.has_method("graph_layer_triangles"):
		return false
	if _fb == null:
		_fb = ClassDB.instantiate("FrameBudget")
	var gems: int = int(layers.get("gems", 0))
	var faded: int = int(layers.get("faded", 0))
	var impostors: int = int(layers.get("impostors", 0))
	var cylinders: int = int(layers.get("cylinders", 0))
	var ribbons: int = int(layers.get("ribbons", 0))
	var hull_tris: int = int(layers.get("hull_tris", 0))
	var graph_tris: int = int(client.graph_layer_triangles(gems, faded, int(layers.get("halos", 0)), impostors,
		cylinders, ribbons, hull_tris))
	var demand: Dictionary = {"cloud_rows": 0, "route_rows": 0, "route_sidecar": 0}
	var memory_tris: int = 0
	if memory != null and memory.has_method("frame_demand"):
		demand = memory.frame_demand()
		var b: Dictionary = memory.budget() if memory.has_method("budget") else {}
		memory_tris = int(b.get("cloud_triangles", 0)) + int(b.get("route_triangles", 0))
	var other_tris: int = maxi(0, measured_tris - graph_tris - memory_tris) if measured_tris > 0 else 0
	var result: Dictionary = _fb.allocate(gems + impostors + faded, cylinders + ribbons, int(layers.get("hulls", 0)),
		hull_tris, faded, other_tris, int(layers.get("draw_calls", 0)), int(demand.get("cloud_rows", 0)),
		int(demand.get("route_rows", 0)), int(demand.get("route_sidecar", 0)))
	caps = result
	if memory != null and memory.has_method("apply_frame_caps"):
		memory.apply_frame_caps(result)
	last_report = {"graph_tris": graph_tris, "memory_tris": memory_tris, "other_tris": other_tris, "measured_tris": measured_tris}
	return true
