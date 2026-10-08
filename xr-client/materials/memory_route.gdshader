shader_type spatial;
// Memory-query route ribbon (XR WP7). One ArrayMesh surface holds five tubes
// (outer + inner glow sheath, gradient body, white core, comet tail) built by
// rust/src/memory_route.rs. Each vertex sits ON the centreline; NORMAL is its
// unit ring direction, UV2.x the layer radius, UV.x the route parameter u and
// UV.y the layer id. The shader pushes the vertex out to its radius, reveals
// the trace up to `head_u`, and shapes the tail as a tapering, fading window
// behind `comet_u` — so the animation needs no per-frame mesh rebuild.
// Additive emissive geometry only; glow is never a post-process (Invariant 2).
// Drawn over the scene like the desktop overlay (depthTest false): no depth
// test, so the opaque glass nodes cannot hide the route; ordering comes from
// render_priority (memory_route.rs ROUTE_RENDER_PRIORITY, below the HUD's
// OVERLAY_RENDER_PRIORITY). Depth test is pipeline state of the one multiview
// draw, so both eyes agree.
render_mode blend_add, unshaded, cull_back, depth_draw_never, depth_test_disabled, shadows_disabled, fog_disabled;

uniform float head_u = 1.0;     // trace head, 0..1
uniform float comet_u = -1.0;   // comet position, < 0 = no comet
uniform float tail_u = 0.18;    // tail length as a fraction of the route
uniform float glow = 1.0;       // live glow relative to the baked glow (beat lift)
// Drawing-in segment (memory_route.rs walk_envelope): the four line layers are
// baked at the settled line scale; behind the trace head they widen by
// walk_width and brighten by walk_glow, easing back over walk_window, all
// times the draw-in envelope `walk` (1 while tracing, 0 once settled or under
// reduced motion). The comet tail (layer 4) is baked at the walk scale.
uniform float walk = 0.0;
uniform float walk_window = 0.18;
uniform float walk_width = 1.0;
uniform float walk_glow = 1.0;

varying float v_u;
varying float v_fade;
varying float v_glow;

void vertex() {
	float w = 1.0;
	v_fade = 1.0;
	v_glow = 1.0;
	if (UV.y < 3.5) {
		float behind = head_u - UV.x;
		if (walk > 0.0 && behind >= 0.0 && behind < walk_window) {
			float f = 1.0 - behind / walk_window;
			float e = min(walk, 1.0) * f * f * (3.0 - 2.0 * f);
			w = mix(1.0, walk_width, e);
			v_glow = mix(1.0, walk_glow, e);
		}
	} else {
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
	ALBEDO = COLOR.rgb * glow * v_glow;
	ALPHA = COLOR.a * v_fade;
}
