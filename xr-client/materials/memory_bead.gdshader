shader_type spatial;
// Route beads, halos and the comet head/glow (XR WP7): camera-facing discs on
// the cloud's one-triangle sprite (BEAD_TRIANGLES in memory_route.rs).
// Unshaded and additive, a sphere would draw as this same flat disc, and a
// coarse one shows its facets. Billboarded on the MAIN camera so both eyes see
// the same disc (per-eye billboards shear in stereo). Glow is emissive
// geometry only (XR-client Invariant 2); no screen-space pass.
render_mode blend_add, unshaded, cull_disabled, depth_draw_never, shadows_disabled, fog_disabled;

// Where the edge fade starts, as a fraction of the radius.
uniform float edge : hint_range(0.0, 1.0) = 0.55;

void vertex() {
	float s = length(MODEL_MATRIX[0].xyz);
	MODELVIEW_MATRIX = VIEW_MATRIX * mat4(
		MAIN_CAM_INV_VIEW_MATRIX[0] * s,
		MAIN_CAM_INV_VIEW_MATRIX[1] * s,
		MAIN_CAM_INV_VIEW_MATRIX[2] * s,
		MODEL_MATRIX[3]);
}

void fragment() {
	float d = length(UV - vec2(0.5)) * 2.0;
	float a = COLOR.a * (1.0 - smoothstep(edge, 1.0, d));
	if (a <= 0.004) {
		discard;
	}
	ALBEDO = COLOR.rgb;
	ALPHA = a;
}
