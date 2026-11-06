extends "res://addons/gut/test.gd"

# XR interactive filtering (ADR-2188 D4, ADR-2183 P3).
# Drives the real gdext BinaryProtocolClient through the same doors the socket
# uses (ingest_text / ingest / handle_settings_text, as graph_parity.gd routes a
# settingsUpdated frame) and the HUD uses (set_type_visible).
#
# Before ADR-2188 (characterised in commit 8b48931ac, which this file pinned
# first): the received nodeFilter and the HUD type show/hide filter REMOVED nodes
# from the node buffer, and their edges vanished. Now an interactive filter greys
# out: the node stays drawn (highlight level 0, custom.a = 0, desaturated colour)
# and its edges stay drawn with the grey flag in custom.r.


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


func _custom_a(buf: PackedFloat32Array, i: int) -> float:
	return buf[i * 20 + 19]


func test_received_node_filter_greys_the_node_and_keeps_its_edges() -> void:
	var client := _client()
	_seed(client)
	assert_eq(_nodes(client), 3, "unfiltered: three nodes drawn")
	assert_eq(_edges(client), 2, "unfiltered: both edges drawn")
	client.handle_settings_text(_quality_filter())
	var buf: PackedFloat32Array = client.build_node_buffer(PackedInt32Array([1, 2, 3]), 1.0, 0.7, 1.9)
	assert_eq(buf.size() / 20, 3, "the weak node is still drawn")
	assert_eq(int(client.highlight_level_of(2)), 0, "…greyed out")
	assert_eq(_custom_a(buf, 1), 0.0, "custom.a carries the grey level to the shaders")
	assert_eq(_custom_a(buf, 0), 1.0, "a passing node is normal")
	assert_eq(int(client.filter_hidden_count()), 1, "the filter still counts what it excludes")
	assert_eq(int(client.filtered_out_count()), 1)
	var ebuf: PackedFloat32Array = client.build_edge_buffer(PackedInt32Array([1, 2, 1, 3]), 1.0)
	assert_eq(ebuf.size() / 16, 2, "its edge stays")
	assert_eq(ebuf[12], 1.0, "1-2 has a greyed end (custom.r)")
	assert_eq(ebuf[16 + 12], 0.0, "1-3 does not")


func test_type_filter_greys_the_class_and_reshowing_restores_it() -> void:
	var client := _client()
	_seed(client)
	assert_eq(_nodes(client), 3)
	# Unflagged wire ids decode as class 3 (plain, binary_protocol.rs node_class_code).
	client.set_type_visible(3, false)
	assert_eq(_nodes(client), 3, "a hidden class is greyed, not removed")
	assert_eq(_edges(client), 2)
	assert_eq(int(client.filtered_out_count()), 3)
	client.set_type_visible(3, true)
	assert_eq(_nodes(client), 3)
	assert_eq(int(client.filtered_out_count()), 0)


func test_selection_outranks_the_filter_and_search_does_not() -> void:
	var client := _client()
	_seed(client)
	client.handle_settings_text(_quality_filter())
	_nodes(client)
	assert_eq(int(client.highlight_level_of(2)), 0)
	assert_true(client.set_highlight_ids(4, PackedInt32Array([2])), "SEARCH_HIT written")
	assert_eq(int(client.highlight_level_of(2)), 0, "a search hit cannot pull a filtered node back")
	assert_true(client.set_highlight_ids(1, PackedInt32Array([2])), "SELECTED written")
	assert_eq(int(client.highlight_level_of(2)), 2, "the selected node is emphasised even when filtered")
	assert_false(client.set_highlight_ids(8, PackedInt32Array([1])), "FILTERED_OUT is the filters' bit")
	assert_eq(int(client.highlight_level_of(1)), 1)
