shader_type spatial;
// Far-field edge ribbon (edge LOD, PRD-008 triangle budget; Rust
// lod::split_tiers via build_edge_buffer_lod). One 2-triangle quad per edge
// instead of the 32-triangle cylinder: the quad spans the edge along the
// instance's local Y (same transform the cylinder uses, 16-float stride,
// Invariant 3) and turns about that axis to face the viewer. Shading is the
// shared edge_flow_common.gdshaderinc, so pulse, relation grammar and palette
// match the near-field cylinders.
//
// Compatibility/multiview-safe: no screen texture, no post-process. The facing
// direction is taken from the main camera (MAIN_CAM_INV_VIEW_MATRIX), so both
// eyes see the same ribbon; each eye still projects it with its own view.
render_mode blend_mix, unshaded, cull_disabled, depth_draw_never, shadows_disabled, fog_disabled;

#include "res://materials/edge_flow_common.gdshaderinc"

// GraphScene CylinderMesh_edge radius: the ribbon is as wide as the cylinder.
uniform float ribbon_radius = 0.03;
// A cull-disabled cylinder blends its front and back walls, so it shows about
// two layers of alpha; the ribbon is one layer. Composite it as `layers` layers.
uniform float layers = 2.0;

void vertex() {
	v_style = INSTANCE_CUSTOM.a;
	v_phase = fract(float(INSTANCE_ID) * 0.61803398875);
	vec3 centre = MODEL_MATRIX[3].xyz;
	vec3 axis = MODEL_MATRIX[1].xyz;             // world edge vector (length = span)
	float width = length(MODEL_MATRIX[0].xyz) * ribbon_radius * 2.0;
	vec3 eye = MAIN_CAM_INV_VIEW_MATRIX[3].xyz;
	vec3 dir = normalize(axis);
	vec3 side = cross(dir, normalize(eye - centre));
	float sl = length(side);
	// Edge pointing straight at the eye: any perpendicular will do.
	side = sl > 1e-5 ? side / sl : normalize(cross(dir, abs(dir.y) < 0.9 ? vec3(0.0, 1.0, 0.0) : vec3(1.0, 0.0, 0.0)));
	vec3 world = centre + axis * VERTEX.y + side * (VERTEX.x * width);
	POSITION = PROJECTION_MATRIX * (VIEW_MATRIX * vec4(world, 1.0));
}

void fragment() {
	vec3 albedo;
	float alpha;
	edge_flow_shade(UV, TIME, v_style, v_phase, albedo, alpha);
	ALBEDO = albedo;
	ALPHA = 1.0 - pow(1.0 - alpha, layers);
}
