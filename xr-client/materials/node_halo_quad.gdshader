shader_type spatial;
// XR-safe node halo as one camera-facing quad per node (NodesHaloMulti), replacing
// gem.tres's halo next_pass, which drew every node sphere a second time (288 → 576
// triangles; PRD-008 triangle budget). Same look, computed analytically: the old
// pass pushed the sphere shell out along its normal by `halo_width`, culled the
// front faces and showed the far wall around the silhouette with a fresnel ramp.
// For a shell of radius R seen at projected radius ρ, that wall's normal has a
// view component sqrt(1 - (ρ/R)²), so the ring is faint at the sphere and
// brightest at its outer edge — reproduced here per fragment. Inside the sphere
// (ρ < 0.5) the gem itself covers the halo, so those fragments are discarded.
//
// Additive, unshaded, no depth write; no post-process or screen texture
// (Invariants 1-2). The quad is billboarded with the main camera basis like the
// node impostor, so both eyes agree. Instance layout = the node buffer
// (render_store.rs): COLOR = node colour (raw, like gem.tres), COLOR.a = label
// fade, INSTANCE_CUSTOM .r centrality / .g fold badge / .b query flag.
render_mode blend_add, unshaded, cull_disabled, depth_draw_never, shadows_disabled, fog_disabled;

// Parameters and defaults carried over from node_halo.gdshader / node_halo.tres.
uniform float halo_width = 0.07;
uniform float fresnel_power = 2.5;
uniform float halo_energy = 0.65;
uniform float cen_boost = 0.25;
uniform float badge_width = 0.04;
uniform float badge_boost = 0.45;
uniform float query_width = 0.06;
uniform float query_pulse_speed = 1.2;
uniform float query_pulse_depth = 0.12; // 0..1 fraction the pulse swings energy

varying float halo_r;      // shell radius in mesh units (sphere radius 0.5)
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
	float extra = (halo_badge > 0.5) ? badge_width : 0.0;
	extra += (halo_query > 0.5) ? query_width : 0.0;
	halo_r = 0.5 + halo_width + extra;
	// The 1×1 quad covers radius 0.5; grow it to the shell radius.
	VERTEX.xy *= halo_r / 0.5;
	MODELVIEW_MATRIX = VIEW_MATRIX * mat4(MAIN_CAM_INV_VIEW_MATRIX[0], MAIN_CAM_INV_VIEW_MATRIX[1], MAIN_CAM_INV_VIEW_MATRIX[2], MODEL_MATRIX[3]);
	MODELVIEW_MATRIX = MODELVIEW_MATRIX * mat4(
		vec4(length(MODEL_MATRIX[0].xyz), 0.0, 0.0, 0.0),
		vec4(0.0, length(MODEL_MATRIX[1].xyz), 0.0, 0.0),
		vec4(0.0, 0.0, length(MODEL_MATRIX[2].xyz), 0.0),
		vec4(0.0, 0.0, 0.0, 1.0));
	MODELVIEW_NORMAL_MATRIX = mat3(MODELVIEW_MATRIX);
}

void fragment() {
	// Projected radius ρ in mesh units: the quad spans [-halo_r, halo_r].
	float rho = length(UV * 2.0 - 1.0) * halo_r;
	if (rho > halo_r || rho < 0.5) {
		discard;
	}
	float q = rho / halo_r;
	float ndotv = sqrt(max(1.0 - q * q, 0.0));
	float fres = pow(1.0 - ndotv, fresnel_power);
	float cen_mul = mix(1.0 - cen_boost, 1.0 + cen_boost, clamp(halo_cen, 0.0, 1.0));
	float badge_mul = (halo_badge > 0.5) ? (1.0 + badge_boost) : 1.0;
	float query_mul = 1.0;
	if (halo_query > 0.5) {
		float pulse = sin(TIME * query_pulse_speed) * 0.5 + 0.5;
		query_mul = 1.0 + query_pulse_depth * (pulse * 2.0 - 1.0);
	}
	ALBEDO = halo_tint * halo_energy * cen_mul * badge_mul * query_mul;
	ALPHA = fres * cen_mul * query_mul * halo_alpha;
}
