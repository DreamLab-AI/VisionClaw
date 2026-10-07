extends Node

## XR ↔ desktop parity wiring (WP1 domain colour, WP2 settings/filter sync,
## WP4 cluster hulls). GraphScene owns one of these and forwards three hooks
## to it: HUD intents (`handle_control`), routed text frames (`route_text`) and
## nothing else — the per-frame work runs in this node's own `_process`. All
## decision logic lives in Rust (`domain_palette.rs`, `settings_sync.rs`,
## `hulls.rs`, fronted by BinaryProtocolClient); this script only moves data
## between the client, the scene's tracked HUD state and the hull mesh.
##
## Receipt never writes: a peer's change is applied locally and is never
## echoed back to the server (ADR-2047; NIP-98 writes stay on the existing
## HUD-press path, XR-client.md Invariant 6).

const HULL_SHADER := preload("res://materials/cluster_hull.gdshader")
## ≤ 2 Hz rebuild poll; Rust skips the build when the drawn positions have not
## moved by a server unit (signature check), so a settled graph costs a hash.
const HULL_REBUILD_SEC: float = 0.5
const HULL_PADDING: float = 0.15        # desktop clusterHulls.padding
const HULL_MAX: int = 32                # desktop maxHulls; Rust ceiling is also 32
const HULL_OPACITY: float = 0.08        # desktop clusterHulls.opacity
const PHYSICS_PATH := "/api/settings/physics"
const HANDLED_TYPES: Array[String] = ["settingsUpdated", "filter_update_success", "graphUpdated"]
## Rust PhysicsView field → GraphScene tracked var. dag_bias_k is mapped to the
## Hierarchy toggle separately (any positive bias = on, as the HUD writes 0.6).
const PHYSICS_FIELDS: Dictionary = {
	"repel_k": "_repel_k",
	"rest_length": "_rest_length",
	"dag_level_distance": "_dag_level_distance",
	"plane_bias_k": "_plane_bias_k",
	"plane_spacing": "_plane_spacing",
	"axis_compression_z": "_z_compression",
	"graph_separation_x": "_graph_separation",
}

var hull_source: int = 0            # 0 off, 1 clusters, 2 communities
## Hull cap from the FrameBudget pass (GraphScene); HULL_MAX until it runs.
var max_hulls: int = HULL_MAX
var hull_count: int = 0
var hull_triangles: int = 0
var last_event: Dictionary = {}     # last handle_settings_text result (debug/tests)
var physics_refetches: int = 0      # GETs dispatched (debug/tests)

var _scene: Node = null
var _client: RefCounted = null
var _hud: Node = null
var _hull_inst: MeshInstance3D = null
var _hull_accum: float = 0.0
var _physics_get: HTTPRequest = null
var _physics_wanted: bool = false
var _physics_inflight: bool = false


## Wire to the scene. `graph_root` is the fitted server-space root the node
## MultiMeshes live under; the hull mesh goes there so it shares their space.
func setup(scene: Node, client: RefCounted, graph_root: Node3D, hud: Node) -> void:
	_scene = scene
	_client = client
	_hud = hud
	if graph_root != null:
		_hull_inst = MeshInstance3D.new()
		_hull_inst.name = "ClusterHulls"
		_hull_inst.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
		var mat := ShaderMaterial.new()
		mat.shader = HULL_SHADER
		mat.set_shader_parameter("opacity", HULL_OPACITY)
		_hull_inst.material_override = mat
		_hull_inst.visible = false
		graph_root.add_child(_hull_inst)
	_physics_get = HTTPRequest.new()
	_physics_get.timeout = 8.0
	add_child(_physics_get)
	_physics_get.request_completed.connect(_on_physics_get_completed)
	_refresh_hud()


## HUD intents this helper owns. Returns true when the action was consumed.
func handle_control(action: String) -> bool:
	match action:
		"color_mode_toggle":
			if _client != null and _client.has_method("set_color_mode"):
				_client.set_color_mode(1 - int(_client.get_color_mode()))
			_refresh_hud()
			return true
		"hulls_cycle":
			hull_source = (hull_source + 1) % 3
			_hull_accum = HULL_REBUILD_SEC   # rebuild on the next frame
			if hull_source == 0:
				_clear_hulls()
			_refresh_hud()
			return true
	return false


## Route one text frame of a type this helper handles. Returns false (frame not
## consumed) for every other type so GraphScene's own match keeps routing.
func route_text(json: String, msg_type: String) -> bool:
	if not HANDLED_TYPES.has(msg_type):
		return false
	if _client == null or not _client.has_method("handle_settings_text"):
		return true
	last_event = _client.handle_settings_text(json)
	match str(last_event.get("kind", "")):
		"node_filter":
			# Rust already applied the filter to the render store; rebuild the draw
			# domain so hidden nodes (and their edges) drop on the next frame.
			_set_scene("_selection_dirty", true)
			_notice("Node filter changed in another session — view updated")
		"refetch":
			if str(last_event.get("category", "")) == "physics":
				_physics_wanted = true
		"filter_ack", "graph_updated", "ignored":
			pass
	return true


func _process(delta: float) -> void:
	if _client != null and _client.has_method("poll_graph_refetch"):
		_client.poll_graph_refetch()
	_maybe_fetch_physics()
	if hull_source == 0 or _client == null or not _client.has_method("build_hull_mesh"):
		return
	_hull_accum += delta
	if _hull_accum < HULL_REBUILD_SEC:
		return
	_hull_accum = 0.0
	var d: Dictionary = _client.build_hull_mesh(hull_source, HULL_PADDING, max_hulls, true)
	if bool(d.get("changed", false)):
		_apply_hull_dict(d)


## Build the one-surface hull ArrayMesh from a Rust hull dictionary. Returns
## null for an empty dictionary. Shared with perf/benchmark.gd.
static func make_hull_mesh(d: Dictionary) -> ArrayMesh:
	var verts: PackedVector3Array = d.get("vertices", PackedVector3Array())
	if verts.is_empty():
		return null
	var arrays: Array = []
	arrays.resize(Mesh.ARRAY_MAX)
	arrays[Mesh.ARRAY_VERTEX] = verts
	arrays[Mesh.ARRAY_NORMAL] = d.get("normals", PackedVector3Array())
	arrays[Mesh.ARRAY_COLOR] = d.get("colors", PackedColorArray())
	var mesh := ArrayMesh.new()
	mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
	return mesh


func _apply_hull_dict(d: Dictionary) -> void:
	var prev: int = hull_count
	hull_count = int(d.get("hulls", 0))
	hull_triangles = int(d.get("triangles", 0))
	if _hull_inst != null:
		_hull_inst.mesh = make_hull_mesh(d)
		_hull_inst.visible = _hull_inst.mesh != null
	if prev != hull_count:
		_refresh_hud()


func _clear_hulls() -> void:
	hull_count = 0
	hull_triangles = 0
	if _hull_inst != null:
		_hull_inst.mesh = null
		_hull_inst.visible = false


# --- physics read-back (the fix for the one-way write) ----------------------

# A peer changed physics: re-read it (the server sends no values on the wire).
# Deferred while one of our own writes is in flight, so the read can never race
# the staged commit in GraphScene._on_physics_completed.
func _maybe_fetch_physics() -> void:
	if not _physics_wanted or _physics_inflight or _physics_get == null or _scene == null:
		return
	if bool(_get_scene("_physics_pending", false)):
		return
	if not _scene.has_method("_http_base") or not _scene.has_method("_auth_headers"):
		_physics_wanted = false
		return
	var url: String = "%s%s" % [_scene._http_base(), PHYSICS_PATH]
	var headers: PackedStringArray = _scene._auth_headers(url, "GET")
	if _physics_get.request(url, headers, HTTPClient.METHOD_GET) != OK:
		return   # retry next frame
	_physics_wanted = false
	_physics_inflight = true
	physics_refetches += 1


func _on_physics_get_completed(result: int, code: int, _headers: PackedStringArray, body: PackedByteArray) -> void:
	_physics_inflight = false
	if result != HTTPRequest.RESULT_SUCCESS or code < 200 or code >= 300:
		push_warning("GraphParity: physics re-read failed (result=%d code=%d)" % [result, code])
		return
	apply_physics_body(body.get_string_from_utf8())


## Apply a GET /api/settings/physics body to the scene's tracked HUD state.
## Public so tests can drive it without a server.
func apply_physics_body(text: String) -> Dictionary:
	if _client == null or not _client.has_method("parse_physics_view"):
		return {}
	var view: Dictionary = _client.parse_physics_view(text)
	for k: String in PHYSICS_FIELDS:
		if view.has(k):
			_set_scene(PHYSICS_FIELDS[k], float(view[k]))
	if view.has("dag_bias_k"):
		_set_scene("_dag_bias_on", float(view["dag_bias_k"]) > 0.0)
	if not view.is_empty() and _scene != null and _scene.has_method("_refresh_controls_status"):
		_scene._refresh_controls_status()
	return view


# --- helpers -----------------------------------------------------------------

func _refresh_hud() -> void:
	if _hud == null or not _hud.has_method("set_visual_modes"):
		return
	var mode: int = int(_client.get_color_mode()) if _client != null and _client.has_method("get_color_mode") else 0
	_hud.set_visual_modes(mode, hull_source, hull_count)


func _notice(text: String) -> void:
	if _hud != null and _hud.has_method("flash_notice"):
		_hud.flash_notice(text, 4.0)


func _set_scene(prop: String, value: Variant) -> void:
	if _scene != null and prop in _scene:
		_scene.set(prop, value)


func _get_scene(prop: String, fallback: Variant) -> Variant:
	if _scene != null and prop in _scene:
		return _scene.get(prop)
	return fallback
