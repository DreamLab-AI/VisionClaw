extends "res://addons/gut/test.gd"
const BackendConnection := preload("res://scripts/backend_connection.gd")
const BackendCaps := preload("res://scripts/backend_caps.gd")

# XR phase-1 contract §2 / §8 (ADR-2159): one BackendConnection per content
# source. A fake client records every call so "nothing was sent" is checkable
# (E9) without a socket or the gdext library.


class FakeClient:
	extends RefCounted
	signal connection_changed(connected: bool)
	signal topology_updated(edge_count: int)
	signal text_message(json: String)
	var calls: Array = []
	var aabb := PackedFloat32Array()
	var ids := PackedInt32Array()

	func connect_to_url(url: String, secret: String) -> void:
		calls.append(["connect_to_url", url, secret])

	func close() -> void:
		calls.append(["close"])

	func poll() -> void:
		calls.append(["poll"])

	func send_drag_start(id: int, p: Vector3) -> void:
		calls.append(["send_drag_start", id, p])

	func send_drag_update(id: int, p: Vector3) -> void:
		calls.append(["send_drag_update", id, p])

	func send_drag_end(id: int) -> void:
		calls.append(["send_drag_end", id])

	func send_node_unpin(id: int) -> void:
		calls.append(["send_node_unpin", id])

	func node_ids_for_mask(mask: int) -> PackedInt32Array:
		calls.append(["node_ids_for_mask", mask])
		return ids

	func render_aabb_ids(i: PackedInt32Array, exclude: int) -> PackedFloat32Array:
		calls.append(["render_aabb_ids", i, exclude])
		return aabb

	func names() -> Array:
		var out: Array = []
		for c: Array in calls:
			out.append(c[0])
		return out

	func sent(kind: String) -> int:
		return names().count(kind)


func _conn(id: StringName, mode: String, fake: FakeClient) -> Node:
	var c: Node = BackendConnection.make(id, mode, "ws://10.0.0.1:4000/", "", fake)
	add_child_autofree(c)
	return c


func test_caps_are_per_connection() -> void:
	var vc: Node = _conn(&"visionclaw", "visionclaw", FakeClient.new())
	var cs: Node = _conn(&"codescape", "codescape", FakeClient.new())
	for key: String in BackendCaps.ALL_KEYS:
		assert_true(vc.supports(key), "visionclaw serves %s" % key)
		assert_eq(cs.supports(key), BackendCaps.CODESCAPE_SUPPORTED.has(key), "codescape flag for %s" % key)
	assert_false(cs.supports(BackendCaps.DRAG), "codescape never applies a drag (E9)")
	cs.caps[BackendCaps.FOLD] = true
	assert_false(vc.caps.is_empty())
	assert_true(cs.supports(BackendCaps.FOLD))
	assert_true(vc.supports(BackendCaps.FOLD), "changing one connection's caps leaves the other alone")


func test_urls_carry_the_codescape_prefix_once() -> void:
	var vc: Node = _conn(&"visionclaw", "visionclaw", FakeClient.new())
	var cs: Node = _conn(&"codescape", "codescape", FakeClient.new())
	assert_eq(vc.graph_ws_url(), "ws://10.0.0.1:4000/wss")
	assert_eq(cs.graph_ws_url(), "ws://10.0.0.1:4000/codescape/wss")
	assert_eq(BackendConnection.with_backend_prefix("ws://h:4000/codescape/", "codescape"), "ws://h:4000/codescape", "never doubled")
	assert_eq(cs.http_base(), "http://10.0.0.1:4000/codescape")
	assert_eq(BackendConnection.ws_to_http("wss://h/x"), "https://h/x")
	assert_eq(BackendConnection.ws_to_http("ws://h:1"), "http://h:1")


func test_codescape_drag_returns_false_and_sends_nothing() -> void:
	var fake := FakeClient.new()
	var cs: Node = _conn(&"codescape", "codescape", fake)
	watch_signals(cs)
	cs.open()
	fake.calls.clear()
	assert_false(cs.send_drag_start(7, Vector3.ONE), "drag start refused")
	assert_false(cs.send_drag_update(7, Vector3.ONE), "drag update refused")
	assert_false(cs.send_drag_end(7), "drag end refused")
	assert_eq(fake.sent("send_drag_start") + fake.sent("send_drag_update") + fake.sent("send_drag_end"), 0, "no nodeDrag frame sent")
	assert_signal_emit_count(cs, "refused", 1, "one notice per gesture: start only")
	assert_signal_emitted_with_parameters(cs, "refused", [&"codescape", BackendCaps.DRAG, "drag a node"])
	assert_eq(cs.pinned_ids(), [], "a refused drag pins nothing")


func test_visionclaw_drag_reaches_its_client_and_records_the_pin() -> void:
	var fake := FakeClient.new()
	var vc: Node = _conn(&"visionclaw", "visionclaw", fake)
	assert_true(vc.send_drag_start(3, Vector3(1, 2, 3)))
	assert_true(vc.send_drag_update(3, Vector3(2, 2, 3)))
	assert_true(vc.send_drag_end(3))
	assert_eq(fake.names(), ["send_drag_start", "send_drag_update", "send_drag_end"])
	assert_eq(vc.pinned_ids(), [3])


func test_refused_fires_with_label_and_quiet_suppresses_it() -> void:
	var cs: Node = _conn(&"codescape", "codescape", FakeClient.new())
	watch_signals(cs)
	assert_false(cs.gate(BackendCaps.FOLD, "fold the graph"))
	assert_signal_emitted_with_parameters(cs, "refused", [&"codescape", BackendCaps.FOLD, "fold the graph"])
	assert_false(cs.gate(BackendCaps.FOLD, "fold again", true))
	assert_signal_emit_count(cs, "refused", 1, "quiet refusal announces nothing")
	var vc: Node = _conn(&"visionclaw", "visionclaw", FakeClient.new())
	watch_signals(vc)
	assert_true(vc.gate(BackendCaps.FOLD, "fold the graph"))
	assert_signal_not_emitted(vc, "refused")


func test_close_cancels_requests_releases_pins_and_bumps_generation() -> void:
	var fake := FakeClient.new()
	var vc: Node = _conn(&"visionclaw", "visionclaw", fake)
	var req: HTTPRequest = vc.make_http("fold", 5.0)
	assert_eq(req.get_parent(), vc, "the connection owns its requests")
	# A request in flight to an address nothing listens on, so close() has something to cancel.
	assert_eq(req.request("http://127.0.0.1:9/never"), OK)
	assert_ne(req.get_http_client_status(), HTTPClient.STATUS_DISCONNECTED, "request in flight")
	vc.open()
	fake.connection_changed.emit(true)
	assert_true(vc.send_drag_end(11))
	watch_signals(vc)
	var gen_before: int = vc.generation
	fake.calls.clear()
	vc.close()
	assert_eq(req.get_http_client_status(), HTTPClient.STATUS_DISCONNECTED, "request cancelled")
	assert_eq(fake.names(), ["send_node_unpin", "close"], "pins released while the socket is still open, then closed")
	assert_eq(vc.pinned_ids(), [], "pin set cleared")
	assert_eq(vc.generation, gen_before + 1, "generation bumped")
	assert_signal_emitted_with_parameters(vc, "generation_changed", [&"visionclaw", gen_before + 1])
	assert_signal_emitted_with_parameters(vc, "connected_changed", [&"visionclaw", false])
	assert_false(vc.is_connected_now())


func test_codescape_close_sends_no_unpin() -> void:
	var fake := FakeClient.new()
	var cs: Node = _conn(&"codescape", "codescape", fake)
	cs.open()
	fake.calls.clear()
	cs.close()
	assert_eq(fake.sent("send_node_unpin"), 0)


func test_generation_counts_connects_and_topologies() -> void:
	var fake := FakeClient.new()
	var vc: Node = _conn(&"visionclaw", "visionclaw", fake)
	watch_signals(vc)
	assert_eq(vc.generation, 0)
	vc.open()
	assert_eq(vc.generation, 1, "+1 on connect")
	assert_eq(fake.sent("connect_to_url"), 1)
	assert_eq(fake.calls[fake.names().find("connect_to_url")][1], "ws://10.0.0.1:4000/wss")
	fake.topology_updated.emit(42)
	assert_eq(vc.generation, 2, "+1 on initialGraphLoad")
	assert_signal_emitted_with_parameters(vc, "topology_changed", [&"visionclaw", 2])


func test_drop_schedules_backoff_and_poll_redials() -> void:
	var fake := FakeClient.new()
	var vc: Node = _conn(&"visionclaw", "visionclaw", fake)
	vc.open()
	fake.connection_changed.emit(true)
	assert_true(vc.is_connected_now())
	fake.connection_changed.emit(false)
	assert_false(vc.is_connected_now())
	assert_true(vc.reconnect_pending())
	assert_eq(vc.reconnect_attempts(), 1)
	var gen: int = vc.generation
	vc.poll(1.0)
	assert_eq(fake.sent("connect_to_url"), 1, "not before the 2 s backoff")
	vc.poll(1.5)
	assert_eq(fake.sent("connect_to_url"), 2, "redialled after the backoff")
	assert_eq(vc.generation, gen + 1, "+1 on reconnect")
	assert_gt(fake.sent("poll"), 1, "poll pumps the client")
	# A connect resets the backoff.
	fake.connection_changed.emit(true)
	assert_eq(vc.reconnect_attempts(), 0)
	assert_false(vc.reconnect_pending())


func test_closed_connection_never_redials() -> void:
	var fake := FakeClient.new()
	var vc: Node = _conn(&"visionclaw", "visionclaw", fake)
	vc.open()
	vc.close()
	fake.connection_changed.emit(false)
	assert_false(vc.reconnect_pending(), "a closed connection does not schedule a reconnect")
	vc.poll(120.0)
	assert_eq(fake.sent("connect_to_url"), 1)


func test_backoff_doubles_to_the_cap() -> void:
	assert_eq(BackendConnection.backoff_delay(1), 2.0)
	assert_eq(BackendConnection.backoff_delay(2), 4.0)
	assert_eq(BackendConnection.backoff_delay(5), 32.0)
	assert_eq(BackendConnection.backoff_delay(6), 60.0)
	assert_eq(BackendConnection.backoff_delay(30), 60.0)


func test_text_frames_are_routed_with_their_type() -> void:
	var fake := FakeClient.new()
	var vc: Node = _conn(&"visionclaw", "visionclaw", fake)
	watch_signals(vc)
	var json := "{\"type\":\"broker:new_case\",\"caseId\":\"c1\"}"
	fake.text_message.emit(json)
	assert_signal_emitted_with_parameters(vc, "text_received", [&"visionclaw", "broker:new_case", json])
	fake.text_message.emit("not json")
	assert_signal_emitted_with_parameters(vc, "text_received", [&"visionclaw", "", "not json"])


func test_two_connections_never_share_a_client() -> void:
	var a: Node = _conn(&"visionclaw", "visionclaw", FakeClient.new())
	var b: Node = _conn(&"codescape", "codescape", FakeClient.new())
	assert_ne(a.client, b.client)
	if ClassDB.class_exists("BinaryProtocolClient"):
		var ra: Node = BackendConnection.make(&"visionclaw", "visionclaw", "ws://h:1", "")
		var rb: Node = BackendConnection.make(&"codescape", "codescape", "ws://h:1", "")
		add_child_autofree(ra)
		add_child_autofree(rb)
		assert_not_null(ra.client, "a real client is built per connection")
		assert_not_null(rb.client)
		assert_ne(ra.client, rb.client, "each connection builds its own BinaryProtocolClient")
		assert_eq(ra.client.get_class(), "BinaryProtocolClient")


func test_aabb_and_ids_wrap_the_client() -> void:
	var fake := FakeClient.new()
	fake.ids = PackedInt32Array([1, 2])
	fake.aabb = PackedFloat32Array([-1, -2, -3, 1, 2, 3])
	var vc: Node = _conn(&"visionclaw", "visionclaw", fake)
	assert_eq(vc.node_ids_for_mask(0b0001), PackedInt32Array([1, 2]))
	var box: AABB = vc.render_aabb_for_ids(PackedInt32Array([1, 2]), -1)
	assert_eq(box.position, Vector3(-1, -2, -3))
	assert_eq(box.size, Vector3(2, 4, 6))
	fake.aabb = PackedFloat32Array()
	assert_eq(vc.render_aabb_for_ids(PackedInt32Array([9]), -1), AABB(), "empty when none known")
