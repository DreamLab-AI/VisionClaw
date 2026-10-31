shader_type spatial;
// Ambient motes (eye_candy.gd tier 3-4): one MultiMesh of single-triangle
// camera-facing sprites, positioned once and animated only here in the vertex
// stage (the memory_point pattern). A soft round mask inside the triangle's
// incircle stands in for a texture. Additive, so overlapping motes need no
// sorting; depth-tested so the graph occludes them, never written.
//
// Per instance (MultiMesh custom data): x = phase, y = size scale, z = gain.
// `motion` is 0 under reduced motion: the motes then hold still, with no drift
// and no twinkle (TIME is read only when motion > 0). Billboarded about the
// MAIN camera so both eyes agree. A mote nearer the head than NEAR_FADE_END
// fades out so none sits in front of the eyes.
//
// No SCREEN_TEXTURE, no depth read, no post-process: safe on gl_compatibility +
// multiview (XR-client Invariants 1-2).
render_mode blend_add, unshaded, cull_disabled, depth_draw_never, shadows_disabled, fog_disabled;

uniform vec3 mote_color : source_color = vec3(0.55, 0.8, 0.9);
uniform float intensity : hint_range(0.0, 1.0) = 0.32;
// 0 = still (reduced motion), 1 = drift and twinkle.
uniform float motion : hint_range(0.0, 1.0) = 0.0;
uniform float drift_m = 0.06;
// Sprite diameter in metres at instance size 1.
uniform float base_size = 0.011;
uniform float near_fade_start = 0.35;
uniform float near_fade_end = 1.2;

varying vec2 local;
varying float level;

void vertex() {
	local = VERTEX.xy;
	float phase = INSTANCE_CUSTOM.x;
	float size = INSTANCE_CUSTOM.y;
	float gain = INSTANCE_CUSTOM.z;
	vec4 o = MODEL_MATRIX[3];
	float t = 0.0;
	if (motion > 0.0) {
		t = TIME;
		o.xyz += drift_m * motion * vec3(sin(t * 0.13 + phase * 6.283), sin(t * 0.09 + phase * 17.0), cos(t * 0.11 + phase * 11.0));
	}
	float twinkle = motion > 0.0 ? 0.75 + 0.25 * sin(t * 0.9 + phase * 40.0) : 1.0;
	float d = length(o.xyz - MAIN_CAM_INV_VIEW_MATRIX[3].xyz);
	level = gain * twinkle * smoothstep(near_fade_start, near_fade_end, d);
	float s = base_size * size * length(MODEL_MATRIX[0].xyz);
	MODELVIEW_MATRIX = VIEW_MATRIX * mat4(
		MAIN_CAM_INV_VIEW_MATRIX[0] * s,
		MAIN_CAM_INV_VIEW_MATRIX[1] * s,
		MAIN_CAM_INV_VIEW_MATRIX[2] * s,
		o);
}

void fragment() {
	// The mesh triangle has circumradius 2, so its incircle has radius 1.
	float d = length(local);
	float m = 1.0 - smoothstep(0.2, 1.0, d);
	float a = m * m * level * intensity;
	if (a <= 0.004) {
		discard;
	}
	ALBEDO = mote_color * a;
	ALPHA = 1.0;
}
