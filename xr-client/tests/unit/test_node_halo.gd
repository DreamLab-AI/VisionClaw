extends "res://addons/gut/test.gd"

# The node halo is a camera-facing quad layer (NodesHaloMulti), not a second
# sphere pass: gem.tres's halo next_pass drew every node sphere twice (288 → 576
# triangles). The look is kept by node_halo_quad.gdshader, which reproduces the
# shell's fresnel ring analytically; the comfort controls that used to toggle
# the next_pass now drive the layer.

const NodeLod := preload("res://scripts/node_lod.gd")
const HALO_PATH := "GraphRoot/NodesHaloMulti"


func test_gem_materials_are_a_single_pass() -> void:
	for path in ["res://materials/gem.tres", "res://materials/gem_faded.tres"]:
		var m := load(path) as StandardMaterial3D
		assert_null(m.next_pass, "%s draws its sphere once; the halo is NodesHaloMulti" % path)


# GraphScene with its script detached: the static scene (materials, layers and
# the SpatialEnvironment comfort node) without connecting to a backend.
func _static_graph_scene() -> Node3D:
	var scene: Node3D = (load("res://scenes/GraphScene.tscn") as PackedScene).instantiate()
	scene.set_script(null)
	add_child_autofree(scene)
	return scene


func test_graph_scene_has_the_halo_quad_layer() -> void:
	var scene := _static_graph_scene()
	await get_tree().process_frame
	var halo := scene.get_node_or_null(HALO_PATH) as MultiMeshInstance3D
	assert_not_null(halo, "NodesHaloMulti under GraphRoot")
	var mm: MultiMesh = halo.multimesh
	assert_true(mm.use_colors and mm.use_custom_data, "same 20-float layout as NodesMulti")
	assert_true(mm.mesh is QuadMesh, "2 triangles per halo")
	var mat := halo.material_override as ShaderMaterial
	assert_eq(mat.shader.resource_path, "res://materials/node_halo_quad.gdshader")


func test_comfort_controls_drive_the_halo_layer() -> void:
	var scene := _static_graph_scene()
	await get_tree().process_frame
	var env: Node = scene.get_node("SpatialEnvironment")
	var halo := scene.get_node(HALO_PATH) as MultiMeshInstance3D
	env.call("set_visual_comfort", false, true)
	assert_false(halo.visible, "low cost removes the halo layer")
	env.call("set_visual_comfort", true, false)
	assert_true(halo.visible, "balanced restores it")
	assert_eq((halo.material_override as ShaderMaterial).get_shader_parameter("query_pulse_depth"), 0.0, "reduced motion stops the query pulse")
	env.call("set_visual_comfort", false, false)
	assert_almost_eq(float((halo.material_override as ShaderMaterial).get_shader_parameter("query_pulse_depth")), 0.12, 1e-6)


func test_halo_buffer_covers_gem_tier_and_faded_nodes() -> void:
	var halo := MultiMeshInstance3D.new()
	autofree(halo)
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.use_colors = true
	mm.use_custom_data = true
	halo.multimesh = mm
	var near := PackedFloat32Array()
	near.resize(20 * 3)
	var faded := PackedFloat32Array()
	faded.resize(20 * 2)
	assert_eq(NodeLod.assign_halo(halo, near, faded), 5, "halo on every full-mesh node")
	assert_eq(NodeLod.assign_halo(halo, PackedFloat32Array(), PackedFloat32Array()), 0)


func test_graph_scene_feeds_the_halo_layer() -> void:
	var src: String = FileAccess.get_file_as_string("res://scripts/graph_scene.gd")
	assert_true(src.contains("NodeLod.assign_halo("), "GraphScene fills NodesHaloMulti each frame")
