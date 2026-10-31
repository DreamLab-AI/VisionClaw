extends "res://addons/gut/test.gd"

# The live scene's FrameBudget pass (scripts/frame_budget_pass.gd): measured
# layer counts + the memory layer's demand go through xr-cloud's
# FrameBudget.allocate (rust frame_budget.rs); the graph's near-tier caps shrink
# when the memory cloud is shown, and the memory layer receives its caps.

const FrameBudgetPass := preload("res://scripts/frame_budget_pass.gd")
const NodeLod := preload("res://scripts/node_lod.gd")


class FakeMemory:
	extends RefCounted
	var rows: int = 0
	var route_rows: int = 0
	var route_sidecar: int = 0
	var cloud_tris: int = 0
	var applied: Array = []
	func frame_demand() -> Dictionary:
		return {"cloud_rows": rows, "route_rows": route_rows, "route_sidecar": route_sidecar}
	func budget() -> Dictionary:
		return {"cloud_triangles": cloud_tris, "route_triangles": 0}
	func apply_frame_caps(caps: Dictionary) -> void:
		applied.append(caps)


# Production graph drawn with full near tiers (13 164 nodes, 20 000 edges, hulls).
func _production_layers() -> Dictionary:
	return {"gems": 80, "faded": 0, "halos": 80, "impostors": 13084, "cylinders": 96,
		"ribbons": 19904, "hulls": 32, "hull_tris": 2938, "draw_calls": 6}


func _client() -> RefCounted:
	assert_true(ClassDB.class_exists("BinaryProtocolClient") and ClassDB.class_exists("FrameBudget"), "gdext library loaded")
	return BinaryProtocolClient.create()


func _measured(client: RefCounted, layers: Dictionary, extra: int, memory_tris: int) -> int:
	return int(client.graph_layer_triangles(layers["gems"], layers["faded"], layers["halos"], layers["impostors"],
		layers["cylinders"], layers["ribbons"], layers["hull_tris"])) + memory_tris + extra


func test_defaults_until_the_first_pass() -> void:
	var p = FrameBudgetPass.new()
	assert_eq(int(p.caps["gem_nodes"]), NodeLod.NEAR_CAP)
	assert_eq(int(p.caps["cylinder_edges"]), NodeLod.NEAR_EDGE_CAP)
	assert_eq(int(p.caps["max_hulls"]), 32)


func test_graph_alone_fills_the_budget_with_near_detail() -> void:
	var client := _client()
	var p = FrameBudgetPass.new()
	var layers := _production_layers()
	assert_true(p.tick(1.0, client, layers, null, _measured(client, layers, 0, 0)))
	# 95 186 at full caps is just over the 95k (5 % reserve) limit: near detail
	# trims a little, nothing else gives way.
	assert_gt(int(p.caps["gem_nodes"]), 16)
	assert_eq(int(p.caps["max_hulls"]), 32)
	assert_false(bool(p.caps["over_budget"]))
	assert_lte(int(p.caps["tris_total"]), 95000)


func test_showing_the_cloud_shrinks_the_graph_near_tiers() -> void:
	var client := _client()
	var p = FrameBudgetPass.new()
	var layers := _production_layers()
	var mem := FakeMemory.new()
	mem.rows = 20000
	mem.route_rows = 64
	mem.route_sidecar = 64
	assert_true(p.tick(1.0, client, layers, mem, _measured(client, layers, 0, 0)))
	var alone0 = FrameBudgetPass.new()
	alone0.tick(1.0, client, layers, null, _measured(client, layers, 0, 0))
	assert_lt(int(p.caps["gem_nodes"]) + int(p.caps["cylinder_edges"]),
		int(alone0.caps["gem_nodes"]) + int(alone0.caps["cylinder_edges"]), "near tiers give way to the cloud")
	assert_gt(int(p.caps["cloud_sprites"]), 0)
	assert_eq(mem.applied.size(), 1, "memory layer receives its caps")
	assert_lte(int(p.caps["tris_total"]), 95000, "95k: FrameBudget keeps a 5 % reserve")
	# Hiding the cloud gives the near tiers back (to the graph-alone allocation).
	var alone = FrameBudgetPass.new()
	alone.tick(1.0, client, layers, null, _measured(client, layers, 0, 0))
	mem.rows = 0
	mem.route_rows = 0
	assert_true(p.tick(1.0, client, layers, mem, _measured(client, layers, 0, 0)))
	assert_eq(int(p.caps["gem_nodes"]), int(alone.caps["gem_nodes"]))
	assert_eq(int(p.caps["cylinder_edges"]), int(alone.caps["cylinder_edges"]))


func test_measured_other_triangles_are_reserved_first() -> void:
	var client := _client()
	var layers := _production_layers()
	var a = FrameBudgetPass.new()
	a.tick(1.0, client, layers, null, _measured(client, layers, 0, 0))
	var b = FrameBudgetPass.new()
	b.tick(1.0, client, layers, null, _measured(client, layers, 9000, 0))  # HUD, avatars, controllers
	assert_eq(int(b.last_report["other_tris"]), 9000)
	assert_lt(int(b.caps["gem_nodes"]) + int(b.caps["cylinder_edges"]) + int(b.caps["max_hulls"]),
		int(a.caps["gem_nodes"]) + int(a.caps["cylinder_edges"]) + int(a.caps["max_hulls"]))


func test_hud_dirty_frames_are_reserved_not_averaged_away() -> void:
	# The HUD canvas re-renders about once a second (~8k triangles on that frame).
	# A pass sampling one frame in four would mostly miss it and caps would
	# oscillate; the pass keeps a 2 s peak of the measured other_tris instead.
	var client := _client()
	var layers := _production_layers()
	var p = FrameBudgetPass.new()
	var base := _measured(client, layers, 700, 0)       # idle HUD + controllers + avatars
	var dirty := _measured(client, layers, 8900, 0)     # the frame the HUD canvas re-renders
	p.tick(0.011, client, layers, null, dirty)
	for i in range(60):                                 # ~0.7 s of idle frames
		p.tick(0.011, client, layers, null, base)
	assert_eq(int(p.last_report["other_tris"]), 8900, "dirty-frame cost held inside the window")
	for i in range(240):                                # > 2 s later the peak has expired
		p.tick(0.011, client, layers, null, base)
	assert_eq(int(p.last_report["other_tris"]), 700)


func test_burst_pool_is_budgeted_only_while_the_cloud_is_hidden() -> void:
	var client := _client()
	var layers := _production_layers()
	var p = FrameBudgetPass.new()
	p.tick(1.0, client, layers, null, 0, true)
	assert_eq(int(p.caps["burst_slots"]), 64, "bursts on, no cloud: the pool is reserved")
	var mem := FakeMemory.new()
	mem.rows = 20000
	p.tick(1.0, client, layers, mem, 0, true)
	assert_eq(int(p.caps["burst_slots"]), 0, "cloud shown: bursts restyle sprites, no rings")
	p.tick(1.0, client, layers, null, 0, false)
	assert_eq(int(p.caps["burst_slots"]), 0)


func test_runs_at_a_low_rate() -> void:
	var client := _client()
	var p = FrameBudgetPass.new()
	var layers := _production_layers()
	assert_true(p.tick(0.0, client, layers, null, 0), "first frame runs at once")
	assert_false(p.tick(0.1, client, layers, null, 0))
	assert_false(p.tick(0.1, client, layers, null, 0))
	assert_true(p.tick(0.1, client, layers, null, 0), "again after BUDGET_SEC")


func test_graph_scene_runs_the_pass_and_uses_its_caps() -> void:
	var src: String = FileAccess.get_file_as_string("res://scripts/graph_scene.gd")
	assert_true(src.contains("_budget.tick("), "GraphScene runs the budget pass")
	assert_true(src.contains("_budget.caps[\"gem_nodes\"]"), "gem tier capped by the budget")
	assert_true(src.contains("_budget.caps[\"cylinder_edges\"]"), "cylinder tier capped by the budget")


# ADR-2159 D4: one budget, split per visible volume; a hidden volume gets 0.
func test_near_caps_are_split_between_visible_volumes() -> void:
	var client := _client()
	var p = FrameBudgetPass.new()
	var layers := _production_layers()
	var volumes := {
		&"knowledge": {"nodes": 9000, "edges": 15000},
		&"ontology": {"nodes": 3000, "edges": 5000},
		&"codescape": {"nodes": 0, "edges": 0},
	}
	assert_true(p.tick(1.0, client, layers, null, _measured(client, layers, 0, 0), false, volumes))
	var k: Dictionary = p.volume_caps[&"knowledge"]
	var o: Dictionary = p.volume_caps[&"ontology"]
	var c: Dictionary = p.volume_caps[&"codescape"]
	assert_eq(int(c["gem_nodes"]), 0, "hidden volume: no gems")
	assert_eq(int(c["cylinder_edges"]), 0, "hidden volume: no cylinders")
	assert_eq(int(k["gem_nodes"]) + int(o["gem_nodes"]), int(p.caps["gem_nodes"]), "the one gem cap is shared out whole")
	assert_eq(int(k["cylinder_edges"]) + int(o["cylinder_edges"]), int(p.caps["cylinder_edges"]))
	assert_gt(int(k["gem_nodes"]), int(o["gem_nodes"]), "proportional to demand")
	assert_gte(int(o["gem_nodes"]), 16, "the smaller visible volume keeps its floor")


func test_no_volumes_means_no_split() -> void:
	var client := _client()
	var p = FrameBudgetPass.new()
	var layers := _production_layers()
	assert_true(p.tick(1.0, client, layers, null, _measured(client, layers, 0, 0)))
	assert_eq(p.volume_caps, {}, "the single-graph scene uses caps directly")


# ── portal volume weights (set_volume_weights) ──

func _three_volumes() -> Dictionary:
	return {
		&"knowledge": {"nodes": 9000, "edges": 15000},
		&"ontology": {"nodes": 9000, "edges": 15000},
		&"codescape": {"nodes": 0, "edges": 0},
	}


func _split_with(weights: Variant, volumes: Dictionary) -> Dictionary:
	var client := _client()
	var p = FrameBudgetPass.new()
	if weights != null:
		p.set_volume_weights(weights)
	var layers := _production_layers()
	assert_true(p.tick(1.0, client, layers, null, _measured(client, layers, 0, 0), false, volumes))
	return p.volume_caps


func test_empty_or_equal_weights_leave_the_split_byte_identical() -> void:
	var v := _three_volumes()
	var plain := _split_with(null, v)
	assert_eq(_split_with({}, v), plain, "empty weights")
	assert_eq(_split_with({&"knowledge": 1.0, &"ontology": 1.0, &"codescape": 1.0}, v), plain, "all 1.0")
	assert_eq(_split_with({&"knowledge": 2.5, &"ontology": 2.5}, v), plain, "equal over the visible volumes")
	assert_eq(_split_with({&"knowledge": 2.5, &"ontology": 2.5, &"codescape": 40.0}, v), plain, "a hidden volume's weight is ignored")
	assert_eq(var_to_bytes(_split_with({&"knowledge": 7.0, &"ontology": 7.0}, v)), var_to_bytes(plain), "bytes")


func test_weights_shift_the_split_towards_the_heavier_volume() -> void:
	var v := _three_volumes()
	var plain := _split_with(null, v)
	var weighted := _split_with({&"knowledge": 1.0, &"ontology": 4.0}, v)
	assert_eq(int(plain[&"knowledge"]["gem_nodes"]), int(plain[&"ontology"]["gem_nodes"]), "same drawn counts split evenly")
	assert_gt(int(weighted[&"ontology"]["gem_nodes"]), int(weighted[&"knowledge"]["gem_nodes"]), "weight 4 gets more")
	assert_gt(int(weighted[&"ontology"]["cylinder_edges"]), int(weighted[&"knowledge"]["cylinder_edges"]))
	var client := _client()
	var p = FrameBudgetPass.new()
	p.set_volume_weights({&"knowledge": 1.0, &"ontology": 4.0})
	var layers := _production_layers()
	p.tick(1.0, client, layers, null, _measured(client, layers, 0, 0), false, v)
	assert_eq(int(p.volume_caps[&"knowledge"]["gem_nodes"]) + int(p.volume_caps[&"ontology"]["gem_nodes"]), int(p.caps["gem_nodes"]), "still one cap, shared out whole")
	assert_eq(int(p.volume_caps[&"codescape"]["gem_nodes"]), 0, "hidden stays 0")


func test_zero_weight_keeps_the_gem_floor() -> void:
	var w := _split_with({&"knowledge": 0.0, &"ontology": 1.0}, _three_volumes())
	assert_gte(int(w[&"knowledge"]["gem_nodes"]), 16, "GEM_MIN holds for a zero weight")


func test_bad_weights_are_zero_and_missing_ids_are_one() -> void:
	var client := _client()
	var p = FrameBudgetPass.new()
	p.set_volume_weights({&"knowledge": NAN, &"ontology": -5.0, &"x": "heavy"})
	assert_eq(p.volume_weights[&"knowledge"], 0.0)
	assert_eq(p.volume_weights[&"ontology"], 0.0)
	assert_eq(p.volume_weights[&"x"], 0.0)
	var v := _three_volumes()
	var a := _split_with({&"knowledge": 3.0}, v) # ontology missing = 1.0
	var b := _split_with({&"knowledge": 3.0, &"ontology": 1.0}, v)
	assert_eq(a, b, "a missing id weighs 1.0")
	assert_not_null(client)


# The shape stream XP's graph_scene hands over (space_portals.gd budget_weights):
# every volume id (a StringName) to 1.0 for the entered space and OTHER_WEIGHT
# (0.15) for the rest, including a volume that is hidden this frame.
func test_the_portal_stream_weight_shape_weights_the_split() -> void:
	var v := _three_volumes()
	var plain := _split_with(null, v)
	var entered := {&"knowledge": 1.0, &"ontology": 0.15, &"codescape": 0.15}
	var w := _split_with(entered, v)
	assert_gt(int(w[&"knowledge"]["gem_nodes"]), int(w[&"ontology"]["gem_nodes"]), "the entered space gets more")
	assert_ne(w, plain, "the pass really weighted (not a silent miss on the keys)")
	assert_eq(int(w[&"codescape"]["gem_nodes"]), 0, "hidden stays 0 whatever its weight")
	assert_gte(int(w[&"ontology"]["gem_nodes"]), 16, "the floor holds for the portals")
	# String keys (a caller that stringified the ids) land on the same volumes.
	assert_eq(_split_with({"knowledge": 1.0, "ontology": 0.15, "codescape": 0.15}, v), w, "String keys")
	# Leaving the space: all 1.0 is the plain split again.
	assert_eq(_split_with({&"knowledge": 1.0, &"ontology": 1.0, &"codescape": 1.0}, v), plain)


func test_new_weights_apply_to_the_last_seen_volumes_at_once() -> void:
	var client := _client()
	var p = FrameBudgetPass.new()
	var layers := _production_layers()
	p.tick(1.0, client, layers, null, _measured(client, layers, 0, 0), false, _three_volumes())
	var before: Dictionary = p.volume_caps.duplicate(true)
	p.set_volume_weights({&"knowledge": 1.0, &"ontology": 5.0})
	assert_ne(p.volume_caps, before, "no 0.25 s wait on entering a space")
	p.set_volume_weights({})
	assert_eq(p.volume_caps, before, "leaving the space restores the plain split")


# ── the extras layer through the pass ──

func test_extras_are_granted_from_what_the_graph_leaves() -> void:
	var client := _client()
	var p = FrameBudgetPass.new()
	var small := {"gems": 80, "faded": 0, "halos": 80, "impostors": 920, "cylinders": 96, "ribbons": 1404, "hulls": 12, "hull_tris": 1100, "draw_calls": 6}
	p.set_extras_demand(4, 3)
	assert_true(p.tick(1.0, client, small, null, _measured(client, small, 0, 0)))
	assert_eq(int(p.caps["extras_tier"]), 4, "a small graph leaves room for every tier")
	var cost: PackedInt32Array = FrameBudget.new().extras_cost(4, 3)
	assert_eq(int(p.caps["extras_tris"]), cost[0])
	assert_eq(int(p.caps["extras_draw_calls"]), cost[1])
	assert_false(bool(p.caps["over_budget"]))


func test_extras_cost_the_graph_only_their_allowance() -> void:
	# ADR-2167 amended (2026-10-31): the cloud, route, burst and hull caps do not
	# move; the graph gives up at most the allowance, in gems and cylinders.
	var client := _client()
	var layers := _production_layers()
	var off = FrameBudgetPass.new()
	var on = FrameBudgetPass.new()
	on.set_extras_demand(4, 3)
	off.tick(1.0, client, layers, null, _measured(client, layers, 0, 0))
	on.tick(1.0, client, layers, null, _measured(client, layers, 0, 0))
	for k: String in ["max_hulls", "cloud_sprites", "route_tris", "burst_slots"]:
		assert_eq(on.caps[k], off.caps[k], k)
	var allowance: int = int(on.caps["extras_allowance"])
	assert_eq(int(off.caps["extras_tier"]), 0, "no demand, no grant")
	assert_eq(int(off.caps["extras_allowance"]), 0, "and no allowance")
	assert_eq(int(on.caps["extras_tier"]), 4)
	assert_eq(allowance, int(on.caps["extras_tris"]), "the grant is the held-back allowance")
	var gem_step: int = 288
	assert_lte(int(off.caps["gem_nodes"]) - int(on.caps["gem_nodes"]), ceili(float(allowance) / gem_step), "gems lost")
	assert_lte(int(on.caps["gem_nodes"]), int(off.caps["gem_nodes"]))
	var lost_tris: int = (int(off.caps["gem_nodes"]) - int(on.caps["gem_nodes"])) * gem_step \
		+ (int(off.caps["cylinder_edges"]) - int(on.caps["cylinder_edges"])) * 30
	assert_lte(lost_tris, allowance + gem_step, "the graph pays no more than the allowance (plus one step of rounding)")
	assert_lte(int(on.caps["tris_total"]), 95000)
	assert_lte(int(on.caps["draw_calls"]), 48)


func test_drawn_extras_are_not_charged_twice() -> void:
	var client := _client()
	var layers := _production_layers()
	var drawn := 600
	var p = FrameBudgetPass.new()
	# The renderer's total includes the extras it drew; the pass subtracts them.
	p.set_extras_applied(drawn, 5)
	p.tick(1.0, client, layers, null, _measured(client, layers, 2000 + drawn, 0))
	assert_eq(int(p.last_report["other_tris"]), 2000, "other_tris excludes what the extras drew")
	var q = FrameBudgetPass.new()
	q.tick(1.0, client, layers, null, _measured(client, layers, 2000 + drawn, 0))
	assert_eq(int(q.last_report["other_tris"]), 2000 + drawn, "and counts it when nobody reports it")


func test_the_extras_demand_is_clamped_to_the_allocators_top_tier() -> void:
	var spec: Dictionary = FrameBudget.new().extras_spec()
	var p = FrameBudgetPass.new()
	p.set_extras_demand(99, 2)
	assert_eq(int(p.extras_demand["tier"]), int(spec["max_tier"]))
	p.set_extras_demand(-3, -1)
	assert_eq(p.extras_demand, {"tier": 0, "frames": 0})


func test_the_pass_keeps_no_unread_extras_call_state() -> void:
	var p = FrameBudgetPass.new()
	assert_false("_extras_calls" in p, "the extras' calls are already in caps.draw_calls")
	p.set_extras_applied(10, 5)
	assert_eq(p._extras_tris, 10)


func test_one_is_the_neutral_portal_weight() -> void:
	var src: String = FileAccess.get_file_as_string("res://scripts/frame_budget_pass.gd")
	assert_true(src.contains("1.0 is the neutral weight; all-1.0 is exactly `{}`"))


func test_extras_constants_match_the_allocator() -> void:
	var spec: Dictionary = FrameBudget.new().extras_spec()
	var EyeCandy = load("res://scripts/eye_candy.gd")
	assert_eq(int(spec["motes_low"]), EyeCandy.MOTES_LOW)
	assert_eq(int(spec["motes_high"]), EyeCandy.MOTES_HIGH)
	assert_eq(int(spec["frame_tris"]), EyeCandy.FRAME_RIBBON_TRIS)
	assert_eq(int(spec["max_tier"]), EyeCandy.MAX_TIER)
