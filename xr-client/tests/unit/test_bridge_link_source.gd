extends "res://addons/gut/test.gd"
const BridgeLinkSource := preload("res://scripts/bridge_link_source.gd")

# ADR-2158 D6 / ADR-2164 L5: knowledge↔ontology bridge records. Only edges
# whose two ends sit in different volumes qualify, ranked by weight, at most
# 256. The client and connection are duck-typed fakes.


class FakeClient extends RefCounted:
	var edges := PackedInt32Array()
	var weights := PackedFloat32Array()

	func get_edges() -> PackedInt32Array:
		return edges

	func get_edge_weights() -> PackedFloat32Array:
		return weights


class FakeConn extends RefCounted:
	var conn_id: StringName = &"visionclaw"
	var generation := 3
	var client: RefCounted


func _conn(pairs: Array, weights: Array) -> FakeConn:
	var c := FakeClient.new()
	for p: Array in pairs:
		c.edges.append(p[0])
		c.edges.append(p[1])
	c.weights = PackedFloat32Array(weights)
	var conn := FakeConn.new()
	conn.client = c
	return conn


func test_only_cross_volume_edges_qualify() -> void:
	var by_vol := {
		&"knowledge": PackedInt32Array([1, 2]),
		&"ontology": PackedInt32Array([10, 11]),
	}
	# 1-2 same volume; 1-10 and 11-2 cross; 1-99 has an end in no volume.
	var conn := _conn([[1, 2], [1, 10], [11, 2], [1, 99]], [5.0, 1.0, 2.0, 9.0])
	var recs := BridgeLinkSource.records(conn, by_vol)
	assert_eq(recs.size(), 2)
	assert_eq([recs[0].source.id, recs[0].target.id], [11, 2], "heavier first")
	assert_eq([recs[1].source.id, recs[1].target.id], [1, 10])
	for r: Dictionary in recs:
		assert_ne(r.source.volume, r.target.volume, "ends in different volumes")


func test_record_shape_is_d5() -> void:
	var by_vol := {&"knowledge": PackedInt32Array([4]), &"ontology": PackedInt32Array([7])}
	var recs := BridgeLinkSource.records(_conn([[7, 4]], [0.5]), by_vol)
	assert_eq(recs.size(), 1)
	var r: Dictionary = recs[0]
	assert_eq(r.kind, "bridge")
	assert_eq(r.source, {"volume": "ontology", "conn": "visionclaw", "key": "7", "id": 7})
	assert_eq(r.target, {"volume": "knowledge", "conn": "visionclaw", "key": "4", "id": 4})
	assert_eq(r.provenance.generations, [3, 3], "both ends on the connection's generation")
	assert_almost_eq(float(r.provenance.weight), 0.5, 1e-6)


func test_cap_is_256_ranked_by_weight_with_stable_ties() -> void:
	var know := PackedInt32Array()
	var onto := PackedInt32Array()
	var pairs: Array = []
	var weights: Array = []
	for i: int in 400:
		know.append(i)
		onto.append(1000 + i)
		pairs.append([i, 1000 + i])
		# Weights 0..199 twice: every weight is tied once.
		weights.append(float(i % 200))
	var recs := BridgeLinkSource.records(_conn(pairs, weights),
		{&"knowledge": know, &"ontology": onto})
	assert_eq(recs.size(), BridgeLinkSource.MAX_BRIDGES)
	assert_eq(BridgeLinkSource.MAX_BRIDGES, 256)
	# Highest weight 199 on ids 199 and 399: the lower source id wins the tie.
	assert_eq(recs[0].source.id, 199)
	assert_eq(recs[1].source.id, 399)
	for i: int in range(1, recs.size()):
		assert_true(float(recs[i - 1].provenance.weight) >= float(recs[i].provenance.weight), "ranked")
	# 256 = 128 weights × 2 ties: the lowest weight kept is 72.
	assert_almost_eq(float(recs[-1].provenance.weight), 72.0, 1e-6)


func test_string_volume_keys_and_missing_weights() -> void:
	var by_vol := {"knowledge": PackedInt32Array([1]), "ontology": PackedInt32Array([2])}
	var conn := _conn([[1, 2]], [])
	var recs := BridgeLinkSource.records(conn, by_vol)
	assert_eq(recs.size(), 1)
	assert_almost_eq(float(recs[0].provenance.weight), 1.0, 1e-6, "absent weight reads as 1")


func test_no_client_or_no_volumes_yields_nothing() -> void:
	assert_eq(BridgeLinkSource.records(null, {}), [])
	var conn := FakeConn.new()
	assert_eq(BridgeLinkSource.records(conn, {&"knowledge": PackedInt32Array([1])}), [])
	assert_eq(BridgeLinkSource.records(_conn([[1, 2]], [1.0]), {}), [])
