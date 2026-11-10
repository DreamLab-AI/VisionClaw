extends "res://addons/gut/test.gd"
## The HUD build stamp (what is deployed): the exported commit, the export and
## library build times, and the backend in use. tools/export.sh writes the file.

const BuildStamp := preload("res://scripts/build_stamp.gd")

var _tmp := "user://build_stamp_test.json"


func after_each() -> void:
	if FileAccess.file_exists(_tmp):
		DirAccess.remove_absolute(ProjectSettings.globalize_path(_tmp))


func _write(text: String) -> void:
	var f := FileAccess.open(_tmp, FileAccess.WRITE)
	f.store_string(text)
	f.close()


func test_an_exported_tree_shows_sha_times_and_backend() -> void:
	_write('{"sha": "1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d", "ref": "main", "exported_at": "2026-11-10T17:02:33Z"}')
	var stamp: Dictionary = BuildStamp.load_stamp(_tmp)
	assert_eq(str(stamp["sha"]).length(), 40)
	var lib := int(Time.get_unix_time_from_datetime_string("2026-11-10T17:05:00"))
	assert_eq(BuildStamp.line(stamp, lib, "ws://backend.test:4000"),
		"build 1a2b3c4d5 · exported 2026-11-10 17:02Z · lib 2026-11-10 17:05Z · ws://backend.test:4000")


func test_a_checkout_says_unstamped_never_a_guess() -> void:
	assert_eq(BuildStamp.load_stamp("user://no_such_stamp.json"), {})
	assert_eq(BuildStamp.line({}, 0, "ws://backend.test:4000"),
		"build unstamped (not an export) · ws://backend.test:4000")


func test_a_malformed_stamp_is_treated_as_absent() -> void:
	_write("not json")
	assert_eq(BuildStamp.load_stamp(_tmp), {})
	_write('{"sha": "nothex!", "exported_at": "x"}')
	assert_eq(BuildStamp.load_stamp(_tmp), {})


func test_short_utc() -> void:
	assert_eq(BuildStamp.short_utc("2026-11-10T17:02:33Z"), "2026-11-10 17:02Z")
	assert_eq(BuildStamp.short_utc("2026-11-10T17:02:33"), "2026-11-10 17:02Z")


func test_the_hud_session_page_shows_the_line() -> void:
	var hud: Node3D = (load("res://scenes/HUD.tscn") as PackedScene).instantiate()
	add_child_autofree(hud)
	await get_tree().process_frame
	hud.set_build_info("build 1a2b3c4d5 · ws://backend.test:4000")
	assert_eq(hud.build_info(), "build 1a2b3c4d5 · ws://backend.test:4000")
	assert_not_null(hud._build_label, "the Session page carries the stamp label")
	assert_eq(hud._build_label.text, "build 1a2b3c4d5 · ws://backend.test:4000")
