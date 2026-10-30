extends "res://addons/gut/test.gd"
const Shim := preload("res://scripts/native_container_shim.gd")
const FakeExt := preload("res://tests/fixtures/fake_spatial_container_ext.gd")

# ADR-2158 decision 2 / XR-PHASE1-CONTRACT §4: the native spatial-container
# shim's decision logic and fallback, driven through evaluate()'s env keys and
# a fake extension. No runtime we own has XR_EXT_spatial_container (SteamVR,
# Monado), so the native path is proved here and nowhere else.
#
# Names found on the real engines (probe script, HP, headless, --xr-mode off):
# 2026-10-30, engine 4.8-beta2 (cafc17a19) and 4.6.1-stable.
#   4.8-beta2:
#     class OpenXRSpatialContainerExtension (parent OpenXRExtensionWrapper);
#       ClassDB.class_exists = true, Engine.has_singleton = false under
#       --xr-mode off (register_types.cpp registers the singleton only when
#       OpenXR is enabled for the run); no bound get_singleton.
#       methods: is_enabled, is_spatial_container_active,
#         get_supported_bounds_modes, get_spatial_container_state,
#         get_spatial_container_bounds, request_spatial_container_bounds_mode,
#         request_spatial_container_visible
#       signals: spatial_container_bounds_changed(spatial_container_rid: RID,
#           spatial_container_infinite_bounds: bool,
#           spatial_container_bounds_mode: int, spatial_container_bounds: Vector3)
#         spatial_container_bounds_mode_request_denied(spatial_container_rid: RID)
#         spatial_container_closed(spatial_container_rid: RID)
#         spatial_container_interactable_changed(spatial_container_rid: RID,
#           spatial_container_interactable: bool)
#         spatial_container_visible_changed(spatial_container_rid: RID,
#           spatial_container_visible: bool)
#         spatial_container_visible_request_denied(spatial_container_rid: RID)
#     class OpenXRSpatialContainerState (parent RefCounted): get_bounds_mode,
#       is_interactable, is_visible; constants BOUNDS_MODE_BOUNDED = 0,
#       BOUNDS_MODE_IMMERSIVE = 1.
#     class OpenXRSpatialContainerSelfRenderingExtension (parent
#       OpenXRExtensionWrapper): is_enabled.
#     settings xr/openxr/extensions/spatial_container/{enabled = false,
#       bounds_mode = 0, bounds = (0.5, 0.5, 0.5)}.
#   4.6.1: none of these classes or settings exist.
#   Both: visionclaw/xr/native_spatial_container is absent (so: off).
# The Android XR guide's spellings (…_bounds_mode_changed,
# …_visibility_changed, …_interactability_changed) exist on neither engine;
# they are covered by the fake only.


var shim: Node
var arr: Node3D


func before_each() -> void:
	FakeExt.FitScaleStub.reset(0.25)
	shim = Shim.new()
	add_child_autofree(shim)
	shim.fit_script = FakeExt.FitScaleStub
	arr = FakeExt.FakeArrangement.new()
	add_child_autofree(arr)
	watch_signals(shim)


## Every condition passes: the arm goes native.
func _env_native(ext: Object) -> Dictionary:
	return {
		"setting": true,
		"is_android": true,
		"godot_setting": true,
		"engine_version": {"major": 4, "minor": 8, "patch": 0, "status": "beta"},
		"class_exists": true,
		"ext": ext,
	}


func _start_native(ext: Object) -> void:
	var r: Dictionary = shim.evaluate(_env_native(ext))
	assert_eq(r.mode, "native", "precondition: native decision")
	shim.start(arr)


# --- evaluate ---------------------------------------------------------------

func test_real_headless_engine_gives_setting_off() -> void:
	var r: Dictionary = shim.evaluate()
	assert_eq(r, {"mode": "emulated", "reason": "setting_off"})
	assert_signal_emitted_with_parameters(shim, "mode_decided", ["emulated", "setting_off"])
	assert_false(ProjectSettings.has_setting(Shim.SETTING), "phase 1 leaves the setting absent")


func test_each_reason_in_order() -> void:
	var fake := FakeExt.new()
	# Failing condition k and every later one must still report k: the first
	# reason that applies wins.
	var steps: Array = [
		["setting", false, "setting_off"],
		["is_android", false, "not_android"],
		["godot_setting", false, "godot_setting_off"],
		["engine_version", {"major": 4, "minor": 7}, "engine_lt_4_8"],
		["class_exists", false, "class_missing"],
		["ext", null, "no_instance"],
	]
	for k: int in steps.size():
		var env := _env_native(fake)
		for j: int in range(k, steps.size()):
			env[steps[j][0]] = steps[j][1]
		fake.enabled = false
		var r: Dictionary = shim.evaluate(env)
		assert_eq(r, {"mode": "emulated", "reason": steps[k][2]}, "step %d" % k)
	fake.enabled = false
	assert_eq(shim.evaluate(_env_native(fake)).reason, "extension_disabled")
	fake.enabled = true
	assert_eq(shim.evaluate(_env_native(fake)), {"mode": "native", "reason": "native"})


func test_engine_version_gate() -> void:
	assert_false(Shim.engine_at_least_4_8({"major": 4, "minor": 6, "patch": 1}))
	assert_false(Shim.engine_at_least_4_8({"major": 4, "minor": 7}))
	assert_true(Shim.engine_at_least_4_8({"major": 4, "minor": 8}))
	assert_true(Shim.engine_at_least_4_8({"major": 4, "minor": 10}))
	assert_true(Shim.engine_at_least_4_8({"major": 5, "minor": 0}))
	assert_false(Shim.engine_at_least_4_8({}), "missing keys read as 0")


func test_extension_without_is_enabled_counts_as_disabled() -> void:
	var r: Dictionary = shim.evaluate(_env_native(RefCounted.new()))
	assert_eq(r.reason, "extension_disabled")


## With the VisionClaw flags forced, the rest is read from the real engine:
## 4.6.1 stops at the version; 4.8 headless has the class but, under
## --xr-mode off, no singleton (register_types.cpp only registers it when
## OpenXR is enabled for the run).
func test_real_engine_with_flags_forced() -> void:
	var r: Dictionary = shim.evaluate({"setting": true, "is_android": true, "godot_setting": true})
	assert_eq(r.mode, "emulated", "no runtime here can host a container")
	if not Shim.engine_at_least_4_8(Engine.get_version_info()):
		assert_eq(r.reason, "engine_lt_4_8")
		assert_false(ClassDB.class_exists(Shim.EXT_CLASS), "4.6 has no container class")
	elif Engine.has_singleton(Shim.EXT_CLASS):
		assert_eq(r.reason, "extension_disabled")
	else:
		assert_true(ClassDB.class_exists(Shim.EXT_CLASS), "4.8 registers the class")
		assert_eq(r.reason, "no_instance")
		assert_null(Shim.resolve_extension(), "static get_singleton is not bound either")


# --- native arm with the fake extension --------------------------------------

func test_native_active_at_start_fits_through_fit_scale() -> void:
	var fake := FakeExt.new()
	fake.active = true
	fake.bounds = Vector3(0.8, 0.6, 0.4)
	_start_native(fake)
	assert_true(shim.is_native())
	assert_eq(FakeExt.FitScaleStub.calls.size(), 1, "delegated once")
	assert_eq(FakeExt.FitScaleStub.calls[0][0], Vector3(0.8, 0.6, 0.4), "container bounds passed")
	assert_eq(FakeExt.FitScaleStub.calls[0][1], arr.base_aabb.size, "arrangement measured at scale 1")
	assert_eq(arr.arr_scale, 0.25, "stub's result applied")
	assert_eq(shim.arrangement_scale, 0.25)
	assert_signal_emitted_with_parameters(shim, "container_bounds_changed", [Vector3(0.8, 0.6, 0.4), 0])
	assert_signal_not_emitted(shim, "fallback")


func test_refit_never_compounds() -> void:
	var fake := FakeExt.new()
	fake.active = true
	_start_native(fake)
	fake.emit_bounds_changed(Vector3(1.0, 1.0, 1.0), FakeExt.BOUNDS_MODE_BOUNDED)
	fake.emit_bounds_changed(Vector3(2.0, 1.0, 1.0), FakeExt.BOUNDS_MODE_BOUNDED)
	assert_eq(FakeExt.FitScaleStub.calls.size(), 3)
	for c: Array in FakeExt.FitScaleStub.calls:
		assert_eq(c[1], arr.base_aabb.size, "every fit measures the unscaled arrangement")
	for s: float in arr.aabb_calls:
		assert_eq(s, 1.0)


func test_activation_within_timeout() -> void:
	var fake := FakeExt.new()
	_start_native(fake)
	assert_false(shim.is_native(), "waiting for the container")
	shim.tick(1.0)
	shim.tick(1.0)
	fake.active = true
	shim.tick(0.5)
	assert_true(shim.is_native())
	assert_eq(FakeExt.FitScaleStub.calls.size(), 1)
	shim.tick(5.0)
	assert_signal_not_emitted(shim, "fallback")


func test_activation_timeout_falls_back() -> void:
	var fake := FakeExt.new()
	_start_native(fake)
	shim.tick(1.0)
	shim.tick(1.0)
	shim.tick(0.99)
	assert_signal_not_emitted(shim, "fallback", "2.99 s is inside the window")
	shim.tick(0.02)
	assert_signal_emitted_with_parameters(shim, "fallback", ["container_inactive"])
	assert_eq(shim.mode, "emulated")
	assert_eq(shim.reason, "container_inactive")
	assert_eq(arr.arr_scale, 1.0, "arrangement at scale 1")
	assert_eq(arr.reset_calls, 1, "arrangement back on its ring default")
	assert_eq(fake.connection_count(), 0, "every extension signal disconnected")


func test_closed_falls_back_once_and_restores() -> void:
	var fake := FakeExt.new()
	fake.active = true
	_start_native(fake)
	assert_eq(arr.arr_scale, 0.25)
	fake.emit_closed()
	assert_signal_emitted_with_parameters(shim, "fallback", ["closed"])
	assert_eq(arr.arr_scale, 1.0)
	assert_eq(arr.reset_calls, 1)
	assert_eq(fake.connection_count(), 0)
	shim.tick(10.0)
	fake.emit_closed()
	assert_signal_emit_count(shim, "fallback", 1, "fallback is one-shot")
	assert_eq(arr.reset_calls, 1)


func test_timeout_then_closed_emits_once() -> void:
	var fake := FakeExt.new()
	_start_native(fake)
	shim.tick(3.5)
	fake.active = true
	fake.emit_closed()
	fake.emit_bounds_changed(Vector3(1, 1, 1), FakeExt.BOUNDS_MODE_BOUNDED)
	assert_signal_emit_count(shim, "fallback", 1)
	assert_eq(shim.reason, "container_inactive")
	assert_eq(FakeExt.FitScaleStub.calls.size(), 0, "no fit after fallback")


func test_container_lost_without_closed_falls_back() -> void:
	var fake := FakeExt.new()
	fake.active = true
	_start_native(fake)
	fake.active = false   # engine destroyed it on state loss, no closed event
	shim.tick(0.011)
	assert_signal_emitted_with_parameters(shim, "fallback", ["container_lost"])
	assert_eq(arr.arr_scale, 1.0)


func test_immersive_and_infinite_restore_one_to_one() -> void:
	var fake := FakeExt.new()
	fake.active = true
	_start_native(fake)
	FakeExt.FitScaleStub.reset(0.25)
	fake.emit_bounds_changed(Vector3(0.5, 0.5, 0.5), FakeExt.BOUNDS_MODE_IMMERSIVE)
	assert_eq(arr.arr_scale, 1.0, "immersive ⇒ scale 1")
	assert_signal_emitted_with_parameters(shim, "container_bounds_changed", [Vector3(0.5, 0.5, 0.5), 1])
	fake.emit_bounds_changed(Vector3.ZERO, FakeExt.BOUNDS_MODE_BOUNDED, true)
	assert_eq(arr.arr_scale, 1.0, "infinite bounds ⇒ scale 1, not a collapse")
	assert_eq(FakeExt.FitScaleStub.calls.size(), 0, "no fit in either case")


func test_degenerate_bounds_keep_current_fit() -> void:
	var fake := FakeExt.new()
	fake.active = true
	_start_native(fake)
	var before: int = arr.scale_calls.size()
	fake.emit_bounds_changed(Vector3(0.5, 0.0, 0.5), FakeExt.BOUNDS_MODE_BOUNDED)
	fake.emit_bounds_changed(Vector3(INF, 1.0, 1.0), FakeExt.BOUNDS_MODE_BOUNDED)
	assert_eq(arr.scale_calls.size(), before, "arrangement untouched")
	assert_eq(arr.arr_scale, 0.25)
	assert_signal_not_emitted(shim, "fallback")


func test_master_signal_names_connect() -> void:
	var fake := FakeExt.new(FakeExt.VARIANT_MASTER)
	fake.active = true
	_start_native(fake)
	var names: PackedStringArray = shim.connected_signals()
	for n: String in ["spatial_container_bounds_changed", "spatial_container_visible_changed",
			"spatial_container_interactable_changed", "spatial_container_closed"]:
		assert_true(names.has(n), "%s connected" % n)
	assert_eq(names.size(), 4)
	fake.emit_visible_changed(false)
	fake.emit_interactable_changed(false)
	assert_false(shim.container_visible)
	assert_false(shim.container_interactable)


func test_android_guide_signal_names_connect() -> void:
	var fake := FakeExt.new(FakeExt.VARIANT_ANDROID)
	fake.active = true
	_start_native(fake)
	var names: PackedStringArray = shim.connected_signals()
	for n: String in ["spatial_container_bounds_mode_changed", "spatial_container_visibility_changed",
			"spatial_container_interactability_changed", "spatial_container_closed"]:
		assert_true(names.has(n), "%s connected" % n)
	assert_eq(names.size(), 4)
	FakeExt.FitScaleStub.reset(0.5)
	fake.emit_bounds_changed(Vector3(1.2, 1.0, 0.8), FakeExt.BOUNDS_MODE_BOUNDED)
	assert_eq(FakeExt.FitScaleStub.calls.size(), 1, "the other arity still fits")
	assert_eq(FakeExt.FitScaleStub.calls[0][0], Vector3(1.2, 1.0, 0.8))
	assert_eq(arr.arr_scale, 0.5)
	fake.emit_visible_changed(false)
	assert_false(shim.container_visible)
	fake.emit_closed()
	assert_signal_emitted_with_parameters(shim, "fallback", ["closed"])


func test_start_on_emulated_decision_is_inert() -> void:
	shim.evaluate()
	shim.start(arr)
	shim.tick(10.0)
	assert_signal_not_emitted(shim, "fallback")
	assert_eq(arr.scale_calls.size(), 0, "arrangement untouched")
	assert_eq(arr.reset_calls, 0)


func test_fit_unavailable_falls_back() -> void:
	shim.fit_script = null
	shim.spatial_volume_path = "res://scripts/__absent_spatial_volume.gd"
	var fake := FakeExt.new()
	fake.active = true
	_start_native(fake)
	assert_signal_emitted_with_parameters(shim, "fallback", ["fit_unavailable"])
	assert_eq(fake.connection_count(), 0)
	assert_eq(arr.arr_scale, 1.0)


## Before B merges, spatial_volume.gd is absent and the arm must fall back;
## after, the shim resolves B's static fit_scale by path.
func test_default_fit_resolves_spatial_volume() -> void:
	shim.fit_script = null
	var fake := FakeExt.new()
	_start_native(fake)
	if ResourceLoader.exists(Shim.SPATIAL_VOLUME_PATH):
		assert_signal_not_emitted(shim, "fallback")
		assert_not_null(shim.fit_script)
		assert_eq(shim.fit_script.resource_path, Shim.SPATIAL_VOLUME_PATH)
	else:
		assert_signal_emitted_with_parameters(shim, "fallback", ["fit_unavailable"])


# --- tables and notice --------------------------------------------------------

func test_enum_tables() -> void:
	assert_eq(Shim.to_app_mode(0), 0, "Godot BOUNDED → app BOUNDED")
	assert_eq(Shim.to_app_mode(1), 1, "Godot IMMERSIVE → app IMMERSIVE")
	assert_eq(Shim.to_app_mode(7), -1)
	assert_eq(Shim.from_spec_mode(1), 0, "spec BOUNDED_EXT → app BOUNDED")
	assert_eq(Shim.from_spec_mode(2), 1, "spec IMMERSIVE_EXT → app IMMERSIVE")
	assert_eq(Shim.from_spec_mode(0), -1)
	if ClassDB.class_exists("OpenXRSpatialContainerState"):
		assert_eq(ClassDB.class_get_integer_constant("OpenXRSpatialContainerState", "BOUNDS_MODE_BOUNDED"), Shim.GODOT_BOUNDS_MODE_BOUNDED)
		assert_eq(ClassDB.class_get_integer_constant("OpenXRSpatialContainerState", "BOUNDS_MODE_IMMERSIVE"), Shim.GODOT_BOUNDS_MODE_IMMERSIVE)


func test_notice_only_in_debug_builds() -> void:
	assert_eq(Shim.notice_for("closed", false), "", "release: no notice")
	var text: String = Shim.notice_for("closed", true)
	assert_true(text.contains("closed"), "debug: names the reason")
	assert_true(text.contains("emulated"))


## On 4.8 the fake must stay faithful to the real class: every method and
## signal the shim calls exists on OpenXRSpatialContainerExtension.
func test_fake_matches_real_class_when_present() -> void:
	if not ClassDB.class_exists(Shim.EXT_CLASS):
		pass_test("engine before 4.8: no class to compare")
		return
	for m: String in ["is_enabled", "is_spatial_container_active", "get_spatial_container_bounds",
			"get_spatial_container_state", "get_supported_bounds_modes",
			"request_spatial_container_bounds_mode", "request_spatial_container_visible"]:
		assert_true(ClassDB.class_has_method(Shim.EXT_CLASS, m), "real class has %s" % m)
	var fake := FakeExt.new()
	for s: String in [fake.sig_bounds, fake.sig_visible, fake.sig_interactable, fake.sig_closed,
			fake.sig_mode_denied, fake.sig_visible_denied]:
		assert_true(ClassDB.class_has_signal(Shim.EXT_CLASS, s), "real class has signal %s" % s)
	var real: Array = []
	for a: Dictionary in ClassDB.class_get_signal("OpenXRSpatialContainerExtension", fake.sig_bounds).args:
		real.append(a.type)
	assert_eq(real, [TYPE_RID, TYPE_BOOL, TYPE_INT, TYPE_VECTOR3], "bounds_changed argument types")
