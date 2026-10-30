extends Node
## Used via `const NativeContainerShim := preload("res://scripts/native_container_shim.gd")`
## rather than a class_name, so a headless run with a stale global class cache
## still resolves it (the backend_caps.gd rule).

## Native spatial-container shim (ADR-2158 decision 2, XR-PHASE1-CONTRACT §4).
##
## Godot 4.8 gives an application **one main** OpenXR spatial container
## (`XR_EXT_spatial_container`). The native arm puts the whole emulated volume
## arrangement inside it: the container's bounds and bounds mode drive the
## arrangement's fit, the way a volume's bounds drive its content. Nothing here
## creates a volume, and no graph volume is ever itself native.
##
## The engine does the re-hosting. On session creation Godot creates the
## container and makes its space the custom play space (bounded mode), or keeps
## the normal play space (immersive), so the XR origin, and with it the
## arrangement, already lives in container space. This shim only decides
## whether the arm runs, fits the arrangement and falls back.
##
## This is the ONLY file that names Godot's spatial-container classes, signals
## and enum values. Every 4.8 name is reached by string (ClassDB,
## Engine.get_singleton, has_signal), never statically, so it parses on 4.6.1.
## Names verified against the engine source at cafc17a192acf67903910a68aeb3c076c0907a4b
## (4.8-beta2): modules/openxr/extensions/spatial_container/
## openxr_spatial_container_extension.cpp:67-82 (bindings and signals),
## openxr_spatial_container_state.cpp:42-47 (BoundsMode), register_types.cpp:236-240.
##
## Fallback is silent in the engine. Without XR_EXT_spatial_container and its
## self-rendering companion, is_enabled() is false and the session runs on the
## ordinary play space (openxr_spatial_container_extension.cpp:54-65,139-147).
## So the arm must ask, and it falls back to plain emulated volumes, recording
## why, on every condition ADR-2158 decision 2 names. Fallback is one-shot:
## the arrangement goes back to scale 1 on its ring default and every
## connection is left alone.

## VisionClaw's switch for the arm. Absent from project.godot in phase 1, so off.
const SETTING := "visionclaw/xr/native_spatial_container"
## Godot's own switch; read by the engine when OpenXR initialises.
const GODOT_SETTING := "xr/openxr/extensions/spatial_container/enabled"
const ACTIVATE_TIMEOUT_SEC := 3.0

## B's script that owns the fit maths (`static func fit_scale`). Loaded at run
## time, never preloaded: the shim must not copy the maths, and must not fail
## to parse while that file is absent.
const SPATIAL_VOLUME_PATH := "res://scripts/spatial_volume.gd"

const EXT_CLASS := "OpenXRSpatialContainerExtension"
const MIN_ENGINE_MAJOR := 4
const MIN_ENGINE_MINOR := 8

## Bounds-mode values in the three vocabularies. App values are
## SpatialVolume.BoundsMode (contract §1); they are spelt out here because B's
## enum cannot be referenced statically from a file that must parse without it.
const APP_BOUNDED := 0
const APP_IMMERSIVE := 1
## Godot: OpenXRSpatialContainerState.BoundsMode (openxr_spatial_container_state.cpp:46-47).
const GODOT_BOUNDS_MODE_BOUNDED := 0
const GODOT_BOUNDS_MODE_IMMERSIVE := 1
## OpenXR spec: XrSpatialContainerBoundsModeEXT (BOUNDED_EXT = 1, IMMERSIVE_EXT = 2).
const SPEC_BOUNDS_MODE_BOUNDED := 1
const SPEC_BOUNDS_MODE_IMMERSIVE := 2

const _GODOT_TO_APP := {
	GODOT_BOUNDS_MODE_BOUNDED: APP_BOUNDED,
	GODOT_BOUNDS_MODE_IMMERSIVE: APP_IMMERSIVE,
}
const _SPEC_TO_APP := {
	SPEC_BOUNDS_MODE_BOUNDED: APP_BOUNDED,
	SPEC_BOUNDS_MODE_IMMERSIVE: APP_IMMERSIVE,
}

## Signal names: Godot master first, then the Android XR guide's spelling.
const SIGNALS_BOUNDS: Array[String] = ["spatial_container_bounds_changed", "spatial_container_bounds_mode_changed"]
const SIGNALS_VISIBLE: Array[String] = ["spatial_container_visible_changed", "spatial_container_visibility_changed"]
const SIGNALS_INTERACTABLE: Array[String] = ["spatial_container_interactable_changed", "spatial_container_interactability_changed"]
const SIGNALS_CLOSED: Array[String] = ["spatial_container_closed"]

## Reasons, in the order evaluate() tests them.
const REASON_SETTING_OFF := "setting_off"
const REASON_NOT_ANDROID := "not_android"
const REASON_GODOT_SETTING_OFF := "godot_setting_off"
const REASON_ENGINE_LT_4_8 := "engine_lt_4_8"
const REASON_CLASS_MISSING := "class_missing"
const REASON_NO_INSTANCE := "no_instance"
const REASON_EXTENSION_DISABLED := "extension_disabled"
const REASON_NATIVE := "native"
## Reasons for a fallback after start().
const REASON_CONTAINER_INACTIVE := "container_inactive"
const REASON_CLOSED := "closed"
const REASON_CONTAINER_LOST := "container_lost"   # was active, then destroyed without a closed event (state loss)
const REASON_FIT_UNAVAILABLE := "fit_unavailable" # SpatialVolume.fit_scale could not be resolved
const REASON_NOT_STARTED := "not_started"         # start() called on an emulated decision

const MODE_NATIVE := "native"
const MODE_EMULATED := "emulated"

signal mode_decided(mode: String, reason: String)    # "native" | "emulated"
signal fallback(reason: String)                       # native → emulated during the session
signal container_bounds_changed(bounds: Vector3, app_mode: int)

## Current decision; empty until evaluate().
var mode := ""
var reason := ""
## The script whose static fit_scale(bounds, content_extent) -> float fits the
## arrangement. Null resolves SPATIAL_VOLUME_PATH at start(); tests inject a stub.
var fit_script: Script = null
## Overridable so tests can model the path being absent.
var spatial_volume_path := SPATIAL_VOLUME_PATH
## Last state reported by the container.
var container_visible := true
var container_interactable := true
var container_bounds := Vector3.ZERO
var container_app_mode := APP_BOUNDED
var arrangement_scale := 1.0

var _ext: Object = null
var _arrangement: Node3D = null
var _started := false
var _fallen_back := false
var _waiting_for_active := false
var _was_active := false
var _elapsed := 0.0
var _connected: Array[Dictionary] = []   # [{signal: String, callable: Callable}]


## Decides native or emulated. Pure given `env`: each key overrides the real
## value it names, an absent key reads the engine. Keys: setting, is_android,
## godot_setting (bool), engine_version (Engine.get_version_info shape),
## class_exists (bool), ext (Object or null: the extension instance).
## Returns {mode, reason} and emits mode_decided once per call.
func evaluate(env: Dictionary = {}) -> Dictionary:
	_ext = null
	var result := _decide(env)
	mode = result.mode
	reason = result.reason
	mode_decided.emit(mode, reason)
	return result


func _decide(env: Dictionary) -> Dictionary:
	var setting := bool(env.get("setting", ProjectSettings.get_setting(SETTING, false)))
	if not setting:
		return _emulated(REASON_SETTING_OFF)
	var is_android := bool(env.get("is_android", OS.has_feature("android")))
	if not is_android:
		return _emulated(REASON_NOT_ANDROID)
	var godot_setting := bool(env.get("godot_setting", ProjectSettings.get_setting(GODOT_SETTING, false)))
	if not godot_setting:
		return _emulated(REASON_GODOT_SETTING_OFF)
	var version: Dictionary = env.get("engine_version", Engine.get_version_info())
	if not engine_at_least_4_8(version):
		return _emulated(REASON_ENGINE_LT_4_8)
	var class_exists := bool(env.get("class_exists", ClassDB.class_exists(EXT_CLASS)))
	if not class_exists:
		return _emulated(REASON_CLASS_MISSING)
	var ext: Object = env.get("ext") if env.has("ext") else resolve_extension()
	if ext == null:
		return _emulated(REASON_NO_INSTANCE)
	if not ext.has_method("is_enabled") or not bool(ext.call("is_enabled")):
		return _emulated(REASON_EXTENSION_DISABLED)
	_ext = ext
	return {"mode": MODE_NATIVE, "reason": REASON_NATIVE}


static func _emulated(why: String) -> Dictionary:
	return {"mode": MODE_EMULATED, "reason": why}


## True for 4.8 and later. A missing key reads as 0.
static func engine_at_least_4_8(version: Dictionary) -> bool:
	var major := int(version.get("major", 0))
	var minor := int(version.get("minor", 0))
	return major > MIN_ENGINE_MAJOR or (major == MIN_ENGINE_MAJOR and minor >= MIN_ENGINE_MINOR)


## The extension instance, or null. Godot registers it as an Engine singleton
## (register_types.cpp:138,237), but only when OpenXR is enabled for the run, so
## `--xr-mode off` leaves the class registered and the singleton absent. The
## static get_singleton() is not bound at 4.8-beta2; the second branch is kept
## for a later engine that binds it.
static func resolve_extension() -> Object:
	if Engine.has_singleton(EXT_CLASS):
		return Engine.get_singleton(EXT_CLASS)
	if ClassDB.class_exists(EXT_CLASS) and ClassDB.class_has_method(EXT_CLASS, "get_singleton") \
			and ClassDB.has_method("class_call_static"):
		var inst: Variant = ClassDB.call("class_call_static", EXT_CLASS, "get_singleton")
		if inst is Object:
			return inst
	return null


## Starts the native arm around an arrangement that already exists (the
## emulated volumes are always built first). Does nothing on an emulated
## decision. `arrangement` must offer arrangement_aabb(), set_arrangement_scale()
## and reset_default() (VolumeArrangement, contract §3.4).
func start(arrangement: Node3D) -> void:
	_arrangement = arrangement
	if mode != MODE_NATIVE or _ext == null:
		if mode == MODE_NATIVE:
			_fall_back(REASON_NOT_STARTED)
		return
	if _started:
		return
	_started = true
	if not _resolve_fit():
		_fall_back(REASON_FIT_UNAVAILABLE)
		return
	_connect_any(SIGNALS_BOUNDS, _on_bounds_signal)
	_connect_any(SIGNALS_VISIBLE, _on_visible_signal)
	_connect_any(SIGNALS_INTERACTABLE, _on_interactable_signal)
	_connect_any(SIGNALS_CLOSED, _on_closed_signal)
	_elapsed = 0.0
	_waiting_for_active = true
	_poll_active()


## Per frame. Runs the activation timeout and notices a container that has gone
## away without a closed event. One method call per frame while native; nothing
## at all once emulated.
func tick(delta: float) -> void:
	if not _started or _fallen_back:
		return
	if _waiting_for_active:
		if _poll_active():
			return
		_elapsed += delta
		if _elapsed >= ACTIVATE_TIMEOUT_SEC:
			_fall_back(REASON_CONTAINER_INACTIVE)
		return
	if not _is_active():
		_fall_back(REASON_CONTAINER_LOST)


func is_native() -> bool:
	return mode == MODE_NATIVE and _started and not _fallen_back and not _waiting_for_active


func has_fallen_back() -> bool:
	return _fallen_back


## Names of the extension signals start() connected (for logs and tests).
func connected_signals() -> PackedStringArray:
	var out := PackedStringArray()
	for c: Dictionary in _connected:
		out.append(c.signal)
	return out


## Godot BoundsMode → app enum, by table. -1 for a value the table lacks.
static func to_app_mode(godot_mode: int) -> int:
	return int(_GODOT_TO_APP.get(godot_mode, -1))


## OpenXR spec XrSpatialContainerBoundsModeEXT → app enum. -1 if unknown.
static func from_spec_mode(spec_mode: int) -> int:
	return int(_SPEC_TO_APP.get(spec_mode, -1))


## The HUD notice A shows on fallback (contract §7.3): text in debug builds,
## "" in release. Call as `notice_for(reason, OS.is_debug_build())`.
static func notice_for(why: String, debug: bool) -> String:
	if not debug:
		return ""
	return "Native spatial container unavailable (%s): using emulated volumes" % why


func _resolve_fit() -> bool:
	if fit_script == null and ResourceLoader.exists(spatial_volume_path):
		var s: Variant = load(spatial_volume_path)
		if s is Script:
			fit_script = s
	if fit_script == null:
		return false
	for m: Dictionary in fit_script.get_script_method_list():
		if m.get("name", "") == "fit_scale":
			return true
	return false


func _connect_any(names: Array[String], handler: Callable) -> void:
	for n: String in names:
		if _ext.has_signal(n) and not _ext.is_connected(n, handler):
			_ext.connect(n, handler)
			_connected.append({"signal": n, "callable": handler})


func _disconnect_all() -> void:
	if _ext != null and is_instance_valid(_ext):
		for c: Dictionary in _connected:
			if _ext.is_connected(c.signal, c.callable):
				_ext.disconnect(c.signal, c.callable)
	_connected.clear()


func _is_active() -> bool:
	if _ext == null or not is_instance_valid(_ext) or not _ext.has_method("is_spatial_container_active"):
		return false
	return bool(_ext.call("is_spatial_container_active"))


## While waiting: if the container has become active, read its bounds and mode
## (no bounds_changed event is guaranteed at creation) and fit once.
func _poll_active() -> bool:
	if not _waiting_for_active or not _is_active():
		return false
	_waiting_for_active = false
	_was_active = true
	var b: Vector3 = _read_bounds()
	_apply_bounds(b, _read_app_mode(), false)
	return true


func _read_bounds() -> Vector3:
	if _ext.has_method("get_spatial_container_bounds"):
		var v: Variant = _ext.call("get_spatial_container_bounds")
		if v is Vector3:
			return v
	return Vector3.ZERO


func _read_app_mode() -> int:
	if _ext.has_method("get_spatial_container_state"):
		var st: Variant = _ext.call("get_spatial_container_state")
		if st is Object and st.has_method("get_bounds_mode"):
			return to_app_mode(int(st.call("get_bounds_mode")))
	return APP_BOUNDED


## Fits the arrangement into the container. Infinite bounds or immersive mode
## restore 1:1. Degenerate bounds (any axis <= 0 or not finite) keep the
## current fit rather than collapse the arrangement. The arrangement is put
## back to scale 1 before it is measured, so the fit never compounds whether
## arrangement_aabb() reports world or local extents; both calls happen in one
## frame, so nothing renders at the intermediate scale.
func _apply_bounds(b: Vector3, app_mode: int, infinite: bool) -> void:
	if _fallen_back or _arrangement == null or not is_instance_valid(_arrangement):
		return
	var m := APP_BOUNDED if app_mode < 0 else app_mode
	container_bounds = b
	container_app_mode = m
	var s := 1.0
	if not infinite and m == APP_BOUNDED:
		if not (b.is_finite() and b.x > 0.0 and b.y > 0.0 and b.z > 0.0):
			return
		_arrangement.call("set_arrangement_scale", 1.0)
		var aabb: AABB = _arrangement.call("arrangement_aabb")
		s = float(fit_script.call("fit_scale", b, aabb.size))
	_arrangement.call("set_arrangement_scale", s)
	arrangement_scale = s
	container_bounds_changed.emit(b, m)


## Godot master: (rid: RID, infinite_bounds: bool, bounds_mode: int, bounds: Vector3).
## The Android guide's spelling may carry a different list, so the handler
## takes up to six arguments and picks them by type: the first bool is
## infinite_bounds, the first int the Godot bounds mode, the first Vector3 the
## bounds. Anything missing is read back from the extension's getters.
func _on_bounds_signal(a0: Variant = null, a1: Variant = null, a2: Variant = null,
		a3: Variant = null, a4: Variant = null, a5: Variant = null) -> void:
	if _fallen_back:
		return
	var infinite := false
	var godot_mode := -1
	var b := Vector3.ZERO
	var have_b := false
	var have_inf := false
	for a: Variant in [a0, a1, a2, a3, a4, a5]:
		if a is bool and not have_inf:
			infinite = a
			have_inf = true
		elif a is int and godot_mode < 0:
			godot_mode = a
		elif a is Vector3 and not have_b:
			b = a
			have_b = true
	if not have_b:
		b = _read_bounds()
	var app_mode := to_app_mode(godot_mode) if godot_mode >= 0 else _read_app_mode()
	if _waiting_for_active:
		_waiting_for_active = false
		_was_active = true
	_apply_bounds(b, app_mode, infinite)


func _on_visible_signal(a0: Variant = null, a1: Variant = null, a2: Variant = null, a3: Variant = null) -> void:
	var v: Variant = _first_bool([a0, a1, a2, a3])
	if v != null:
		container_visible = v


func _on_interactable_signal(a0: Variant = null, a1: Variant = null, a2: Variant = null, a3: Variant = null) -> void:
	var v: Variant = _first_bool([a0, a1, a2, a3])
	if v != null:
		container_interactable = v


func _on_closed_signal(_a0: Variant = null, _a1: Variant = null, _a2: Variant = null, _a3: Variant = null) -> void:
	_fall_back(REASON_CLOSED)


static func _first_bool(args: Array) -> Variant:
	for a: Variant in args:
		if a is bool:
			return a
	return null


## One-shot. Restores the arrangement (scale 1, ring default), drops every
## extension connection and leaves backend connections alone.
func _fall_back(why: String) -> void:
	if _fallen_back:
		return
	_fallen_back = true
	_waiting_for_active = false
	_disconnect_all()
	mode = MODE_EMULATED
	reason = why
	arrangement_scale = 1.0
	if _arrangement != null and is_instance_valid(_arrangement):
		_arrangement.call("set_arrangement_scale", 1.0)
		_arrangement.call("reset_default")
	fallback.emit(why)


func _exit_tree() -> void:
	_disconnect_all()
