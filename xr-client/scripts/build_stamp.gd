extends RefCounted
## Used via `const BuildStamp := preload("res://scripts/build_stamp.gd")`.
##
## What is deployed, shown on the HUD Session page: the commit the tree was
## exported from and when (`build_stamp.json`, written by tools/export.sh and
## never tracked), when the gdext library was built, and the backend URL the
## client is actually using. A tree that was not exported (a git checkout, the
## editor) says "unstamped" rather than guessing.

const STAMP_PATH := "res://build_stamp.json"
const LIBRARY_PATH := "res://rust/target/debug/libvisionclaw_xr_gdext.so"


## `{sha, exported_at}` from the stamp file, or `{}` when absent or malformed.
static func load_stamp(path: String = STAMP_PATH) -> Dictionary:
	if not FileAccess.file_exists(path):
		return {}
	var parser := JSON.new()
	if parser.parse(FileAccess.get_file_as_string(path)) != OK or not (parser.data is Dictionary):
		return {}
	var d: Dictionary = parser.data
	var sha: String = str(d.get("sha", ""))
	if sha.length() < 7 or not sha.is_valid_hex_number():
		return {}
	return {"sha": sha, "exported_at": str(d.get("exported_at", ""))}


## Unix time the gdext library was last written, or 0 when it is not on disk.
static func library_mtime(path: String = LIBRARY_PATH) -> int:
	return int(FileAccess.get_modified_time(path)) if FileAccess.file_exists(path) else 0


## Pure: the one-line readout, e.g.
## `build 1a2b3c4d5 · exported 2026-11-10 17:02Z · lib 2026-11-10 17:05Z · ws://host:4000`.
static func line(stamp: Dictionary, lib_unix: int, backend: String) -> String:
	var parts: Array[String] = []
	if stamp.is_empty():
		parts.append("build unstamped (not an export)")
	else:
		parts.append("build %s" % str(stamp["sha"]).substr(0, 9))
		var at: String = str(stamp.get("exported_at", ""))
		if not at.is_empty():
			parts.append("exported %s" % short_utc(at))
	if lib_unix > 0:
		parts.append("lib %s" % short_utc(Time.get_datetime_string_from_unix_time(lib_unix)))
	parts.append(backend if not backend.is_empty() else "no backend")
	return " · ".join(parts)


## Pure: `2026-11-10T17:02:33Z` (or without the Z) → `2026-11-10 17:02Z`.
static func short_utc(iso: String) -> String:
	var s: String = iso.replace("T", " ").trim_suffix("Z")
	return (s.substr(0, 16) if s.length() >= 16 else s) + "Z"
