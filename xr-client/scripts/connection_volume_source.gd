extends RefCounted
## Used via `const ConnectionVolumeSource := preload("res://scripts/connection_volume_source.gd")`
## rather than a class_name, so a headless run with a stale global class cache
## still resolves it (see backend_caps.gd).

## The real content source behind a SpatialVolume (XR phase-1 contract §3.3):
## one BackendConnection plus a class mask (E3 bindings: knowledge ←
## visionclaw/0b0001, ontology ← visionclaw/0b0010, codescape ← codescape/0b1000).
##
## It owns its own node, faded, halo, impostor, edge and ribbon
## MultiMeshInstance3Ds and mounts them under the volume's Content node, so the
## volume's fit transform applies to them and to nothing else. Positions are in
## graph units; the volume maps them to world.
##
## Duck-typed interface (§3.3): content_id, content_ids, content_aabb, mount,
## unmount, position_of. The scene also calls pack() once per frame for each
## visible volume; a hidden volume calls clear_instances() and does not pack.
##
## Packing uses the connection client's pack slot `slot` (render_store pack
## slots, ADR-2159 D4): two volumes fed by one connection keep separate pack
## plans, so neither invalidates the other's.

## How often the masked id list is re-read while a generation is live. Nodes
## arrive with position frames after the topology, so the list grows over the
## first frames of a generation; a generation change re-reads at once.
const ID_REFRESH_SEC := 0.25

## Instance strides the gdext packers write (render_store NODE_STRIDE and the
## typed edge stride): 12 transform floats + colour + custom for nodes, 12 +
## custom for edges.
const NODE_STRIDE := 20
const EDGE_STRIDE := 16

## MultiMesh roles this source draws, in draw order.
const ROLES: Array[String] = ["nodes", "faded", "halos", "impostors", "edges", "ribbons"]
const EDGE_ROLES: Array[String] = ["edges", "ribbons"]
## Node size range passed to the packer (graph_scene's 0.7 .. 1.9 centrality lerp).
const SIZE_LO := 0.7
const SIZE_HI := 1.9
## Most edges one volume draws (graph_scene EDGE_SAFETY_CEILING).
const EDGE_CEILING := 20000

const NodeLod := preload("res://scripts/node_lod.gd")

var conn: Node = null
var mask: int = 0
## Node excluded from content_aabb while grabbed (E7: a dragged outlier must
## not refit the volume under the hand). -1 = none.
var grabbed_id: int = -1
## Mounted container and its MultiMeshInstance3Ds by role; empty when unmounted.
var root: Node3D = null
var instances: Dictionary = {}
## Render-store pack slot on the connection's client (unique per client).
var slot: int = 0
## Instance counts from the last pack, by role.
var counts: Dictionary = {}

## role → MultiMeshInstance3D whose mesh and material this source copies
## (GraphScene's own NodesMulti, EdgesMulti, … at wiring time).
var _templates: Dictionary = {}
var _ids := PackedInt32Array()
var _ids_generation: int = -1
var _ids_read_ms: int = -1
var _pairs := PackedInt32Array()
var _pairs_generation: int = -1
var _pairs_read_ms: int = -1


func _init(connection: Node = null, class_mask: int = 0) -> void:
	conn = connection
	mask = class_mask
	# The mask is unique among the volumes one connection feeds, so it makes a
	# collision-free pack slot; slot 0 stays the default for other callers.
	slot = class_mask


## Pure: `0b0001`-style label for a 4-bit class mask.
static func mask_label(m: int) -> String:
	var bits: String = ""
	for b: int in range(3, -1, -1):
		bits += "1" if m & (1 << b) else "0"
	return "0b" + bits


func content_id() -> StringName:
	var cid: String = String(conn.conn_id) if conn != null else "none"
	return StringName("%s:%s" % [cid, mask_label(mask)])


## Use `templates[role]`'s mesh and material for that role. Call before mount.
func set_templates(templates: Dictionary) -> void:
	_templates = templates.duplicate()


## Wire ids this volume draws, current generation.
func content_ids() -> PackedInt32Array:
	if conn == null:
		return PackedInt32Array()
	var now: int = Time.get_ticks_msec()
	var gen: int = int(conn.generation)
	if gen != _ids_generation or _ids_read_ms < 0 or now - _ids_read_ms >= int(ID_REFRESH_SEC * 1000.0):
		_ids = conn.node_ids_for_mask(mask)
		_ids_generation = gen
		_ids_read_ms = now
	return _ids


## Forget the cached id list (topology change, reconnect).
func invalidate() -> void:
	_ids_read_ms = -1
	_ids_generation = -1
	_pairs_read_ms = -1
	_pairs_generation = -1


## This volume's own edges (both ends in the mask), heaviest first, re-read
## like content_ids(). An unchanged list keeps the edge pack plan valid.
func content_pairs() -> PackedInt32Array:
	if conn == null:
		return PackedInt32Array()
	var now: int = Time.get_ticks_msec()
	var gen: int = int(conn.generation)
	if gen != _pairs_generation or _pairs_read_ms < 0 or now - _pairs_read_ms >= int(ID_REFRESH_SEC * 1000.0):
		var fresh: PackedInt32Array = conn.edges_for_mask(mask, EDGE_CEILING)
		if fresh != _pairs:
			_pairs = fresh
		_pairs_generation = gen
		_pairs_read_ms = now
	return _pairs


## Pack this volume's instances for one frame. `cam_world`: the eye in world
## space. `node_ratio` / `edge_ratio`: world radius over mesh radius for a node
## and an edge (the HUD size factor folded in); the source divides by its own
## world-per-graph-unit scale. `gem_cap` / `cyl_cap`: this volume's share of the
## frame budget's near caps. `near_m` / `near_edge_m`: near-tier radii in metres.
func pack(cam_world: Vector3, node_ratio: float, edge_ratio: float, gem_cap: int, cyl_cap: int, near_m: float, near_edge_m: float) -> void:
	if root == null or conn == null or not root.is_inside_tree():
		return
	var client: RefCounted = conn.client
	if client == null or not client.has_method("build_node_buffer_lod"):
		return
	if client.has_method("set_pack_slot"):
		client.set_pack_slot(slot)
	var xf: Transform3D = root.global_transform
	var wpg: float = maxf(xf.basis.get_scale().x, 1.0e-9)
	var cam_g: Vector3 = xf.affine_inverse() * cam_world
	var nbuf: PackedFloat32Array = client.build_node_buffer_lod(content_ids(), node_ratio / wpg, SIZE_LO, SIZE_HI,
		cam_g, maxi(gem_cap, 0), near_m / wpg)
	counts["nodes"] = NodeLod.assign(instances["nodes"], nbuf)
	counts["impostors"] = NodeLod.assign(instances["impostors"], client.impostor_node_buffer())
	var fbuf: PackedFloat32Array = client.faded_node_buffer()
	counts["faded"] = NodeLod.assign(instances["faded"], fbuf)
	counts["halos"] = NodeLod.assign_halo(instances["halos"], nbuf, fbuf)
	var ebuf: PackedFloat32Array = client.build_edge_buffer_lod(content_pairs(), edge_ratio / wpg, cam_g,
		maxi(cyl_cap, 0), near_edge_m / wpg)
	counts["edges"] = NodeLod.assign_stride(instances["edges"], ebuf, EDGE_STRIDE)
	counts["ribbons"] = NodeLod.assign_stride(instances["ribbons"], client.ribbon_edge_buffer(), EDGE_STRIDE)
	var e_mmi: MultiMeshInstance3D = instances["edges"]
	var r_mmi: MultiMeshInstance3D = instances["ribbons"]
	NodeLod.sync_edge_params(e_mmi.material_override, r_mmi.material_override)


## Empty every instance buffer (volume hidden or connection offline).
func clear_instances() -> void:
	for role: String in instances:
		var mmi: MultiMeshInstance3D = instances[role]
		if mmi != null and mmi.multimesh != null and mmi.multimesh.instance_count != 0:
			mmi.multimesh.instance_count = 0
	counts.clear()


## Drawn totals for the frame budget: {"nodes", "edges"} plus per-role counts.
func demand() -> Dictionary:
	return {
		"nodes": int(counts.get("nodes", 0)) + int(counts.get("impostors", 0)) + int(counts.get("faded", 0)),
		"edges": int(counts.get("edges", 0)) + int(counts.get("ribbons", 0)),
	}


## Full extent over content_ids() in graph units, excluding the grabbed node.
func content_aabb() -> AABB:
	if conn == null:
		return AABB()
	return conn.render_aabb_for_ids(content_ids(), grabbed_id)


func position_of(id: int) -> Vector3:
	if conn == null:
		return Vector3.ZERO
	return conn.node_position(id)


func mount(parent: Node3D) -> void:
	unmount()
	if parent == null:
		return
	root = Node3D.new()
	root.name = "Source_%s" % String(content_id()).replace(":", "_")
	for role: String in ROLES:
		var mmi := _make_instance(role)
		root.add_child(mmi)
		instances[role] = mmi
	parent.add_child(root)


func unmount() -> void:
	if root == null:
		return
	if root.get_parent() != null:
		root.get_parent().remove_child(root)
	root.free()
	root = null
	instances.clear()
	counts.clear()
	if conn != null and conn.client != null and conn.client.has_method("drop_pack_slot"):
		conn.client.drop_pack_slot(slot)


func is_mounted() -> bool:
	return root != null


func instance(role: String) -> MultiMeshInstance3D:
	return instances.get(role, null)


func _make_instance(role: String) -> MultiMeshInstance3D:
	var mmi := MultiMeshInstance3D.new()
	mmi.name = role.capitalize().replace(" ", "")
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	var is_edge: bool = EDGE_ROLES.has(role)
	mm.use_colors = not is_edge
	mm.use_custom_data = true
	var tpl: MultiMeshInstance3D = _templates.get(role, null)
	if tpl != null:
		if tpl.multimesh != null:
			mm.mesh = tpl.multimesh.mesh
			mm.use_colors = tpl.multimesh.use_colors
			mm.use_custom_data = tpl.multimesh.use_custom_data
		mmi.material_override = tpl.material_override
		mmi.cast_shadow = tpl.cast_shadow
		mmi.layers = tpl.layers
	mm.instance_count = 0
	mmi.multimesh = mm
	return mmi
