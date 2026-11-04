extends "res://scripts/emulated_volume.gd"
## Used via `const SplatVolume := preload("res://scripts/splat_volume.gd")` rather
## than a class_name, so a headless run with a stale global class cache still
## resolves it.

## A Gaussian splat scene as a spatial volume (ADR-2170 decisions 1, 2, 6 to 9).
##
## It is an EmulatedVolume under the ADR-2158 contract: bounds, fit rule 5, the
## frame and handles, and a place in the VolumeArrangement, so SpacePortals enters
## it, lays the other volumes out as portals and leaves it with no code of its own.
## Its content is one MultiMeshInstance3D of camera-facing quads (materials/
## splat.gdshader) behind the duck-typed source interface (mount, unmount,
## content_aabb). Four gdext classes do the work off the render thread:
## `SplatLoader` (verify, cache, decode, pack), `SplatSorter` (the CPU depth sort),
## `SplatGovernor` (the tier) and the splat account in `FrameBudget`.
##
## Placement (decision 1, Invariant 8). The volume is added to the arrangement, a
## sibling of GraphRoot under the unit-scale scene root, never a child of a node or of
## GraphRoot: physics must not move it and a moving cloud would re-sort every frame.
## It is world-locked and static: nothing here animates its transform.
##
## Transparency (decision 2). Splats blend per object, so they must not overlap graph
## transparency. Un-entered, the box must not intersect another volume's box
## (`placement_ok`; the volume draws nothing while it does) and it shares the graph's
## transparent `render_priority` (PRIORITY_SHARED = 0, the edge and halo layers' 0 and 1),
## so the per-object distance sort orders whole volumes. Entered, it is the backdrop: a
## `render_priority` below every graph layer and portal (PRIORITY_ENTERED), fitted to
## ROOM_BOUNDS by SpacePortals. Splat content between the user and a portal is
## overdrawn by the portal; that artefact is accepted and named.
##
## Budget (decision 7). `update_splat` states the demand with
## `FrameBudgetPass.set_splat_demand(tier, volumes, entered)` and draws no more than the
## grant (`caps["splat_tier"]`): a volume that is not the entered room is granted tier 1
## at most, however high the governor has climbed. It reports the triangles it drew with
## `set_splat_applied` so the measured primitives are not charged to the graph. The
## tier itself is `SplatGovernor` (the ADR-2167 thresholds), fed the GPU time or the
## frame time `eye_candy.gd` has already established, from this node's `_process` (the
## render frame: `update_splat` runs on the physics tick, whose fixed delta would read as
## a constant frame time) and only while splats are drawing, so a load the splats did not
## cause never lowers or raises their tier; this script runs no probe of its own. The
## governor's floor is tier 1 (splat_governor.rs): only low cost removes the scene. A grant below the loaded tier hides the volume at once (fail closed) and loads
## the smaller variant; a larger grant loads first and swaps at a dim.
##
## Comfort (decision 9). Reduced motion (the default) makes a tier swap, and the first
## appearance, a dim and a snap (FADE_SEC out, the swap while dark, FADE_SEC in); with it
## off the same fades take EASE_SEC in all. Entering and leaving are SpacePortals' own
## dim or ease. `XR_VISUAL_QUALITY=low` hides the volume and withdraws its demand in the
## same call (`set_comfort`).
##
## Credits (decision 10). `credits_line()` is the attribution, the licence link and
## "modified (decimated)", for the HUD's resting line while the scene is drawn.

const EmulatedVolume := preload("res://scripts/emulated_volume.gd")
const SpacePortals := preload("res://scripts/space_portals.gd")
const SPLAT_SHADER := preload("res://materials/splat.gdshader")

const VOLUME_ID := &"splat"
const KIND := &"splat"
## Ring slot (degrees) the volume is added at: behind the user, clear of the three
## graph volumes at -60, 0 and 60.
const RING_YAW := 180.0
## render_priority: shared with the graph's transparent layers when un-entered
## (edge_flow.tres 0; the halos, beads and beams are 1), below every graph layer and
## portal (the membrane is 10, the route 10, the overlay 20) when entered.
const PRIORITY_SHARED := 0
const PRIORITY_ENTERED := -10
## Reduced-motion dim: seconds to go dark, and again to come back (SpacePortals.FADE_SEC).
const FADE_SEC := 0.12
## The eased swap's total length, seconds (ADR-2107's ceiling).
const EASE_SEC := 0.5
## Seconds a listing or blob request may take.
const LISTING_TIMEOUT_SEC := 15.0
const BLOB_TIMEOUT_SEC := 90.0
## Frames between lookups of the comfort node while it is missing.
const COMFORT_RETRY_FRAMES := 30
const LISTING_PATH := "/api/splats"
const CACHE_SUBDIR := "splat-cache"
## Mirrors rust/src/frame_budget.rs (SPLAT_TIER_COUNTS); test_splat_volume.gd checks it
## against FrameBudget.splat_spec() when the library is loaded.
const TIER_COUNTS: Array[int] = [0, 25000, 50000, 100000]
const MAX_TIER: int = 3
const TRIS_PER_SPLAT: int = 2
## Mirrors splat_loader.rs and splat_sort.rs (checked against SplatLoader.layout()).
const TEXELS_PER_SPLAT: int = 4
const DATA_ROW_SPLATS: int = 1024
const INDEX_W: int = 1024

signal loaded(tier: int, splats: int)
signal load_failed(reason: String, refused: bool)
signal credits_changed(line: String)

enum Phase { IDLE, LISTING, LOADING, DOWNLOADING, READY, FAILED }
enum Fade { NONE, OUT, IN }

## The backend connection whose HTTP the loader uses (`make_http`, `http_base`).
var conn: Node = null
## `(url, method) -> PackedStringArray`: the scene's NIP-98 signer for the exact URL.
var auth: Callable = Callable()
## Tests only: `(url: String, done: Callable)`. Replaces the HTTP request; `done` takes
## `(status: int, body: PackedByteArray)`.
var http_override: Callable = Callable()
## Mirrors the comfort node's `visual_comfort_changed(reduced_motion, …)`.
var reduced_motion := true
var phase: Phase = Phase.IDLE
var asset_id := ""
## Tier of the variant now loaded, 0 when none.
var loaded_tier := 0
## The last failure, "" while healthy; `refused` is true for a hash or size refusal.
var failure := ""
var failure_refused := false
var splat_count := 0
## 0..1 dim applied to every splat (the shader's `fade`).
var fade_alpha := 0.0

var _splat_source: SplatSource = null
var _mmi: MultiMeshInstance3D = null
var _material: ShaderMaterial = null
var _quad: QuadMesh = null
var _data_tex: ImageTexture = null
var _index_tex: ImageTexture = null
var _index_h := 1
var _have_order := false
var _loader: RefCounted = null
var _sorter: RefCounted = null
var _governor: RefCounted = null
var _signal_mode := "pending"
var _listing_json := ""
var _plan: Dictionary = {}
var _loading_tier := 0
var _pending: Dictionary = {}          # decoded data waiting for the dim to land
var _pending_tier := 0
var _pending_plan: Dictionary = {}
var _credits := ""
var _credits_shown := ""
var _low_cost := false
var _fade := Fade.NONE
var _fade_t := 0.0
var _granted := 0
var _comfort_node: Node = null
var _comfort_tries := 0
var _budget_pass_obj: Object = null


## The duck-typed content source (contract §3.3): the MultiMeshInstance3D and the
## scene's bounds in splat units. A volume has one source at most.
class SplatSource:
	extends RefCounted

	var bounds := AABB(Vector3(-0.5, -0.5, -0.5), Vector3.ONE)
	var node: Node3D = null
	var mounted := false


	func content_id() -> StringName:
		return &"splat"


	func content_aabb() -> AABB:
		return bounds


	func mount(parent: Node3D) -> void:
		unmount()
		mounted = true
		if node != null:
			parent.add_child(node)


	func unmount() -> void:
		mounted = false
		if node != null and node.get_parent() != null:
			node.get_parent().remove_child(node)


# --- construction -------------------------------------------------------------

func _init() -> void:
	# The arrangement keys on `volume_id` when the volume is added, before the children exist.
	volume_id = VOLUME_ID


func _build_extra() -> void:
	super._build_extra()
	add_to_group(&"xr_splat_volumes")
	_splat_source = SplatSource.new()
	_quad = QuadMesh.new()
	_quad.size = Vector2(2.0, 2.0) # corners at ±1: the shader scales them to the ellipse
	_material = ShaderMaterial.new()
	_material.shader = SPLAT_SHADER
	_material.render_priority = PRIORITY_SHARED
	_material.set_shader_parameter("fade", 0.0)
	_quad.material = _material
	_mmi = MultiMeshInstance3D.new()
	_mmi.name = "Splats"
	_mmi.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	_mmi.visible = false
	_splat_source.node = _mmi
	if ClassDB.class_exists("SplatLoader"):
		_loader = ClassDB.instantiate("SplatLoader")
		_loader.configure(ProjectSettings.globalize_path("user://%s" % CACHE_SUBDIR))
	if ClassDB.class_exists("SplatSorter"):
		_sorter = ClassDB.instantiate("SplatSorter")
	if ClassDB.class_exists("SplatGovernor"):
		_governor = ClassDB.instantiate("SplatGovernor")
	attach(_splat_source)


func _ready() -> void:
	super._ready()
	_bind_comfort()


## Finds the comfort node (spatial_environment.gd, group `xr_visual_environment`), listens
## for its `visual_comfort_changed` and takes its current state. Retried from `_process`
## while it is missing: the environment may join the tree after this volume is built, and
## without it low cost and reduced motion would never arrive.
func _bind_comfort() -> void:
	if _comfort_node != null or not is_inside_tree():
		return
	var env := get_tree().get_first_node_in_group(&"xr_visual_environment")
	if env == null:
		return
	_comfort_node = env
	if env.has_signal("visual_comfort_changed") and not env.visual_comfort_changed.is_connected(set_comfort):
		env.visual_comfort_changed.connect(set_comfort)
	if env.has_method("get_visual_comfort"):
		var st: Dictionary = env.get_visual_comfort()
		set_comfort(bool(st.get("reduced_motion", true)), bool(st.get("low_cost", false)))


func _process(delta: float) -> void:
	if _comfort_node == null:
		_comfort_tries += 1
		if _comfort_tries % COMFORT_RETRY_FRAMES == 1:
			_bind_comfort()
	# Fed here, on the render frame, and only while splats are on screen.
	if is_drawing():
		_feed_governor(delta)


## Wires the volume to the scene: the backend connection (HTTP) and the signer.
func configure(connection: Node, signer: Callable) -> void:
	conn = connection
	auth = signer


# --- comfort ------------------------------------------------------------------

## Comfort switches (spatial_environment.gd `visual_comfort_changed`). Low cost is tier 0
## in this call: the volume hides, the governor reads 0 and the demand is withdrawn from
## the budget pass now, not at the next tick (the ADR-2167 low-cost rule).
func set_comfort(reduced: bool, low_cost: bool) -> void:
	reduced_motion = reduced
	_low_cost = low_cost
	if _governor != null:
		_governor.set_low_cost(low_cost)
	if low_cost:
		_hide_now()
		var p: Object = _pass()
		if p != null:
			p.set_splat_demand(0, 0, false)
			p.set_splat_applied(0, 0)


func is_low_cost() -> bool:
	return _low_cost


# --- the asset ----------------------------------------------------------------

## Fetches `GET /api/splats`, picks the variant for the granted tier and loads it. An empty
## `asset` takes the first headset asset the server lists.
func request_asset(asset := "") -> void:
	asset_id = asset
	failure = ""
	failure_refused = false
	if _loader == null:
		_fail("the splat library is not loaded", false)
		return
	phase = Phase.LISTING
	var url := "%s%s" % [_http_base(), LISTING_PATH]
	_http_get(url, LISTING_TIMEOUT_SEC, _on_listing)


## The listing body, when a caller already has it (the HTTP callback and the tests).
func begin_with_listing(json: String) -> void:
	_listing_json = json
	# READY with nothing loaded: `_manage_tier` starts the load as soon as a tier is granted.
	phase = Phase.READY


func _on_listing(status: int, body: PackedByteArray) -> void:
	if status != 200:
		_fail("splat listing: HTTP %d" % status, false)
		return
	begin_with_listing(body.get_string_from_utf8())


func _start_load(tier: int) -> void:
	if _loader == null or _listing_json.is_empty() or tier <= 0:
		return
	var plan: Dictionary = _loader.plan(_listing_json, asset_id, tier)
	if not bool(plan.get("ok", false)):
		_fail(str(plan.get("error", "no splat variant")), bool(plan.get("refused", false)))
		return
	_plan = plan
	_loading_tier = int(plan["tier"])
	phase = Phase.LOADING
	_loader.begin()


func _poll_loader() -> void:
	if _loader == null or (phase != Phase.LOADING and phase != Phase.DOWNLOADING):
		return
	var st: Dictionary = _loader.poll()
	match str(st.get("state", "idle")):
		"need_download":
			if phase == Phase.LOADING:
				phase = Phase.DOWNLOADING
				var url := "%s%s" % [_http_base(), str(st["url"])]
				_http_get(url, BLOB_TIMEOUT_SEC, _on_blob)
		"ready":
			_on_decoded(_loader.take_data(), _plan)
		"failed":
			_fail(str(st.get("error", "")), bool(st.get("refused", false)))


func _on_blob(status: int, body: PackedByteArray) -> void:
	if status != 200:
		_loader.fail("splat blob: HTTP %d" % status)
		return
	# A SHA-256 or size mismatch is a refusal in the loader: final, never retried.
	phase = Phase.LOADING
	_loader.supply(body)


func _on_decoded(data: Dictionary, plan: Dictionary) -> void:
	_loader.reset()
	var tier: int = int(plan.get("tier", 0))
	# Never draw above the grant: a variant larger than the grant that arrived late is dropped.
	if _pass() != null and tier > _granted:
		# READY, not IDLE: the listing is known, and `_manage_tier` loads the granted variant.
		phase = Phase.READY
		return
	_pending = data
	_pending_plan = plan
	_pending_tier = tier
	if loaded_tier <= 0:
		# First scene: nothing to dim out of, the swap lands now and the dim comes in.
		_land_pending()
		_fade = Fade.IN
		_fade_t = 0.0
	else:
		_begin_swap()


## Lands the decoded scene: textures, MultiMesh, sorter, bounds and credits.
func _land_pending() -> void:
	if _pending.is_empty():
		return
	var data: Dictionary = _pending
	var n: int = int(data["count"])
	var img := Image.create_from_data(int(data["width"]), int(data["height"]), false, Image.FORMAT_RGBAF, data["data"])
	_data_tex = ImageTexture.create_from_image(img)
	_material.set_shader_parameter("splat_data", _data_tex)
	if _sorter != null:
		_sorter.set_positions(data["positions"])
		_index_h = int(_sorter.index_height())
	else:
		_index_h = maxi(1, ceili(float(n) / float(INDEX_W)))
	_index_tex = null # created from the first sorted order
	_have_order = false
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.mesh = _quad
	# The MultiMesh's own transform buffer is never read (the shader takes the content
	# transform as a uniform) and never rewritten: a sort uploads only the index texture.
	mm.instance_count = n
	mm.visible_instance_count = n
	_mmi.multimesh = mm
	var lo: Vector3 = data["bounds_min"]
	var hi: Vector3 = data["bounds_max"]
	var box := AABB(lo, hi - lo)
	# One call, culled as a whole: a splat cloud has no per-instance culling worth a draw call.
	_mmi.custom_aabb = box.grow(0.5)
	_splat_source.bounds = AABB(_plan_vec(_pending_plan, "bounds_min", lo), _plan_vec(_pending_plan, "bounds_max", hi) - _plan_vec(_pending_plan, "bounds_min", lo))
	splat_count = n
	loaded_tier = _pending_tier
	_credits = str(_pending_plan.get("credits", ""))
	_pending = {}
	phase = Phase.READY
	_refit_now = true
	loaded.emit(loaded_tier, n)


func _plan_vec(plan: Dictionary, key: String, fallback: Vector3) -> Vector3:
	var v: Variant = plan.get(key, fallback)
	return v as Vector3 if v is Vector3 else fallback


func _fail(reason: String, refused: bool) -> void:
	failure = reason
	failure_refused = refused
	phase = Phase.FAILED
	push_warning("SplatVolume: %s" % reason)
	load_failed.emit(reason, refused)


# --- state the scene reads ----------------------------------------------------

## True while this volume is the entered room (ADR-2166).
func is_entered() -> bool:
	return portal_room


## The tier the governor asks for: 0 under low cost, else its step.
func wanted_tier() -> int:
	if _low_cost:
		return 0
	if _governor == null:
		return 1
	return int(_governor.tier())


## The tier the volume may draw now: the budget pass's grant, or, with no pass (an
## environment preview), what the governor asks for capped as the account caps it.
func granted_tier() -> int:
	return _granted


## Whether the volume wants a draw call at all: an asset requested and not failed, not low
## cost, visible and open, and clear of the graph volumes.
func wants_draw() -> bool:
	return phase != Phase.IDLE and phase != Phase.FAILED and not _low_cost \
			and visible_state and not is_closed and placement_ok()


## True while the volume draws splats this frame.
func is_drawing() -> bool:
	return _mmi != null and _mmi.visible


## Splats drawn this frame (0 when hidden).
func drawn_splats() -> int:
	return splat_count if is_drawing() else 0


## The credits line while a scene is drawn, else "".
func credits_line() -> String:
	return _credits if (is_drawing() or (loaded_tier > 0 and wants_draw())) else ""


## Draw calls the volume spends outside its frame: one while it draws.
func splat_draw_calls() -> int:
	return 1 if is_drawing() else 0


## Un-entered, the box must not intersect another volume's box (ADR-2170 decision 2).
## Entered, the room has the whole view and nothing to clear. True with no arrangement.
func placement_ok() -> bool:
	if portal_room:
		return true
	var arr := get_parent()
	if arr == null or not arr.has_method("volumes"):
		return true
	for o: Node3D in arr.volumes():
		if o == self or o.is_closed or not o.visible_state:
			continue
		if SpacePortals.boxes_intersect(transform, bounds, o.transform, o.bounds):
			return false
	return true


## The render_priority the volume's material holds now.
func render_priority() -> int:
	return _material.render_priority


# --- per-frame ----------------------------------------------------------------

## Call once per frame from the scene, after `tick` (the fit) and before the budget pass.
func update_splat(delta: float) -> void:
	_poll_loader()
	var p: Object = _pass()
	var drawing_wanted := wants_draw()
	if p != null:
		p.set_splat_demand(wanted_tier() if drawing_wanted else 0, 1 if drawing_wanted else 0, portal_room)
		_granted = mini(int(p.caps.get("splat_tier", 0)), wanted_tier())
	else:
		_granted = mini(wanted_tier(), 1 if not portal_room else MAX_TIER)
	if not drawing_wanted or _granted <= 0:
		_hide_now()
		_report_applied(p)
		return
	_manage_tier()
	_tick_fade(delta)
	_present(delta)
	_report_applied(p)
	_sync_credits()


func _manage_tier() -> void:
	if phase == Phase.READY and loaded_tier > _granted:
		# Fail closed: nothing above the grant draws. The smaller variant loads (the cache
		# usually has it) and lands at the dim.
		_mmi.visible = false
		_have_order = false
		if _loader != null and not _listing_json.is_empty():
			_start_load(_granted)
	elif phase == Phase.READY and loaded_tier < _granted and _fade == Fade.NONE:
		_start_load(_granted)


func _present(delta: float) -> void:
	# Never above the grant: a loaded variant larger than the grant stays hidden until the
	# smaller one has landed.
	if loaded_tier <= 0 or _mmi.multimesh == null or loaded_tier > _granted:
		_mmi.visible = false
		return
	var cam: Camera3D = get_viewport().get_camera_3d() if is_inside_tree() else null
	var xf: Transform3D = content_to_world()
	_material.set_shader_parameter("splat_model", Projection(xf))
	_material.set_shader_parameter("fade", fade_alpha)
	_material.render_priority = PRIORITY_ENTERED if portal_room else PRIORITY_SHARED
	if cam != null:
		_material.set_shader_parameter("near_plane", cam.near)
		if _sorter != null and _sorter.update(cam.global_position, -cam.global_transform.basis.z, xf,
				float(Time.get_ticks_usec()) / 1.0e6):
			var bytes: PackedByteArray = _sorter.take_index_bytes()
			if bytes.size() > 0:
				_upload_index(bytes)
	_mmi.visible = _have_order and fade_alpha > 0.0


func _upload_index(bytes: PackedByteArray) -> void:
	var img := Image.create_from_data(INDEX_W, _index_h, false, Image.FORMAT_RF, bytes)
	if _index_tex == null or _index_tex.get_height() != _index_h:
		_index_tex = ImageTexture.create_from_image(img)
		_material.set_shader_parameter("splat_index", _index_tex)
	else:
		_index_tex.update(img)
	_have_order = true


func _hide_now() -> void:
	if _mmi != null:
		_mmi.visible = false
	_sync_credits()


# The HUD credits line follows what is drawn: emitted on change only.
func _sync_credits() -> void:
	var line := credits_line()
	if line != _credits_shown:
		_credits_shown = line
		credits_changed.emit(line)


func _report_applied(p: Object) -> void:
	if p != null:
		p.set_splat_applied(drawn_splats() * TRIS_PER_SPLAT, splat_draw_calls())


## Fires the swap of the pending variant through the dim: out, land while dark, in.
func _begin_swap() -> void:
	_fade = Fade.OUT
	_fade_t = 0.0


func _fade_half() -> float:
	return FADE_SEC if reduced_motion else EASE_SEC * 0.5


func _tick_fade(delta: float) -> void:
	match _fade:
		Fade.NONE:
			fade_alpha = 1.0 if loaded_tier > 0 else 0.0
		Fade.OUT:
			_fade_t += maxf(delta, 0.0)
			var u := clampf(_fade_t / _fade_half(), 0.0, 1.0)
			fade_alpha = 1.0 - u * u * (3.0 - 2.0 * u)
			if _fade_t >= _fade_half():
				fade_alpha = 0.0
				_land_pending()
				_fade = Fade.IN
				_fade_t = 0.0
		Fade.IN:
			_fade_t += maxf(delta, 0.0)
			var v := clampf(_fade_t / _fade_half(), 0.0, 1.0)
			fade_alpha = v * v * (3.0 - 2.0 * v)
			if _fade_t >= _fade_half():
				fade_alpha = 1.0
				_fade = Fade.NONE


## True while a swap or the first appearance is dimming.
func is_fading() -> bool:
	return _fade != Fade.NONE


# --- governor -----------------------------------------------------------------

# The governor is fed what eye_candy.gd has established: its GPU timer reading, or frame
# time once it has fallen back. No second probe, no second measure flag.
func _feed_governor(delta: float) -> void:
	if _governor == null or _low_cost or delta <= 0.0 or not is_inside_tree():
		return
	var candy: Node = _eye_candy()
	var mode: String = str(candy.get("signal_mode")) if candy != null else "frame"
	if mode == "pending":
		return
	if mode != _signal_mode:
		var first: bool = _signal_mode == "pending"
		_signal_mode = mode
		if mode == "gpu":
			_governor.use_gpu()
		elif first:
			_governor.use_frame(_frame_budget_ms())
		else:
			_governor.switch_to_frame(_frame_budget_ms())
	if mode == "gpu":
		# A zero reading is a late or missing result, never evidence of an idle GPU:
		# eye_candy.gd owns the zero-run fallback to frame time and we follow its mode.
		var ms: float = _gpu_ms()
		if ms > 0.0:
			_governor.feed(ms, delta, portal_room)
	else:
		_governor.feed(delta * 1000.0, delta, portal_room)


func _gpu_ms() -> float:
	return RenderingServer.viewport_get_measured_render_time_gpu(get_viewport().get_viewport_rid())


func _eye_candy() -> Node:
	if _comfort_node != null and _comfort_node.has_method("get_eye_candy"):
		return _comfort_node.get_eye_candy()
	return null


func _frame_budget_ms() -> float:
	var hz := 90.0
	var xr: XRInterface = XRServer.primary_interface
	if xr != null and xr.get_display_refresh_rate() > 0.0:
		hz = xr.get_display_refresh_rate()
	return 1000.0 / hz


# --- ADR-2158 hooks -----------------------------------------------------------

func _on_state_changed() -> void:
	super._on_state_changed()
	if _material != null:
		_material.render_priority = PRIORITY_ENTERED if portal_room else PRIORITY_SHARED


# --- helpers ------------------------------------------------------------------

func _pass() -> Object:
	if _budget_pass_obj != null and is_instance_valid(_budget_pass_obj):
		return _budget_pass_obj
	return null


## The scene's FrameBudgetPass (set by the scene).
func bind_budget(pass_obj: Object) -> void:
	_budget_pass_obj = pass_obj


func _http_base() -> String:
	if conn != null and conn.has_method("http_base"):
		return str(conn.http_base()).rstrip("/")
	return ""


func _http_get(url: String, timeout: float, done: Callable) -> void:
	if http_override.is_valid():
		http_override.call(url, done)
		return
	if conn == null or not conn.has_method("make_http"):
		_fail("no backend connection for %s" % url, false)
		return
	var req: HTTPRequest = conn.make_http("SplatHttp", timeout)
	req.request_completed.connect(func(result: int, code: int, _headers: PackedStringArray, body: PackedByteArray) -> void:
		req.queue_free()
		done.call(code if result == HTTPRequest.RESULT_SUCCESS else 0, body))
	var headers := PackedStringArray()
	if auth.is_valid():
		headers = auth.call(url, "GET")
	var err := req.request(url, headers, HTTPClient.METHOD_GET)
	if err != OK:
		req.queue_free()
		done.call(0, PackedByteArray())
