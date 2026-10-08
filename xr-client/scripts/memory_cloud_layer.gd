extends Node3D

## Live RuVector memory cloud + query route in the headset (XR WP6/WP7,
## ADR-2133). Mirrors the desktop EmbeddingCloudLayer / TrajectoryLayer:
##
##   * points: GET /api/memory-cloud (positions + metadata; the vectors blob is
##     never fetched — the headset never runs HNSW), one MultiMesh of
##     camera-facing quads (memory_point.gdshader), coloured by namespace /
##     source type / age with the desktop palette (Rust MemoryCloud);
##   * placement (desktop cloudFrame.ts, Rust CloudFrame): this node sits under
##     GraphRoot (server space). CloudRoot (outer) sits at the graph's robust
##     centre and scales the cloud's robust radius to the graph's, times
##     cloud_scale / 5; CloudCore (inner) shifts the cloud so its own robust
##     centre is the pivot. The graph extent is re-read at 1 Hz and the cloud
##     glides to it (snaps under reduced motion), so physics never jitters it;
##   * route: a `memoryRoute` frame relayed from the desktop draws the winning
##     path as one additive tube surface + bead/comet and ring MultiMeshes
##     (Rust MemoryRoute), under CloudCore so it turns and scales with the
##     cloud; rows off the route dim (focus pull). Like the desktop overlay it
##     draws without depth testing (glass nodes cannot hide it), below the HUD;
##   * framing cue: the headset never moves the user's head (ADR-2107), so a
##     new route instead shows guide dots from the controller to the answer and
##     brightens the answer ring for a few seconds (steady under reduced motion);
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
## The desktop's sidecar agreement for the shown route changed (HUD line).
signal route_stats_changed(line: String)

const ENDPOINT := "/api/memory-cloud"
const POINT_SHADER := preload("res://materials/memory_point.gdshader")
const ROUTE_SHADER := preload("res://materials/memory_route.gdshader")
const RING_SHADER := preload("res://materials/memory_ring.gdshader")
const BEAD_SHADER := preload("res://materials/memory_bead.gdshader")
const STRIDE := 16        # route bead/ring MultiMeshes: 12 transform + 4 colour
const CLOUD_STRIDE := 20  # cloud sprites: + 4 custom (flash emphasis), memory_cloud.rs CLOUD_STRIDE
## memory_flash emphasis bounds (xr-pulse memory_bursts.gd ROW_MAX_*).
const MAX_EMPHASIS_ROWS := 64
const MAX_EMPHASIS_GAIN := 2.5
const MAX_EMPHASIS_SCALE := 2.0
const NEUTRAL_EMPHASIS := Color(0, 0, 0, 1)
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
## Hover reach, label lift and label pixel size for a one-graph cloud; all
## three are multiplied by the memory body scale (10, ADR-2135) so the label
## stays readable on a cloud ten graphs wide and the far side is reachable.
const HOVER_REACH_M := 12.0
const LABEL_LIFT_M := 0.04
const LABEL_PIXEL := 0.0009
const REQUEST_TIMEOUT_SEC := 45.0  # the first snapshot build may take up to 45 s

var reduced_motion: bool = true
## Optional beat clock (xr-pulse). Read with beat_phase()/beat_pulse() and
## is_locked(); absent or unlocked → the route pulses on its own clock.
var beat_source: Object = null
## Wand whose ray drives hover and anchors the framing cue (set by GraphScene).
var pointer: Node3D = null
## Returns the graph's robust bounds `[cx, cy, cz, radius]` in GraphRoot space
## (GraphScene: BinaryProtocolClient.graph_robust_bounds(), which folds the
## always-separated layout out so the bounds are one graph's). Unset or empty
## → the cloud is placed for a live-sized graph.
var graph_bounds_source: Callable = Callable()
## The desktop `embeddingCloud.cloudScale`: 5 makes the cloud's radius ten
## times the graph's (MEMORY_BODY_SCALE, ADR-2135); linear from there. The
## cloud sits behind the graphs on the memory vertex's ray, clear of both.
var cloud_scale: float = 5.0

var _cloud: RefCounted = null   # Rust MemoryCloud
var _route: RefCounted = null   # Rust MemoryRoute
var _frame: RefCounted = null   # Rust CloudFrame (placement)
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

var _cloud_root: Node3D = null   # outer: graph centre, placement scale, rotation
var _cloud_core: Node3D = null   # inner: minus the cloud's centre (cloud-local space)
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
var _stats_line: String = ""
var _bead_tris: int = 0   # triangles per bead disc, read from the mesh
var _sprite_cap: int = -1  # last FrameBudget cloud cap applied
var _points_buf := PackedFloat32Array()  # last uploaded cloud buffer, unemphasised
var _emph_live: int = 0                  # instances restyled in the uploaded buffer
var _emph_spec: Array = []       # last set_row_emphasis arrays, re-applied after a buffer upload


func _ready() -> void:
	name = "MemoryCloud" if name.is_empty() else name
	# gdext classes are no_init: construct through their static create().
	_cloud = MemoryCloud.create()
	_route = MemoryRoute.create()
	_frame = CloudFrame.create()
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
	_cloud_core = Node3D.new()
	_cloud_core.name = "CloudCore"
	_cloud_root.add_child(_cloud_core)
	var route_prio: int = int(_route.route_render_priority()) if _route != null else 10
	var overlay_prio: int = int(_route.overlay_render_priority()) if _route != null else 20

	var sprite := _sprite_triangle_mesh()
	var pmat := ShaderMaterial.new()
	pmat.shader = POINT_SHADER
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.use_colors = true
	mm.use_custom_data = true  # (tint rgb, gain) per sprite: flash emphasis, no geometry
	mm.mesh = sprite
	_points = MultiMeshInstance3D.new()
	_points.name = "Points"
	_points.multimesh = mm
	_points.material_override = pmat
	_points.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	_cloud_core.add_child(_points)

	_route_root = Node3D.new()
	_route_root.name = "Route"
	_route_root.visible = false
	_cloud_core.add_child(_route_root)

	_tube_mat = ShaderMaterial.new()
	_tube_mat.shader = ROUTE_SHADER
	_tube_mat.render_priority = route_prio
	_tube = MeshInstance3D.new()
	_tube.name = "Tube"
	_tube.material_override = _tube_mat
	_tube.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	_route_root.add_child(_tube)

	var bmat := ShaderMaterial.new()
	bmat.shader = BEAD_SHADER
	bmat.render_priority = route_prio
	var bmm := MultiMesh.new()
	bmm.transform_format = MultiMesh.TRANSFORM_3D
	bmm.use_colors = true
	# bead_buffer scales by radius: a diameter-2 disc
	bmm.mesh = _sprite_triangle_mesh(2.0)
	_bead_tris = bmm.mesh.get_faces().size() / 3
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
	rmat.render_priority = route_prio
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
	_label.render_priority = overlay_prio  # above the depth-test-free route
	_label.fixed_size = false
	_label.pixel_size = LABEL_PIXEL * body_scale()
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
			if _frame != null:
				_frame.set_cloud_positions(_cloud.positions())
				_apply_placement(0.0)
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


## Cloud-local position of a row (parent a burst under cloud_root() to use it;
## cloud_root() is the inner CloudCore, whose space is the snapshot's).
func point_local(row: int) -> Vector3:
	return _cloud.point_local(row) if _cloud != null else Vector3.ZERO


## World position of a row (for a pool under a unit-scale effects root).
func world_point(row: int) -> Vector3:
	return _cloud_core.global_transform * point_local(row)


func cloud_root() -> Node3D:
	return _cloud_core


## The outer placement node (graph centre, placement scale, rotation).
func placement_root() -> Node3D:
	return _cloud_root


## Placement as applied: {position, scale, offset} (tests, diagnostics).
func placement() -> Dictionary:
	return {
		"position": _cloud_root.position,
		"scale": _cloud_root.scale.x,
		"offset": _cloud_core.position,
	}


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


## Draw a route from this headset's own `POST /api/memory-cloud/query`
## response (memory_search.gd): a "sidecar top-k" route through the sampled
## hits, through the same gate and verdicts as a relayed frame.
func apply_query_response(json: String) -> String:
	if _route == null or _cloud == null:
		return "error"
	var now_ms: float = Time.get_unix_time_from_system() * 1000.0
	var verdict := str(_route.offer_query_response(json, _cloud.snapshot_id(), _cloud.positions(), now_ms))
	_handle_route_verdict(verdict)
	if verdict == "reload" and _enabled:
		reload()
	elif verdict == "error":
		push_warning("MemoryCloud: %s" % str(_route.last_error()))
	return verdict


## Send the guide cue to snapshot row `row` (a hit picked on the HUD). False
## without a drawn route or for a row outside the cloud.
func focus_row(row: int) -> bool:
	if _route == null or _cloud == null or not route_active():
		return false
	return bool(_route.focus_row(row, _cloud.positions()))


## Send the guide cue to a cloud-local point: a hit outside the sample, drawn
## from its own `position` (ADR-2136 amendment).
func focus_point(p: Vector3) -> bool:
	if _route == null or not route_active():
		return false
	return bool(_route.focus_point(p))


## "relay" (the desktop's traversal), "sidecar_top_k" (this headset's query)
## or "" without a route.
func route_source() -> String:
	return str(_route.route_source()) if _route != null and route_active() else ""


## Namespaces of the loaded snapshot, most sampled rows first, and their row
## counts (parallel), for the search presets.
func namespaces() -> PackedStringArray:
	return _cloud.namespaces() if _cloud != null else PackedStringArray()


func namespace_row_counts() -> PackedInt32Array:
	return _cloud.namespace_row_counts() if _cloud != null else PackedInt32Array()


func _route_after_reload() -> void:
	if _route == null or not _route.has_pending():
		return
	_handle_route_verdict(str(_route.after_reload(_cloud.snapshot_id(), _cloud.positions())))


func _handle_route_verdict(verdict: String) -> void:
	match verdict:
		"apply":
			_build_route()
		"refresh":
			# the desktop's 10 s repeat: marks, query and stats may have moved;
			# the trace, geometry and cue stay as they are
			_cloud.set_keep(_route.keep_rows())
			_buffer_dirty = true
			_emit_stats()
		"clear":
			_clear_route()
	if verdict == "apply" or verdict == "clear":
		_emit_stats()


func _emit_stats() -> void:
	var line := agreement_line()
	if line != _stats_line:
		_stats_line = line
		route_stats_changed.emit(line)


## HUD Memory-row text for the shown route: the desktop's honest sidecar
## agreement ("n of k sidecar hits are in the sample · a of n sampled agree
## with the local top-k"), "" without a route.
func agreement_line() -> String:
	return str(_route.agreement_line()) if _route != null and route_active() else ""


func _build_route() -> void:
	if not _rebuild_route_mesh():
		_clear_route()
		return
	_cloud.set_keep(_route.keep_rows())
	_buffer_dirty = true
	_route_root.visible = true
	_tick_route(0.0)
	route_changed.emit(true, int(_route.hop_count()))


func _rebuild_route_mesh() -> bool:
	var arrays: Array = _route.mesh_arrays(ROUTE_GLOW)
	if arrays.size() <= Mesh.ARRAY_INDEX or arrays[Mesh.ARRAY_VERTEX] == null:
		return false
	var mesh := ArrayMesh.new()
	mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
	_tube.mesh = mesh
	return true


func _clear_route() -> void:
	if _route != null:
		_route.clear()
	if _cloud != null:
		_cloud.set_keep(PackedInt32Array())
	_buffer_dirty = true
	_emit_stats()
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
	var o := _cue_origin()
	_write_mm(_beads.multimesh, _route.bead_buffer(pulse, o[0], o[1]))
	var root_pulse := 1.0
	if not reduced_motion:
		root_pulse = 1.0 + 0.12 * pulse if b.size() == 2 else 1.0 + 0.06 * sin(Time.get_ticks_msec() / 500.0)
	_write_mm(_rings.multimesh, _route.ring_buffer(root_pulse))


## Framing-cue anchor in cloud-local space: [from: Vector3, local_per_m: float].
## The pointer (right controller) tip, else 0.3 m in front of and 0.25 m below
## the camera; local_per_m 0 hides the cue (no anchor in the tree).
func _cue_origin() -> Array:
	if _cloud_core == null or not _cloud_core.is_inside_tree():
		return [Vector3.ZERO, 0.0]
	var from_world: Vector3
	if pointer != null and is_instance_valid(pointer) and pointer.is_inside_tree():
		from_world = pointer.global_position
	else:
		var cam := get_viewport().get_camera_3d() if get_viewport() != null else null
		if cam == null:
			return [Vector3.ZERO, 0.0]
		from_world = cam.global_position - cam.global_transform.basis.z * 0.3 + Vector3(0.0, -0.25, 0.0)
	var xf: Transform3D = _cloud_core.global_transform
	var world_per_local: float = xf.basis.get_scale().x
	if world_per_local <= 0.0:
		return [Vector3.ZERO, 0.0]
	return [xf.affine_inverse() * from_world, 1.0 / world_per_local]


## Test seam: framing-cue opacity now (0 when not showing).
func cue_alpha() -> float:
	return float(_route.cue_alpha()) if _route != null and route_active() else 0.0


func _write_mm(mm: MultiMesh, buf: PackedFloat32Array, stride: int = STRIDE) -> void:
	var n: int = buf.size() / stride
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
	_apply_placement(delta)
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


## Frame the cloud on the graph (desktop cloudPlacement): re-read the graph's
## robust bounds at 1 Hz, glide the outer node to its centre and scale, shift
## the inner node by the cloud's own centre. The sprite size follows the
## desktop's cloudPointSize floor (constant in cloud-local units above it).
func _apply_placement(delta: float) -> void:
	if _frame == null or _cloud_root == null:
		return
	if bool(_frame.graph_read_due(delta)):
		var b := PackedFloat32Array()
		if graph_bounds_source.is_valid():
			b = graph_bounds_source.call()
		_frame.set_graph_bounds(b)
	var p: Dictionary = _frame.step(delta, cloud_scale, reduced_motion)
	_cloud_root.position = p["position"]
	var s: float = float(p["scale"])
	_cloud_root.scale = Vector3(s, s, s)
	_cloud_core.position = p["offset"]
	var sprite: float = float(p["sprite"])
	if absf(sprite - _sprite) > 0.02 * maxf(_sprite, 1e-6):
		_sprite = sprite
		_buffer_dirty = true


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
	_write_mm(_points.multimesh, buf, CLOUD_STRIDE)
	_points_buf = buf
	# the upload reset every sprite to neutral: re-apply the live emphasis
	_emph_live = 0
	if not _emph_spec.is_empty():
		_apply_emphasis(_emph_spec[0], _emph_spec[1], _emph_spec[2], _emph_spec[3])


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
	var inv: Transform3D = _cloud_core.global_transform.affine_inverse()
	var o: Vector3 = inv * origin_world
	var d: Vector3 = inv.basis * dir_world
	var row: int = _cloud.pick(o, d, HOVER_ANGLE)
	if row >= 0:
		var wp := world_point(row)
		if wp.distance_to(origin_world) > HOVER_REACH_M * body_scale():
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
	_label.global_position = world_point(row) + Vector3(0.0, LABEL_LIFT_M * body_scale(), 0.0)
	_label.visible = true
	return row


## The memory body's size relative to one graph (Rust CloudFrame; 10).
func body_scale() -> float:
	return float(_frame.memory_body_scale()) if _frame != null else 1.0


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

# --- memory_flash emphasis ----------------------------------------------------

## Restyle cloud sprites for memory_flash bursts (xr-pulse beat_pulse.gd calls
## this each frame while a flash is live, then once with empty arrays). Arrays
## are parallel; each call replaces the previous set, so rows left out return
## to normal. gain in [1, 2.5] brightens and blends toward the tint by
## (gain - 1) / 1.5; scale in [1, 2] multiplies the sprite size. At most 64
## rows; rows not drawn are ignored. Per-instance custom data and basis
## scale in the existing sprite buffer only: no geometry is added.
func set_row_emphasis(rows: PackedInt32Array, tints: PackedColorArray, gains: PackedFloat32Array, scales: PackedFloat32Array) -> void:
	var n: int = mini(mini(rows.size(), tints.size()), mini(gains.size(), scales.size()))
	if n == 0:
		_emph_spec = []
	else:
		_emph_spec = [rows.slice(0, n), tints.slice(0, n), gains.slice(0, n), scales.slice(0, n)]
	if _points == null or _cloud == null:
		return
	if n == 0:
		_apply_emphasis(PackedInt32Array(), PackedColorArray(), PackedFloat32Array(), PackedFloat32Array())
	else:
		_apply_emphasis(_emph_spec[0], _emph_spec[1], _emph_spec[2], _emph_spec[3])


func _apply_emphasis(rows: PackedInt32Array, tints: PackedColorArray, gains: PackedFloat32Array, scales: PackedFloat32Array) -> void:
	# Restyle a copy of the last uploaded buffer (pristine in _points_buf) and
	# upload it: custom floats 16..19 = (tint, gain), the 3x3 basis scaled.
	var mm: MultiMesh = _points.multimesh
	var n_inst: int = _points_buf.size() / CLOUD_STRIDE
	var live := {}
	var buf: PackedFloat32Array = _points_buf if rows.is_empty() else _points_buf.duplicate()
	for i in mini(rows.size(), MAX_EMPHASIS_ROWS):
		var idx: int = int(_cloud.instance_of_row(rows[i]))
		if idx < 0 or idx >= n_inst or live.has(idx):
			continue
		var o: int = idx * CLOUD_STRIDE
		var sc: float = clampf(scales[i], 1.0, MAX_EMPHASIS_SCALE)
		for j in [0, 1, 2, 4, 5, 6, 8, 9, 10]:
			buf[o + j] *= sc
		var t: Color = tints[i]
		buf[o + 16] = t.r
		buf[o + 17] = t.g
		buf[o + 18] = t.b
		buf[o + 19] = clampf(gains[i], 1.0, MAX_EMPHASIS_GAIN)
		live[idx] = true
	if live.is_empty() and _emph_live == 0:
		return
	_emph_live = live.size()
	if mm.instance_count == n_inst and n_inst > 0:
		mm.buffer = buf


## Test seam: instances currently restyled.
func emphasised_count() -> int:
	return _emph_live


# --- frame budget -------------------------------------------------------------

## This layer's demand for FrameBudget.allocate(): snapshot rows to draw (0
## when hidden) and the shown route's [rows, sidecar].
func frame_demand() -> Dictionary:
	var shape: PackedInt32Array = _route.route_shape() if _route != null and _enabled else PackedInt32Array([0, 0])
	return {
		"cloud_rows": point_count() if _enabled else 0,
		"route_rows": shape[0],
		"route_sidecar": shape[1],
	}


## Apply FrameBudget.allocate()'s caps: the cloud sprite cap (re-selects the
## drawn rows only when it changes) and the route's centreline cap (rebuilds
## the tube only when its samples per hop change; the animation keeps going).
func apply_frame_caps(caps: Dictionary) -> void:
	if _cloud == null:
		return
	var sprites: int = int(caps.get("cloud_sprites", 0))
	if sprites > 0 and sprites != _sprite_cap:
		_sprite_cap = sprites
		_cloud.set_sprite_cap(sprites)
		_buffer_dirty = true
	if _route != null and caps.has("route_ring_cap"):
		if bool(_route.set_ring_cap(int(caps["route_ring_cap"]), _cloud.positions())) and route_active():
			_rebuild_route_mesh()
			_tick_route(0.0)


## One equilateral triangle whose incircle is the shader's disc (of
## `diameter`): half a quad's triangles per sprite (memory_cloud.rs
## SPRITE_TRIANGLE_UV); the fragment mask discards the corners.
func _sprite_triangle_mesh(diameter: float = 1.0) -> ArrayMesh:
	var uvs: PackedVector2Array = _cloud.sprite_triangle_uv() if _cloud != null else PackedVector2Array([Vector2(0.5, -0.5), Vector2(-0.3660254, 1.0), Vector2(1.3660254, 1.0)])
	var verts := PackedVector3Array()
	var normals := PackedVector3Array()
	for uv in uvs:
		verts.append(Vector3(uv.x - 0.5, 0.5 - uv.y, 0.0) * diameter)
		normals.append(Vector3(0, 0, 1))
	var arrays := []
	arrays.resize(Mesh.ARRAY_MAX)
	arrays[Mesh.ARRAY_VERTEX] = verts
	arrays[Mesh.ARRAY_NORMAL] = normals
	arrays[Mesh.ARRAY_TEX_UV] = uvs
	var mesh := ArrayMesh.new()
	mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
	return mesh


## Test seam: triangles in the sprite and bead meshes as built.
func mesh_triangles() -> Dictionary:
	return {
		"sprite": _points.multimesh.mesh.get_faces().size() / 3 if _points != null else 0,
		"bead": _bead_tris,
		"bead_expected": int(_route.bead_triangles()) if _route != null else -1,
	}


## Draw calls and triangles these layers add (benchmark.gd).
func budget() -> Dictionary:
	var cloud_tris: int = int(_cloud.triangle_estimate()) if _cloud != null and _enabled else 0
	var route_tris: int = 0
	var route_calls: int = 0
	if route_active() and _enabled:
		route_tris = int(_route.triangle_estimate())  # tube + beads + rings
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
