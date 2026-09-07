shader_type spatial;
// XR-safe fake-bloom halo (no post-process / WorldEnvironment glow — this runs on
// Godot 4.6 gl_compatibility + OpenXR multiview). It is a second pass over the node
// mesh: the shell is pushed out along the normal, front faces are culled so we see
// its far side around the silhouette, and a fresnel falloff makes a soft additive
// rim that reads as a glow. Additive + depth_draw_never so it never occludes.
render_mode blend_add, unshaded, cull_front, depth_draw_never, shadows_disabled, fog_disabled;

// Shell expansion in local mesh units (node sphere radius is 0.5). Scales with the
// per-instance transform, so the halo stays proportional to node size.
uniform float halo_width = 0.07;
// Fresnel exponent — higher = tighter rim.
uniform float fresnel_power = 2.5;
// Additive brightness multiplier for the halo tint.
uniform float halo_energy = 0.65;

// How much node centrality (INSTANCE_CUSTOM.r, 0..1) lifts halo intensity. The
// halo brightness is multiplied by mix(1-cen_boost, 1+cen_boost, centrality), so
// central nodes glow a little harder without low-centrality nodes going dark.
uniform float cen_boost = 0.25;

// Fold badge (Wave 3): a node that is a fold representative carries a collapsed
// member count in INSTANCE_CUSTOM.g (>0). It gets a wider, brighter halo ring so
// an aggregate node reads as "denser than a leaf" at a glance — the visual half
// of the "+N collapsed" badge (the proximity label carries the exact count).
uniform float badge_width = 0.04;
uniform float badge_boost = 0.45;

// Query variable (flagship visual query builder): a node marked as a query
// variable carries a flag in INSTANCE_CUSTOM.b (>0, set in render_store.rs). Its
// COLOR is already the saturated query palette, so the tint follows it; here we
// add a distinct animated PULSE + a slightly wider shell so a marked ?vN node
// reads apart from both leaf nodes and fold representatives.
uniform float query_width = 0.06;
uniform float query_pulse_speed = 1.2;
uniform float query_pulse_depth = 0.12; // 0..1 fraction the pulse swings energy

// INSTANCE_CUSTOM channel map (coordinate before repurposing a channel):
//   .r = normalised centrality (base glow strength)
//   .g = fold badge count > 0        — Wave 3 fold ladder
//   .b = query-variable flag > 0     — flagship query builder
//   .a = reserved (1)
// Per-instance colour arrives via MultiMesh use_colors as vertex COLOR — the halo
// tint follows the node's colour exactly like the base pass. COLOR.a is the
// co-proximate label fade (render_store.rs LABEL_FADE_ALPHA, 1.0 when no label is
// showing): the rim dims with the sphere so the text inside stays legible.
varying vec3 halo_tint;
varying float halo_alpha;
varying float halo_cen;
varying float halo_badge;
varying float halo_query;

void vertex() {
	halo_tint = COLOR.rgb;
	halo_alpha = COLOR.a;
	halo_cen = INSTANCE_CUSTOM.r;
	halo_badge = INSTANCE_CUSTOM.g;
	halo_query = INSTANCE_CUSTOM.b;
	// A fold representative and/or a query variable push the shell out further for a
	// fatter ring (the two boosts stack if a node is both).
	float extra = (halo_badge > 0.5) ? badge_width : 0.0;
	extra += (halo_query > 0.5) ? query_width : 0.0;
	VERTEX += normalize(NORMAL) * (halo_width + extra);
}

void fragment() {
	// VIEW points from the fragment toward the camera (view space); NORMAL is the
	// view-space geometric normal. Rim-lit fresnel: bright at grazing angles.
	float ndotv = clamp(dot(normalize(NORMAL), normalize(VIEW)), 0.0, 1.0);
	float fres = pow(1.0 - ndotv, fresnel_power);
	float cen_mul = mix(1.0 - cen_boost, 1.0 + cen_boost, clamp(halo_cen, 0.0, 1.0));
	// Fold representatives glow harder (additive energy only — the alpha rim shape
	// is unchanged so the silhouette stays clean).
	float badge_mul = (halo_badge > 0.5) ? (1.0 + badge_boost) : 1.0;
	// Query variables pulse: energy swings ±query_pulse_depth at query_pulse_speed
	// so a marked ?vN node visibly breathes (distinct from the steady fold/leaf
	// rims). TIME is seconds; the same phase drives every marked node.
	float query_mul = 1.0;
	if (halo_query > 0.5) {
		float pulse = sin(TIME * query_pulse_speed) * 0.5 + 0.5; // 0..1
		query_mul = 1.0 + query_pulse_depth * (pulse * 2.0 - 1.0);
	}
	// Unshaded + additive: ALBEDO is the emitted colour, ALPHA (with SRC_ALPHA,ONE)
	// scales how much is added, so a thin bright rim fades to nothing face-on.
	ALBEDO = halo_tint * halo_energy * cen_mul * badge_mul * query_mul;
	ALPHA = fres * cen_mul * query_mul * halo_alpha;
}
