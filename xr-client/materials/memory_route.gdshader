shader_type spatial;
// Memory-query route ribbon (XR WP7). One ArrayMesh surface holds five tubes
// (outer + inner glow sheath, gradient body, white core, comet tail) built by
// rust/src/memory_route.rs. Each vertex sits ON the centreline; NORMAL is its
// unit ring direction, UV2.x the layer radius, UV.x the route parameter u and
// UV.y the layer id. The shader pushes the vertex out to its radius, reveals
// the trace up to `head_u`, and shapes the tail as a tapering, fading window
// behind `comet_u` — so the animation needs no per-frame mesh rebuild.
// Additive emissive geometry only; glow is never a post-process (Invariant 2).
render_mode blend_add, unshaded, cull_back, depth_draw_never, shadows_disabled, fog_disabled;

uniform float head_u = 1.0;     // trace head, 0..1
uniform float comet_u = -1.0;   // comet position, < 0 = no comet
uniform float tail_u = 0.18;    // tail length as a fraction of the route
uniform float glow = 1.0;       // live glow relative to the baked glow (beat lift)

varying float v_u;
varying float v_fade;

void vertex() {
	float w = 1.0;
	v_fade = 1.0;
	if (UV.y > 3.5) {
		// comet tail: covers min(comet, tail_u) behind the comet; width and
		// alpha fall towards the back (routeMath.ts cometTail)
		float tail = min(comet_u, tail_u);
		float behind = comet_u - UV.x;
		if (comet_u < 0.0 || tail <= 0.0 || behind < 0.0 || behind > tail) {
			w = 0.0;
			v_fade = 0.0;
		} else {
			float f = 1.0 - behind / tail;
			w = (2.6 * f + 0.4) / 3.0;
			v_fade = f;
		}
	}
	VERTEX += NORMAL * UV2.x * w;
	v_u = UV.x;
}

void fragment() {
	if (v_u > head_u + 0.0005 || v_fade <= 0.001) {
		discard;
	}
	ALBEDO = COLOR.rgb * glow;
	ALPHA = COLOR.a * v_fade;
}
