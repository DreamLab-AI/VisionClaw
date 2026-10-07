shader_type spatial;
// WP4 cluster hulls (desktop ClusterHulls.tsx parity). One translucent ArrayMesh
// surface holds every hull, so the layer is a single draw call. Desktop look:
// meshBasicMaterial, opacity 0.08, DoubleSide, depthWrite false. XR adds a
// fresnel edge so a hull reads as a shell in stereo rather than a faint fog; it
// is pure per-fragment alpha, no post-process, no screen texture, no depth read
// (XR-client.md Invariants 1-2, ADR-2107). Vertex COLOR is the hull palette,
// passed through raw like the node gem material, so a hull and its member
// nodes read as the same hue.
render_mode unshaded, blend_mix, depth_draw_never, cull_disabled, shadows_disabled, fog_disabled;

// Desktop clusterHulls.opacity default.
uniform float opacity : hint_range(0.0, 0.5) = 0.08;
// Extra alpha at grazing angles (silhouette); 0 reproduces the flat desktop look.
uniform float edge_boost : hint_range(0.0, 0.5) = 0.18;
uniform float edge_power : hint_range(0.5, 8.0) = 3.0;
// Ceiling so overlapping hull edges never turn opaque.
uniform float max_alpha : hint_range(0.0, 1.0) = 0.35;

void fragment() {
	// cull_disabled: flip the normal on back faces so both sides get the rim.
	vec3 n = normalize(FRONT_FACING ? NORMAL : -NORMAL);
	float facing = abs(dot(n, normalize(VIEW)));
	float rim = pow(1.0 - facing, edge_power);
	ALBEDO = COLOR.rgb;
	ALPHA = clamp(opacity + edge_boost * rim, 0.0, max_alpha);
}
