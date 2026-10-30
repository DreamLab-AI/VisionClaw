extends Node3D
## Used via `const SpatialVolume := preload("res://scripts/spatial_volume.gd")`
## rather than a class_name, so a headless run with a stale global class
## cache still resolves it.

## The spatial-volume contract (ADR-2158 D1, phase-1 contract §3).
##
## A volume presents one content source inside a box of `bounds` metres. Its
## shape follows `XR_EXT_spatial_container`: bounds, a bounds mode (bounded or
## immersive), and visible, interactable and closed states. Changes are
## requests, answered by a granted signal (`bounds_changed`, …) or a denied one,
## and the runtime may change the bounds at any time (`set_runtime_bounds`).
## Payloads carry an app-level `volume_id` in place of Godot's RID.
##
## This script holds the contract and the shared fit maths. `emulated_volume.gd`
## extends it with the frame, the resize handles and the emulated grant policy.
## No graph volume is ever native (ADR-2158 D2): the native shim only re-hosts
## the arrangement, and calls the static `fit_scale` below for its own fit.
##
## The fit rule (ADR-2158 D5, contract §3.2 E7). With
## `R = Basis(UP, deg_to_rad(undo_yaw_deg))`, `c` the centre of the content's
## full-extent AABB and `s = fit_scale(bounds, (R * aabb).size)`:
##     Content.transform = Transform3D(R.scaled(s), -(R * c) * s)
## yaw first, then measure, then scale, then centre on the volume origin.
## Immersive mode keeps 1:1 (s = 1). The drawn transform eases toward that
## target by FIT_EASE per 90 Hz frame, except that an eased step which would
## put the content outside the box snaps straight to the target. Growth eases;
## a shrink is immediate, so no node centre is ever drawn outside the box
## (contract clarification, recorded in the phase-1 report).

enum BoundsMode { BOUNDED = 0, IMMERSIVE = 1 }

## Axes whose content extent is below this are ignored by the fit.
const MIN_EXTENT := 1.0e-6
## Ease factor per 90 Hz frame toward the fit target (frame-rate independent).
const FIT_EASE := 0.15
## The source AABB is re-read every FIT_RECOMPUTE ticks (≈15 Hz at 90 Hz).
## A grant or runtime bounds change refits at once, whatever the cadence.
const FIT_RECOMPUTE := 6
## Reference rate for FIT_EASE.
const EASE_REFERENCE_HZ := 90.0
## Absolute slack, in metres, for the inside-the-box tests.
const BOX_TOLERANCE := 1.0e-4
## Name of the child that carries the fit transform.
const CONTENT_NODE := "Content"

signal bounds_changed(volume_id: StringName, bounds: Vector3, mode: int)
signal bounds_request_denied(volume_id: StringName, requested: Vector3)
signal bounds_mode_request_denied(volume_id: StringName, mode: int)
signal visible_changed(volume_id: StringName, visible: bool)
signal interactable_changed(volume_id: StringName, interactable: bool)
signal closed(volume_id: StringName)
signal fit_changed(volume_id: StringName, scale: float, centre: Vector3)
signal focus_changed(volume_id: StringName, focused: bool)
signal selection_changed(volume_id: StringName, conn_id: StringName, node_id: int)

var volume_id: StringName
var bounds: Vector3 = Vector3.ONE
var bounds_mode: int = BoundsMode.BOUNDED
var visible_state := true
var interactable := true
var is_closed := false
## The Content child. Valid after `instantiate()`, `add_child`, or the first
## call into this API.
var content: Node3D
## Yaw (degrees about +Y) applied on Content to cancel a per-population yaw the
## stream may carry (ADR-2158 D9). Setting it refits at once.
var undo_yaw_deg := 0.0:
	set(value):
		undo_yaw_deg = value
		_refit_now = true

var focused := false
var selected_conn: StringName = &""
var selected_node := -1
## True while a manual transform holds authority: tick neither refits nor
## eases. Any bounds or mode change un-latches it.
var manual_latched := false

var _source: Object = null
var _fit_frame := 0
var _refit_now := true
var _has_target := false
var _target_scale := 1.0
var _target_centre := Vector3.ZERO
var _fit_aabb := AABB()
var _has_current := false
var _cur_scale := 1.0
var _cur_centre := Vector3.ZERO

var _resize_handle := -1
var _resize_start_local := Vector3.ZERO
var _resize_start_bounds := Vector3.ONE


# --- fit maths ---------------------------------------------------------------

## Uniform scale that fits `content_extent` inside `bounds`: the minimum of
## `bounds[i] / content_extent[i]` over the axes whose extent is at least
## MIN_EXTENT, or 1.0 when no axis qualifies (a single point or empty content).
## Pure; the native shim calls it for the arrangement fit.
static func fit_scale(bounds: Vector3, content_extent: Vector3) -> float:
	var s := INF
	for i: int in 3:
		if content_extent[i] >= MIN_EXTENT:
			s = minf(s, bounds[i] / content_extent[i])
	return 1.0 if s == INF else s


## The undo-yaw basis R.
func yaw_basis() -> Basis:
	return Basis(Vector3.UP, deg_to_rad(undo_yaw_deg))


## Sets the fit target from `content_aabb` (graph units, full extent).
## Immersive mode targets scale 1.0.
func fit_into(content_aabb: AABB) -> void:
	_ensure_built()
	var r := yaw_basis()
	var s := 1.0
	if bounds_mode == BoundsMode.BOUNDED:
		s = fit_scale(bounds, (Transform3D(r, Vector3.ZERO) * content_aabb).size)
	var c := content_aabb.get_center()
	var changed := not _has_target \
		or absf(s - _target_scale) > 1.0e-6 * maxf(1.0, absf(_target_scale)) \
		or not c.is_equal_approx(_target_centre)
	_target_scale = s
	_target_centre = c
	_fit_aabb = content_aabb
	_has_target = true
	_refit_now = false
	if changed:
		fit_changed.emit(volume_id, s, c)


## The transform Content eases toward.
func target_content_transform() -> Transform3D:
	return _content_xform(_target_scale, _target_centre)


func target_scale() -> float:
	return _target_scale


## The scale Content is drawn at this frame.
func current_scale() -> float:
	return _cur_scale if _has_current else _target_scale


func _content_xform(s: float, c: Vector3) -> Transform3D:
	var r := yaw_basis()
	return Transform3D(r.scaled(Vector3.ONE * s), -(r * c) * s)


# --- per-frame ---------------------------------------------------------------

## Called by the scene once per frame. Re-reads the source AABB every
## FIT_RECOMPUTE ticks (or at once after a grant) and eases Content. A closed,
## hidden or manually latched volume does nothing.
func tick(delta: float) -> void:
	if is_closed or not visible_state or manual_latched:
		return
	_ensure_built()
	_fit_frame += 1
	if _source != null and (_refit_now or not _has_target or _fit_frame % FIT_RECOMPUTE == 0):
		fit_into(_source.content_aabb())
	elif _refit_now and _has_target:
		fit_into(_fit_aabb)
	if not _has_target:
		return
	if not _has_current:
		_cur_scale = _target_scale
		_cur_centre = _target_centre
		_has_current = true
	else:
		var a := FIT_EASE
		if delta > 0.0:
			a = clampf(1.0 - pow(1.0 - FIT_EASE, delta * EASE_REFERENCE_HZ), 0.0, 1.0)
		var s := lerpf(_cur_scale, _target_scale, a)
		var c := _cur_centre.lerp(_target_centre, a)
		if bounds_mode == BoundsMode.BOUNDED and not _aabb_inside(_content_xform(s, c), _fit_aabb):
			s = _target_scale
			c = _target_centre
		_cur_scale = s
		_cur_centre = c
	content.transform = _content_xform(_cur_scale, _cur_centre)


## True when the 8 corners of `box`, under `xf`, lie inside the volume box.
func _aabb_inside(xf: Transform3D, box: AABB) -> bool:
	var half := bounds * 0.5
	for i: int in 8:
		var p: Vector3 = xf * box.get_endpoint(i)
		for k: int in 3:
			if absf(p[k]) > half[k] + BOX_TOLERANCE + half[k] * 1.0e-5:
				return false
	return true


# --- requests ----------------------------------------------------------------

## Asks for new bounds (full extents, metres). The grant policy is
## `_policy_bounds`; a closed volume or a non-finite or non-positive axis is
## denied.
func request_bounds(b: Vector3) -> void:
	if is_closed or not _valid_bounds(b):
		bounds_request_denied.emit(volume_id, b)
		return
	_apply_bounds(_policy_bounds(b), bounds_mode)


func request_bounds_mode(m: int) -> void:
	if is_closed or (m != BoundsMode.BOUNDED and m != BoundsMode.IMMERSIVE):
		bounds_mode_request_denied.emit(volume_id, m)
		return
	_apply_bounds(bounds, m)


func request_visible(v: bool) -> void:
	if is_closed or v == visible_state:
		return
	visible_state = v
	visible = v
	if v:
		_refit_now = true
	_on_state_changed()
	visible_changed.emit(volume_id, v)


## Addition to the contract: the interactable state has a signal but no
## request in §3.2.
func request_interactable(i: bool) -> void:
	if is_closed or i == interactable:
		return
	interactable = i
	if not i:
		end_resize()
	_on_state_changed()
	interactable_changed.emit(volume_id, i)


## Runtime-driven change (native shim or resize handle). Not subject to the
## grant policy; a non-finite or non-positive axis, or an unknown mode, is
## ignored. Un-latches a manual fit and refits at once.
func set_runtime_bounds(b: Vector3, m: int) -> void:
	if is_closed or not _valid_bounds(b) or (m != BoundsMode.BOUNDED and m != BoundsMode.IMMERSIVE):
		return
	_apply_bounds(b, m)


## Addition to the contract: closes the volume for good. Ends a resize,
## detaches the source, hides the volume and emits `closed`. Every later
## request is denied or ignored and `tick` does nothing.
func close() -> void:
	if is_closed:
		return
	end_resize()
	detach()
	visible_state = false
	visible = false
	is_closed = true
	_on_state_changed()
	closed.emit(volume_id)


func _apply_bounds(b: Vector3, m: int) -> void:
	_ensure_built()
	bounds = b
	bounds_mode = m
	manual_latched = false
	if _source != null:
		fit_into(_source.content_aabb())
	elif _has_target:
		fit_into(_fit_aabb)
	_on_state_changed()
	bounds_changed.emit(volume_id, bounds, bounds_mode)


func _valid_bounds(b: Vector3) -> bool:
	for k: int in 3:
		if not is_finite(b[k]) or b[k] <= 0.0:
			return false
	return true


## Grant policy for `request_bounds`; the base grants as asked.
func _policy_bounds(b: Vector3) -> Vector3:
	return b


## Hook for subclasses: bounds, mode, visibility, focus or interactable changed.
func _on_state_changed() -> void:
	pass


# --- content -----------------------------------------------------------------

## Attaches a content source (duck-typed, contract §3.3). At most one source:
## attaching replaces the current one. Refits at once.
func attach(source: Object) -> void:
	if is_closed or source == null:
		return
	_ensure_built()
	if _source != null:
		detach()
	_source = source
	_source.mount(content)
	_has_current = false
	_refit_now = true
	fit_into(_source.content_aabb())


func detach() -> void:
	if _source == null:
		return
	_source.unmount()
	_source = null


func source() -> Object:
	return _source


## Graph units → world, through the drawn Content transform.
func world_from_content(p: Vector3) -> Vector3:
	_ensure_built()
	return _content_world() * p


## True when world point `p` lies inside the box (always true in immersive
## mode, which has no box).
func contains_world(p: Vector3, tol := 1.0e-4) -> bool:
	if bounds_mode == BoundsMode.IMMERSIVE:
		return true
	var local := _self_world().affine_inverse() * p
	var half := bounds * 0.5
	for k: int in 3:
		if absf(local[k]) > half[k] + tol:
			return false
	return true


func _self_world() -> Transform3D:
	return global_transform if is_inside_tree() else transform


func _content_world() -> Transform3D:
	if content == null:
		return _self_world()
	return content.global_transform if content.is_inside_tree() else transform * content.transform


# --- resize ------------------------------------------------------------------

## Corner index → unit corner sign. Bit 0 is +X, bit 1 +Y, bit 2 +Z.
static func corner_sign(handle: int) -> Vector3:
	return Vector3(
		1.0 if handle & 1 else -1.0,
		1.0 if handle & 2 else -1.0,
		1.0 if handle & 4 else -1.0)


## Addition to the contract: the corner handle within `radius` metres of world
## point `p`, or -1. The scene picks by distance, not physics.
func handle_at(p: Vector3, radius: float) -> int:
	if is_closed or not visible_state or not interactable or bounds_mode != BoundsMode.BOUNDED:
		return -1
	var local := _self_world().affine_inverse() * p
	var best := -1
	var best_d := radius
	for i: int in 8:
		var d := local.distance_to(corner_sign(i) * bounds * 0.5)
		if d <= best_d:
			best_d = d
			best = i
	return best


## Starts a resize from corner `handle` held by `ctrl`. The opposite corner
## stays put relative to the volume origin: the box grows symmetrically by
## twice the hand's travel along each axis.
func begin_resize(handle: int, ctrl: Node3D) -> void:
	if is_closed or not interactable or bounds_mode != BoundsMode.BOUNDED \
			or handle < 0 or handle > 7 or ctrl == null:
		return
	_resize_handle = handle
	_resize_start_local = _self_world().affine_inverse() * _node_world(ctrl).origin
	_resize_start_bounds = bounds


func update_resize(ctrl: Node3D) -> void:
	if _resize_handle < 0 or ctrl == null:
		return
	var local := _self_world().affine_inverse() * _node_world(ctrl).origin
	var sgn := corner_sign(_resize_handle)
	var b := _resize_start_bounds + 2.0 * sgn * (local - _resize_start_local)
	set_runtime_bounds(_policy_bounds(b.max(Vector3.ONE * MIN_EXTENT)), bounds_mode)


func end_resize() -> void:
	_resize_handle = -1


func is_resizing() -> bool:
	return _resize_handle >= 0


func _node_world(n: Node3D) -> Transform3D:
	return n.global_transform if n.is_inside_tree() else n.transform


# --- focus and selection -----------------------------------------------------

func set_focused(f: bool) -> void:
	if f == focused:
		return
	focused = f
	_on_state_changed()
	focus_changed.emit(volume_id, f)


## Selects wire id `node_id` from connection `conn_id`; -1 clears.
func select(conn_id: StringName, node_id: int) -> void:
	selected_conn = conn_id if node_id >= 0 else &""
	selected_node = node_id
	selection_changed.emit(volume_id, conn_id, node_id)


# --- construction ------------------------------------------------------------

func _notification(what: int) -> void:
	if what == NOTIFICATION_SCENE_INSTANTIATED:
		_ensure_built()


func _ready() -> void:
	_ensure_built()


## Resolves (or creates) the children. Lazy, so the API works after
## `instantiate()` or `new()` and before `add_child`.
func _ensure_built() -> void:
	if content != null:
		return
	content = get_node_or_null(CONTENT_NODE) as Node3D
	if content == null:
		content = Node3D.new()
		content.name = CONTENT_NODE
		add_child(content)
	_build_extra()


## Hook for subclasses to build their own children once.
func _build_extra() -> void:
	pass
