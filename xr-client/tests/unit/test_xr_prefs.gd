extends "res://addons/gut/test.gd"
const XrPrefs := preload("res://scripts/xr_prefs.gd")

# ADR-2158 D6 / ADR-2164 L5: the cross-volume link switch defaults to the
# build type, and the stored choice is per build type. Every case drives the
# explicit-build-type helpers against a scratch file, so both build types are
# covered headless and the real user://xr_prefs.cfg is never written.

var _path := ""


func before_each() -> void:
	_path = "user://test_xr_prefs_%d.cfg" % Time.get_ticks_usec()


func after_each() -> void:
	if FileAccess.file_exists(_path):
		DirAccess.remove_absolute(ProjectSettings.globalize_path(_path))


func test_key_per_build_type() -> void:
	assert_eq(XrPrefs._key_for(true), "cross_volume_links_debug")
	assert_eq(XrPrefs._key_for(false), "cross_volume_links_release")


func test_build_default_applies_when_nothing_is_stored() -> void:
	assert_false(FileAccess.file_exists(_path))
	assert_true(XrPrefs._read(true, _path), "debug build: on")
	assert_false(XrPrefs._read(false, _path), "release build: off")


func test_stored_choice_overrides_the_build_default() -> void:
	assert_eq(XrPrefs._write(true, false, _path), OK)
	assert_false(XrPrefs._read(true, _path), "debug build turned off")
	assert_eq(XrPrefs._write(false, true, _path), OK)
	assert_true(XrPrefs._read(false, _path), "release build turned on")


func test_other_build_types_key_is_ignored() -> void:
	# A dev build turns links off; a release build of the same package still
	# reads only its own (absent) key and keeps its default.
	XrPrefs._write(true, false, _path)
	assert_false(XrPrefs._read(false, _path), "release still off by default")
	# A release choice of ON never reaches a debug build that chose OFF.
	XrPrefs._write(false, true, _path)
	assert_false(XrPrefs._read(true, _path), "debug keeps its own choice")
	# Writing one key keeps the other.
	var cfg := ConfigFile.new()
	assert_eq(cfg.load(_path), OK)
	assert_eq(cfg.get_value("xr", "cross_volume_links_debug"), false)
	assert_eq(cfg.get_value("xr", "cross_volume_links_release"), true)


func test_non_boolean_value_falls_back_to_the_default() -> void:
	var cfg := ConfigFile.new()
	cfg.set_value("xr", "cross_volume_links_debug", "yes")
	cfg.set_value("xr", "cross_volume_links_release", 1)
	cfg.save(_path)
	assert_true(XrPrefs._read(true, _path))
	assert_false(XrPrefs._read(false, _path))


func test_public_helpers_follow_the_running_build_type() -> void:
	# Read-only check against the real path: the public getter is the
	# explicit helper at OS.is_debug_build().
	assert_eq(XrPrefs.cross_volume_links(), XrPrefs._read(OS.is_debug_build(), XrPrefs.path()))
	assert_eq(XrPrefs.PATH, "user://xr_prefs.cfg")
	assert_eq(XrPrefs.SECTION, "xr")


# The live client shares user:// with every test copy. A GUT run must never
# change user://xr_prefs.cfg, whether a test writes the preference directly or
# toggles links through GraphScene's HUD action.
func _real_snapshot() -> String:
	if not FileAccess.file_exists(XrPrefs.PATH):
		return "<absent>"
	return FileAccess.get_file_as_string(XrPrefs.PATH)


func test_gut_run_is_redirected_away_from_the_real_file() -> void:
	assert_true(XrPrefs._under_gut(), "this process is a GUT run")
	assert_eq(XrPrefs.path_override, "")
	assert_eq(XrPrefs.path(), XrPrefs.TEST_PATH)
	assert_ne(XrPrefs.path(), XrPrefs.PATH)


func test_override_wins() -> void:
	XrPrefs.path_override = _path
	assert_eq(XrPrefs.path(), _path)
	XrPrefs.set_cross_volume_links(false)
	assert_false(XrPrefs._read(OS.is_debug_build(), _path), "write landed in the override")
	XrPrefs.path_override = ""


func test_toggling_links_never_touches_the_real_file() -> void:
	var before := _real_snapshot()
	XrPrefs.set_cross_volume_links(false)
	XrPrefs.set_cross_volume_links(true)
	var scene: Node3D = load("res://scenes/GraphScene.tscn").instantiate()
	add_child(scene)
	await get_tree().process_frame
	scene._on_hud_control("cross_links_toggle")
	scene._on_hud_control("cross_links_toggle")
	scene.queue_free()
	await get_tree().process_frame
	assert_eq(_real_snapshot(), before, "user://xr_prefs.cfg unchanged")
	if FileAccess.file_exists(XrPrefs.TEST_PATH):
		DirAccess.remove_absolute(ProjectSettings.globalize_path(XrPrefs.TEST_PATH))
