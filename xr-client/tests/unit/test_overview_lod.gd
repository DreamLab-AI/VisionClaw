extends "res://addons/gut/test.gd"

# ADR-2188 D1/D2: overview node and edge rules, the LOD thresholds, edge-limit
# sizing and the dropped-edge counter (pure helpers in scripts/overview_lod.gd,
# the shader defaults that mirror them, and the per-volume source + HUD readout).

const OverviewLod := preload("res://scripts/overview_lod.gd")
const NodeLod := preload("res://scripts/node_lod.gd")
const BackendConnection := preload("res://scripts/backend_connection.gd")
const ConnectionVolumeSource := preload("res://scripts/connection_volume_source.gd")


# --- LOD thresholds ------------------------------------------------------------

func test_gem_tier_is_the_nearest_cap_within_the_radius() -> void:
	assert_eq(OverviewLod.node_tier(0.5, 0), OverviewLod.TIER_GEM)
	assert_eq(OverviewLod.node_tier(NodeLod.NEAR_RADIUS_M, NodeLod.NEAR_CAP - 1), OverviewLod.TIER_GEM,
		"the radius and the cap are inclusive / exclusive as in lod::split_tiers")
	assert_eq(OverviewLod.node_tier(NodeLod.NEAR_RADIUS_M + 0.001, 0), OverviewLod.TIER_OVERVIEW, "beyond 1 m")
	assert_eq(OverviewLod.node_tier(0.2, NodeLod.NEAR_CAP), OverviewLod.TIER_OVERVIEW, "past the 80th nearest")
	assert_eq(NodeLod.NEAR_CAP, 80)
	assert_eq(NodeLod.NEAR_RADIUS_M, 1.0)


func test_impostor_matches_the_gem_at_the_tier_boundary() -> void:
	# A 6 cm node (NODE_WORLD_RADIUS 0.03) at the 1 m boundary: metric, no pop.
	var s := 0.06
	assert_almost_eq(OverviewLod.impostor_world_size(s, NodeLod.NEAR_RADIUS_M), s, 1e-6)
	assert_gt(OverviewLod.projected_px(s, 1.0), OverviewLod.NODE_MIN_PX, "well above the floor at 1 m")


func test_impostor_never_projects_below_the_pixel_floor_until_the_metre_cap() -> void:
	var s := 0.004   # a small node: 4 mm
	for d: float in [2.0, 5.0, 8.0]:
		var w: float = OverviewLod.impostor_world_size(s, d)
		assert_almost_eq(OverviewLod.projected_px(w, d), maxf(OverviewLod.NODE_MIN_PX, OverviewLod.projected_px(s, d)), 1e-3,
			"floored at %.0f m" % d)
	# At 20 m the floor would need 8 cm; it is capped at NODE_MAX_FLOOR_M.
	assert_almost_eq(OverviewLod.impostor_world_size(s, 20.0), OverviewLod.NODE_MAX_FLOOR_M, 1e-6)
	# A larger node keeps its own (relative) size: the floor never shrinks it.
	assert_almost_eq(OverviewLod.impostor_world_size(0.2, 20.0), 0.2, 1e-6)


func test_ribbon_width_is_screen_space_and_never_thinner_than_the_cylinder() -> void:
	var cyl := 0.003
	assert_almost_eq(OverviewLod.ribbon_width(cyl, 0.5), cyl, 1e-6, "near the eye: the world width")
	var d := 10.0
	assert_almost_eq(OverviewLod.projected_px(OverviewLod.ribbon_width(cyl, d), d), OverviewLod.EDGE_WIDTH_PX, 1e-3,
		"far: EDGE_WIDTH_PX pixels")


func test_length_fade_follows_adr_2181() -> void:
	assert_eq(OverviewLod.length_fade(0.0), 1.0)
	assert_eq(OverviewLod.length_fade(50.0), 1.0)
	assert_almost_eq(OverviewLod.length_fade(100.0), lerpf(1.0, 0.25, 0.5), 1e-6)
	assert_almost_eq(OverviewLod.length_fade(150.0), 0.25, 1e-6)
	assert_almost_eq(OverviewLod.length_fade(4000.0), 0.25, 1e-6)
	# A 1 m edge seen side-on from 5 m: 0.2 rad ≈ 150 px → faded to the minimum.
	var px: float = OverviewLod.projected_length_px(Vector3(-0.5, 0, -5), Vector3(0.5, 0, -5), Vector3.ZERO)
	assert_almost_eq(px, 2.0 * atan(0.1) * OverviewLod.DEFAULT_PX_PER_RAD, 0.01)


func test_px_per_rad_from_the_eye_buffer() -> void:
	# 1600 px tall, 90° vertical FOV → 800 px per radian of tangent at the centre.
	assert_almost_eq(OverviewLod.px_per_rad(1600.0, 90.0), 800.0, 1e-3)
	assert_eq(OverviewLod.px_per_rad(0.0, 90.0), OverviewLod.DEFAULT_PX_PER_RAD, "unknown viewport → default")


func _shader_default(path: String, uniform: String) -> float:
	var src: String = FileAccess.get_file_as_string(path)
	var re := RegEx.new()
	re.compile("uniform\\s+float\\s+" + uniform + "\\s*(?::[^=]*)?=\\s*([0-9.]+)")
	var m := re.search(src)
	assert_not_null(m, "%s declares %s" % [path, uniform])
	return float(m.get_string(1)) if m != null else NAN


func test_shader_defaults_mirror_the_gdscript_rules() -> void:
	# Headless Godot does not compile shaders; this pins the source the GL run compiles.
	var imp := "res://materials/node_impostor.gdshader"
	var rib := "res://materials/edge_ribbon.gdshader"
	assert_eq(_shader_default(imp, "px_per_rad"), OverviewLod.DEFAULT_PX_PER_RAD)
	assert_eq(_shader_default(imp, "min_px"), OverviewLod.NODE_MIN_PX)
	assert_eq(_shader_default(imp, "max_floor_m"), OverviewLod.NODE_MAX_FLOOR_M)
	assert_eq(_shader_default(rib, "px_per_rad"), OverviewLod.DEFAULT_PX_PER_RAD)
	assert_eq(_shader_default(rib, "width_px"), OverviewLod.EDGE_WIDTH_PX)
	assert_eq(_shader_default(rib, "fade_near_px"), OverviewLod.FADE_NEAR_PX)
	assert_eq(_shader_default(rib, "fade_far_px"), OverviewLod.FADE_FAR_PX)
	assert_eq(_shader_default(rib, "fade_min"), OverviewLod.FADE_MIN)
	assert_eq(_shader_default(rib, "grey_alpha"), OverviewLod.GREY_ALPHA)
	assert_eq(_shader_default("res://materials/edge_flow.gdshader", "grey_alpha"), OverviewLod.GREY_ALPHA)


func test_no_shader_or_scene_turns_glow_on() -> void:
	# ADR-2188 D3 / Invariant 2: emphasis is colour, size and outline, never bloom.
	var scene: String = FileAccess.get_file_as_string("res://scenes/GraphScene.tscn")
	assert_true(scene.contains("glow_enabled = false"))
	assert_false(scene.contains("glow_enabled = true"))
	for p: String in ["res://materials/node_impostor.gdshader", "res://materials/edge_ribbon.gdshader",
			"res://materials/edge_flow.gdshader", "res://materials/node_halo_quad.gdshader"]:
		var src: String = FileAccess.get_file_as_string(p)
		assert_false(src.contains("SCREEN_TEXTURE") or src.contains("hint_screen_texture"), "%s reads no screen" % p)


# --- edge limit and the dropped-edge counter -----------------------------------

func test_edge_limit_is_sized_from_the_data_up_to_the_budget_ceiling() -> void:
	assert_eq(OverviewLod.edge_draw_limit(0), 0)
	assert_eq(OverviewLod.edge_draw_limit(1500), 1500, "a small graph asks for exactly its edges")
	assert_eq(OverviewLod.edge_draw_limit(OverviewLod.EDGE_DRAW_CEILING), OverviewLod.EDGE_DRAW_CEILING)
	assert_eq(OverviewLod.edge_draw_limit(145692), OverviewLod.EDGE_DRAW_CEILING, "the ADR-2188 graph hits the triangle ceiling")
	assert_eq(OverviewLod.edge_draw_limit(-3), 0)


func test_dropped_counter_and_readout() -> void:
	assert_eq(OverviewLod.dropped(145692, 20000), 125692)
	assert_eq(OverviewLod.dropped(10, 12), 0, "never negative")
	assert_eq(OverviewLod.edge_readout(1500, 1500), "", "nothing dropped → no readout")
	assert_eq(OverviewLod.edge_readout(20000, 145692), "Edges 20 000 of 145 692 drawn")
	assert_eq(OverviewLod.edge_readout(999, 1000), "Edges 999 of 1 000 drawn")


class CountingClient:
	extends RefCounted
	signal connection_changed(connected: bool)
	signal topology_updated(edge_count: int)
	var total: int = 0
	var asked: int = -1
	var drawn_cyl: int = 0
	var drawn_rib: int = 0
	func edge_count_for_mask(_mask: int) -> int:
		return total
	func edges_for_mask(_mask: int, max_pairs: int) -> PackedInt32Array:
		asked = max_pairs
		var out := PackedInt32Array()
		out.resize(mini(total, max_pairs) * 2)
		return out


func _conn_with(client: RefCounted) -> Node:
	var conn: Node = BackendConnection.make(&"visionclaw", "visionclaw", "ws://h:4000", "", client)
	add_child_autofree(conn)
	return conn


func test_a_volume_asks_for_its_own_edge_count_and_reports_what_it_did_not_draw() -> void:
	var c := CountingClient.new()
	c.total = 145692
	var src := ConnectionVolumeSource.new(_conn_with(c), 0b0001)
	src.content_pairs()
	assert_eq(c.asked, OverviewLod.EDGE_DRAW_CEILING, "limit sized from the data, bounded by the ceiling")
	assert_eq(src.edge_total, 145692)
	src.counts = {"edges": 96, "ribbons": 19800}
	assert_eq(src.drawn_edges(), 19896)
	assert_eq(src.dropped_edges(), 145692 - 19896, "the ceiling AND the endpoint drops are both counted")


func test_a_small_volume_asks_for_exactly_its_edges() -> void:
	var c := CountingClient.new()
	c.total = 1500
	var src := ConnectionVolumeSource.new(_conn_with(c), 0b0001)
	src.content_pairs()
	assert_eq(c.asked, 1500)
	src.counts = {"edges": 96, "ribbons": 1404}
	assert_eq(src.dropped_edges(), 0)


func test_the_hud_header_shows_the_readout_only_while_edges_are_dropped() -> void:
	var hud: Node3D = (load("res://scenes/HUD.tscn") as PackedScene).instantiate()
	add_child_autofree(hud)
	await get_tree().process_frame
	hud.set_edge_readout(OverviewLod.edge_readout(20000, 145692))
	assert_eq(hud.edge_readout(), "Edges 20 000 of 145 692 drawn")
	var badge: Label = hud.find_child("EdgeBadge", true, false)
	assert_not_null(badge)
	assert_true(badge.visible)
	hud.set_edge_readout(OverviewLod.edge_readout(1500, 1500))
	assert_false(badge.visible, "hidden when nothing is dropped")
	assert_eq(hud.edge_readout(), "")
