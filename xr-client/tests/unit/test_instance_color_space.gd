extends "res://addons/gut/test.gd"

# Instance colours are display (sRGB) values: the palette hexes, the HUD Key
# swatches, the desktop's displayed colours. Measured on HP (Godot 4.6.1,
# Compatibility/opengl3, 2026-10-07): an instance colour reaches the framebuffer
# unchanged — unlit gem.tres renders #646b9f as #646b9f on both a SubViewport and
# the root window, and `vertex_color_is_srgb` does not change that. So the
# convention is "COLOR is used raw", and every instance-colour shader must follow
# it or its nodes stop matching their swatch. These pixel tests pin that for the
# gem, the far-tier impostor and the cluster hull.
#
# They need a real renderer (CI runs this suite under Xvfb with GL
# Compatibility); under the headless dummy renderer they are marked pending.

const HEXES := ["#FFB74D", "#646b9f"]   # robotics, space-science (domainColors.ts)
const TOL := 3.0 / 255.0


func _gl_available() -> bool:
	return DisplayServer.get_name() != "headless"


# Render one quad filling a small SubViewport and read the centre pixel.
func _render_pixel(material: Material, colour: Color) -> Color:
	var vp := SubViewport.new()
	vp.size = Vector2i(16, 16)
	vp.own_world_3d = true
	vp.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	vp.msaa_3d = Viewport.MSAA_DISABLED
	add_child_autofree(vp)
	var cam := Camera3D.new()
	cam.projection = Camera3D.PROJECTION_ORTHOGONAL
	cam.size = 1.0
	cam.position = Vector3(0, 0, 2)
	vp.add_child(cam)
	cam.current = true
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.use_colors = true
	mm.use_custom_data = true
	var quad := QuadMesh.new()
	quad.size = Vector2(4, 4)
	mm.mesh = quad
	mm.instance_count = 1
	mm.set_instance_transform(0, Transform3D.IDENTITY)
	mm.set_instance_color(0, colour)
	mm.set_instance_custom_data(0, Color(0, 0, 0, 1))
	var inst := MultiMeshInstance3D.new()
	inst.multimesh = mm
	inst.material_override = material
	vp.add_child(inst)
	for _i in range(4):
		await RenderingServer.frame_post_draw
	return vp.get_texture().get_image().get_pixel(8, 8)


func _assert_matches(px: Color, hex: String, label: String) -> void:
	var want := Color(hex)
	assert_almost_eq(px.r, want.r, TOL, "%s red vs %s" % [label, hex])
	assert_almost_eq(px.g, want.g, TOL, "%s green vs %s" % [label, hex])
	assert_almost_eq(px.b, want.b, TOL, "%s blue vs %s" % [label, hex])


func test_gem_instance_colour_renders_the_swatch_hex() -> void:
	if not _gl_available():
		pending("needs a GL renderer (runs in CI under Xvfb)")
		return
	# The production gem with lighting switched off: what remains is exactly the
	# instance-colour → albedo path.
	var m := (load("res://materials/gem.tres") as StandardMaterial3D).duplicate() as StandardMaterial3D
	m.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	m.emission_enabled = false
	m.rim_enabled = false
	m.next_pass = null
	for hex in HEXES:
		_assert_matches(await _render_pixel(m, Color(hex)), hex, "gem")


func test_impostor_shader_uses_the_same_colour_convention() -> void:
	if not _gl_available():
		pending("needs a GL renderer (runs in CI under Xvfb)")
		return
	# Neutral lighting: at the disc centre the impostor's colour is base × ambient.
	var m := ShaderMaterial.new()
	m.shader = load("res://materials/node_impostor.gdshader")
	m.set_shader_parameter("ambient", 1.0)
	m.set_shader_parameter("diffuse", 0.0)
	m.set_shader_parameter("specular", 0.0)
	m.set_shader_parameter("rim_strength", 0.0)
	m.set_shader_parameter("cen_boost", 0.0)
	for hex in HEXES:
		_assert_matches(await _render_pixel(m, Color(hex)), hex, "node_impostor")


func test_hull_shader_uses_the_same_colour_convention() -> void:
	if not _gl_available():
		pending("needs a GL renderer (runs in CI under Xvfb)")
		return
	var m := ShaderMaterial.new()
	m.shader = load("res://materials/cluster_hull.gdshader")
	m.set_shader_parameter("opacity", 1.0)
	m.set_shader_parameter("edge_boost", 0.0)
	m.set_shader_parameter("max_alpha", 1.0)
	for hex in HEXES:
		_assert_matches(await _render_pixel(m, Color(hex)), hex, "cluster_hull")
