extends RefCounted
## Used via `const XrPrefs := preload("res://scripts/xr_prefs.gd")` rather than
## a class_name, so a headless run with a stale global class cache still
## resolves it (see backend_caps.gd).

## Per-device XR preferences (ADR-2158 D6).
##
## `xr.cross_volume_links` is the one switch for every cross-volume link kind
## (ADR-2164 D7). It defaults to `OS.is_debug_build()`: on in the editor and
## debug exports, off in a release export. The operator's choice is stored in
## `user://xr_prefs.cfg`, section `xr`, under a key per build type, because a
## debug and a release install of one package share `user://`: a choice made in
## a dev build must never turn links on in a release build. Static helpers, no
## state; the `_`-prefixed variants take the build type and path explicitly so
## the tests can cover both build types headless.

const PATH := "user://xr_prefs.cfg"
## Where a test run's writes go instead of PATH (see `path()`).
const TEST_PATH := "user://xr_prefs.test.cfg"
const SECTION := "xr"
const KEY_DEBUG := "cross_volume_links_debug"
const KEY_RELEASE := "cross_volume_links_release"
## `xr.portal_walkthrough` (ADR-2166): walking the headset into a portal enters
## it. One key for both build types, off by default.
const KEY_WALKTHROUGH := "portal_walkthrough"
## `xr.splat_scene` (ADR-2170): show the Gaussian splat volume. Off by default until it
## has been measured on the headset; `XR_SPLATS=1` turns it on for a run.
const KEY_SPLATS := "splat_scene"
## `xr.merged_splat_scale` (ADR-2178): the viewer-scale multiplier of the merged splat
## space, over the stage's measured metres-per-unit. 1.0 by default.
const KEY_MERGED_SCALE := "merged_splat_scale"


# Every test copy on a machine shares `user://` (app_userdata/"VisionClaw XR")
# with the live client, so a GUT run that toggles links through GraphScene
# used to rewrite the operator's real preference. `path_override` redirects
# the public helpers; left empty, `path()` redirects on its own whenever the
# process is a GUT run, so no test can reach PATH even if it forgets to set it.
static var path_override: String = ""


## The file the public helpers read and write: `path_override` if set, else
## TEST_PATH under GUT, else PATH.
static func path() -> String:
	if not path_override.is_empty():
		return path_override
	return TEST_PATH if _under_gut() else PATH


## True when GUT drives this process: the command-line runner is the main
## loop's script, or the editor panel's runner scene is the current scene.
static func _under_gut() -> bool:
	var loop := Engine.get_main_loop()
	if loop == null:
		return false
	var script: Script = loop.get_script() as Script
	if script != null and script.resource_path.begins_with("res://addons/gut/"):
		return true
	var tree := loop as SceneTree
	if tree != null and tree.current_scene != null:
		return tree.current_scene.scene_file_path.begins_with("res://addons/gut/")
	return false


## The switch for the running build type.
static func cross_volume_links() -> bool:
	return _read(OS.is_debug_build(), path())


## Store the switch for the running build type only.
static func set_cross_volume_links(on: bool) -> void:
	_write(OS.is_debug_build(), on, path())


static func _key_for(debug: bool) -> String:
	return KEY_DEBUG if debug else KEY_RELEASE


## Stored value for `debug`'s key, or the build default (`debug`) when the
## file, the key or a boolean value is absent. The other build type's key is
## never read.
static func _read(debug: bool, path: String) -> bool:
	var cfg := ConfigFile.new()
	var key := _key_for(debug)
	if cfg.load(path) != OK or not cfg.has_section_key(SECTION, key):
		return debug
	var v: Variant = cfg.get_value(SECTION, key)
	if typeof(v) == TYPE_BOOL:
		return v
	return debug


## Write `on` under `debug`'s key, keeping every other key in the file.
static func _write(debug: bool, on: bool, path: String) -> int:
	var cfg := ConfigFile.new()
	var _ignored := cfg.load(path)  # a missing file starts empty
	cfg.set_value(SECTION, _key_for(debug), on)
	return cfg.save(path)


## Whether the headset entering a portal's box enters that space. Off by default.
static func portal_walkthrough() -> bool:
	var cfg := ConfigFile.new()
	if cfg.load(path()) != OK or not cfg.has_section_key(SECTION, KEY_WALKTHROUGH):
		return false
	var v: Variant = cfg.get_value(SECTION, KEY_WALKTHROUGH)
	return typeof(v) == TYPE_BOOL and v


static func set_portal_walkthrough(on: bool) -> int:
	var cfg := ConfigFile.new()
	var _ignored := cfg.load(path())
	cfg.set_value(SECTION, KEY_WALKTHROUGH, on)
	return cfg.save(path())


## Whether the Gaussian splat volume is built (ADR-2170). Off by default: the stored
## choice, else the XR_SPLATS environment variable ("1", "true" or "on").
static func splat_scene() -> bool:
	var cfg := ConfigFile.new()
	if cfg.load(path()) == OK and cfg.has_section_key(SECTION, KEY_SPLATS):
		var v: Variant = cfg.get_value(SECTION, KEY_SPLATS)
		if typeof(v) == TYPE_BOOL:
			return v
	return OS.get_environment("XR_SPLATS").to_lower() in ["1", "true", "on"]


static func set_splat_scene(on: bool) -> int:
	var cfg := ConfigFile.new()
	var _ignored := cfg.load(path())
	cfg.set_value(SECTION, KEY_SPLATS, on)
	return cfg.save(path())


## The merged splat space's scale multiplier (ADR-2178): the stored positive value,
## else 1.0.
static func merged_splat_scale() -> float:
	var cfg := ConfigFile.new()
	if cfg.load(path()) == OK and cfg.has_section_key(SECTION, KEY_MERGED_SCALE):
		var v: Variant = cfg.get_value(SECTION, KEY_MERGED_SCALE)
		if (typeof(v) == TYPE_FLOAT or typeof(v) == TYPE_INT) and is_finite(float(v)) and float(v) > 0.0:
			return float(v)
	return 1.0


static func set_merged_splat_scale(mult: float) -> int:
	var cfg := ConfigFile.new()
	var _ignored := cfg.load(path())
	cfg.set_value(SECTION, KEY_MERGED_SCALE, mult)
	return cfg.save(path())
