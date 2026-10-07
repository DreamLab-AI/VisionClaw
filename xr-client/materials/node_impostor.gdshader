shader_type spatial;
// Far-tier node impostor (PRD-008 triangle budget; Rust lod::split_node_tiers).
// One 2-triangle quad per node instead of the 576-triangle gem sphere + halo:
// the quad faces the camera and the fragment shader draws an analytically
// shaded sphere in it, so at the distances where it is used (beyond the gem
// tier's nearest-N / 1 m radius) it reads as the same lit gem.
//
// Compatibility/multiview-safe (XR-client.md Invariants 1-2): no screen
// texture, no post-process, opaque with discard. The billboard uses the main
// camera's basis (MAIN_CAM_INV_VIEW_MATRIX), the same construction as Godot's
// StandardMaterial BILLBOARD_ENABLED keep-scale path, so both eyes see one quad
// orientation and stereo stays consistent.
//
// Instance data is the node MultiMesh layout (render_store.rs): COLOR = node
// colour (raw, like gem.tres), INSTANCE_CUSTOM.r = centrality halo tell,
// .b = query-variable flag.
render_mode unshaded, cull_disabled, shadows_disabled, fog_disabled, depth_draw_opaque;

// View-space key light (up-left-front), approximating the scene light on gem.tres.
uniform vec3 light_dir_view = vec3(-0.45, 0.6, 0.66);
uniform float ambient : hint_range(0.0, 1.0) = 0.38;
uniform float diffuse : hint_range(0.0, 1.0) = 0.62;
// gem.tres roughness 0.36 → a moderately tight highlight.
uniform float shininess = 24.0;
uniform float specular : hint_range(0.0, 1.0) = 0.22;
// gem.tres rim 0.28, plus the halo pass's centrality lift (node_halo cen_boost).
uniform float rim_strength : hint_range(0.0, 1.0) = 0.28;
uniform float rim_power = 2.5;
uniform float cen_boost : hint_range(0.0, 1.0) = 0.25;
uniform vec3 query_rim = vec3(0.9, 0.95, 1.0);

varying vec4 inst_custom;

void vertex() {
	MODELVIEW_MATRIX = VIEW_MATRIX * mat4(MAIN_CAM_INV_VIEW_MATRIX[0], MAIN_CAM_INV_VIEW_MATRIX[1], MAIN_CAM_INV_VIEW_MATRIX[2], MODEL_MATRIX[3]);
	MODELVIEW_MATRIX = MODELVIEW_MATRIX * mat4(
		vec4(length(MODEL_MATRIX[0].xyz), 0.0, 0.0, 0.0),
		vec4(0.0, length(MODEL_MATRIX[1].xyz), 0.0, 0.0),
		vec4(0.0, 0.0, length(MODEL_MATRIX[2].xyz), 0.0),
		vec4(0.0, 0.0, 0.0, 1.0));
	MODELVIEW_NORMAL_MATRIX = mat3(MODELVIEW_MATRIX);
	inst_custom = INSTANCE_CUSTOM;
}

void fragment() {
	// QuadMesh 1×1 ↔ sphere radius 0.5: the unit disc is the silhouette.
	vec2 p = UV * 2.0 - 1.0;
	p.y = -p.y;
	float r2 = dot(p, p);
	if (r2 > 1.0) {
		discard;
	}
	vec3 n = vec3(p, sqrt(1.0 - r2));
	vec3 l = normalize(light_dir_view);
	float lambert = max(dot(n, l), 0.0);
	vec3 h = normalize(l + vec3(0.0, 0.0, 1.0));
	float spec = pow(max(dot(n, h), 0.0), shininess) * specular;
	float rim = pow(1.0 - n.z, rim_power);
	vec3 base = COLOR.rgb;
	vec3 col = base * (ambient + diffuse * lambert) + vec3(spec);
	col += base * rim * (rim_strength + cen_boost * clamp(inst_custom.r, 0.0, 1.0));
	col += query_rim * rim * step(0.5, inst_custom.b);
	ALBEDO = col;
}
