// Shared edge shading for edge_flow.gdshader (near-field cylinders) and
// edge_ribbon.gdshader (far-field ribbons), so both tiers of the edge LOD carry
// the same relation grammar, pulse and palette. Moved verbatim from
// edge_flow.gdshader (2026-10-07); materials set these uniforms by name.
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

// Each including shader declares nothing else: it latches the per-instance
// style code and pulse phase into these flat varyings in vertex() and passes them
// to edge_flow_shade() from fragment().
varying flat float v_style;
varying flat float v_phase;

void edge_flow_shade(vec2 uv, float time, float style, float phase, out vec3 albedo, out float alpha_out) {
	// uv.y runs along the unit cylinder's height; the per-instance transform
	// stretches the edge but leaves UV in [0,1], so the pulse always sweeps the
	// full length uniformly regardless of edge length.
	float t = uv.y;
	float head = fract(time * pulse_speed + phase);
	float d = abs(t - head);
	d = min(d, 1.0 - d); // wrap so the pulse loops seamlessly end-to-end
	float pulse = 1.0 - smoothstep(0.0, pulse_width, d);

	bool is_inferred = style > 2.5;                     // code 3 (Wave 3)
	bool is_subclass = style > 1.5 && style < 2.5;    // code 2
	bool is_untyped = style < 0.5;                      // code 0

	vec3 tint = is_inferred ? inferred_color : (is_subclass ? subclass_color : flow_color);
	albedo = tint * (1.0 + pulse * (pulse_gain - 1.0));

	float alpha = base_alpha + pulse * pulse_energy;
	if (is_inferred) {
		// Reasoner-entailed: amber, tightly dashed, gently dimmed.
		float phase = fract(uv.y * inferred_dash_freq);
		float aa = max(fwidth(uv.y * inferred_dash_freq), 0.005);
		float dash = smoothstep(inferred_dash_gap, inferred_dash_gap + aa, phase) * (1.0 - smoothstep(1.0 - aa, 1.0, phase));
		alpha *= inferred_dim * dash;
	} else if (is_subclass) {
		// Asserted subclass: dashed + dimmer (lilac tint above).
		float phase = fract(uv.y * subclass_dash_freq);
		float aa = max(fwidth(uv.y * subclass_dash_freq), 0.005);
		float dash = smoothstep(subclass_dash_gap, subclass_dash_gap + aa, phase) * (1.0 - smoothstep(1.0 - aa, 1.0, phase));
		alpha *= subclass_dim * dash;
	} else if (is_untyped) {
		alpha *= untyped_dim;
	}
	alpha_out = clamp(alpha, 0.0, 1.0);
}
