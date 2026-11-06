extends "res://addons/gut/test.gd"

# Characterisation of XR interactive filtering (ADR-2188 D4, ADR-2183 P3).
# Drives the real gdext BinaryProtocolClient through the same doors the socket
# uses (ingest_text / ingest / handle_settings_text, as graph_parity.gd routes a
# settingsUpdated frame) and the HUD uses (set_type_visible).
#
# Pre-ADR-2188 behaviour, recorded before the change: the received nodeFilter and
# the HUD type show/hide filter REMOVED nodes from the node buffer, and their
# edges failed the both-endpoints-drawn test and vanished.


func _client() -> RefCounted:
	assert_true(ClassDB.class_exists("BinaryProtocolClient"), "gdext library loaded")
	return BinaryProtocolClient.create()


# V3 frame: [0x03] + 52-byte records, little-endian (binary_protocol.rs layout).
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
		b.put_u32(0)
		b.put_float(0.0)
		b.put_u32(0)
		b.put_float(0.5)
	return b.data_array


func _seed(client: RefCounted) -> void:
	client.ingest_text(JSON.stringify({
		"type": "initialGraphLoad",
		"nodes": [
			{"id": 1, "label": "Good", "metadata": {"quality_score": "0.9"}},
			{"id": 2, "label": "Weak", "metadata": {"quality_score": "0.1"}},
			{"id": 3, "label": "Also good", "metadata": {"quality_score": "0.8"}},
		],
		"edges": [
			{"id": "a", "source_id": 1, "target_id": 2},
			{"id": "b", "source_id": 1, "target_id": 3},
		],
		"timestamp": 1,
	}))
	client.ingest(_v3([
		{"id": 1, "pos": Vector3(0, 0, 0)},
		{"id": 2, "pos": Vector3(10, 0, 0)},
		{"id": 3, "pos": Vector3(0, 10, 0)},
	]))


func _quality_filter() -> String:
	return JSON.stringify({
		"type": "settingsUpdated", "category": "nodeFilter", "updatedBy": "peer",
		"timestamp": 10,
		"settings": {"enabled": true, "qualityThreshold": 0.5, "filterByQuality": true,
			"filterByAuthority": false, "filterMode": "and", "includeLinkedPages": true},
	})


func _nodes(client: RefCounted) -> int:
	return client.build_node_buffer(PackedInt32Array([1, 2, 3]), 1.0, 0.7, 1.9).size() / 20


func _edges(client: RefCounted) -> int:
	return client.build_edge_buffer(PackedInt32Array([1, 2, 1, 3]), 1.0).size() / 16


func test_received_node_filter_removes_the_node_and_its_edges() -> void:
	var client := _client()
	_seed(client)
	assert_eq(_nodes(client), 3, "unfiltered: three nodes drawn")
	assert_eq(_edges(client), 2, "unfiltered: both edges drawn")
	client.handle_settings_text(_quality_filter())
	assert_eq(_nodes(client), 2, "pre-ADR-2188: the weak node is removed")
	assert_eq(_edges(client), 1, "pre-ADR-2188: its edge is removed with it")
	assert_eq(int(client.filter_hidden_count()), 1)


func test_type_filter_removes_the_class() -> void:
	var client := _client()
	_seed(client)
	assert_eq(_nodes(client), 3)
	# Unflagged wire ids decode as class 3 (plain, binary_protocol.rs node_class_code).
	client.set_type_visible(3, false)
	assert_eq(_nodes(client), 0, "pre-ADR-2188: a hidden class is removed")
	assert_eq(_edges(client), 0)
	client.set_type_visible(3, true)
	assert_eq(_nodes(client), 3)
