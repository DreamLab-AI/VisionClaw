extends RefCounted
## Test double for Godot 4.8's OpenXRSpatialContainerExtension (owner: agent C).
## Used via preload from tests/unit/test_native_container_shim.gd.
##
## It mirrors the bound API at engine commit cafc17a19
## (modules/openxr/extensions/spatial_container/openxr_spatial_container_extension.cpp:67-82):
## is_enabled, is_spatial_container_active, get_supported_bounds_modes,
## get_spatial_container_state, get_spatial_container_bounds,
## request_spatial_container_bounds_mode, request_spatial_container_visible,
## and the six spatial_container_* signals with their argument lists.
##
## Signals are added with add_user_signal at construction, so one fake serves
## both spellings: VARIANT_MASTER uses Godot master's names, VARIANT_ANDROID the
## Android XR guide's (…_bounds_mode_changed, …_visibility_changed,
## …_interactability_changed). The Android argument lists are not published in
## any engine source we can read; the fake gives bounds_mode_changed
## (rid, bounds_mode, bounds) to prove the shim does not depend on arity.
##
## Also holds two in-file doubles the shim's tests need, so this agent touches
## no other agent's fixture: FitScaleStub (the signature of agent B's
## SpatialVolume.fit_scale, recording its calls and returning a canned value,
## so the tests prove delegation and the maths is never copied) and
## FakeArrangement (VolumeArrangement's §3.4 surface the shim uses).

const VARIANT_MASTER := "master"
const VARIANT_ANDROID := "android"

## Godot OpenXRSpatialContainerState.BoundsMode values.
const BOUNDS_MODE_BOUNDED := 0
const BOUNDS_MODE_IMMERSIVE := 1

var variant := VARIANT_MASTER
var enabled := true
var active := false
var bounds := Vector3(0.5, 0.5, 0.5)
var bounds_mode := BOUNDS_MODE_BOUNDED
var interactable := true
var visible := true
var supported_modes: Array = [BOUNDS_MODE_BOUNDED, BOUNDS_MODE_IMMERSIVE]
var requests: Array = []   # [[method, arg], …]
var rid := RID()

var sig_bounds := ""
var sig_mode_denied := "spatial_container_bounds_mode_request_denied"
var sig_closed := "spatial_container_closed"
var sig_interactable := ""
var sig_visible := ""
var sig_visible_denied := "spatial_container_visible_request_denied"


func _init(p_variant: String = VARIANT_MASTER) -> void:
	variant = p_variant
	var rid_arg := {"name": "spatial_container_rid", "type": TYPE_RID}
	if variant == VARIANT_ANDROID:
		sig_bounds = "spatial_container_bounds_mode_changed"
		sig_visible = "spatial_container_visibility_changed"
		sig_interactable = "spatial_container_interactability_changed"
		add_user_signal(sig_bounds, [rid_arg,
			{"name": "bounds_mode", "type": TYPE_INT},
			{"name": "bounds", "type": TYPE_VECTOR3}])
		add_user_signal(sig_visible, [rid_arg, {"name": "visible", "type": TYPE_BOOL}])
		add_user_signal(sig_interactable, [rid_arg, {"name": "interactable", "type": TYPE_BOOL}])
	else:
		sig_bounds = "spatial_container_bounds_changed"
		sig_visible = "spatial_container_visible_changed"
		sig_interactable = "spatial_container_interactable_changed"
		add_user_signal(sig_bounds, [rid_arg,
			{"name": "spatial_container_infinite_bounds", "type": TYPE_BOOL},
			{"name": "spatial_container_bounds_mode", "type": TYPE_INT},
			{"name": "spatial_container_bounds", "type": TYPE_VECTOR3}])
		add_user_signal(sig_visible, [rid_arg, {"name": "spatial_container_visible", "type": TYPE_BOOL}])
		add_user_signal(sig_interactable, [rid_arg, {"name": "spatial_container_interactable", "type": TYPE_BOOL}])
	add_user_signal(sig_mode_denied, [rid_arg])
	add_user_signal(sig_closed, [rid_arg])
	add_user_signal(sig_visible_denied, [rid_arg])


# --- bound API (names as at cafc17a19) ---

func is_enabled() -> bool:
	return enabled


func is_spatial_container_active() -> bool:
	return enabled and active


func get_supported_bounds_modes() -> Array:
	return supported_modes.duplicate()


func get_spatial_container_state() -> Object:
	if not is_spatial_container_active():
		return null
	var st := FakeState.new()
	st.bounds_mode = bounds_mode
	st.interactable = interactable
	st.visible = visible
	return st


func get_spatial_container_bounds() -> Vector3:
	return bounds if is_spatial_container_active() else Vector3.ZERO


func request_spatial_container_bounds_mode(p_mode: int) -> bool:
	requests.append(["request_spatial_container_bounds_mode", p_mode])
	return is_spatial_container_active() and supported_modes.has(p_mode)


func request_spatial_container_visible(p_visible: bool) -> bool:
	requests.append(["request_spatial_container_visible", p_visible])
	return is_spatial_container_active()


# --- event drivers (what the engine does in on_event_polled) ---

func emit_bounds_changed(p_bounds: Vector3, p_mode: int, p_infinite := false) -> void:
	bounds = p_bounds
	bounds_mode = p_mode
	if variant == VARIANT_ANDROID:
		emit_signal(sig_bounds, rid, p_mode, p_bounds)
	else:
		emit_signal(sig_bounds, rid, p_infinite, p_mode, p_bounds)


func emit_visible_changed(p_visible: bool) -> void:
	visible = p_visible
	emit_signal(sig_visible, rid, p_visible)


func emit_interactable_changed(p_interactable: bool) -> void:
	interactable = p_interactable
	emit_signal(sig_interactable, rid, p_interactable)


## The engine emits closed, then destroys the container (cpp:220-228).
func emit_closed() -> void:
	emit_signal(sig_closed, rid)
	active = false


func connection_count() -> int:
	var n := 0
	for s: Dictionary in get_signal_list():
		n += get_signal_connection_list(s.name).size()
	return n


class FakeState:
	extends RefCounted
	var bounds_mode := 0
	var interactable := true
	var visible := true

	func get_bounds_mode() -> int:
		return bounds_mode

	func is_interactable() -> bool:
		return interactable

	func is_visible() -> bool:
		return visible


## Same signature as SpatialVolume.fit_scale (contract §3.2). Records each call
## and returns `result`, so a test can tell the shim delegated.
class FitScaleStub:
	static var calls: Array = []
	static var result := 0.25

	static func reset(p_result := 0.25) -> void:
		calls = []
		result = p_result

	static func fit_scale(p_bounds: Vector3, content_extent: Vector3) -> float:
		calls.append([p_bounds, content_extent])
		return result


## VolumeArrangement's surface used by the shim (contract §3.4). The AABB it
## reports is the world AABB, so it scales with the arrangement: a shim that
## measured after scaling would see the shrunken box.
class FakeArrangement:
	extends Node3D
	var base_aabb := AABB(Vector3(-1.6, 0.8, -1.6), Vector3(3.2, 1.0, 1.6))
	var arr_scale := 1.0
	var scale_calls: Array = []
	var reset_calls := 0
	var aabb_calls: Array = []   # arr_scale at each arrangement_aabb() call

	func arrangement_aabb() -> AABB:
		aabb_calls.append(arr_scale)
		return AABB(base_aabb.position * arr_scale, base_aabb.size * arr_scale)

	func set_arrangement_scale(s: float) -> void:
		arr_scale = s
		scale_calls.append(s)

	func reset_default() -> void:
		reset_calls += 1
