extends "res://addons/gut/test.gd"

# Demo director (scripts/agent_demo_director.gd): the ONLY demo code. It must
# drive the real registry doors (ingest / apply_agent_state / retire_agents),
# encode 0x23 frames byte-for-byte like the server, pick real nodes, follow real
# edges, keep evidence alive under the TTL, complete explicitly, and leave no
# trace on Stop. A fake client records every call so no gdext is needed.

const Director := preload("res://scripts/agent_demo_director.gd")


class FakeClient:
	extends RefCounted
	var frames: Array = []          # PackedByteArray per ingest()
	var states: Array = []          # [id, status, task]
	var retired: PackedInt32Array = PackedInt32Array()
	var clock: int = 1_000_000
	var edges := PackedInt32Array([1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 1])
	var positions: Dictionary = {}

	func _init() -> void:
		for i: int in range(1, 9):
			positions[i] = Vector3(float(i) * 30.0, 0.0, 0.0)  # 30 server units ≈ 0.9 m apart

	func ingest(bytes: PackedByteArray) -> void:
		frames.append(bytes)

	func apply_agent_state(id: int, status: String, task: String) -> void:
		states.append([id, status, task])

	func retire_agents(ids: PackedInt32Array) -> int:
		retired.append_array(ids)
		return ids.size()

	func get_edges() -> PackedInt32Array:
		return edges

	func top_labels(_max: int) -> PackedInt32Array:
		return PackedInt32Array([1, 2, 3, 4, 5, 6, 7, 8])

	func get_render_ids() -> PackedInt32Array:
		return PackedInt32Array([1, 2, 3, 4, 5, 6, 7, 8])

	func node_position(id: int) -> Vector3:
		return positions.get(id, Vector3.ZERO)

	func label_of(id: int) -> String:
		return "Node %d" % id

	func server_clock_ms() -> int:
		return clock


func _decode(frame: PackedByteArray) -> Array:
	# Mirror of decode_agent_action_frame (binary_protocol.rs).
	assert_eq(frame.decode_u8(0), 0x23, "message type byte")
	var count: int = frame.decode_u16(1)
	var out: Array = []
	var off: int = 3
	for _i: int in range(count):
		var len_: int = frame.decode_u16(off)
		off += 2
		var ev := {
			"source": frame.decode_u32(off), "target": frame.decode_u32(off + 4),
			"action": frame.decode_u8(off + 8), "ts": frame.decode_u32(off + 9),
			"dur": frame.decode_u16(off + 13),
			"payload": frame.slice(off + 15, off + len_).get_string_from_utf8(),
		}
		out.append(ev)
		off += len_
	assert_eq(off, frame.size(), "frame fully consumed")
	return out


func test_encoder_matches_server_wire_layout() -> void:
	var payload := "{\"intent\":\"Reviewing: X\"}".to_utf8_buffer()
	var frame: PackedByteArray = Director.encode_action_frame([
		{"source": 0x8000D001, "target": 20, "action": 3, "ts": 123456, "duration_ms": 2000, "payload": payload},
		{"source": 0x8000D002, "target": 21, "action": 0, "ts": 123457, "duration_ms": 0},
	])
	assert_eq(frame.size(), 3 + (2 + 15 + payload.size()) + (2 + 15), "header + two events")
	var evs: Array = _decode(frame)
	assert_eq(evs.size(), 2)
	assert_eq(int(evs[0]["source"]), 0x8000D001, "agent flag survives as u32")
	assert_eq(int(evs[0]["target"]), 20)
	assert_eq(int(evs[0]["action"]), 3)
	assert_eq(int(evs[0]["ts"]), 123456)
	assert_eq(int(evs[0]["dur"]), 2000)
	assert_true(String(evs[0]["payload"]).contains("\"intent\""), "payload carries the intent line")
	assert_eq(String(evs[1]["payload"]), "", "empty payload allowed")


func test_ids_are_reserved_and_named() -> void:
	var ids: PackedInt32Array = Director.demo_wire_ids()
	assert_eq(ids.size(), 6)
	for id: int in ids:
		assert_true(Director.is_demo_id(id), "every synthetic id is in the reserved range")
		var name_s: String = Director.display_name_for(id)
		assert_gt(name_s.length(), 0, "synthetic agent has a display name")
		assert_false(name_s.to_lower().contains("demo"), "names play as real agents (no 'demo' marker)")
	assert_false(Director.is_demo_id(0x80000000 | 42), "a live agent id is never in the reserved range")
	assert_eq(Director.display_name_for(42), "")


func test_start_stagger_real_targets_keepalive_and_stop_retires() -> void:
	var fake := FakeClient.new()
	var d: RefCounted = Director.new()
	assert_true(d.start(fake, Callable(self, "_world_of"), 7), "starts with a loaded graph")
	assert_true(d.is_running())
	d.tick(0.05)
	assert_eq(fake.frames.size(), 1, "first agent starts at t=0, the rest are staggered")
	var first: Array = _decode(fake.frames[0])
	assert_true(Director.is_demo_id(int(first[0]["source"])), "source is a demo wire id")
	assert_between(int(first[0]["target"]), 1, 8, "target is a REAL node id from the candidate pool")
	# JSON.stringify sorts keys, so assert on the keys, not their order — the Rust
	# extractor (extract_action_task) reads "intent" by name.
	var payload_s: String = String(first[0]["payload"])
	assert_true(payload_s.contains("\"intent\":\""), "intent caption in payload")
	assert_false(payload_s.to_lower().contains("demo"), "frames are indistinguishable from a live swarm's")
	# All six have started by 9.2 s; keep-alives keep evidence under the 30 s TTL.
	var t := 0.0
	while t < 9.5:
		d.tick(0.1)
		t += 0.1
	var sources: Dictionary = {}
	for f: PackedByteArray in fake.frames:
		for ev: Dictionary in _decode(f):
			sources[int(ev["source"])] = true
	assert_eq(sources.size(), 6, "all six demo agents have acted")
	var frames_at_9s: int = fake.frames.size()
	while t < 14.7:
		d.tick(0.1)
		t += 0.1
	assert_gt(fake.frames.size(), frames_at_9s, "keep-alive actions continue while working")
	# Timestamps are strictly increasing and follow the server clock.
	var last_ts := -1
	for f: PackedByteArray in fake.frames:
		for ev: Dictionary in _decode(f):
			assert_gt(int(ev["ts"]), last_ts, "monotonic timestamps")
			last_ts = int(ev["ts"])
	assert_gte(last_ts, fake.clock, "stamped from the registry's server clock")
	# Stop retires exactly the demo ids.
	d.stop()
	assert_false(d.is_running())
	assert_eq(fake.retired, Director.demo_wire_ids(), "stop retires the six demo records")


func _world_of(p: Vector3) -> Vector3:
	return p * 0.03


func test_multi_minute_loop_completes_rests_and_retasks_along_edges() -> void:
	var fake := FakeClient.new()
	var d: RefCounted = Director.new()
	assert_true(d.start(fake, Callable(self, "_world_of"), 11))
	var t := 0.0
	while t < 150.0:
		d.tick(0.1)
		t += 0.1
	# Every agent has reported done at least once via the real state channel …
	var done_ids: Dictionary = {}
	for s: Array in fake.states:
		assert_eq(String(s[1]), "done")
		assert_true(String(s[2]).begins_with("Done: "))
		done_ids[int(s[0])] = true
	assert_eq(done_ids.size(), 6, "all six completed at least once in 150 s")
	# … and has been re-tasked after resting (more than one distinct target).
	var targets_by_agent: Dictionary = {}
	for f: PackedByteArray in fake.frames:
		for ev: Dictionary in _decode(f):
			var src: int = int(ev["source"])
			if not targets_by_agent.has(src):
				targets_by_agent[src] = {}
			targets_by_agent[src][int(ev["target"])] = true
	for src: int in targets_by_agent:
		assert_gt((targets_by_agent[src] as Dictionary).size(), 1, "agent %X visited more than one node" % src)
	# Hand-offs follow real edges: for consecutive distinct targets of one agent
	# that were a "follow", the pair is adjacent in the fake ring graph.
	var adjacent := 0
	var jumps := 0
	var seq: Dictionary = {}
	for f: PackedByteArray in fake.frames:
		for ev: Dictionary in _decode(f):
			var src: int = int(ev["source"])
			var tgt: int = int(ev["target"])
			if seq.has(src) and int(seq[src]) != tgt:
				var a: int = int(seq[src])
				if absi(a - tgt) == 1 or absi(a - tgt) == 7:
					adjacent += 1
				else:
					jumps += 1
			seq[src] = tgt
	assert_gt(adjacent, 0, "at least one real-edge hand-off happened (%d adjacent, %d jumps)" % [adjacent, jumps])


func test_start_refuses_without_a_client_but_runs_on_an_edgeless_graph() -> void:
	var d: RefCounted = Director.new()
	assert_false(d.start(null, Callable(self, "_world_of")), "no client")
	var edgeless := FakeClient.new()
	edgeless.edges = PackedInt32Array()
	var d2: RefCounted = Director.new()
	assert_true(d2.start(edgeless, Callable(self, "_world_of"), 3), "nodes without edges still start (no hand-offs)")
	d2.stop()
	assert_eq(edgeless.retired.size(), 6)
