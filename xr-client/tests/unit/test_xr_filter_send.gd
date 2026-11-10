extends "res://addons/gut/test.gd"
## ADR-2183 D1 / ADR-2188 D4: the headset can change the quality grey-out filter.
## Before this, graph_parity.gd handled `filter_update_success` but nothing on the
## headset ever sent `filter_update` (register VC-36, Debt loose end), so the
## filter could only be changed from the desktop.

const ConnScript := preload("res://scripts/backend_connection.gd")


class ClientStub:
	extends RefCounted
	var sent: Array[String] = []
	var local: Array = []
	func send_text(json: String) -> void:
		sent.append(json)
	func set_local_quality_filter(enabled: bool, threshold: float) -> void:
		local.append([enabled, threshold])


func _conn_with_stub() -> Array:
	var conn: Node = ConnScript.new()
	var stub := ClientStub.new()
	conn.client = stub
	add_child_autofree(conn)
	return [conn, stub]


func test_send_filter_update_emits_the_desktop_shaped_frame() -> void:
	var pair := _conn_with_stub()
	var conn: Node = pair[0]
	var stub: ClientStub = pair[1]
	assert_true(conn.send_filter_update(true), "a live client accepts the frame")
	assert_eq(stub.sent.size(), 1)
	var frame: Dictionary = JSON.parse_string(stub.sent[0])
	assert_eq(str(frame.get("type")), "filter_update")
	assert_true(bool(frame.get("enabled")))
	assert_true(bool(frame.get("filter_by_quality")))
	assert_almost_eq(float(frame.get("quality_threshold")), 0.4, 0.0001, "shipped default threshold")
	assert_false(bool(frame.get("filter_by_authority")))
	assert_eq(str(frame.get("filter_mode")), "and")
	# The headset only ever greys; it never asks the server to remove nodes.
	assert_false(bool(frame.get("server_remove")))
	assert_false(bool(frame.get("include_linked_pages")))


func test_disabling_sends_enabled_false_and_quality_off() -> void:
	var pair := _conn_with_stub()
	var conn: Node = pair[0]
	var stub: ClientStub = pair[1]
	conn.send_filter_update(false)
	var frame: Dictionary = JSON.parse_string(stub.sent[0])
	assert_false(bool(frame.get("enabled")))
	assert_false(bool(frame.get("filter_by_quality")))


func test_no_client_means_nothing_is_sent_and_false_is_returned() -> void:
	var conn: Node = ConnScript.new()
	add_child_autofree(conn)
	assert_false(conn.send_filter_update(true))


func test_set_quality_filter_greys_locally_and_tells_the_server() -> void:
	# Audit A4: the toggle used to send only, so nothing on the headset changed.
	var pair := _conn_with_stub()
	var conn: Node = pair[0]
	var stub: ClientStub = pair[1]
	assert_true(conn.set_quality_filter(true))
	assert_eq(stub.local.size(), 1, "applied to the local render store")
	assert_true(bool(stub.local[0][0]))
	assert_almost_eq(float(stub.local[0][1]), 0.4, 0.0001, "the same threshold the frame carries")
	var frame: Dictionary = JSON.parse_string(stub.sent[0])
	assert_almost_eq(float(frame.get("quality_threshold")), float(stub.local[0][1]), 0.0001)
	conn.set_quality_filter(false)
	assert_false(bool(stub.local[1][0]), "off restores locally")
	assert_false(bool(JSON.parse_string(stub.sent[1]).get("enabled")))
