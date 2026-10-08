extends "res://addons/gut/test.gd"

# WP1 domain colour toggle, WP2 settings/filter sync, WP4 cluster hulls.
# The data path goes through the real gdext BinaryProtocolClient (ingest_text /
# ingest = the socket's own classify → poll and frame-decode doors), so these
# need the native library built for this host (see xr-client/README.md).

const ParityScript := preload("res://scripts/graph_parity.gd")
const TABS := "HudViewport/HudControl/Root/Tabs"


# Stands in for GraphScene: exactly the members graph_parity.gd reads/writes.
class StubScene:
	extends Node
	var _selection_dirty: bool = false
	var _physics_pending: bool = false
	var _repel_k: float = 190.0
	var _rest_length: float = 85.0
	var _dag_bias_on: bool = false
	var _dag_level_distance: float = 60.0
	var _plane_bias_k: float = 0.0
	var _plane_spacing: float = 60.0
	var _z_compression: float = 1.0
	var refreshed: int = 0
	func _refresh_controls_status() -> void:
		refreshed += 1
	func _http_base() -> String:
		return "http://127.0.0.1:9"
	func _auth_headers(_url: String, _method: String) -> PackedStringArray:
		return PackedStringArray()


class StubHud:
	extends Node
	var modes: Array = []
	var notices: Array = []
	func set_visual_modes(color_mode: int, hull_source: int, hull_count: int = -1) -> void:
		modes.append([color_mode, hull_source, hull_count])
	func flash_notice(text: String, _seconds: float = 8.0) -> void:
		notices.append(text)


func _client() -> RefCounted:
	assert_true(ClassDB.class_exists("BinaryProtocolClient"), "gdext library loaded")
	return BinaryProtocolClient.create()


# V3 frame: [0x03] + 52-byte records (id, pos×3, vel×3, sssp, parent, cluster,
# anomaly, community, centrality), little-endian — binary_protocol.rs layout.
func _v3(nodes: Array) -> PackedByteArray:
	var b := StreamPeerBuffer.new()
	b.big_endian = false
	b.put_u8(3)
	for n: Dictionary in nodes:
		b.put_u32(int(n["id"]))
		var p: Vector3 = n["pos"]
		b.put_float(p.x); b.put_float(p.y); b.put_float(p.z)
		b.put_float(0.0); b.put_float(0.0); b.put_float(0.0)
		b.put_float(0.0)
		b.put_32(-1)
		b.put_u32(int(n.get("cluster", 0)))
		b.put_float(0.0)
		b.put_u32(int(n.get("community", 0)))
		b.put_float(0.5)
	return b.data_array


func _drawn(client: RefCounted, ids: Array) -> int:
	var buf: PackedFloat32Array = client.build_node_buffer(PackedInt32Array(ids), 1.0, 0.7, 1.9)
	return buf.size() / 20


func _seed_graph(client: RefCounted) -> void:
	client.ingest_text(JSON.stringify({
		"type": "initialGraphLoad",
		"nodes": [
			{"id": 1, "label": "Good", "metadata": {"quality_score": "0.9", "source_domain": "robotics"}},
			{"id": 2, "label": "Weak", "metadata": {"quality_score": "0.1"}},
			{"id": 3, "label": "Stub", "metadata": {"type": "linked_page", "quality_score": "0.95"}},
		],
		"edges": [{"id": "e", "source_id": 1, "target_id": 2}],
		"timestamp": 1,
	}))
	client.ingest(_v3([
		{"id": 1, "pos": Vector3(0, 0, 0)},
		{"id": 2, "pos": Vector3(10, 0, 0)},
		{"id": 3, "pos": Vector3(0, 10, 0)},
	]))


func _parity_with(client: RefCounted) -> Array:
	var scene := StubScene.new()
	var hud := StubHud.new()
	var root := Node3D.new()
	add_child_autofree(scene)
	add_child_autofree(hud)
	add_child_autofree(root)
	var parity: Node = ParityScript.new()
	add_child_autofree(parity)
	parity.setup(scene, client, root, hud)
	return [parity, scene, hud, root]


# --- WP2 ---------------------------------------------------------------------

func test_node_filter_frame_changes_the_visible_node_count() -> void:
	var client := _client()
	_seed_graph(client)
	assert_eq(_drawn(client, [1, 2, 3]), 3, "no filter received → all three drawn")
	var p: Array = _parity_with(client)
	var parity: Node = p[0]
	var consumed: bool = parity.route_text(JSON.stringify({
		"type": "settingsUpdated", "category": "nodeFilter", "updatedBy": "peer",
		"timestamp": 10,
		"settings": {"enabled": true, "qualityThreshold": 0.5, "filterByQuality": true,
			"filterByAuthority": false, "filterMode": "and", "includeLinkedPages": false},
	}), "settingsUpdated")
	assert_true(consumed)
	assert_eq(str(parity.last_event.get("kind")), "node_filter")
	assert_true(p[1]._selection_dirty, "draw domain marked for rebuild")
	assert_eq(_drawn(client, [1, 2, 3]), 1, "low quality and linked_page nodes hidden")
	assert_eq(int(client.filter_hidden_count()), 2)
	assert_eq(p[2].notices.size(), 1, "operator told why the view changed")


func test_stale_and_own_echo_frames_change_nothing() -> void:
	var client := _client()
	_seed_graph(client)
	client.set_own_pubkey("abcd")
	var parity: Node = _parity_with(client)[0]
	var f := {"type": "settingsUpdated", "category": "nodeFilter", "updatedBy": "peer", "timestamp": 50,
		"settings": {"enabled": false, "includeLinkedPages": true}}
	parity.route_text(JSON.stringify(f), "settingsUpdated")
	assert_eq(_drawn(client, [1, 2, 3]), 3)
	f["timestamp"] = 40
	f["settings"] = {"enabled": true, "filterMode": "and", "qualityThreshold": 0.99}
	parity.route_text(JSON.stringify(f), "settingsUpdated")
	assert_eq(str(parity.last_event.get("reason")), "stale")
	assert_eq(_drawn(client, [1, 2, 3]), 3, "older frame ignored")
	f["timestamp"] = 60
	f["updatedBy"] = "ABCD"
	parity.route_text(JSON.stringify(f), "settingsUpdated")
	assert_eq(str(parity.last_event.get("reason")), "own_echo")
	assert_eq(_drawn(client, [1, 2, 3]), 3, "own echo ignored")


func test_unhandled_types_fall_through_to_graph_scene() -> void:
	var parity: Node = _parity_with(_client())[0]
	assert_false(parity.route_text("{\"type\":\"nodeUnpinAck\"}", "nodeUnpinAck"))
	assert_false(parity.route_text("{\"type\":\"broker:new_case\"}", "broker:new_case"))


func test_physics_category_queues_a_read_and_the_body_syncs_hud_state() -> void:
	var client := _client()
	var p: Array = _parity_with(client)
	var parity: Node = p[0]
	var scene: StubScene = p[1]
	parity.route_text(JSON.stringify({"type": "settingsUpdated", "category": "physics",
		"updatedBy": "peer", "timestamp": 5}), "settingsUpdated")
	assert_eq(str(parity.last_event.get("kind")), "refetch")
	# A local write in flight defers the read (never races the staged commit).
	scene._physics_pending = true
	parity._maybe_fetch_physics()
	assert_eq(parity.physics_refetches, 0)
	scene._physics_pending = false
	parity._maybe_fetch_physics()
	assert_eq(parity.physics_refetches, 1, "GET dispatched once the gate clears")
	# The response body maps onto the tracked HUD state; nothing is written back.
	var view: Dictionary = parity.apply_physics_body(
		"{\"repelK\":320.0,\"restLength\":70.0,\"dagBiasK\":0.6,\"dagLevelDistance\":100.0,\"axisCompressionZ\":0.3,\"graphSeparationX\":250.0}")
	assert_eq(view.size(), 5, "a retired graphSeparationX is ignored (ADR-2135, 2026-10-08)")
	assert_almost_eq(scene._repel_k, 320.0, 0.001)
	assert_almost_eq(scene._rest_length, 70.0, 0.001)
	assert_true(scene._dag_bias_on)
	assert_almost_eq(scene._dag_level_distance, 100.0, 0.001)
	assert_almost_eq(scene._z_compression, 0.3, 0.001)
	assert_false("_graph_separation" in scene, "no separation state to write")
	assert_almost_eq(scene._plane_spacing, 60.0, 0.001, "absent field untouched")
	assert_gt(scene.refreshed, 0, "status line refreshed")


# --- WP1 ---------------------------------------------------------------------

func test_colour_toggle_flips_the_mode_and_the_node_colour() -> void:
	var client := _client()
	_seed_graph(client)
	var p: Array = _parity_with(client)
	var parity: Node = p[0]
	assert_eq(int(client.get_color_mode()), 0, "domain is the default")
	var dom: PackedFloat32Array = client.build_node_buffer(PackedInt32Array([1]), 1.0, 0.7, 1.9)
	# robotics #FFB74D (node 1 has degree 1 → slight lift, red stays ~1.0).
	assert_almost_eq(dom[12], 1.0, 0.02)
	assert_true(parity.handle_control("color_mode_toggle"))
	assert_eq(int(client.get_color_mode()), 1)
	var com: PackedFloat32Array = client.build_node_buffer(PackedInt32Array([1]), 1.0, 0.7, 1.9)
	assert_ne(Color(dom[12], dom[13], dom[14]), Color(com[12], com[13], com[14]), "colour changed")
	assert_eq(p[2].modes.back()[0], 1, "HUD face updated")


func test_hud_colour_and_hull_buttons_fire_on_press_and_fit() -> void:
	var hud: Node3D = load("res://scenes/HUD.tscn").instantiate()
	add_child_autofree(hud)
	await get_tree().process_frame
	await get_tree().process_frame
	var graph: Control = hud.get_node("%s/GraphPage" % TABS)
	var colour := _find_button(graph, "Colour: Domain")
	var hulls := _find_button(graph, "Hulls: Off")
	assert_not_null(colour)
	assert_not_null(hulls)
	assert_eq(colour.action_mode, BaseButton.ACTION_MODE_BUTTON_PRESS, "Invariant 4")
	assert_eq(hulls.action_mode, BaseButton.ACTION_MODE_BUTTON_PRESS, "Invariant 4")
	watch_signals(hud)
	colour.pressed.emit()
	assert_signal_emitted_with_parameters(hud, "control_pressed", ["color_mode_toggle"])
	hulls.pressed.emit()
	assert_signal_emitted_with_parameters(hud, "control_pressed", ["hulls_cycle"])
	hud.set_visual_modes(1, 1, 7)
	assert_eq(colour.text, "Colour: Community")
	assert_eq(hulls.text, "Hulls: Clusters (7)")
	var host: Control = hud.get_node(TABS)
	for page_name in ["GraphPage", "KeyPage"]:
		var need: float = (hud.get_node("%s/%s" % [TABS, page_name]) as Control).get_combined_minimum_size().y
		assert_lte(need, host.size.y + 1.0, "%s fits the page host (Invariant 5)" % page_name)


func test_key_page_lists_every_domain_swatch() -> void:
	var hud: Node3D = load("res://scenes/HUD.tscn").instantiate()
	add_child_autofree(hud)
	await get_tree().process_frame
	var keys: Array = []
	_collect_keys(hud.get_node("%s/KeyPage" % TABS), keys)
	for label in ["AI", "Blockchain", "Robotics", "Spatial", "Collab", "Infra", "Space", "Earth obs", "Other / none", "Cluster hulls"]:
		assert_has(keys, label, "Key row %s" % label)


# --- WP4 ---------------------------------------------------------------------

func test_hull_cycle_builds_one_surface_over_server_clusters() -> void:
	var client := _client()
	var nodes: Array = []
	for i in range(12):
		var a := float(i)
		nodes.append({"id": i + 1, "pos": Vector3(cos(a) * 50.0, sin(a * 1.3) * 50.0, a * 7.0),
			"cluster": 3 if i < 6 else 4})
	client.ingest(_v3(nodes))
	var ids: Array = []
	for i in range(12):
		ids.append(i + 1)
	_drawn(client, ids)
	var p: Array = _parity_with(client)
	var parity: Node = p[0]
	assert_true(parity.handle_control("hulls_cycle"))
	assert_eq(parity.hull_source, 1)
	parity._process(1.0)
	assert_eq(parity.hull_count, 2)
	var inst: MeshInstance3D = (p[3] as Node3D).get_node("ClusterHulls")
	assert_true(inst.visible)
	assert_eq(inst.mesh.get_surface_count(), 1, "all hulls in one surface → one draw call")
	assert_gt(parity.hull_triangles, 0)
	# Off clears the mesh.
	parity.handle_control("hulls_cycle")
	parity.handle_control("hulls_cycle")
	assert_eq(parity.hull_source, 0)
	assert_false(inst.visible)


func test_hull_mesh_from_points_at_the_cap_stays_in_budget() -> void:
	var client := _client()
	var pts := PackedVector3Array()
	var groups := PackedInt32Array()
	for c in range(40):
		for i in range(50):
			var a := float(i) * 2.399
			pts.append(Vector3(c * 80.0 + cos(a) * 20.0, float(i % 9) * 3.0, sin(a) * 20.0))
			groups.append(c + 1)
	var d: Dictionary = client.hull_mesh_from_points(pts, groups, 0.15, 1000)
	assert_eq(int(d["hulls"]), 32, "capped at 32")
	assert_lte(int(d["triangles"]), 32 * 124)
	var mesh: ArrayMesh = ParityScript.make_hull_mesh(d)
	assert_eq(mesh.get_surface_count(), 1)


func _find_button(root: Node, text: String) -> Button:
	for n in root.get_children():
		if n is Button and (n as Button).text == text:
			return n
		var found := _find_button(n, text)
		if found != null:
			return found
	return null


func _collect_keys(root: Node, out: Array) -> void:
	for n in root.get_children():
		if n.has_meta(&"key"):
			out.append(String(n.get_meta(&"key")))
		_collect_keys(n, out)
