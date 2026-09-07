extends "res://addons/gut/test.gd"

# Visual query builder state tests (flagship Phase B). Exercise the pure
# QueryBuilder model that graph_scene.gd drives — variable marking, palette
# cycling, and the shared node-menu item model that coexists with future Wave-2
# expansion items.


func test_mark_assigns_sequential_var_names():
	var qb := QueryBuilder.new()
	assert_eq(qb.mark(101), "?v1", "first mark is ?v1")
	assert_eq(qb.mark(202), "?v2", "second mark is ?v2")
	assert_true(qb.is_marked(101) and qb.is_marked(202))
	assert_eq(qb.var_count(), 2)


func test_mark_is_idempotent_per_node():
	var qb := QueryBuilder.new()
	assert_eq(qb.mark(5), "?v1")
	assert_eq(qb.mark(5), "?v1", "re-marking returns the same name, no new var")
	assert_eq(qb.var_count(), 1)


func test_palette_cycles_and_indices_are_distinct_early():
	var qb := QueryBuilder.new()
	qb.mark(1)
	qb.mark(2)
	assert_eq(qb.palette_index(1), 0)
	assert_eq(qb.palette_index(2), 1)
	# The (QUERY_PALETTE_LEN+1)-th variable wraps to palette 0.
	for i in range(QueryBuilder.QUERY_PALETTE_LEN - 2):
		qb.mark(100 + i)
	var wrap_id := 500
	qb.mark(wrap_id)
	assert_eq(qb.palette_index(wrap_id), 0, "palette wraps at QUERY_PALETTE_LEN")


func test_unmark_removes_variable():
	var qb := QueryBuilder.new()
	qb.mark(9)
	assert_true(qb.unmark(9), "unmark reports it removed a variable")
	assert_false(qb.is_marked(9))
	assert_eq(qb.var_name(9), "")
	assert_false(qb.unmark(9), "unmarking an unmarked node returns false")


func test_names_do_not_reuse_after_unmark():
	var qb := QueryBuilder.new()
	qb.mark(1)          # ?v1
	qb.mark(2)          # ?v2
	qb.unmark(1)
	assert_eq(qb.mark(3), "?v3", "next name keeps counting; ?v1 is not reused")


func test_clear_resets_all_state():
	var qb := QueryBuilder.new()
	qb.mark(1)
	qb.mark(2)
	qb.clear()
	assert_false(qb.is_active())
	assert_eq(qb.var_count(), 0)
	assert_eq(qb.mark(7), "?v1", "after clear, numbering restarts at ?v1")


func test_menu_items_unmarked_node_offers_mark():
	var qb := QueryBuilder.new()
	var items := qb.build_node_menu_items(42)
	assert_eq(items.size(), 1, "no query yet: only the Mark action")
	assert_eq(items[0]["action"], "qb_mark:42")
	assert_true(String(items[0]["label"]).contains("?v1"))


func test_menu_items_marked_node_offers_unmark_and_clear():
	var qb := QueryBuilder.new()
	qb.mark(42)
	var items := qb.build_node_menu_items(42)
	var actions: Array = []
	for it in items:
		actions.append(it["action"])
	assert_true(actions.has("qb_unmark:42"), "marked node offers unmark")
	assert_true(actions.has("qb_clear"), "active query offers clear")


func test_execute_and_toggle_offered_when_active():
	# Phase D: Execute is enabled; an active query also offers the edge-type toggle
	# and Clear.
	assert_eq(QueryBuilder.EXECUTE_ENABLED, true, "Phase D: execute wired")
	var qb := QueryBuilder.new()
	qb.mark(1)
	var actions: Array = []
	for it in qb.build_node_menu_items(1):
		actions.append(it["action"])
	assert_true(actions.has("qb_execute"), "Execute offered")
	assert_true(actions.has("qb_toggle_edges"), "edge-type toggle offered")
	assert_true(actions.has("qb_clear"), "Clear offered")


func test_menu_items_pass_through_extra_items_in_order():
	var qb := QueryBuilder.new()
	var extra := [{"label": "Expand: references (12)", "action": "expand:references:outgoing"}]
	var items := qb.build_node_menu_items(1, extra)
	# Section order: variable action, then the extra (Wave-2) item.
	assert_eq(items[0]["action"], "qb_mark:1")
	assert_eq(items[1]["action"], "expand:references:outgoing", "extra items follow verbatim")


func test_derive_triples_only_marked_marked_edges():
	var qb := QueryBuilder.new()
	qb.mark(10)  # ?v1
	qb.mark(20)  # ?v2
	# Edges: 10->20 (both marked, kept), 20->30 (30 unmarked, dropped),
	# 40->10 (40 unmarked, dropped).
	var edges := PackedInt32Array([10, 20, 20, 30, 40, 10])
	var triples := qb.derive_triples(edges)
	assert_eq(triples.size(), 1, "only the marked-marked edge becomes a triple")
	assert_eq(triples[0]["src"], "?v1")
	assert_eq(triples[0]["tgt"], "?v2")
	assert_eq(triples[0]["edgeType"], "any", "v1 edges are wildcard")


func test_derive_triples_is_directed_and_deduped():
	var qb := QueryBuilder.new()
	qb.mark(1)   # ?v1
	qb.mark(2)   # ?v2
	# Two parallel 1->2 edges collapse (wildcard makes them identical); the 2->1
	# edge is a distinct directed triple.
	var edges := PackedInt32Array([1, 2, 1, 2, 2, 1])
	var triples := qb.derive_triples(edges)
	assert_eq(triples.size(), 2, "parallel edges dedupe; reverse direction kept")
	var dirs: Array = []
	for tr in triples:
		dirs.append("%s>%s" % [tr["src"], tr["tgt"]])
	assert_true(dirs.has("?v1>?v2"))
	assert_true(dirs.has("?v2>?v1"))


func test_build_pattern_payload_shape():
	var qb := QueryBuilder.new()
	qb.mark(1)
	qb.mark(2)
	var payload := qb.build_pattern_payload(PackedInt32Array([1, 2]), PackedStringArray(["references"]), true, 24)
	assert_eq(payload["countOnly"], true)
	assert_eq(payload["limit"], 24)
	assert_eq((payload["triples"] as Array).size(), 1)


func test_concrete_edge_type_used_when_toggle_on():
	var qb := QueryBuilder.new()
	qb.mark(1)
	qb.mark(2)
	# use_concrete_edges defaults true → the real predicate is emitted.
	var triples := qb.derive_triples(PackedInt32Array([1, 2]), PackedStringArray(["references"]))
	assert_eq(triples[0]["edgeType"], "references")
	# Toggle off → wildcard regardless of the supplied type.
	qb.use_concrete_edges = false
	triples = qb.derive_triples(PackedInt32Array([1, 2]), PackedStringArray(["references"]))
	assert_eq(triples[0]["edgeType"], "any")


func test_untyped_edge_falls_back_to_any():
	var qb := QueryBuilder.new()
	qb.mark(1)
	qb.mark(2)
	# Concrete toggle on but the edge has no type → "any".
	var triples := qb.derive_triples(PackedInt32Array([1, 2]), PackedStringArray([""]))
	assert_eq(triples[0]["edgeType"], "any")


func test_pattern_summary_pluralises():
	var qb := QueryBuilder.new()
	qb.mark(1)
	qb.derive_triples(PackedInt32Array([]))
	assert_eq(qb.pattern_summary(), "1 var · 0 edges")
	qb.mark(2)
	qb.derive_triples(PackedInt32Array([1, 2]))
	assert_eq(qb.pattern_summary(), "2 vars · 1 edge")
