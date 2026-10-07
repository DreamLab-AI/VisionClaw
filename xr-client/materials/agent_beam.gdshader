shader_type spatial;
// XR-safe agent WORK BEAM (VisionClaw ADR-140, Pillar 2). A fast, bright, directional
// particle-like stream travelling the cylinder's length from the AGENT end to the
// TARGET node — the Rust render store orients each beam's local +Y onto agent→target,
// and the fragment derives a longitudinal L (0 at agent, 1 at target) from the side
// UVs (see fragment()) so the flow always reads as "this agent is acting on that
// node" regardless of Godot's cylinder UV convention. Driven purely by
// TIME: zero per-frame GDScript/material touches. Restyle of edge_flow.gdshader — same
// render_mode (safe on gl_compatibility + multiview), but INSTANCE_CUSTOM.a now carries
// the AGENT STATUS CODE (1 working / 2 blocked) rather than the edge relation grammar.
//
// Semantic encoding (desktop parity, client/.../semanticEncoding.ts): INSTANCE_CUSTOM.r
// is the AgentActionType code (0 Query .. 5 Transform, 6 = unknown → white), .g/.b the
// beam taper at the TARGET (+Y) and AGENT (−Y) ends. The colour comes from the
// action, as on the desktop: Create widens green into the node, Delete narrows red
// into it, Query is a thin blue probe. The status still shapes the motion: a BLOCKED
// beam keeps its action hue but is pulled toward amber-red, slowed and dimmed.
// The action_*_color defaults are pinned to AGENT_ACTION_COLORS by the Rust test
// semantic::tests::beam_shader_uniform_defaults_match_the_action_palette.
render_mode blend_add, unshaded, cull_disabled, depth_draw_never, shadows_disabled, fog_disabled;

// Blocked: a stalled amber-red beam, slower and dimmer, so a stuck agent reads apart.
uniform vec3 blocked_color : source_color = vec3(1.0, 0.35, 0.20);
// AGENT_ACTION_COLORS (frameTypes.ts), sRGB hex / 255.
uniform vec3 action_query_color : source_color = vec3(0.231373, 0.509804, 0.964706);     // #3b82f6
uniform vec3 action_update_color : source_color = vec3(0.917647, 0.701961, 0.031373);    // #eab308
uniform vec3 action_create_color : source_color = vec3(0.133333, 0.772549, 0.368627);    // #22c55e
uniform vec3 action_delete_color : source_color = vec3(0.937255, 0.266667, 0.266667);    // #ef4444
uniform vec3 action_link_color : source_color = vec3(0.658824, 0.333333, 0.968627);      // #a855f7
uniform vec3 action_transform_color : source_color = vec3(0.023529, 0.713725, 0.831373); // #06b6d4
// How far a blocked beam's hue is pulled toward blocked_color.
uniform float blocked_tint = 0.6;
// Faint baseline so the beam's path is visible between pulse heads.
uniform float base_alpha = 0.12;
// Several pulse heads travel the beam at once for a particle-stream feel.
uniform float stream_heads = 4.0;
// Head travel speed (cycles/sec). Fast — this is the "bright fast stream" affordance.
uniform float stream_speed = 1.8;
// Half-width of each travelling head in per-head UV space.
uniform float head_width = 0.18;
// Extra alpha at a head's peak.
uniform float head_energy = 0.85;
// Brightness boost of a head (additive blend, so heads read as hot cores).
uniform float head_gain = 2.2;
// Blocked beams slow down and dim to signal a stall (multipliers on the above).
uniform float blocked_speed_scale = 0.35;
uniform float blocked_dim = 0.55;

varying flat float v_status;
varying flat float v_action;

vec3 action_color(float code) {
	if (code < 0.5) return action_query_color;
	if (code < 1.5) return action_update_color;
	if (code < 2.5) return action_create_color;
	if (code < 3.5) return action_delete_color;
	if (code < 4.5) return action_link_color;
	if (code < 5.5) return action_transform_color;
	return vec3(1.0);
}

void vertex() {
	// Latch the per-instance status and action codes once; fragment reads the flat varyings.
	v_status = INSTANCE_CUSTOM.a;
	v_action = INSTANCE_CUSTOM.r;
	// Taper: the unit cylinder spans local y in [-0.5, 0.5], agent (−Y) to target (+Y).
	// A zero taper (an old buffer with r/g/b = 0) falls back to the authored radius.
	float top = INSTANCE_CUSTOM.g > 0.0 ? INSTANCE_CUSTOM.g : 1.0;
	float bottom = INSTANCE_CUSTOM.b > 0.0 ? INSTANCE_CUSTOM.b : 1.0;
	VERTEX.xz *= mix(bottom, top, clamp(VERTEX.y + 0.5, 0.0, 1.0));
}

void fragment() {
	bool is_blocked = v_status > 1.5; // code 2 = blocked; 1 = working
	float speed = stream_speed * (is_blocked ? blocked_speed_scale : 1.0);

	// Godot CylinderMesh UV: the SIDE surface runs UV.y = 0 at the +Y (top) rim to
	// UV.y = 0.5 at the −Y (bottom) rim; the two end caps occupy UV.y ∈ (0.5, 1.0].
	// edge_transform12 maps the mesh's local +Y onto the AGENT→TARGET direction, so
	// the +Y rim (UV.y = 0) sits at the TARGET and the −Y rim (UV.y = 0.5) at the
	// AGENT. Define a longitudinal coordinate L that is 0 at the agent and 1 at the
	// target: L = 1 − UV.y*2  ⇒  agent(UV.y=0.5)→0, target(UV.y=0)→1. A head at
	// parameter `head` (0→1 over time) then travels agent→target, the correct flow.
	// Only the side (UV.y ≤ 0.5) streams; caps render base tint so their squashed
	// UVs never inject spurious pulses.
	bool is_side = UV.y <= 0.5;
	float uL = clamp(1.0 - UV.y * 2.0, 0.0, 1.0);

	float seg = fract(uL * stream_heads);
	float head = fract(TIME * speed);
	float d = abs(seg - head);
	d = min(d, 1.0 - d); // wrap so heads loop seamlessly along the beam
	float pulse = is_side ? smoothstep(head_width, 0.0, d) : 0.0;

	vec3 hue = action_color(v_action);
	vec3 tint = is_blocked ? mix(hue, blocked_color, blocked_tint) : hue;
	ALBEDO = tint * (1.0 + pulse * (head_gain - 1.0));

	float alpha = base_alpha + pulse * head_energy;
	if (is_blocked) {
		alpha *= blocked_dim;
	}
	ALPHA = clamp(alpha, 0.0, 1.0);
}
