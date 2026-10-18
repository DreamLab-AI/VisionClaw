extends "res://addons/gut/test.gd"

# M6 + eye-gaze-hazard static assertions (PRD-023 WP-9, ADR-130 Decision 1;
# copresence brief / godot #113717). The live XR session cannot run headless
# (no godot binary in this container), so these guard the two invariants at the
# source level: the Godot boot sets use_xr, and the eye-gaze extension is never
# enabled blindly — support is only ever queried behind a has_method guard.


func _read(path: String) -> String:
	var f := FileAccess.open(path, FileAccess.READ)
	assert_not_null(f, "must open %s" % path)
	if f == null:
		return ""
	var text := f.get_as_text()
	f.close()
	return text


func test_boot_sets_use_xr_synchronously():
	# ADR-130 D1: the Godot client has no isXRMode defect — xr_boot sets
	# use_xr = true after OpenXR initialise(). This is the M6 closure locus.
	var src := _read("res://scripts/xr_boot.gd")
	assert_true(src.contains("get_viewport().use_xr = true"), "xr_boot must set use_xr = true")
	assert_true(src.contains("initialize()"), "use_xr is set after an explicit OpenXR initialize()")


func test_eye_gaze_is_probed_not_blindly_enabled_in_boot():
	# godot #113717: enabling the eye-gaze action-map binding without the
	# extension present trips an action-map error. xr_boot only QUERIES support,
	# behind a has_method guard, and never enables the binding.
	var src := _read("res://scripts/xr_boot.gd")
	assert_true(
		src.contains('has_method("is_eye_gaze_interaction_supported")'),
		"eye-gaze support is queried behind a has_method guard"
	)


func test_graph_scene_probes_eye_gaze_and_feeds_resolver():
	# GraphScene queries eye-gaze support after init and feeds it to the Rust
	# gaze resolver, which keeps head-gaze primary and degrades eye-gaze to head
	# when unsupported.
	var src := _read("res://scripts/graph_scene.gd")
	assert_true(src.contains("_probe_eye_gaze"), "GraphScene has an eye-gaze probe")
	assert_true(
		src.contains('has_method("is_eye_gaze_interaction_supported")'),
		"GraphScene queries support behind a has_method guard"
	)
	assert_true(src.contains("set_eye_gaze_supported"), "the probe feeds the Rust gaze resolver")


func test_action_map_has_no_static_eye_gaze_binding():
	# ADR-2036: the .tres must never carry the eye-gaze profile, binding path or
	# top-level path (a blind static bind breaks Quest 3). Capability is queried only.
	var src := _read("res://openxr_action_map.tres")
	assert_false(src.contains("eyes_ext"), "no static /user/eyes_ext path in the action map")
	assert_false(src.contains("eye_gaze_interaction"), "no static eye-gaze interaction profile")
