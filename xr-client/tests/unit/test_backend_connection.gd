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
	signal stream_live()
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

	func render_aabb_ids(i: PackedInt32Array, exclude: PackedInt32Array) -> PackedFloat32Array:
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
	# Exact backoff timings in these tests; the jitter has its own test.
	c.reconnect_jitter = 0.0
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


func test_visionclaw_drag_reaches_its_client_and_records_no_pin() -> void:
	var fake := FakeClient.new()
	var vc: Node = _conn(&"visionclaw", "visionclaw", fake)
	assert_true(vc.send_drag_start(3, Vector3(1, 2, 3)))
	assert_true(vc.send_drag_update(3, Vector3(2, 2, 3)))
	assert_true(vc.send_drag_end(3))
	assert_eq(fake.names(), ["send_drag_start", "send_drag_update", "send_drag_end"])
	assert_eq(vc.pinned_ids(), [], "ADR-2172 F3: a drag end records no pin; the server releases the node, nothing is left to unpin")


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
	assert_eq(fake.names(), ["close"], "a drag end left no pin, so close sends no nodeUnpin (and no reheat)")
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
	# A connect alone cancels the timer but keeps the count (audit D3): only the
	# first applied position frame proves the server is really serving us.
	fake.connection_changed.emit(true)
	assert_false(vc.reconnect_pending())
	assert_eq(vc.reconnect_attempts(), 1, "the upgrade alone does not reset the backoff")
	fake.stream_live.emit()
	assert_eq(vc.reconnect_attempts(), 0, "the first applied frame resets it")


func test_a_server_that_accepts_then_closes_backs_off_instead_of_hammering() -> void:
	# Audit D3: the old FSM reset the count on every upgrade, so a server that
	# accepted and then closed (auth reject, frame too big) was redialled every 2 s
	# for ever, each time rebuilding a full snapshot.
	var fake := FakeClient.new()
	var vc: Node = _conn(&"visionclaw", "visionclaw", fake)
	vc.open()
	var delays: Array = []
	for i: int in 6:
		fake.connection_changed.emit(true)
		fake.connection_changed.emit(false)
		delays.append(vc._reconnect_timer)
		vc.poll(vc._reconnect_timer + 0.01)
	assert_eq(delays, [2.0, 4.0, 8.0, 16.0, 32.0, 60.0], "doubling to the cap, not 2 s each time")
	assert_eq(fake.sent("connect_to_url"), 7)


func test_backoff_jitter_stays_within_twenty_percent_below_the_cap() -> void:
	var fake := FakeClient.new()
	var vc: Node = _conn(&"visionclaw", "visionclaw", fake)
	vc.reconnect_jitter = 0.2
	vc.set_jitter_seed(12345)
	vc.open()
	var seen := {}
	for i: int in 40:
		fake.connection_changed.emit(false)
		var d: float = vc._reconnect_timer
		var nominal: float = BackendConnection.backoff_delay(vc.reconnect_attempts())
		assert_between(d, nominal * 0.8, nominal, "attempt %d: %.3f within [0.8, 1.0] x %.1f" % [vc.reconnect_attempts(), d, nominal])
		assert_true(d <= BackendConnection.RECONNECT_MAX_DELAY_SEC)
		seen[snappedf(d / nominal, 0.01)] = true
		vc.poll(d + 0.01)
	assert_gt(seen.size(), 5, "the jitter actually varies")
	assert_eq(BackendConnection.jittered_delay(3, 0.2, 0.0), 8.0)
	assert_almost_eq(BackendConnection.jittered_delay(3, 0.2, 1.0), 6.4, 0.0001)
	assert_eq(BackendConnection.jittered_delay(30, 0.2, 0.0), 60.0)


func test_resume_dials_at_once_when_down_and_resets_the_backoff() -> void:
	# Headset doffed while the network was away: the backoff may have grown to
	# 32 s or more. Putting it back on must not wait that out.
	var fake := FakeClient.new()
	var vc: Node = _conn(&"visionclaw", "visionclaw", fake)
	vc.open()
	for i: int in 5:
		fake.connection_changed.emit(false)
		vc.poll(vc._reconnect_timer + 0.01)
	fake.connection_changed.emit(false)
	assert_eq(vc.reconnect_attempts(), 6)
	assert_eq(vc._reconnect_timer, 60.0)
	var dials: int = fake.sent("connect_to_url")
	assert_true(vc.resume(), "a down connection redials on resume")
	assert_eq(fake.sent("connect_to_url"), dials + 1, "dialled now, not in 60 s")
	assert_false(vc.reconnect_pending())
	assert_eq(vc.reconnect_attempts(), 0)
	assert_false(vc.resume(), "no second dial while the first is in flight")
	assert_eq(fake.sent("connect_to_url"), dials + 1)


func test_resume_leaves_a_live_connection_alone() -> void:
	# A healthy socket is not redialled (a redial costs a full snapshot); a dead one
	# that still looks connected is caught by the client's read deadline.
	var fake := FakeClient.new()
	var vc: Node = _conn(&"visionclaw", "visionclaw", fake)
	vc.open()
	fake.connection_changed.emit(true)
	assert_false(vc.resume())
	assert_eq(fake.sent("connect_to_url"), 1)


func test_resume_does_nothing_on_a_closed_connection() -> void:
	var fake := FakeClient.new()
	var vc: Node = _conn(&"visionclaw", "visionclaw", fake)
	vc.open()
	vc.close()
	assert_false(vc.resume())
	assert_eq(fake.sent("connect_to_url"), 1)


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
	var box: AABB = vc.render_aabb_for_ids(PackedInt32Array([1, 2]), PackedInt32Array())
	assert_eq(box.position, Vector3(-1, -2, -3))
	assert_eq(box.size, Vector3(2, 4, 6))
	fake.aabb = PackedFloat32Array()
	assert_eq(vc.render_aabb_for_ids(PackedInt32Array([9]), PackedInt32Array()), AABB(), "empty when none known")
