extends "res://addons/gut/test.gd"

# Server-routed HUD writes (View 3D/Flat, Hierarchy, Radial, Layout Mode…) share
# _on_physics_completed. A rejected write must (1) reopen the in-flight gate,
# (2) discard the staged value so the HUD never drifts from the backend, and
# (3) record an operator-readable reason WITH the remedy — the dev-headset 401
# trap used to vanish into push_warning. The scene is instantiated without
# entering the tree (no _ready, no sockets), so `hud` is null and only the
# tracked state is exercised here; the HUD strip itself is covered in
# test_hud_tabs.gd::test_flash_notice_overrides_hint_then_expires.

func _scene() -> Node3D:
	return (load("res://scenes/GraphScene.tscn") as PackedScene).instantiate()


func test_denied_write_discards_stage_and_records_remedy() -> void:
	var gs: Node3D = _scene()
	gs._physics_pending = true
	gs._physics_staged = {"_z_compression": 0.3}
	gs._on_physics_completed(HTTPRequest.RESULT_SUCCESS, 401, PackedStringArray(), PackedByteArray())
	assert_false(gs._physics_pending, "in-flight gate reopens")
	assert_eq(gs._z_compression, 1.0, "staged flat toggle discarded on 401")
	assert_eq(gs._physics_staged.size(), 0, "stage cleared")
	assert_true(gs._last_write_error.contains("HTTP 401"), "reason names the status")
	assert_true(gs._last_write_error.contains("VISIONCLAW_DEV_MODE=1"), "reason names the dev remedy")
	assert_true(gs._last_write_error.contains("XR_NOSTR_SECRET"), "reason names the auth remedy")
	gs.free()


func test_forbidden_and_transport_failures_are_described() -> void:
	var gs: Node3D = _scene()
	assert_true(gs._describe_write_failure(HTTPRequest.RESULT_SUCCESS, 403).contains("denied (HTTP 403"), "403 reads as denied")
	assert_true(gs._describe_write_failure(HTTPRequest.RESULT_CANT_CONNECT, 0).contains("unreachable"), "transport failure reads as unreachable")
	assert_true(gs._describe_write_failure(HTTPRequest.RESULT_SUCCESS, 500).contains("HTTP 500"), "other codes surface the status")
	gs.free()


func test_successful_write_commits_stage_and_clears_error() -> void:
	var gs: Node3D = _scene()
	gs._last_write_error = "stale"
	gs._physics_pending = true
	gs._physics_staged = {"_z_compression": 0.3}
	gs._on_physics_completed(HTTPRequest.RESULT_SUCCESS, 200, PackedStringArray(), PackedByteArray())
	assert_false(gs._physics_pending, "gate reopens")
	assert_eq(gs._z_compression, 0.3, "staged value committed on 2xx")
	assert_eq(gs._last_write_error, "", "error cleared on success")
	gs.free()
