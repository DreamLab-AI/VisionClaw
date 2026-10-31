shader_type spatial;
// Portal membrane (ADR-2166). One quad per portal, all drawn from a single
// MultiMesh. The quad sits on the box face that looks at the user, so the
// portal's own content shows through the nearly clear centre. Unshaded, added
// over the content, no depth write, depth test left on. Nothing here samples
// the screen or the depth buffer (no second eye history to go blank).
render_mode unshaded, blend_add, depth_draw_never, cull_disabled;

// 0 when reduced motion is on (the comfort default), 1 when it is off.
uniform float motion : hint_range(0.0, 1.0) = 0.0;
uniform float rim_width : hint_range(0.01, 0.5) = 0.12;
uniform float rim_strength : hint_range(0.0, 2.0) = 0.7;
uniform float centre_alpha : hint_range(0.0, 0.3) = 0.05;
uniform float fresnel_power : hint_range(0.5, 8.0) = 3.0;
uniform float pulse_amount : hint_range(0.0, 0.6) = 0.25;
uniform float pulse_hz : hint_range(0.1, 4.0) = 0.6;

// Per instance: COLOR.rgb is the portal tint, COLOR.a the focus highlight.
varying vec3 tint;
varying float focus;

void vertex() {
	tint = COLOR.rgb;
	focus = COLOR.a;
}

void fragment() {
	vec2 edge_d = min(UV, vec2(1.0) - UV);
	float edge = 1.0 - smoothstep(0.0, rim_width, min(edge_d.x, edge_d.y));
	float fres = pow(1.0 - clamp(abs(dot(normalize(NORMAL), normalize(VIEW))), 0.0, 1.0), fresnel_power);
	float pulse = 1.0 + motion * pulse_amount * sin(TIME * 6.2831853 * pulse_hz);
	float rim = (edge * (0.6 + 0.4 * focus) + fres * 0.35) * rim_strength * pulse;
	ALBEDO = tint * (0.7 + 0.5 * focus + 0.4 * edge);
	ALPHA = clamp(centre_alpha + rim, 0.0, 1.0);
}
