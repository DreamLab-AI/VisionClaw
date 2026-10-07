shader_type spatial;
// Memory-cloud point sprite (XR WP6). One camera-facing quad per MultiMesh
// instance: the instance basis carries only the sprite diameter (uniform
// scale), the origin carries the row position, INSTANCE colour the palette
// colour and opacity. Billboarded against the MAIN camera so both eyes see the
// same quad orientation (per-eye billboards shear in stereo). A soft round
// mask replaces three.js's square-ish points; no texture, no post-process —
// safe on gl_compatibility + multiview (XR-client Invariants 1-2).
render_mode blend_mix, unshaded, cull_disabled, depth_draw_never, shadows_disabled, fog_disabled;

// memory_flash emphasis (INSTANCE_CUSTOM = tint rgb, gain; gain 1 = untouched,
// memory_cloud.rs NEUTRAL_EMPHASIS). Emission only, no post-process.
varying vec4 emph;

// Fraction of the radius over which the disc edge fades out.
uniform float softness : hint_range(0.0, 1.0) = 0.35;

void vertex() {
	emph = INSTANCE_CUSTOM;
	// Keep the instance origin and its full world scale (GraphRoot fit x cloud
	// scale x sprite size); replace the rotation with the camera's.
	float s = length(MODEL_MATRIX[0].xyz);
	MODELVIEW_MATRIX = VIEW_MATRIX * mat4(
		MAIN_CAM_INV_VIEW_MATRIX[0] * s,
		MAIN_CAM_INV_VIEW_MATRIX[1] * s,
		MAIN_CAM_INV_VIEW_MATRIX[2] * s,
		MODEL_MATRIX[3]);
}

void fragment() {
	float d = length(UV - vec2(0.5)) * 2.0;
	float k = clamp((emph.a - 1.0) / 1.5, 0.0, 1.0);
	float a = mix(COLOR.a, 1.0, k) * (1.0 - smoothstep(1.0 - softness, 1.0, d));
	if (a <= 0.004) {
		discard;
	}
	ALBEDO = mix(COLOR.rgb, emph.rgb, k) * max(emph.a, 1.0);
	ALPHA = a;
}
