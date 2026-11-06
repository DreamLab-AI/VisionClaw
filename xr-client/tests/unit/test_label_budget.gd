extends "res://addons/gut/test.gd"

# ADR-2188 D5 (ADR-2186): the priority-ranked label budget, and ADR-2188 D4's
# highlight writer. The pure selection is tested directly; the ranking goes
# through the real gdext BinaryProtocolClient (rank_labels).

const LabelBudget := preload("res://scripts/label_budget.gd")
const HighlightWriter := preload("res://scripts/highlight_writer.gd")


func test_budget_and_pool_are_smaller_than_the_desktop_and_keep_the_old_pool() -> void:
	assert_eq(LabelBudget.DEFAULT_BUDGET, 8)
	assert_eq(LabelBudget.PINNED_MAX, 4)
	assert_eq(LabelBudget.POOL_SIZE, 12, "the pool the scene had before ADR-2188")
	assert_lt(LabelBudget.DEFAULT_BUDGET, 40, "ADR-2186's desktop overview tier is 40")
	# Worst-case draw calls: ranked = title only, pinned = title + detail.
	assert_eq(LabelBudget.DEFAULT_BUDGET + 2 * LabelBudget.PINNED_MAX, 16)


func test_pinned_come_first_then_rank_order_up_to_the_budget() -> void:
	var ranked := PackedInt32Array([10, 11, 12, 13, 14, 15, 16, 17, 18, 19])
	var out: PackedInt32Array = LabelBudget.select(PackedInt32Array([99, 98]), ranked, 3)
	assert_eq(out, PackedInt32Array([99, 98, 10, 11, 12]), "selected/hovered always shown, then the top 3")


func test_a_pinned_node_is_not_counted_twice_and_pins_are_capped() -> void:
	var out: PackedInt32Array = LabelBudget.select(PackedInt32Array([5, 5, 6, 7, 8, 9]), PackedInt32Array([5, 1, 2]), 2)
	assert_eq(out, PackedInt32Array([5, 6, 7, 8, 1, 2]))


func test_collisions_skip_a_lower_ranked_label_in_a_taken_cell() -> void:
	var cells := {10: Vector2i(0, 0), 11: Vector2i(0, 0), 12: Vector2i(1, 0), 99: Vector2i(1, 0)}
	var out: PackedInt32Array = LabelBudget.select(PackedInt32Array([99]), PackedInt32Array([10, 11, 12, 13]), 3, cells)
	# 11 shares 10's cell; 12 shares the pinned 99's cell; 13 has no cell (never collides).
	assert_eq(out, PackedInt32Array([99, 10, 13]))


func test_cells_are_angular_and_behind_the_eye_is_not_labelled() -> void:
	var cam := Transform3D.IDENTITY   # looking down -Z
	assert_eq(LabelBudget.cell_of(cam, Vector3(0, 0, -2)), Vector2i(0, 0))
	assert_null(LabelBudget.cell_of(cam, Vector3(0, 0, 2)), "behind")
	assert_null(LabelBudget.cell_of(cam, Vector3(5, 0, -1)), "outside the view cone")
	# Same direction, different distance → same cell (an angular grid).
	assert_eq(LabelBudget.cell_of(cam, Vector3(0.2, 0, -1)), LabelBudget.cell_of(cam, Vector3(0.4, 0, -2)))


func test_fade_is_full_inside_and_zero_at_the_reach() -> void:
	assert_eq(LabelBudget.fade(0.3), 1.0)
	assert_eq(LabelBudget.fade(LabelBudget.LABEL_FULL_M), 1.0)
	assert_almost_eq(LabelBudget.fade(1.25), 0.5, 1e-6)
	assert_eq(LabelBudget.fade(LabelBudget.LABEL_REACH_M), 0.0)


# --- ranking through the real client -------------------------------------------

func _client() -> RefCounted:
	assert_true(ClassDB.class_exists("BinaryProtocolClient"), "gdext library loaded")
	return BinaryProtocolClient.create()


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
		b.put_float(float(n["cen"]))
	return b.data_array


func _seeded() -> RefCounted:
	var client := _client()
	client.ingest_text(JSON.stringify({
		"type": "initialGraphLoad",
		"nodes": [
			{"id": 1, "label": "Hub", "metadata": {"quality_score": "0.9"}},
			{"id": 2, "label": "Mid", "metadata": {"quality_score": "0.9"}},
			{"id": 3, "label": "Leaf", "metadata": {"quality_score": "0.1"}},
			{"id": 4, "label": "", "metadata": {}},
			{"id": 5, "label": "Tie", "metadata": {"quality_score": "0.9"}},
		],
		"edges": [
			{"id": "a", "source_id": 2, "target_id": 1},
			{"id": "b", "source_id": 5, "target_id": 1},
			{"id": "c", "source_id": 5, "target_id": 3},
		],
		"timestamp": 1,
	}))
	client.ingest(_v3([
		{"id": 1, "pos": Vector3(0, 0, 0), "cen": 0.9},
		{"id": 2, "pos": Vector3(1, 0, 0), "cen": 0.5},
		{"id": 3, "pos": Vector3(2, 0, 0), "cen": 0.95},
		{"id": 4, "pos": Vector3(3, 0, 0), "cen": 1.0},
		{"id": 5, "pos": Vector3(4, 0, 0), "cen": 0.5},
	]))
	return client


func test_rank_is_centrality_then_degree_and_drops_unlabelled() -> void:
	var client := _seeded()
	var ranked: PackedInt32Array = client.rank_labels(PackedInt32Array([2, 5, 4, 1, 3]))
	# 4 has no label; 3 (0.95) > 1 (0.9) > {2, 5} at 0.5, where 5 has degree 2 > 2's 1.
	assert_eq(ranked, PackedInt32Array([3, 1, 5, 2]))


func test_a_greyed_node_gets_no_label() -> void:
	var client := _seeded()
	client.handle_settings_text(JSON.stringify({
		"type": "settingsUpdated", "category": "nodeFilter", "updatedBy": "peer", "timestamp": 10,
		"settings": {"enabled": true, "qualityThreshold": 0.5, "filterByQuality": true,
			"filterByAuthority": false, "filterMode": "and", "includeLinkedPages": true},
	}))
	client.build_node_buffer(PackedInt32Array([1, 2, 3, 4, 5]), 1.0, 0.7, 1.9)
	assert_eq(int(client.highlight_level_of(3)), 0)
	assert_eq(client.rank_labels(PackedInt32Array([1, 2, 3, 5])), PackedInt32Array([1, 5, 2]),
		"the filtered-out leaf drops from the ranking (ADR-2183 D3)")


# --- the highlight writer ------------------------------------------------------

class RecordingClient:
	extends RefCounted
	var calls: Array = []
	func set_highlight_ids(bit: int, ids: PackedInt32Array) -> bool:
		calls.append([bit, ids])
		return true


func test_writer_calls_rust_only_when_the_ids_change() -> void:
	var w: RefCounted = HighlightWriter.new()
	var c := RecordingClient.new()
	assert_true(w.write(&"vc", c, HighlightWriter.HOVERED, PackedInt32Array([7])))
	assert_false(w.write(&"vc", c, HighlightWriter.HOVERED, PackedInt32Array([7])), "same hover: no call")
	assert_true(w.write(&"vc", c, HighlightWriter.HOVERED, PackedInt32Array()))
	assert_false(w.write(&"vc", c, HighlightWriter.FILTERED_OUT, PackedInt32Array([1])), "the filters own that bit")
	assert_eq(c.calls.size(), 2)
	w.forget(&"vc")
	assert_true(w.write(&"vc", c, HighlightWriter.HOVERED, PackedInt32Array()), "after a reset the write goes out again")


func test_neighbours_are_direct_unique_and_capped() -> void:
	var pairs := PackedInt32Array([1, 2, 3, 1, 1, 2, 4, 5, 1, 1, 6, 1])
	assert_eq(HighlightWriter.neighbours_of(pairs, 1), PackedInt32Array([2, 3, 6]))
	assert_eq(HighlightWriter.neighbours_of(pairs, 1, 2), PackedInt32Array([2, 3]))
	assert_eq(HighlightWriter.neighbours_of(pairs, 9), PackedInt32Array())


func test_hover_holds_briefly_after_the_ray_leaves() -> void:
	assert_true(HighlightWriter.hover_live(1000, 1000 + HighlightWriter.HOVER_HOLD_MS))
	assert_false(HighlightWriter.hover_live(1000, 1001 + HighlightWriter.HOVER_HOLD_MS))
	assert_false(HighlightWriter.hover_live(-1, 0))


func test_bits_match_the_rust_mask() -> void:
	var src: String = FileAccess.get_file_as_string("res://rust/src/highlight.rs")
	for pair: Array in [["SELECTED", 0], ["HOVERED", 1], ["SEARCH_HIT", 2], ["FILTERED_OUT", 3], ["FOCUS_NEIGHBOUR", 4]]:
		assert_true(src.contains("pub const %s: u8 = 1 << %d;" % [pair[0], pair[1]]), "%s is bit %d in Rust" % pair)
	assert_eq(HighlightWriter.SELECTED, 1)
	assert_eq(HighlightWriter.HOVERED, 2)
	assert_eq(HighlightWriter.SEARCH_HIT, 4)
	assert_eq(HighlightWriter.FILTERED_OUT, 8)
	assert_eq(HighlightWriter.FOCUS_NEIGHBOUR, 16)
