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
