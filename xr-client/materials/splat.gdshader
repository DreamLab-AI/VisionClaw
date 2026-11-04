shader_type spatial;
// Gaussian splats as camera-facing quads (ADR-2170 decision 6; rust/src/splat_loader.rs
// and splat_sort.rs). One MultiMesh, one draw call per splat volume.
//
// Instance i is the i-th splat of the sorted order: the vertex shader reads the
// splat index from `splat_index` (written by splat_sort.rs, far to near, one order
// for both eyes) and the splat's static data from `splat_data` (written once by
// splat_loader.rs), both with texelFetch by INSTANCE_ID. The MultiMesh's own
// instance transforms are never read or rewritten: MODEL_MATRIX would include them,
// so the volume's content transform arrives as the `splat_model` uniform instead.
//
// Compatibility/multiview-safe by construction, as edge_ribbon.gdshader: no screen
// texture, no depth-texture read, no SubViewport, no compute, no post-process. Both
// eyes project the same quad with their own VIEW_MATRIX and PROJECTION_MATRIX. That
// INSTANCE_ID plus texelFetch behaves under Compatibility multiview is UNVERIFIED on
// device (ADR-2170, XR-client Invariant 1).
//
// Blending. blend_mix is SRC_ALPHA, ONE_MINUS_SRC_ALPHA, so the blend unit already
// does the premultiply: the fragment writes the straight colour and the Gaussian
// alpha (opacity x exp(-d^2 / 2)), not colour x alpha, which would darken twice.
// Colours are used raw, the instance-colour convention (test_instance_color_space.gd).
// Splats write no depth (depth_draw_never) and are depth-tested against the scene.
render_mode blend_mix, unshaded, cull_disabled, depth_draw_never, shadows_disabled, fog_disabled;

// Layout of the data and index textures. These repeat splat_loader.rs
// (TEXELS_PER_SPLAT, DATA_ROW_SPLATS) and splat_sort.rs (INDEX_TEX_WIDTH); a GUT
// test pins them to the Rust constants.
const int TEXELS_PER_SPLAT = 4;
const int DATA_ROW_SPLATS = 1024;
const int INDEX_W = 1024;
// Screen-space half-extent cap of a quad, pixels: a splat that fills the view costs
// a bounded amount of overdraw (ADR-2170 comfort guard).
const float MAX_RADIUS_PX = 64.0;
// Splats within this many metres of the near plane fade out, so a head inside the
// cloud gets neither an overdraw spike nor a wall of noise.
const float NEAR_FADE_M = 0.3;
// Low-pass added to the projected covariance, px^2 (the standard 3DGS dilation).
const float DILATION_PX2 = 0.3;
// Quad half-extent in standard deviations.
const float EXTENT_SIGMA = 3.0;

// texel 0: position xyz, opacity. texel 1: colour rgb. texel 2: cov xx, xy, xz, yy.
// texel 3: cov yz, zz.
uniform sampler2D splat_data : filter_nearest, repeat_disable;
// Sorted splat indices as float32 (exact below 2^24), 4 bytes a splat.
uniform sampler2D splat_index : filter_nearest, repeat_disable;
// The volume content's transform, splat space to world.
uniform mat4 splat_model = mat4(1.0);
// The camera's near plane, metres (both eyes share it).
uniform float near_plane = 0.05;
// 0..1 dim for the reduced-motion enter / leave / tier-change snap and the eased fade.
uniform float fade = 1.0;
// Scales every splat's opacity (the entered room is a backdrop: 1.0).
uniform float opacity_scale = 1.0;

varying vec2 v_quad;     // quad corner in -1..1
varying vec2 v_sigma;    // quad half-extent in standard deviations along the two axes
varying vec4 v_colour;   // rgb, alpha before the Gaussian

// POSITION that clips every vertex of the quad away.
const vec4 CULLED = vec4(0.0, 0.0, 2.0, 1.0);

void vertex() {
	int id = int(INSTANCE_ID);
	int sorted = int(texelFetch(splat_index, ivec2(id % INDEX_W, id / INDEX_W), 0).r + 0.5);
	ivec2 base = ivec2((sorted % DATA_ROW_SPLATS) * TEXELS_PER_SPLAT, sorted / DATA_ROW_SPLATS);
	vec4 t0 = texelFetch(splat_data, base, 0);
	vec4 t1 = texelFetch(splat_data, base + ivec2(1, 0), 0);
	vec4 t2 = texelFetch(splat_data, base + ivec2(2, 0), 0);
	vec4 t3 = texelFetch(splat_data, base + ivec2(3, 0), 0);

	// View space centre; Godot looks down -Z, so depth is -z.
	vec4 vp = VIEW_MATRIX * (splat_model * vec4(t0.xyz, 1.0));
	float depth = -vp.z;
	float near_fade = smoothstep(near_plane, near_plane + NEAR_FADE_M, depth);
	float alpha = t0.w * opacity_scale * fade * near_fade;
	vec4 clip = PROJECTION_MATRIX * vp;
	// Behind the near fade, invisible, or well outside the frustum: no pixels. (A vertex()
	// may not return early, so the projection below is the else branch.)
	bool keep = alpha >= 1.0 / 255.0 && clip.w > 0.0 && abs(clip.x) <= 1.3 * clip.w && abs(clip.y) <= 1.3 * clip.w;
	POSITION = CULLED;
	v_quad = vec2(0.0);
	v_sigma = vec2(1.0);
	v_colour = vec4(0.0);
	if (keep) {
		// 3D covariance, splat space, then into view space (the volume's fit scale is
		// part of splat_model, so the covariance scales with the content).
		mat3 cov = mat3(
			vec3(t2.x, t2.y, t2.z),
			vec3(t2.y, t2.w, t3.x),
			vec3(t2.z, t3.x, t3.y));
		mat3 w = mat3(VIEW_MATRIX) * mat3(splat_model);
		mat3 cv = w * cov * transpose(w);

		// EWA projection: Jacobian of the perspective map at the centre, in pixels.
		vec2 focal = vec2(PROJECTION_MATRIX[0][0], PROJECTION_MATRIX[1][1]) * VIEWPORT_SIZE * 0.5;
		float d = max(depth, near_plane);
		vec3 j0 = vec3(focal.x / d, 0.0, focal.x * vp.x / (d * d));
		vec3 j1 = vec3(0.0, focal.y / d, focal.y * vp.y / (d * d));
		float a = dot(j0, cv * j0) + DILATION_PX2;
		float b = dot(j0, cv * j1);
		float c = dot(j1, cv * j1) + DILATION_PX2;

		// Eigen-decomposition of [[a, b], [b, c]]: the ellipse's axes and variances.
		float mid = 0.5 * (a + c);
		float rad = sqrt(max(0.25 * (a - c) * (a - c) + b * b, 0.0));
		float l1 = max(mid + rad, 0.1);
		float l2 = max(mid - rad, 0.1);
		vec2 axis1 = abs(b) > 1.0e-6 ? normalize(vec2(b, l1 - a)) : (a >= c ? vec2(1.0, 0.0) : vec2(0.0, 1.0));
		vec2 axis2 = vec2(-axis1.y, axis1.x);
		float s1 = sqrt(l1);
		float s2 = sqrt(l2);
		// Quad half-extents in pixels, capped; the Gaussian is still evaluated in
		// standard deviations (v_sigma), so a capped quad is a truncated splat, not a
		// squashed one.
		float e1 = min(EXTENT_SIGMA * s1, MAX_RADIUS_PX);
		float e2 = min(EXTENT_SIGMA * s2, MAX_RADIUS_PX);

		vec2 offset_px = axis1 * (VERTEX.x * e1) + axis2 * (VERTEX.y * e2);
		vec2 offset_ndc = offset_px * 2.0 / VIEWPORT_SIZE;
		POSITION = vec4(clip.xy + offset_ndc * clip.w, clip.z, clip.w);
		v_quad = VERTEX.xy;
		v_sigma = vec2(e1 / s1, e2 / s2);
		v_colour = vec4(t1.rgb, alpha);
	}
}

void fragment() {
	vec2 p = v_quad * v_sigma;
	float power = -0.5 * dot(p, p);
	float alpha = v_colour.a * exp(power);
	if (alpha < 1.0 / 255.0) {
		discard;
	}
	ALBEDO = v_colour.rgb;
	ALPHA = alpha;
}
