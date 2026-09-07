shader_type spatial;
// XR-safe flowing edge. Unshaded additive cyan gradient with a pulse of light that
// travels along the cylinder's length (UV.y, 0 at one end to 1 at the other),
// driven purely by TIME so there are zero per-frame GDScript/material touches.
// Faint base alpha keeps dense edge bundles from blowing out to white. No
// post-process / screen-space anything — safe on gl_compatibility + multiview.
// Alpha (mix) blending: overlapping edges accumulate towards the edge colour
// instead of summing to white the way additive did in dense bundles.
render_mode blend_mix, unshaded, cull_disabled, depth_draw_never, shadows_disabled, fog_disabled;

uniform vec3 flow_color : source_color = vec3(0.30, 0.72, 1.0);
// Baseline alpha so the whole edge network is faintly visible.
uniform float base_alpha = 0.35;
// Pulse travel speed (cycles per second along the edge).
uniform float pulse_speed = 0.6;
// Half-width of the travelling pulse in UV space.
uniform float pulse_width = 0.22;
// Extra alpha at the pulse peak.
uniform float pulse_energy = 0.4;
// Brightness boost of the moving highlight colour (kept gentle so the pulse
// head reads as brightened cyan, not white).
uniform float pulse_gain = 1.2;
// Relation-type grammar (Wave 2, Feature 4 — OntoAir clean-room). INSTANCE_CUSTOM.a
// carries a style code set per-edge by the Rust render store: 0 untyped (faint),
// 1 typed (solid — the baseline look), 2 subclass/taxonomy (dashed + dimmer). The
// r/g/b custom channels are reserved (0).
// How dashed a subclass edge reads (fraction of each dash cycle that is a gap).
uniform float subclass_dash_gap = 0.45;
// Number of dash cycles along the whole edge length.
uniform float subclass_dash_freq = 14.0;
// Alpha multiplier for the dimmer relation classes.
uniform float subclass_dim = 0.5;
uniform float untyped_dim = 0.4;
// Distinct tint for taxonomy edges so subclass structure reads apart from typed
// links at a glance (cool violet vs the cyan of typed edges).
uniform vec3 subclass_color : source_color = vec3(0.62, 0.5, 1.0);

// Asserted/inferred channel (Wave 3). INSTANCE_CUSTOM.a code 3 = a reasoner-
// ENTAILED edge: amber + dashed, so epistemic status reads apart from asserted
// subclass (code 2, lilac-dashed). A tighter dash than subclass so the two dashed
// styles are distinguishable at a glance.
uniform vec3 inferred_color : source_color = vec3(1.0, 0.66, 0.12);
uniform float inferred_dash_gap = 0.5;
uniform float inferred_dash_freq = 22.0;
uniform float inferred_dim = 0.7;

varying flat float v_style;
varying flat float v_phase;

void vertex() {
	// Latch the per-instance style code once; fragment reads the flat varying.
	v_style = INSTANCE_CUSTOM.a;
	v_phase = fract(float(INSTANCE_ID) * 0.61803398875);
}

void fragment() {
	// UV.y runs along the unit cylinder's height; the per-instance transform
	// stretches the edge but leaves UV in [0,1], so the pulse always sweeps the
	// full length uniformly regardless of edge length.
	float t = UV.y;
	float head = fract(TIME * pulse_speed + v_phase);
	float d = abs(t - head);
	d = min(d, 1.0 - d); // wrap so the pulse loops seamlessly end-to-end
	float pulse = 1.0 - smoothstep(0.0, pulse_width, d);

	bool is_inferred = v_style > 2.5;                     // code 3 (Wave 3)
	bool is_subclass = v_style > 1.5 && v_style < 2.5;    // code 2
	bool is_untyped = v_style < 0.5;                      // code 0

	vec3 tint = is_inferred ? inferred_color : (is_subclass ? subclass_color : flow_color);
	ALBEDO = tint * (1.0 + pulse * (pulse_gain - 1.0));

	float alpha = base_alpha + pulse * pulse_energy;
	if (is_inferred) {
		// Reasoner-entailed: amber, tightly dashed, gently dimmed.
		float phase = fract(UV.y * inferred_dash_freq);
		float aa = max(fwidth(UV.y * inferred_dash_freq), 0.005);
		float dash = smoothstep(inferred_dash_gap, inferred_dash_gap + aa, phase) * (1.0 - smoothstep(1.0 - aa, 1.0, phase));
		alpha *= inferred_dim * dash;
	} else if (is_subclass) {
		// Asserted subclass: dashed + dimmer (lilac tint above).
		float phase = fract(UV.y * subclass_dash_freq);
		float aa = max(fwidth(UV.y * subclass_dash_freq), 0.005);
		float dash = smoothstep(subclass_dash_gap, subclass_dash_gap + aa, phase) * (1.0 - smoothstep(1.0 - aa, 1.0, phase));
		alpha *= subclass_dim * dash;
	} else if (is_untyped) {
		alpha *= untyped_dim;
	}
	ALPHA = clamp(alpha, 0.0, 1.0);
}
