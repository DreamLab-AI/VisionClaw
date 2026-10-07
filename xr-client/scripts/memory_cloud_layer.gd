extends Node3D

## Live RuVector memory cloud + query route in the headset (XR WP6/WP7,
## ADR-2133). Mirrors the desktop EmbeddingCloudLayer / TrajectoryLayer:
##
##   * points: GET /api/memory-cloud (positions + metadata; the vectors blob is
##     never fetched — the headset never runs HNSW), one MultiMesh of
##     camera-facing quads (memory_point.gdshader), coloured by namespace /
##     source type / age with the desktop palette (Rust MemoryCloud);
##   * placement: this node sits under GraphRoot at the server origin, and
##     CloudRoot carries the desktop cloudScale (5), exactly as the desktop
##     cloud group sits at world origin around the graph;
##   * route: a `memoryRoute` frame relayed from the desktop draws the winning
##     path as one additive tube surface + bead/comet and ring MultiMeshes
##     (Rust MemoryRoute), under CloudRoot so it turns and scales with the
##     cloud; rows off the route dim (focus pull);
##   * hover: the pointer ray picks a sprite and a world-size label shows its
##     key and namespace / source type;
##   * flashes: resolve_flash / world_point let the memory_flash burst pool
##     (xr-pulse) land on real rows.
##
## Load policy (memoryCloudStore.ts): 401/403 hides the cloud quietly; 503
## retries at Retry-After, else 5 s doubling to 60 s; 409 reloads once at once.
## Every request is signed through the scene's `_auth_headers` for the exact
## URL fetched (XR-client Invariant 6).

signal cloud_loaded(snapshot_id: String, count: int)
signal cloud_cleared()
## state: off | loading | ready | forbidden | unavailable | failed
signal status_changed(state: String, detail: String)
signal route_changed(active: bool, hops: int)

const ENDPOINT := "/api/memory-cloud"
const POINT_SHADER := preload("res://materials/memory_point.gdshader")
const ROUTE_SHADER := preload("res://materials/memory_route.gdshader")
const RING_SHADER := preload("res://materials/memory_ring.gdshader")
const STRIDE := 16
## desktop `routeGlow` default
const ROUTE_GLOW := 1.2
## seconds for the focus-pull dim to fade in or out (DIM_FADE)
const DIM_FADE := 0.6
## rebuild the point buffer only when the dim moved this much (the fade is
## stepped, so a 12k-sprite buffer is re-uploaded ~15 times per fade, not 54)
const DIM_STEP := 0.05
const HOVER_HZ := 15.0
## ray-pick cone half-angle (radians), ~1.5°
const HOVER_ANGLE := 0.026
const HOVER_REACH_M := 12.0
const LABEL_LIFT_M := 0.04
const LABEL_PIXEL := 0.0009
const REQUEST_TIMEOUT_SEC := 45.0  # the first snapshot build may take up to 45 s

var reduced_motion: bool = true
## Optional beat clock (xr-pulse). Read with beat_phase()/beat_pulse() and
## is_locked(); absent or unlocked → the route pulses on its own clock.
var beat_source: Object = null
## Wand whose ray drives hover (set by GraphScene).
var pointer: Node3D = null

var _cloud: RefCounted = null   # Rust MemoryCloud
var _route: RefCounted = null   # Rust MemoryRoute
var _http: HTTPRequest = null
var _http_base: String = ""
var _auth: Callable = Callable()
var _enabled: bool = false
var _state: String = "off"
var _detail: String = ""
var _retry_at_ms: int = -1
var _streak: int = 0
var _stale_retry_used: bool = false
var _requests_sent: int = 0
var _in_flight: bool = false

var _cloud_root: Node3D = null
var _points: MultiMeshInstance3D = null
var _route_root: Node3D = null
var _tube: MeshInstance3D = null
var _tube_mat: ShaderMaterial = null
var _beads: MultiMeshInstance3D = null
var _rings: MultiMeshInstance3D = null
var _label: Label3D = null

var _dim: float = 0.0
var _dim_applied: float = -1.0
var _buffer_dirty: bool = true
var _sprite: float = 1.0
var _opacity: float = 0.6
var _rotation_per_sec: float = 0.03
var _hover_accum: float = 0.0
var _hover_row: int = -1
var _bead_tris: int = 0   # triangles per bead sphere, read from the mesh


func _ready() -> void:
	name = "MemoryCloud" if name.is_empty() else name
	# gdext classes are no_init: construct through their static create().
	_cloud = MemoryCloud.create()
	_route = MemoryRoute.create()
	if _cloud != null:
		_sprite = _cloud.sprite_local_size()
	_build_nodes()
	_http = HTTPRequest.new()
	_http.name = "MemoryCloudHttp"
	_http.timeout = REQUEST_TIMEOUT_SEC
	add_child(_http)
	_http.request_completed.connect(_on_http_completed)
	visible = false


func _build_nodes() -> void:
	_cloud_root = Node3D.new()
	_cloud_root.name = "CloudRoot"
	var cs: float = _cloud.cloud_scale() if _cloud != null else 5.0
	_cloud_root.scale = Vector3(cs, cs, cs)
	add_child(_cloud_root)

	var quad := QuadMesh.new()
	quad.size = Vector2(1.0, 1.0)
	var pmat := ShaderMaterial.new()
	pmat.shader = POINT_SHADER
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.use_colors = true
	mm.mesh = quad
	_points = MultiMeshInstance3D.new()
	_points.name = "Points"
	_points.multimesh = mm
	_points.material_override = pmat
	_points.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	_cloud_root.add_child(_points)

	_route_root = Node3D.new()
	_route_root.name = "Route"
	_route_root.visible = false
	_cloud_root.add_child(_route_root)

	_tube_mat = ShaderMaterial.new()
	_tube_mat.shader = ROUTE_SHADER
	_tube = MeshInstance3D.new()
	_tube.name = "Tube"
	_tube.material_override = _tube_mat
	_tube.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	_route_root.add_child(_tube)

	var sphere := SphereMesh.new()
	sphere.radius = 1.0
	sphere.height = 2.0
	sphere.radial_segments = 8
	sphere.rings = 4
	var bmat := StandardMaterial3D.new()
	bmat.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	bmat.vertex_color_use_as_albedo = true
	bmat.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	bmat.blend_mode = BaseMaterial3D.BLEND_MODE_ADD
	bmat.no_depth_test = false
	bmat.depth_draw_mode = BaseMaterial3D.DEPTH_DRAW_DISABLED
	var bmm := MultiMesh.new()
	bmm.transform_format = MultiMesh.TRANSFORM_3D
	bmm.use_colors = true
	bmm.mesh = sphere
	_bead_tris = sphere.get_faces().size() / 3
	_beads = MultiMeshInstance3D.new()
	_beads.name = "Beads"
	_beads.multimesh = bmm
	_beads.material_override = bmat
	_beads.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	_route_root.add_child(_beads)

	var ring_quad := QuadMesh.new()
	ring_quad.size = Vector2(2.0, 2.0)
	var rmat := ShaderMaterial.new()
	rmat.shader = RING_SHADER
	var rmm := MultiMesh.new()
	rmm.transform_format = MultiMesh.TRANSFORM_3D
	rmm.use_colors = true
	rmm.mesh = ring_quad
	_rings = MultiMeshInstance3D.new()
	_rings.name = "Rings"
	_rings.multimesh = rmm
	_rings.material_override = rmat
	_rings.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	_route_root.add_child(_rings)

	# World-size label: top_level so the GraphRoot fit and cloud scale never
	# shrink the text; positioned in world space each hover tick.
	_label = Label3D.new()
	_label.name = "HoverLabel"
	_label.top_level = true
	_label.billboard = BaseMaterial3D.BILLBOARD_ENABLED
	_label.no_depth_test = true
	_label.fixed_size = false
	_label.pixel_size = LABEL_PIXEL
	_label.font_size = 28
	_label.outline_size = 8
	_label.modulate = Color(1, 1, 1, 0.95)
	_label.outline_modulate = Color(0, 0, 0, 0.85)
	_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_label.vertical_alignment = VERTICAL_ALIGNMENT_BOTTOM
	_label.visible = false
	add_child(_label)


# --- configuration -----------------------------------------------------------

## `headers` is GraphScene._auth_headers: (url, method) -> PackedStringArray.
func configure(http_base: String, headers: Callable) -> void:
	_http_base = http_base.rstrip("/")
	_auth = headers


func set_enabled(on: bool) -> void:
	_enabled = on
	visible = on
	if not on:
		_retry_at_ms = -1
		_hide_label()
		if _state != "ready" and _state != "forbidden":
			_set_state("off", "")
		return
	if _state == "ready" and has_snapshot():
		_points.visible = true
		_set_state("ready", "")
		return
	reload()


func is_enabled() -> bool:
	return _enabled


func cycle_colour_mode() -> String:
	if _cloud == null:
		return ""
	_cloud.set_colour_mode(_cloud.next_colour_mode())
	_buffer_dirty = true
	return colour_mode_label()


func colour_mode_label() -> String:
	return str(_cloud.colour_mode_label()) if _cloud != null else "Namespace"


func state() -> String:
	return _state


func state_detail() -> String:
	return _detail


## Short HUD text for the Memory button.
func status_label() -> String:
	match _state:
		"ready":
			return "Memory: %d" % int(_cloud.count()) if _enabled else "Memory: Off"
		"loading":
			return "Memory: Loading"
		"forbidden":
			return "Memory: Locked"
		"unavailable":
			return "Memory: Waiting"
		"failed":
			return "Memory: Error"
	return "Memory: Off"


# --- loading -----------------------------------------------------------------

func reload() -> void:
	if _http == null:
		return
	if _in_flight:
		_http.cancel_request()
		_in_flight = false
	_retry_at_ms = -1
	var url := "%s%s" % [_http_base, ENDPOINT]
	var headers := PackedStringArray()
	if _auth.is_valid():
		headers = _auth.call(url, "GET")
	_requests_sent += 1
	_set_state("loading", "")
	if _http_base.is_empty():
		_on_http_completed(HTTPRequest.RESULT_CANT_CONNECT, 0, PackedStringArray(), PackedByteArray())
		return
	var err := _http.request(url, headers, HTTPClient.METHOD_GET)
	if err != OK:
		_on_http_completed(HTTPRequest.RESULT_CANT_CONNECT, 0, PackedStringArray(), PackedByteArray())
		return
	_in_flight = true


func _on_http_completed(result: int, code: int, headers: PackedStringArray, body: PackedByteArray) -> void:
	_in_flight = false
	if _cloud == null:
		return
	var outcome: int = _cloud.classify_status(result == HTTPRequest.RESULT_SUCCESS, code)
	match outcome:
		0:
			var n: int = _cloud.load_bytes(body)
			if n < 0:
				_set_state("failed", str(_cloud.last_error()))
				_route_after_reload()
				return
			_streak = 0
			_stale_retry_used = false
			_buffer_dirty = true
			_points.visible = _enabled
			_set_state("ready", "")
			cloud_loaded.emit(str(_cloud.snapshot_id()), n)
			_route_after_reload()
			# A route drawn against the previous sample no longer lines up.
			if _route != null and _route.is_active() and str(_route.snapshot_id()) != str(_cloud.snapshot_id()):
				_clear_route()
		1:
			# Power-user only (ADR-2133 §5): a quiet state, never a toast.
			_points.visible = false
			_clear_route()
			_set_state("forbidden", "HTTP %d" % code)
			_route_after_reload()
		2:
			var ra: int = _cloud.parse_retry_after(_header(headers, "retry-after"), int(Time.get_unix_time_from_system() * 1000.0))
			var d: PackedInt64Array = _cloud.unavailable_delay(ra, _streak)
			_streak = int(d[1])
			_retry_at_ms = Time.get_ticks_msec() + int(d[0])
			_set_state("unavailable", body.get_string_from_utf8().left(120))
		3:
			if not _stale_retry_used:
				_stale_retry_used = true
				reload()
				return
			_set_state("failed", "snapshot rebuilt twice during load")
			_route_after_reload()
		_:
			var why := "HTTP %d" % code if result == HTTPRequest.RESULT_SUCCESS else "unreachable (result %d)" % result
			_set_state("failed", why)
			_route_after_reload()


## Feed a snapshot body through the same path a 200 response takes (benchmark,
## tests, a cached body). Returns true when it parsed.
func ingest_snapshot(body: PackedByteArray) -> bool:
	_on_http_completed(HTTPRequest.RESULT_SUCCESS, 200, PackedStringArray(), body)
	return _state == "ready"


func _header(headers: PackedStringArray, name_lower: String) -> String:
	for h in headers:
		var i := h.find(":")
		if i > 0 and h.substr(0, i).strip_edges().to_lower() == name_lower:
			return h.substr(i + 1).strip_edges()
	return ""


func _set_state(s: String, detail: String) -> void:
	var changed := s != _state or detail != _detail
	_state = s
	_detail = detail
	if changed:
		status_changed.emit(s, detail)


func has_snapshot() -> bool:
	return _cloud != null and str(_cloud.snapshot_id()) != ""


func snapshot_id() -> String:
	return str(_cloud.snapshot_id()) if _cloud != null else ""


func point_count() -> int:
	return int(_cloud.count()) if _cloud != null else 0


func drawn_count() -> int:
	return int(_cloud.drawn_count()) if _cloud != null else 0


# --- flash API (consumed by the memory_flash burst pool) ---------------------

## Rows a memory_flash lands on: exact namespace:key, then bare key, else up to
## three rows of the namespace, else none (`last_flash_match()` = key |
## namespace | none). Empty when no cloud is loaded.
func resolve_flash(key: String, ns: String) -> PackedInt32Array:
	if _cloud == null or not has_snapshot():
		return PackedInt32Array()
	return _cloud.resolve_flash(key, ns, Time.get_ticks_usec())


func last_flash_match() -> String:
	return str(_cloud.last_flash_match()) if _cloud != null else "none"


## Cloud-local position of a row (parent a burst under cloud_root() to use it).
func point_local(row: int) -> Vector3:
	return _cloud.point_local(row) if _cloud != null else Vector3.ZERO


## World position of a row (for a pool under a unit-scale effects root).
func world_point(row: int) -> Vector3:
	return _cloud_root.global_transform * point_local(row)


func cloud_root() -> Node3D:
	return _cloud_root


# --- route (WP7) -------------------------------------------------------------

## Feed a `memoryRoute` text frame. Returns the gate decision
## (apply | clear | stale | reload | drop | error).
func apply_route_json(json: String) -> String:
	if _route == null or _cloud == null:
		return "error"
	var verdict := str(_route.offer_json(json, _cloud.snapshot_id(), _cloud.positions()))
	_handle_route_verdict(verdict)
	if verdict == "reload" and _enabled:
		reload()
	elif verdict == "error":
		push_warning("MemoryCloud: %s" % str(_route.last_error()))
	return verdict


func _route_after_reload() -> void:
	if _route == null or not _route.has_pending():
		return
	_handle_route_verdict(str(_route.after_reload(_cloud.snapshot_id(), _cloud.positions())))


func _handle_route_verdict(verdict: String) -> void:
	match verdict:
		"apply":
			_build_route()
		"clear":
			_clear_route()


func _build_route() -> void:
	var arrays: Array = _route.mesh_arrays(ROUTE_GLOW)
	if arrays.size() <= Mesh.ARRAY_INDEX or arrays[Mesh.ARRAY_VERTEX] == null:
		_clear_route()
		return
	var mesh := ArrayMesh.new()
	mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
	_tube.mesh = mesh
	_cloud.set_keep(_route.keep_rows())
	_buffer_dirty = true
	_route_root.visible = true
	_tick_route(0.0)
	route_changed.emit(true, int(_route.hop_count()))


func _clear_route() -> void:
	if _route != null:
		_route.clear()
	if _cloud != null:
		_cloud.set_keep(PackedInt32Array())
	_buffer_dirty = true
	if _route_root != null:
		_route_root.visible = false
		_tube.mesh = null
		_beads.multimesh.instance_count = 0
		_rings.multimesh.instance_count = 0
	route_changed.emit(false, 0)


func route_active() -> bool:
	return _route != null and _route.is_active()


func _beat() -> Array:
	# [phase, pulse] or [] when no locked beat. xr-pulse exposes the clock.
	if beat_source == null or not is_instance_valid(beat_source):
		return []
	if beat_source.has_method("is_locked") and not bool(beat_source.call("is_locked")):
		return []
	var phase: float = float(beat_source.call("beat_phase")) if beat_source.has_method("beat_phase") else -1.0
	if phase < 0.0:
		return []
	var pulse: float = float(beat_source.call("beat_pulse")) if beat_source.has_method("beat_pulse") else pow(1.0 - phase, 3.0)
	return [phase, pulse]


func _tick_route(delta: float) -> void:
	if not route_active():
		return
	var b := _beat()
	var phase: float = b[0] if b.size() == 2 else -1.0
	var pulse: float = b[1] if b.size() == 2 else 0.0
	if reduced_motion:
		pulse = 0.0
	var p: Dictionary = _route.tick(delta, ROUTE_GLOW, phase, pulse, reduced_motion)
	_tube_mat.set_shader_parameter("head_u", float(p.get("head_u", 1.0)))
	_tube_mat.set_shader_parameter("comet_u", float(p.get("comet_u", -1.0)))
	_tube_mat.set_shader_parameter("tail_u", float(p.get("tail_u", 0.18)))
	_tube_mat.set_shader_parameter("glow", float(p.get("glow", ROUTE_GLOW)) / ROUTE_GLOW)
	_write_mm(_beads.multimesh, _route.bead_buffer(pulse))
	var root_pulse := 1.0
	if not reduced_motion:
		root_pulse = 1.0 + 0.12 * pulse if b.size() == 2 else 1.0 + 0.06 * sin(Time.get_ticks_msec() / 500.0)
	_write_mm(_rings.multimesh, _route.ring_buffer(root_pulse))


func _write_mm(mm: MultiMesh, buf: PackedFloat32Array) -> void:
	var n: int = buf.size() / STRIDE
	if mm.instance_count != n:
		mm.instance_count = n
	if n > 0:
		mm.buffer = buf


# --- per frame ---------------------------------------------------------------

func _process(delta: float) -> void:
	if _retry_at_ms >= 0 and Time.get_ticks_msec() >= _retry_at_ms:
		_retry_at_ms = -1
		if _enabled:
			reload()
	if not _enabled or not has_snapshot() or _state != "ready":
		return
	# Desktop turns the cloud slowly unless a route is shown; comfort default
	# (reduced motion) keeps it still.
	if not reduced_motion and not route_active():
		_cloud_root.rotate_y(_rotation_per_sec * delta)
	_step_dim(delta)
	if _buffer_dirty:
		_rebuild_points()
	_tick_route(delta)
	_hover_accum += delta
	if _hover_accum >= 1.0 / HOVER_HZ:
		_hover_accum = 0.0
		_update_hover_from_pointer()


func _step_dim(delta: float) -> void:
	var goal: float = 0.75 if route_active() else 0.0
	var step: float = 1.0 if reduced_motion else minf(1.0, delta / DIM_FADE)
	_dim += (goal - _dim) * (1.0 if absf(goal - _dim) < 0.005 else step)
	if absf(_dim - _dim_applied) >= DIM_STEP or (_dim == goal and _dim != _dim_applied):
		_buffer_dirty = true


func _rebuild_points() -> void:
	_buffer_dirty = false
	_dim_applied = _dim
	var buf: PackedFloat32Array = _cloud.build_buffer(_sprite, _opacity, _dim)
	_write_mm(_points.multimesh, buf)


## Force the point buffer now (tests, benchmark).
func flush() -> void:
	if has_snapshot():
		_rebuild_points()


# --- hover -------------------------------------------------------------------

func _update_hover_from_pointer() -> void:
	if pointer == null or not is_instance_valid(pointer) or not pointer.is_inside_tree():
		_hide_label()
		return
	var t: Transform3D = pointer.global_transform
	update_hover(t.origin, -t.basis.z)


## Pick the sprite under a world-space ray; shows the label and returns the
## row, or -1.
func update_hover(origin_world: Vector3, dir_world: Vector3) -> int:
	if _cloud == null or not has_snapshot() or not visible:
		_hide_label()
		return -1
	var inv: Transform3D = _cloud_root.global_transform.affine_inverse()
	var o: Vector3 = inv * origin_world
	var d: Vector3 = inv.basis * dir_world
	var row: int = _cloud.pick(o, d, HOVER_ANGLE)
	if row >= 0:
		var wp := world_point(row)
		if wp.distance_to(origin_world) > HOVER_REACH_M:
			row = -1
	if row < 0:
		_hide_label()
		return -1
	if row != _hover_row:
		var info: Dictionary = _cloud.row_info(row)
		_label.text = "%s\n%s / %s" % [
			_ellipsis(str(info.get("key", "")), 48),
			str(info.get("namespace", "")),
			str(info.get("sourceType", ""))]
	_hover_row = row
	_label.global_position = world_point(row) + Vector3(0.0, LABEL_LIFT_M, 0.0)
	_label.visible = true
	return row


func hovered_row() -> int:
	return _hover_row


func hover_text() -> String:
	return _label.text if _label != null and _label.visible else ""


func _hide_label() -> void:
	_hover_row = -1
	if _label != null:
		_label.visible = false


static func _ellipsis(s: String, n: int) -> String:
	return s if s.length() <= n else s.left(n - 1) + "…"


# --- budget ------------------------------------------------------------------

## Draw calls and triangles these layers add (benchmark.gd).
func budget() -> Dictionary:
	var cloud_tris: int = int(_cloud.triangle_estimate()) if _cloud != null and _enabled else 0
	var route_tris: int = 0
	var route_calls: int = 0
	if route_active() and _enabled:
		route_tris = int(_route.triangle_estimate())
		route_tris += _beads.multimesh.instance_count * _bead_tris
		route_tris += _rings.multimesh.instance_count * 2
		route_calls = 3
	return {
		"cloud_draw_calls": 1 if cloud_tris > 0 else 0,
		"cloud_triangles": cloud_tris,
		"route_draw_calls": route_calls,
		"route_triangles": route_tris,
		"sprites": drawn_count() if _enabled else 0,
	}


## Test/diagnostic seam: number of snapshot requests issued.
func requests_sent() -> int:
	return _requests_sent


func retry_in_ms() -> int:
	return -1 if _retry_at_ms < 0 else maxi(0, _retry_at_ms - Time.get_ticks_msec())
