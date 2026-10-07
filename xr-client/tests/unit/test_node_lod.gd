extends "res://addons/gut/test.gd"

# Node-mesh LOD (PRD-008 triangle budget): the gem tier is capped, every other
# drawn node becomes a 2-triangle impostor in one extra MultiMesh. Drives the real
# gdext client through its decode door (ingest), as GraphScene and the benchmark do.

const NodeLod := preload("res://scripts/node_lod.gd")


func _v3_line(n: int) -> PackedByteArray:
	var b := StreamPeerBuffer.new()
	b.big_endian = false
	b.put_u8(3)
	for i in range(n):
		b.put_u32(i + 1)
		b.put_float(float(i)); b.put_float(0.0); b.put_float(0.0)
		for _k in range(4):
			b.put_float(0.0)
		b.put_32(-1)
		b.put_u32(0)
		b.put_float(0.0)
		b.put_u32(0)
		b.put_float(0.5)
	return b.data_array


func test_impostor_instance_matches_the_node_buffer_layout() -> void:
	var inst: MultiMeshInstance3D = NodeLod.make_impostor_instance()
	autofree(inst)
	var mm: MultiMesh = inst.multimesh
	assert_eq(mm.transform_format, MultiMesh.TRANSFORM_3D)
	assert_true(mm.use_colors, "colour channel like NodesMulti")
	assert_true(mm.use_custom_data, "custom channel like NodesMulti")
	assert_true(mm.mesh is QuadMesh)
	assert_eq((mm.mesh as QuadMesh).size, Vector2(1, 1), "quad = sphere diameter (radius 0.5)")
	var mat := inst.material_override as ShaderMaterial
	assert_not_null(mat)
	assert_eq(mat.shader, NodeLod.IMPOSTOR_SHADER)
	assert_eq(mm.mesh.get_faces().size() / 3, 2, "two triangles per impostor")


func test_lod_build_caps_the_gem_tier_and_impostors_take_the_rest() -> void:
	assert_true(ClassDB.class_exists("BinaryProtocolClient"), "gdext library loaded")
	var client: RefCounted = BinaryProtocolClient.create()
	client.ingest(_v3_line(200))
	var ids := PackedInt32Array()
	for i in range(200):
		ids.append(i + 1)
	var near: PackedFloat32Array = client.build_node_buffer_lod(ids, 1.0, 0.7, 1.9, Vector3.ZERO, 10, INF)
	var far: PackedFloat32Array = client.impostor_node_buffer()
	assert_eq(near.size() / 20, 10)
	assert_eq(far.size() / 20, 190)
	# The gem tier is the ten nearest the eye at the origin (x = 0..9).
	for i in range(10):
		assert_lte(near[i * 20 + 3], 9.0)
	# A radius admits only nodes inside it.
	near = client.build_node_buffer_lod(ids, 1.0, 0.7, 1.9, Vector3.ZERO, 96, 4.5)
	assert_eq(near.size() / 20, 5, "x = 0..4 within 4.5")
	var gem := MultiMeshInstance3D.new()
	autofree(gem)
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.use_colors = true
	mm.use_custom_data = true
	gem.multimesh = mm
	assert_eq(NodeLod.assign(gem, near), 5)
	var imp: MultiMeshInstance3D = NodeLod.make_impostor_instance()
	autofree(imp)
	assert_eq(NodeLod.assign(imp, client.impostor_node_buffer()), 195)
	assert_eq(NodeLod.assign(imp, PackedFloat32Array()), 0, "empty buffer clears the tier")
	assert_eq(imp.multimesh.instance_count, 0)


func test_graph_scene_creates_the_impostor_tier_under_graph_root() -> void:
	var src: String = FileAccess.get_file_as_string("res://scripts/graph_scene.gd")
	assert_true(src.contains("NodeLod.make_impostor_instance()"), "GraphScene builds the impostor tier")
	assert_true(src.contains("build_node_buffer_lod("), "GraphScene packs nodes through the LOD split")


# --- edge LOD ------------------------------------------------------------------

func test_ribbon_instance_keeps_the_edge_stride() -> void:
	var inst: MultiMeshInstance3D = NodeLod.make_ribbon_instance()
	autofree(inst)
	var mm: MultiMesh = inst.multimesh
	assert_eq(mm.transform_format, MultiMesh.TRANSFORM_3D)
	assert_true(mm.use_custom_data, "style code in custom.a like EdgesMulti")
	assert_false(mm.use_colors, "12 + 4 = 16 floats per instance (Invariant 3)")
	assert_true(mm.mesh is QuadMesh)
	assert_eq((inst.material_override as ShaderMaterial).shader, NodeLod.RIBBON_SHADER)


func test_edge_lod_build_caps_cylinders_and_ribbons_take_the_rest() -> void:
	assert_true(ClassDB.class_exists("BinaryProtocolClient"), "gdext library loaded")
	var client: RefCounted = BinaryProtocolClient.create()
	client.ingest(_v3_line(60))
	var ids := PackedInt32Array()
	for i in range(60):
		ids.append(i + 1)
	client.build_node_buffer(ids, 1.0, 0.7, 1.9)
	var pairs := PackedInt32Array()
	for i in range(50):
		pairs.append(i + 1)
		pairs.append(i + 2)
	var near: PackedFloat32Array = client.build_edge_buffer_lod(pairs, 1.0, Vector3.ZERO, 5, INF)
	assert_eq(near.size() / 16, 5)
	assert_eq(client.ribbon_edge_buffer().size() / 16, 45)
	var ribbons: MultiMeshInstance3D = NodeLod.make_ribbon_instance()
	autofree(ribbons)
	assert_eq(NodeLod.assign_stride(ribbons, client.ribbon_edge_buffer(), 16), 45)


func test_ribbon_material_follows_the_cylinder_comfort_settings() -> void:
	var cyl := (load("res://materials/edge_flow.tres") as ShaderMaterial).duplicate() as ShaderMaterial
	cyl.set_shader_parameter("pulse_energy", 0.0)   # reduced motion
	cyl.set_shader_parameter("base_alpha", 0.09)    # low cost
	var ribbons: MultiMeshInstance3D = NodeLod.make_ribbon_instance()
	autofree(ribbons)
	NodeLod.sync_edge_params(cyl, ribbons.material_override)
	var r := ribbons.material_override as ShaderMaterial
	assert_eq(r.get_shader_parameter("pulse_energy"), 0.0)
	assert_almost_eq(float(r.get_shader_parameter("base_alpha")), 0.09, 1e-6)
	assert_eq(r.get_shader_parameter("flow_color"), cyl.get_shader_parameter("flow_color"))
