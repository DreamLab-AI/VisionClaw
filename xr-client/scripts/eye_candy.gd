extends Node3D
## Visual extras for the headset, behind one controller (built by
## spatial_environment.gd, which owns the comfort switches).
##
## Tiers, cheapest first (costs live in rust/src/frame_budget.rs `extras_cost`,
## which the FrameBudget pass pays for as a bounded allowance ahead of the graph's
## near detail: the requested tier, capped at tier 4, costs the graph at most
## ceil(allowance / 288) gems; under pressure nothing is held back and an extra
## can only use what the graph left over. ADR-2167, amended 2026-10-31):
##   0  off
##   1  static observatory sky (a sky shader on a copy of the Environment):
##      0 triangles, 0 draw calls; per-pixel work on background pixels only
##   2  ribbon volume frames (emulated_volume.gd): 24 triangles a frame, in the
##      frame's own draw call (the scene counts it), replacing the 1 px lines
##   3  128 ambient motes: one MultiMesh, 128 triangles, 1 draw call
##   4  512 ambient motes: the same MultiMesh and call, 512 triangles
##
## Comfort. Reduced motion is the default (XR_REDUCED_MOTION) and the extras are
## designed to look right with it on: the sky is static, the ribbons are static,
## the motes hold still. Motion (drift, twinkle) is added only when it is off.
## XR_VISUAL_QUALITY=low turns every extra off (tier 0, GPU timer off). Nothing
## here is a post-process: no glow, no bloom, no SCREEN_TEXTURE, no depth read,
## no SubViewport (XR-client Invariants 1-2).
##
## Headroom is measured, not assumed. A steady 90 FPS on the headset is
## vsync-locked and proves nothing about spare GPU time, so the controller asks
## the renderer for GPU time (RenderingServer.viewport_get_measured_render_time_gpu).
## Probed on the HP the call returns nonzero under gl_compatibility on 4.6.1 and
## 4.8 (perf/gpu_time_probe.gd); the controller still checks at start-up, on
## whatever viewport it is given, and falls back to frame-time p95 if it reads
## zero. The Governor steps one tier at a time with hysteresis (see below).
## Tier is then a request: the FrameBudget pass decides what is granted.
##
## XR_EYE_CANDY: "auto" (default) lets the governor climb; "off" or "0".."4"
## pins the requested tier (still budget-limited, still off under low cost).

const SKY_SHADER := preload("res://materials/eye_candy_sky.gdshader")
const MOTE_SHADER := preload("res://materials/ambient_mote.gdshader")
const EmulatedVolume := preload("res://scripts/emulated_volume.gd")

const MAX_TIER: int = 4
const TIER_SKY: int = 1
const TIER_FRAMES: int = 2
const TIER_MOTES_LOW: int = 3
const TIER_MOTES_HIGH: int = 4
## Mirrors rust/src/frame_budget.rs; checked against FrameBudget.extras_spec()
## by test_eye_candy.gd, and read from it at run time when the library is loaded.
const MOTES_LOW: int = 128
const MOTES_HIGH: int = 512
const FRAME_RIBBON_TRIS: int = 24
## The motes live in this box (metres, scene space): the room around the viewer
## and the graph, not the far field.
const MOTE_BOX_CENTRE := Vector3(0.0, 1.5, -1.6)
const MOTE_BOX_SIZE := Vector3(9.0, 4.5, 9.0)
const MOTE_SIZE_M: float = 0.011
const MOTE_SEED: int = 20261031
## Frames the start-up probe watches before deciding whether the GPU timer works.
const PROBE_FRAMES: int = 120
## How often the demand is renewed and the grant applied.
const APPLY_SEC: float = 0.25
## In GPU mode, this many consecutive zero readings (about 2 s at 90 Hz) mean the
## timer has stopped reporting: the controller falls back to frame time.
const GPU_ZERO_FALLBACK_FRAMES: int = 180
## The fall-back ceiling: frame time can only vouch for the ribbons.
const FRAME_FALLBACK_MAX_TIER: int = 2

signal tier_applied(tier: int)

## The tier the controller currently draws.
var applied_tier: int = 0
## "gpu" (measured GPU ms), "frame" (frame-time p95) or "pending" (still probing).
var signal_mode: String = "pending"

var _scene: Node
var _world_env: WorldEnvironment
var _env: Environment
var _orig_sky: Sky
var _candy_sky: Sky
var _motes: MultiMeshInstance3D
var _mote_material: ShaderMaterial
var _reduced_motion: bool = true
var _low_cost: bool = false
var _forced_tier: int = -1
var _governor: Governor
var _probe_frames: int = 0
var _probe_nonzero: int = 0
var _gpu_zero_run: int = 0
var _apply_accum: float = APPLY_SEC
var _measuring: bool = false
var _viewport_rid: RID
var _last_wanted: int = -1
var _spec: Dictionary = {}
## One allocator-table object for cost lookups (RefCounted; made once in _ready).
var _fb: Object = null


## The tier governor: pure arithmetic over a stream of load samples, so it is
## tested with synthetic series. Samples are GPU ms ("gpu") or frame ms
## ("frame"). Once a window of `window_sec` has filled, its p95 decides:
##   * above `down_ms`: step down at once (never below `min_tier`), and double
##     the climb hold-off (up to BACKOFF_MAX), so a tier that does not fit is
##     not retried quickly;
##   * below `up_ms` for `up_hold_sec x backoff` of consecutive windows: step up
##     one tier;
##   * in between: hold, and restart the climb count.
## The floor is TIER_SKY: the sky shader replaces the ProceduralSky draw, so it
## costs no pass, no triangle and no call, and dropping it would free nothing
## but would swap the whole background in both eyes in one frame. Only low cost
## (`wanted_tier`) and XR_EYE_CANDY=0 take the sky away.
## The hold must exceed the pass's OTHER_PEAK_SEC (2 s) so a new tier's
## triangles have settled in the budget before the next step is judged.
class Governor:
	extends RefCounted

	const BACKOFF_MAX: float = 8.0
	## Seconds without a step down before the hold-off halves.
	const BACKOFF_DECAY_SEC: float = 60.0

	var tier: int = 1
	## The lowest tier the governor will request (the free sky).
	var min_tier: int = 1
	var max_tier: int = 4
	var down_ms: float = 8.0
	var up_ms: float = 5.5
	var window_sec: float = 1.0
	var up_hold_sec: float = 8.0
	var backoff: float = 1.0

	var _samples: PackedFloat32Array = PackedFloat32Array()
	var _elapsed: float = 0.0
	var _calm: float = 0.0
	var _since_down: float = 0.0
	## Steps taken, for tests and the HUD: {"up": n, "down": n}.
	var steps: Dictionary = {"up": 0, "down": 0}


	static func gpu_mode() -> Governor:
		var g := Governor.new()
		g.max_tier = 4
		g.down_ms = 8.0        # PRD-008 §6: GPU per frame ≤ 8 ms p95
		g.up_ms = 5.5
		g.up_hold_sec = 8.0
		return g


	## Frame-time fallback: no GPU timer, so only a missed frame is evidence.
	## Conservative on purpose: climbs to tier 2 at most, slowly.
	static func frame_mode(budget_ms: float) -> Governor:
		var g := Governor.new()
		g.max_tier = 2
		g.down_ms = budget_ms * 1.15
		g.up_ms = budget_ms * 1.03
		g.up_hold_sec = 20.0
		return g


	static func percentile(samples: PackedFloat32Array, q: float) -> float:
		if samples.is_empty():
			return 0.0
		var s: PackedFloat32Array = samples.duplicate()
		s.sort()
		return s[mini(s.size() - 1, int(floor(q * float(s.size()))))]


	## Feed one sample taken `delta` seconds after the last. Returns true when
	## the tier changed.
	func feed(ms: float, delta: float) -> bool:
		_samples.append(ms)
		_elapsed += delta
		if _elapsed < window_sec - 1.0e-4: # 90 samples of 1/90 s sum a hair under 1.0
			return false
		var p95: float = percentile(_samples, 0.95)
		var win: float = _elapsed
		_samples.clear()
		_elapsed = 0.0
		_since_down += win
		if p95 > down_ms:
			_calm = 0.0
			_since_down = 0.0
			backoff = minf(backoff * 2.0, BACKOFF_MAX)
			if tier > min_tier:
				tier -= 1
				steps["down"] = int(steps["down"]) + 1
				return true
			return false
		if _since_down >= BACKOFF_DECAY_SEC and backoff > 1.0:
			backoff = maxf(1.0, backoff * 0.5)
			_since_down = 0.0
		if p95 < up_ms:
			_calm += win
			if _calm >= up_hold_sec * backoff and tier < max_tier:
				_calm = 0.0
				tier += 1
				steps["up"] = int(steps["up"]) + 1
				return true
		else:
			_calm = 0.0
		return false


func _ready() -> void:
	name = "EyeCandy"
	_fb = ClassDB.instantiate("FrameBudget") if ClassDB.class_exists("FrameBudget") else null
	_spec = _read_spec()
	var forced := OS.get_environment("XR_EYE_CANDY").to_lower().strip_edges()
	if forced in ["off", "false"]:
		_forced_tier = 0
	elif forced.is_valid_int():
		_forced_tier = clampi(int(forced), 0, MAX_TIER)
	_build_motes()


## Called by spatial_environment.gd once per scene: finds the WorldEnvironment
## whose sky it swaps and the budget pass it reports to. Safe to skip (previews
## without a graph): the controller then only has what it was given.
func setup(scene: Node) -> void:
	_scene = scene
	_world_env = scene.get_node_or_null("WorldEnvironment") as WorldEnvironment
	if _world_env != null and _world_env.environment != null:
		# Copy-on-write, as spatial_environment.gd does for the node materials:
		# the shared scene resource is never mutated.
		_env = _world_env.environment.duplicate() as Environment
		_world_env.environment = _env
		_orig_sky = _env.sky
	var sky_material := ShaderMaterial.new()
	sky_material.shader = SKY_SHADER
	_candy_sky = Sky.new()
	_candy_sky.sky_material = sky_material
	_candy_sky.radiance_size = Sky.RADIANCE_SIZE_32
	_viewport_rid = get_viewport().get_viewport_rid() if is_inside_tree() else RID()


## Comfort switches (spatial_environment.gd `set_visual_comfort`). Low cost turns
## every extra off; reduced motion stops the motes' drift and twinkle.
func set_comfort(reduced_motion: bool, low_cost: bool) -> void:
	_reduced_motion = reduced_motion
	_low_cost = low_cost
	if _mote_material != null:
		_mote_material.set_shader_parameter("motion", 0.0 if reduced_motion else 1.0)
	_set_measuring(not low_cost)
	if low_cost:
		apply_tier(0)
	else:
		apply_tier(applied_tier)


## The tier the controller would like drawn now: 0 under low cost, the pinned
## tier if XR_EYE_CANDY pins one, else the governor's.
func wanted_tier() -> int:
	if _low_cost:
		return 0
	if _forced_tier >= 0:
		return _forced_tier
	if _governor == null:
		return TIER_SKY
	return _governor.tier


## Draw tier `tier` (clamped, and 0 under low cost). Idempotent.
func apply_tier(tier: int) -> void:
	var t: int = 0 if _low_cost else clampi(tier, 0, MAX_TIER)
	# sky
	if _env != null:
		_env.sky = _candy_sky if t >= TIER_SKY else _orig_sky
	# volume frames
	if EmulatedVolume.ribbon_frames != (t >= TIER_FRAMES):
		EmulatedVolume.set_ribbon_frames(get_tree(), t >= TIER_FRAMES)
	# motes: one MultiMesh, one call; the tier picks how many instances draw
	if _motes != null:
		var n: int = 0
		if t >= TIER_MOTES_HIGH:
			n = _mote_count(true)
		elif t >= TIER_MOTES_LOW:
			n = _mote_count(false)
		_motes.visible = n > 0
		_motes.multimesh.visible_instance_count = n
	var changed: bool = t != applied_tier
	applied_tier = t
	_report_applied()
	if changed:
		tier_applied.emit(t)


## Triangles and draw calls the drawn extras cost now, from the allocator's own
## table (FrameBudget.extras_cost) when the library is loaded.
func applied_cost() -> Vector2i:
	return cost_of(applied_tier, _visible_frames())


func cost_of(tier: int, frames: int) -> Vector2i:
	if _fb != null:
		var c: PackedInt32Array = _fb.extras_cost(tier, frames)
		return Vector2i(int(c[0]), int(c[1]))
	return fallback_cost(tier, frames)


## Library absent (previews): the allocator's table, locally. Ribbons replace
## the 1 px lines in the frame's own call, which the scene counts, so tier 2 adds
## triangles and no call; the motes are one MultiMesh and one call.
static func fallback_cost(tier: int, frames: int) -> Vector2i:
	var tris: int = 0
	var calls: int = 0
	var t: int = clampi(tier, 0, MAX_TIER)
	var f: int = clampi(frames, 0, 8)
	if t >= TIER_FRAMES:
		tris += f * FRAME_RIBBON_TRIS
	if t >= TIER_MOTES_LOW:
		tris += MOTES_HIGH if t >= TIER_MOTES_HIGH else MOTES_LOW
		calls += 1
	return Vector2i(tris, calls)


func _process(delta: float) -> void:
	_probe_or_feed(delta)
	_apply_accum += delta
	if _apply_accum < APPLY_SEC:
		return
	_apply_accum = 0.0
	var want: int = wanted_tier()
	var pass_obj: Object = _budget_pass()
	if pass_obj == null:
		# No budget pass (an environment preview): nothing else shares the frame.
		apply_tier(want)
		return
	pass_obj.set_extras_demand(want, _visible_frames())
	var caps: Dictionary = pass_obj.caps
	# Fail closed: before the first pass, or without the allocator, nothing is
	# granted. Never draw above the request (the grant can lag a step down).
	var granted: int = mini(int(caps.get("extras_tier", 0)), want)
	apply_tier(granted)


func _probe_or_feed(delta: float) -> void:
	if _low_cost or delta <= 0.0:
		return
	if signal_mode == "pending":
		_probe_frames += 1
		if _viewport_rid.is_valid() and RenderingServer.viewport_get_measured_render_time_gpu(_viewport_rid) > 0.0:
			_probe_nonzero += 1
		if _probe_frames >= PROBE_FRAMES:
			_finish_probe()
		return
	if signal_mode == "gpu":
		_feed_gpu(RenderingServer.viewport_get_measured_render_time_gpu(_viewport_rid), delta)
	elif _forced_tier < 0:
		_governor.feed(delta * 1000.0, delta)


## Decide the signal once the probe has watched PROBE_FRAMES frames. GPU time is
## trusted only if most of them read nonzero: one stray reading is not a timer.
func _finish_probe() -> void:
	signal_mode = "gpu" if _probe_nonzero * 2 > _probe_frames else "frame"
	_governor = Governor.gpu_mode() if signal_mode == "gpu" else Governor.frame_mode(_frame_budget_ms())
	_governor.tier = mini(_governor.tier, TIER_SKY)
	_gpu_zero_run = 0


## One GPU-time reading in GPU mode. A zero is a late or missing result, never
## evidence of an idle GPU, so it is not fed. But a timer that stays at zero
## (it never reported on the XR viewport, or it died after a climb) would leave
## the tier unmanaged: after GPU_ZERO_FALLBACK_FRAMES in a row, switch to frame
## time and cap the tier at the ribbons.
func _feed_gpu(sample_ms: float, delta: float) -> void:
	if sample_ms <= 0.0:
		_gpu_zero_run += 1
		if _gpu_zero_run > GPU_ZERO_FALLBACK_FRAMES:
			var held: int = _governor.tier
			signal_mode = "frame"
			_governor = Governor.frame_mode(_frame_budget_ms())
			_governor.tier = clampi(held, _governor.min_tier, FRAME_FALLBACK_MAX_TIER)
			_gpu_zero_run = 0
		return
	_gpu_zero_run = 0
	if _forced_tier < 0:
		_governor.feed(sample_ms, delta)


func _set_measuring(on: bool) -> void:
	if not _viewport_rid.is_valid() and is_inside_tree():
		_viewport_rid = get_viewport().get_viewport_rid()
	if _viewport_rid.is_valid() and on != _measuring:
		RenderingServer.viewport_set_measure_render_time(_viewport_rid, on)
		_measuring = on
		if on:
			signal_mode = "pending"
			_probe_frames = 0
			_probe_nonzero = 0
			_gpu_zero_run = 0


func _frame_budget_ms() -> float:
	var hz: float = 90.0
	var xr: XRInterface = XRServer.primary_interface
	if xr != null and xr.get_display_refresh_rate() > 0.0:
		hz = xr.get_display_refresh_rate()
	return 1000.0 / hz


func _build_motes() -> void:
	var tri := ArrayMesh.new()
	var arrays := []
	arrays.resize(Mesh.ARRAY_MAX)
	# Equilateral triangle, circumradius 2 (incircle radius 1), facing +Z.
	arrays[Mesh.ARRAY_VERTEX] = PackedVector3Array([
		Vector3(0.0, 2.0, 0.0), Vector3(-1.7320508, -1.0, 0.0), Vector3(1.7320508, -1.0, 0.0)])
	tri.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
	_mote_material = ShaderMaterial.new()
	_mote_material.shader = MOTE_SHADER
	_mote_material.set_shader_parameter("motion", 0.0 if _reduced_motion else 1.0)
	tri.surface_set_material(0, _mote_material)
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.use_custom_data = true
	mm.mesh = tri
	var count: int = _mote_count(true)
	mm.instance_count = count
	var rng := RandomNumberGenerator.new()
	rng.seed = MOTE_SEED
	for i: int in count:
		var p := MOTE_BOX_CENTRE + Vector3(
			(rng.randf() - 0.5) * MOTE_BOX_SIZE.x,
			(rng.randf() - 0.5) * MOTE_BOX_SIZE.y,
			(rng.randf() - 0.5) * MOTE_BOX_SIZE.z)
		mm.set_instance_transform(i, Transform3D(Basis.IDENTITY, p))
		mm.set_instance_custom_data(i, Color(rng.randf(), 0.6 + 0.8 * rng.randf(), 0.5 + 0.5 * rng.randf(), 0.0))
	mm.visible_instance_count = 0
	_mote_material.set_shader_parameter("base_size", MOTE_SIZE_M)
	_motes = MultiMeshInstance3D.new()
	_motes.name = "AmbientMotes"
	_motes.multimesh = mm
	_motes.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	# Instances are placed once and drift by a few centimetres in the shader:
	# a fixed box is a correct cull bound, and per-instance culling is not worth
	# a draw call.
	_motes.custom_aabb = AABB(MOTE_BOX_CENTRE - MOTE_BOX_SIZE * 0.5 - Vector3.ONE, MOTE_BOX_SIZE + Vector3.ONE * 2.0)
	_motes.visible = false
	add_child(_motes)


func _mote_count(high: bool) -> int:
	return int(_spec.get("motes_high" if high else "motes_low", MOTES_HIGH if high else MOTES_LOW))


func _read_spec() -> Dictionary:
	if _fb != null and _fb.has_method("extras_spec"):
		return _fb.extras_spec()
	return {"motes_low": MOTES_LOW, "motes_high": MOTES_HIGH, "frame_tris": FRAME_RIBBON_TRIS, "mote_tris": 1}


func _budget_pass() -> Object:
	if _scene == null or not ("_budget" in _scene):
		return null
	var b: Variant = _scene.get("_budget")
	return b as Object


func _visible_frames() -> int:
	var n: int = 0
	if not is_inside_tree():
		return 0
	for v: Node in get_tree().get_nodes_in_group(&"xr_volume_frames"):
		var f: Variant = v.get("frame")
		if f != null and (f as Node3D).visible:
			n += 1
	return n


func _report_applied() -> void:
	var pass_obj: Object = _budget_pass()
	if pass_obj != null and pass_obj.has_method("set_extras_applied"):
		var c: Vector2i = applied_cost()
		pass_obj.set_extras_applied(c.x, c.y)
