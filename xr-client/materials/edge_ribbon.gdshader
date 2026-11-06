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
//
// Overview edges (ADR-2188 D2, ADR-2181): the ribbon is a screen-width line.
// Its width is `width_px` pixels at each end (metres per pixel from the main
// camera's distance to that end and `px_per_rad`), never thinner than the
// world cylinder it stands in for, so it meets the near tier without a pop.
// Alpha falls with the edge's projected length, from 1 below `fade_near_px` to
// `fade_min` above `fade_far_px` (the cosmos.gl defaults ADR-2181 adopts). An
// edge with a greyed-out end (INSTANCE_CUSTOM.r = 1, ADR-2188 D4) is drawn at
// `grey_alpha` of its alpha. Mirror: scripts/overview_lod.gd.
render_mode blend_mix, unshaded, cull_disabled, depth_draw_never, shadows_disabled, fog_disabled;

#include "res://materials/edge_flow_common.gdshaderinc"

// GraphScene CylinderMesh_edge radius: the ribbon is as wide as the cylinder.
uniform float ribbon_radius = 0.03;
// A cull-disabled cylinder blends its front and back walls, so it shows about
// two layers of alpha; the ribbon is one layer. Composite it as `layers` layers.
uniform float layers = 2.0;
// Screen-space width and length fade (ADR-2188 D2). px_per_rad: see node_impostor.
uniform float px_per_rad = 750.0;
uniform float width_px = 1.5;
uniform float fade_near_px = 50.0;
uniform float fade_far_px = 150.0;
uniform float fade_min = 0.25;
uniform float grey_alpha = 0.3;

varying float v_fade;

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
	// Width in pixels at this end of the edge (VERTEX.y is -0.5 or +0.5).
	vec3 end_p = centre + axis * VERTEX.y;
	float m_per_px = length(end_p - eye) / max(px_per_rad, 1.0);
	width = max(width, width_px * m_per_px);
	// Projected length as an angle from the main camera, in pixels.
	vec3 a = centre - axis * 0.5 - eye;
	vec3 b = centre + axis * 0.5 - eye;
	float la = length(a);
	float lb = length(b);
	float ang = (la > 1e-6 && lb > 1e-6) ? acos(clamp(dot(a, b) / (la * lb), -1.0, 1.0)) : 0.0;
	float len_px = ang * px_per_rad;
	v_fade = mix(1.0, fade_min, smoothstep(fade_near_px, fade_far_px, len_px));
	v_fade *= INSTANCE_CUSTOM.r > 0.5 ? grey_alpha : 1.0;
	vec3 world = end_p + side * (VERTEX.x * width);
	POSITION = PROJECTION_MATRIX * (VIEW_MATRIX * vec4(world, 1.0));
}

void fragment() {
	vec3 albedo;
	float alpha;
	edge_flow_shade(UV, TIME, v_style, v_phase, albedo, alpha);
	ALBEDO = albedo;
	ALPHA = (1.0 - pow(1.0 - alpha, layers)) * v_fade;
}
