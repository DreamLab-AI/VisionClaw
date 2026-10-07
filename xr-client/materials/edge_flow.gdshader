shader_type spatial;
// XR-safe flowing edge. Unshaded additive cyan gradient with a pulse of light that
// travels along the cylinder's length (UV.y, 0 at one end to 1 at the other),
// driven purely by TIME so there are zero per-frame GDScript/material touches.
// Faint base alpha keeps dense edge bundles from blowing out to white. No
// post-process / screen-space anything — safe on gl_compatibility + multiview.
// Alpha (mix) blending: overlapping edges accumulate towards the edge colour
// instead of summing to white the way additive did in dense bundles.
render_mode blend_mix, unshaded, cull_disabled, depth_draw_never, shadows_disabled, fog_disabled;

#include "res://materials/edge_flow_common.gdshaderinc"

void vertex() {
	// Latch the per-instance style code once; fragment reads the flat varying.
	v_style = INSTANCE_CUSTOM.a;
	v_phase = fract(float(INSTANCE_ID) * 0.61803398875);
}

void fragment() {
	vec3 albedo;
	float alpha;
	edge_flow_shade(UV, TIME, v_style, v_phase, albedo, alpha);
	ALBEDO = albedo;
	ALPHA = alpha;
}
