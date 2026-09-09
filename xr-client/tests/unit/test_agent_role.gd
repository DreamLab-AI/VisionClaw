extends "res://addons/gut/test.gd"

# Role vocabulary + procedural frames (scripts/agent_role.gd). Six roles must be
# distinguishable by silhouette, colour AND badge letters; frames are built at
# runtime from struts and cached per role; inference reads names before tasks.

const Role := preload("res://scripts/agent_role.gd")


func test_six_roles_have_distinct_badges_and_colours() -> void:
	var badges: Dictionary = {}
	var colours: Dictionary = {}
	for k: String in Role.role_keys():
		assert_true(Role.is_role(k))
		badges[Role.badge_of(k)] = true
		colours[Role.color_of(k)] = true
		assert_eq(Role.badge_of(k).length(), 2, "two-letter badge for %s" % k)
	assert_eq(badges.size(), 6, "badges unique")
	assert_eq(colours.size(), 6, "accents unique")
	assert_eq(Role.badge_of("nonsense"), "AG", "unknown → generic badge")


func test_inference_prefers_name_then_task_then_generic() -> void:
	assert_eq(Role.infer("Demo-Architect", ""), "architect")
	assert_eq(Role.infer("Demo-Analyst", ""), "analyst")
	assert_eq(Role.infer("Demo-Coder", ""), "coder")
	assert_eq(Role.infer("Demo-Reviewer", ""), "reviewer")
	assert_eq(Role.infer("Demo-Tester", ""), "tester")
	assert_eq(Role.infer("Demo-Optimizer", ""), "optimizer")
	assert_eq(Role.infer("agent 42", "Reviewing: Access policy"), "reviewer", "task reveals the role")
	assert_eq(Role.infer("Optimizer-7", "Testing: X"), "optimizer", "name wins over task")
	assert_eq(Role.infer("agent 9", ""), "generic")
	assert_eq(Role.infer("", "Refactoring: parser"), "coder")


func test_frames_build_once_per_role_with_real_geometry() -> void:
	for k: String in Role.role_keys() + ["generic"]:
		var m: ArrayMesh = Role.frame_mesh(k)
		assert_not_null(m, "mesh for %s" % k)
		assert_eq(m.get_surface_count(), 1, "one surface for %s" % k)
		var tris: int = Role.triangle_count(k)
		assert_gt(tris, 20, "%s has geometry (%d tris)" % [k, tris])
		assert_lt(tris, 4000, "%s stays cheap (%d tris)" % [k, tris])
		assert_same(m, Role.frame_mesh(k), "cached and shared")
		# Fits the ~0.42 m silhouette envelope and never collapses to a point.
		var aabb: AABB = m.get_aabb()
		assert_between(aabb.get_longest_axis_size(), 0.30, 0.48, "%s width %.2f m" % [k, aabb.get_longest_axis_size()])
	assert_same(Role.frame_mesh("unknown"), Role.frame_mesh("generic"), "unknown role uses the generic hoop")
