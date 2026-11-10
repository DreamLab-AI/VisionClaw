extends Node
## Used via `const BackendConnection := preload("res://scripts/backend_connection.gd")`
## rather than a class_name, so a headless run with a stale global class cache
## still resolves it (see backend_caps.gd).

## One content source's graph connection (ADR-2159; XR phase-1 contract §2).
##
## GraphScene holds one of these per source (`&"visionclaw"`, `&"codescape"`),
## each with its own BinaryProtocolClient, so two backends stream side by side
## and never share wire ids, caps, reconnect backoff, pins or HTTP requests. The
## P3-pre preflight (`rust/tests/two_clients.rs`) showed two clients share the
## process runtime without stalling each other.
##
## A Node so it can own its HTTPRequest children and be freed with the scene.
## Presence stays on the scene: one socket per joined room (ADR-2159 D3).
##
## Generation: `+1` on every (re)connect, every close and every initialGraphLoad.
## Wire ids are valid only within one generation; consumers compare the value
## they cached against `generation` before trusting an id.

const BackendCaps := preload("res://scripts/backend_caps.gd")

const GRAPH_STREAM_PATH := "/wss"
## ADR-2143: Codescape is served under this prefix on the same port as VisionClaw.
const CODESCAPE_PATH := "/codescape"
const RECONNECT_BASE_DELAY_SEC: float = 2.0
const RECONNECT_MAX_DELAY_SEC: float = 60.0
## Each scheduled delay is shortened by up to this fraction at random, so headsets
## that dropped together do not redial a recovering server in lockstep.
const RECONNECT_JITTER: float = 0.2
const BACKEND_NAMES := {"visionclaw": "VisionClaw", "codescape": "Codescape"}

signal connected_changed(conn_id: StringName, connected: bool)
signal topology_changed(conn_id: StringName, generation: int)
signal text_received(conn_id: StringName, msg_type: String, json: String)
signal refused(conn_id: StringName, cap: String, label: String)
signal generation_changed(conn_id: StringName, generation: int)

var conn_id: StringName
var mode: String = "visionclaw"
var ws_base: String = ""
var generation: int = 0
var caps: Dictionary = {}
var client: RefCounted = null

var _nostr_secret_hex: String = ""
var _connected: bool = false
## False after close(): a dropped socket then never schedules a reconnect.
var _open: bool = false
var _reconnect_attempts: int = 0
var _reconnect_timer: float = -1.0
## True from a dial until the client reports the socket up or down: resume()
## never stacks a second dial on one in flight.
var _dialling: bool = false
## Fraction of jitter applied to each backoff delay (tests set 0 for exact timings).
var reconnect_jitter: float = RECONNECT_JITTER
var _rng := RandomNumberGenerator.new()
## Node ids this connection pinned explicitly on its server (A6 hygiene: released on
## close). Nothing writes it until a client-side Pin sender exists (ADR-2172 F5):
## a drag end no longer records a pin.
var _pinned: Dictionary = {}
var _http: Array[HTTPRequest] = []


## Build a connection. `client_override` is for tests (a fake that records
## calls); production passes nothing and gets a fresh BinaryProtocolClient, so
## two connections never share one.
static func make(id: StringName, backend_mode: String, base: String, nostr_secret_hex: String, client_override: RefCounted = null) -> Node:
	var conn: Node = (load("res://scripts/backend_connection.gd") as GDScript).new()
	conn.conn_id = id
	conn.name = "Conn_%s" % String(id).replace(":", "_")
	conn.mode = backend_mode
	conn.ws_base = with_backend_prefix(base, backend_mode)
	conn.caps = BackendCaps.for_mode(backend_mode)
	conn._nostr_secret_hex = nostr_secret_hex
	conn._rng.randomize()
	var c: RefCounted = client_override
	if c == null and ClassDB.class_exists("BinaryProtocolClient"):
		c = BinaryProtocolClient.create()
	conn._bind_client(c)
	return conn


## Pure: strip a trailing "/" and, for "codescape", append CODESCAPE_PATH once.
static func with_backend_prefix(base: String, backend_mode: String) -> String:
	var b: String = base.rstrip("/")
	if backend_mode == "codescape" and not b.ends_with(CODESCAPE_PATH):
		b += CODESCAPE_PATH
	return b


## Pure: the reconnect delay after `attempts` failures (2 s doubling to 60 s).
static func backoff_delay(attempts: int) -> float:
	return minf(RECONNECT_BASE_DELAY_SEC * pow(2.0, float(maxi(attempts, 1) - 1)), RECONNECT_MAX_DELAY_SEC)


## Pure: `backoff_delay(attempts)` shortened by `jitter × u01` of itself
## (u01 in [0, 1]), so it never exceeds the nominal delay or the cap.
static func jittered_delay(attempts: int, jitter: float, u01: float) -> float:
	return backoff_delay(attempts) * (1.0 - clampf(jitter, 0.0, 1.0) * clampf(u01, 0.0, 1.0))


## Seed the jitter (tests); production seeds randomly in make().
func set_jitter_seed(seed_value: int) -> void:
	_rng.seed = seed_value


func _bind_client(c: RefCounted) -> void:
	client = c
	if client == null:
		return
	if client.has_signal("connection_changed"):
		client.connect("connection_changed", _on_client_connection_changed)
	if client.has_signal("topology_updated"):
		client.connect("topology_updated", _on_client_topology_updated)
	if client.has_signal("text_message"):
		client.connect("text_message", _on_client_text)
	if client.has_signal("stream_live"):
		client.connect("stream_live", _on_client_stream_live)


func graph_ws_url() -> String:
	return ws_base + GRAPH_STREAM_PATH


func display_name() -> String:
	return str(BACKEND_NAMES.get(mode, mode))


## Connect `/wss` and reset the reconnect backoff.
func open() -> void:
	_open = true
	_reconnect_attempts = 0
	_reconnect_timer = -1.0
	_dial()


## Tear the connection down: cancel every request it owns, release the pins it
## holds on its server while the socket is still up, close the socket and bump
## the generation. The client's close() aborts the pump without queueing a
## Disconnected event, so the state change is announced here.
func close() -> void:
	_open = false
	_dialling = false
	_reconnect_timer = -1.0
	_reconnect_attempts = 0
	for req: HTTPRequest in _http:
		if is_instance_valid(req):
			req.cancel_request()
	release_pins()
	if client != null and client.has_method("close"):
		client.close()
	_set_connected(false)
	_bump_generation()


## Scene calls this once per frame: pumps the client and the reconnect backoff.
func poll(delta: float) -> void:
	if client != null and client.has_method("poll"):
		client.poll()
	if _reconnect_timer >= 0.0:
		_reconnect_timer -= delta
		if _reconnect_timer <= 0.0:
			_reconnect_timer = -1.0
			if _open:
				_dial()


func is_connected_now() -> bool:
	return _connected


func reconnect_pending() -> bool:
	return _reconnect_timer >= 0.0


func reconnect_attempts() -> int:
	return _reconnect_attempts


## The app or the XR session came back (headset on again, app resumed or
## refocused). A connection that is down redials now, whatever its backoff had
## grown to while the headset was off, and starts its backoff again. A connection
## that looks up is left alone: a redial costs a full snapshot, and one that is
## really dead is closed by the client's read deadline (transport.rs, liveness.rs)
## and then redialled here. True when a dial went out.
func resume() -> bool:
	if not _open or _connected or _dialling:
		return false
	_reconnect_attempts = 0
	_reconnect_timer = -1.0
	_dial()
	return true


func supports(cap: String) -> bool:
	return BackendCaps.supports(caps, cap)


## One gate for every server-bound request on this connection: true when the
## backend serves `cap`. Otherwise nothing is sent, and unless `quiet` the
## refusal is announced through `refused` for the scene's HUD notice.
func gate(cap: String, label: String, quiet := false) -> bool:
	if supports(cap):
		return true
	if not quiet:
		refused.emit(conn_id, cap, label)
	return false


## HTTP origin for this backend: `XR_BACKEND_HTTP` (prefixed for Codescape) when
## set, else the ws base with its scheme swapped (wss → https, ws → http).
func http_base() -> String:
	var override: String = OS.get_environment("XR_BACKEND_HTTP")
	if override != "":
		return with_backend_prefix(override, mode)
	return ws_to_http(ws_base)


static func ws_to_http(ws: String) -> String:
	if ws.begins_with("wss://"):
		return "https://" + ws.substr(6)
	if ws.begins_with("ws://"):
		return "http://" + ws.substr(5)
	return ws


## A request owned by this connection: cancelled by close(), freed with it.
func make_http(req_name: String, timeout_sec: float) -> HTTPRequest:
	var req := HTTPRequest.new()
	req.name = req_name
	req.timeout = timeout_sec
	add_child(req)
	_http.append(req)
	return req


func http_requests() -> Array[HTTPRequest]:
	return _http


func node_ids_for_mask(mask: int) -> PackedInt32Array:
	if client != null and client.has_method("node_ids_for_mask"):
		return client.node_ids_for_mask(mask)
	return PackedInt32Array()


## Edge pairs inside `mask`, heaviest first, at most `max_pairs` (flattened).
## Edges with both ends in `mask`, before any cap (ADR-2188 D2 dropped-edge
## count); -1 when the client cannot say.
func edge_count_for_mask(mask: int) -> int:
	if client != null and client.has_method("edge_count_for_mask"):
		return int(client.edge_count_for_mask(mask))
	return -1


func edges_for_mask(mask: int, max_pairs: int) -> PackedInt32Array:
	if client != null and client.has_method("edges_for_mask"):
		return client.edges_for_mask(mask, max_pairs)
	return PackedInt32Array()


## Class code of a node (0 knowledge, 1 ontology, 2 agent, 3 plain), -1 unknown.
func class_of(id: int) -> int:
	if client != null and client.has_method("class_of"):
		return int(client.class_of(id))
	return -1


## Full-extent AABB of `ids` in graph units, excluding every id in `exclude` (the
## nodes a hand holds; empty = none). An empty AABB when none of the ids is known.
func render_aabb_for_ids(ids: PackedInt32Array, exclude: PackedInt32Array = PackedInt32Array()) -> AABB:
	if client == null or not client.has_method("render_aabb_ids"):
		return AABB()
	return aabb_from_flat(client.render_aabb_ids(ids, exclude))


## Pure: `[minx, miny, minz, maxx, maxy, maxz]` → AABB; empty AABB otherwise.
static func aabb_from_flat(bb: PackedFloat32Array) -> AABB:
	if bb.size() < 6:
		return AABB()
	var lo := Vector3(bb[0], bb[1], bb[2])
	var hi := Vector3(bb[3], bb[4], bb[5])
	return AABB(lo, hi - lo)


func node_position(id: int) -> Vector3:
	if client != null and client.has_method("node_position"):
		return client.node_position(id)
	return Vector3.ZERO


## Drag is server-authoritative on VisionClaw and unserved on Codescape (E9):
## a refused drag sends nothing. Only the start announces the refusal; update
## and end stay quiet so one gesture yields one notice.
## Ask the server to grey out nodes below the quality threshold for this viewer
## (ADR-2183 D1, ADR-2188 D4). The payload mirrors the desktop's `filter_update`
## (client/src/store/websocket/index.ts) and the fallbacks in
## src/handlers/socket_flow_handler/filter_auth.rs. The server answers with a
## filtered initialGraphLoad and `filter_update_success`, which graph_parity.gd
## already handles. `server_remove` stays false: the headset only ever greys.
func send_filter_update(enabled: bool, quality_threshold: float = 0.4) -> bool:
	if client == null or not client.has_method("send_text"):
		return false
	var payload := {
		"type": "filter_update",
		"enabled": enabled,
		"quality_threshold": quality_threshold,
		"authority_threshold": 0.5,
		"filter_by_quality": enabled,
		"filter_by_authority": false,
		"filter_mode": "and",
		"include_linked_pages": false,
		"server_remove": false,
	}
	client.send_text(JSON.stringify(payload))
	return true


func send_drag_start(id: int, p: Vector3) -> bool:
	if not gate(BackendCaps.DRAG, "drag a node"):
		return false
	if client == null or not client.has_method("send_drag_start"):
		return false
	client.send_drag_start(id, p)
	return true


func send_drag_update(id: int, p: Vector3) -> bool:
	if not gate(BackendCaps.DRAG, "drag a node", true):
		return false
	if client == null or not client.has_method("send_drag_update"):
		return false
	client.send_drag_update(id, p)
	return true


func send_drag_end(id: int) -> bool:
	if not gate(BackendCaps.DRAG, "drag a node", true):
		return false
	if client == null or not client.has_method("send_drag_end"):
		return false
	client.send_drag_end(id)
	# ADR-2172 F3: a drag end records nothing. The server releases the node into the
	# simulation (it does not pin it), so there is no pin for close() to undo; a
	# recorded id made every teardown send a nodeUnpin (and a reheat) per node ever dragged.
	return true


## Release one pin this connection holds. False (nothing sent) when the backend
## does not acknowledge pins.
func send_unpin(id: int) -> bool:
	if not gate(BackendCaps.PIN, "unpin a node", true):
		return false
	if client == null or not client.has_method("send_node_unpin"):
		return false
	client.send_node_unpin(id)
	_pinned.erase(id)
	return true


func pinned_ids() -> Array:
	return _pinned.keys()


## Release every pin this connection holds (A6 hygiene). Pins on a backend
## that never acknowledges them are simply forgotten.
func release_pins() -> void:
	if supports(BackendCaps.PIN) and client != null and client.has_method("send_node_unpin"):
		for id: int in _pinned.keys():
			client.send_node_unpin(id)
	_pinned.clear()


## `{iri: PackedInt32Array of wire ids}` for the current generation (D's link layer).
func iri_table() -> Dictionary:
	if client != null and client.has_method("iri_table"):
		return client.iri_table()
	return {}


## `[{key, id, path, is_file, refs: [[iri, line], …]}]` for the current generation.
func code_refs() -> Array:
	if client != null and client.has_method("code_refs"):
		return client.code_refs()
	return []


func _dial() -> void:
	if client == null:
		return
	_dialling = true
	if client.has_method("close"):
		client.close()
	if client.has_method("connect_to_url"):
		client.connect_to_url(graph_ws_url(), _nostr_secret_hex)
	_bump_generation()


func _bump_generation() -> void:
	generation += 1
	generation_changed.emit(conn_id, generation)


func _set_connected(on: bool) -> void:
	if _connected == on:
		return
	_connected = on
	connected_changed.emit(conn_id, on)


func _schedule_reconnect() -> void:
	_reconnect_attempts += 1
	_reconnect_timer = jittered_delay(_reconnect_attempts, reconnect_jitter, _rng.randf())
	push_warning("BackendConnection %s: reconnect attempt %d in %.1fs" % [conn_id, _reconnect_attempts, _reconnect_timer])


func _on_client_connection_changed(on: bool) -> void:
	_dialling = false
	if on:
		# The upgrade alone does not reset the backoff (audit D3): a server that
		# accepts and then closes would otherwise be redialled every 2 s for ever.
		# _on_client_stream_live resets it once a position frame is applied.
		_reconnect_timer = -1.0
		_set_connected(true)
		return
	_set_connected(false)
	# Pins are kept: a drag-end pin persists server-side (fx/fy/fz) across a
	# reconnect to the same server, so it is still ours to release on close().
	if _open and _reconnect_timer < 0.0:
		_schedule_reconnect()


## The client applied this connection's first position frame: the server is
## really serving us, so the next failure starts the backoff from 2 s again.
func _on_client_stream_live() -> void:
	_reconnect_attempts = 0


func _on_client_topology_updated(_edge_count: int) -> void:
	_bump_generation()
	topology_changed.emit(conn_id, generation)


func _on_client_text(json: String) -> void:
	# JSON.parse (instance) returns an error code; parse_string logs an engine
	# error for every non-JSON frame.
	var parser := JSON.new()
	var msg_type: String = ""
	if parser.parse(json) == OK and parser.data is Dictionary:
		msg_type = str((parser.data as Dictionary).get("type", ""))
	text_received.emit(conn_id, msg_type, json)
