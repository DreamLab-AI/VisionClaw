shader_type spatial;
// Camera-facing annulus for the memory route (answer ring, beat pulse ring,
// root ring, sidecar marks). QuadMesh 2x2 → radius 1 at instance scale 1; the
// instance colour's alpha is the ring's additive weight. Billboarded against
// the main camera so both eyes agree. Additive, no post-process.
render_mode blend_add, unshaded, cull_disabled, depth_draw_never, shadows_disabled, fog_disabled;

uniform float inner = 0.82;   // three.js RingGeometry(0.82, 1)

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
	float aa = fwidth(d);
	float m = smoothstep(inner - aa, inner + aa, d) * (1.0 - smoothstep(1.0 - aa, 1.0 + aa, d));
	if (m * COLOR.a <= 0.004) {
		discard;
	}
	ALBEDO = COLOR.rgb;
	ALPHA = COLOR.a * m;
}
