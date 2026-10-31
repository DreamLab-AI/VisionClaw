shader_type spatial;
// Volume frame ribbons (eye_candy.gd tier 2; emulated_volume.gd). The 12 edges
// of a unit box, each a quad (2 triangles), expanded in the vertex stage to a
// constant ANGULAR width so the edge reads the same at 0.5 m and at 4 m and
// never aliases to a 1 px line. One mesh, one draw call a frame.
//
// Mesh layout (emulated_volume.gd `_ribbon_mesh`):
//   VERTEX  this end of the edge in unit-box coordinates (+-0.5)
//   COLOR.r edge axis: 0 = x, 0.5 = y, 1 = z (8-bit, decoded by threshold)
//   UV.x    0 / 1 = the two sides of the ribbon
//
// Billboarded about the MAIN camera, as memory_point does, so both eyes see the
// same ribbon orientation (a per-eye side vector shears in stereo). Unshaded,
// blended, no depth write; a soft falloff across the width, brightest in the
// core, stands in for a fresnel rim. No SCREEN_TEXTURE, no depth read, no
// post-process: safe on gl_compatibility + multiview (XR-client Invariants 1-2).
render_mode blend_mix, unshaded, cull_disabled, depth_draw_never, shadows_disabled, fog_disabled;

uniform vec4 frame_color : source_color = vec4(0.55, 0.8, 1.0, 0.9);
// Ribbon width as an angle (radians): 0.0026 is about 0.15 degrees.
uniform float angular_width = 0.0026;
uniform float min_width = 0.0015;
uniform float max_width = 0.03;
// Fraction of the half-width over which the edge fades out.
uniform float softness : hint_range(0.05, 1.0) = 0.7;

varying float across;

void vertex() {
	float a = COLOR.r * 2.0;
	vec3 axis = vec3(step(a, 0.5), step(0.5, a) * step(a, 1.5), step(1.5, a));
	vec3 wp = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
	vec3 dir = normalize((MODEL_MATRIX * vec4(axis, 0.0)).xyz);
	vec3 to_cam = MAIN_CAM_INV_VIEW_MATRIX[3].xyz - wp;
	float dist = max(length(to_cam), 0.05);
	vec3 side = cross(dir, to_cam);
	float sl = length(side);
	side = sl > 1.0e-6 ? side / sl : normalize(cross(dir, vec3(0.0, 1.0, 0.001)));
	float half_w = 0.5 * clamp(dist * angular_width, min_width, max_width);
	across = UV.x * 2.0 - 1.0;
	wp += side * across * half_w;
	POSITION = PROJECTION_MATRIX * VIEW_MATRIX * vec4(wp, 1.0);
}

void fragment() {
	float k = abs(across);
	float a = 1.0 - smoothstep(1.0 - softness, 1.0, k);
	float core = 1.0 - k * k;
	ALBEDO = frame_color.rgb * (1.0 + 0.5 * core);
	ALPHA = frame_color.a * a;
}
