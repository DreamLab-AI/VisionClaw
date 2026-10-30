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
const SECTION := "xr"
const KEY_DEBUG := "cross_volume_links_debug"
const KEY_RELEASE := "cross_volume_links_release"


## The switch for the running build type.
static func cross_volume_links() -> bool:
	return _read(OS.is_debug_build(), PATH)


## Store the switch for the running build type only.
static func set_cross_volume_links(on: bool) -> void:
	_write(OS.is_debug_build(), on, PATH)


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
