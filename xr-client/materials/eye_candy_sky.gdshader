shader_type sky;
// Static observatory sky (eye_candy.gd tier 1). It replaces the scene's
// ProceduralSkyMaterial on a copy of the Environment: the same colours, plus a
// faint horizon glow and a fixed field of stars. No geometry, no draw call; the
// cost is per-pixel on the pixels the graph does not cover.
//
// Static on purpose: no TIME, so it is identical under reduced motion and the
// engine renders its radiance once. Stars are hashed from the view direction,
// not drawn as sprites, and sit at infinity, so both eyes see them at the same
// direction. Not a post-process; no SCREEN_TEXTURE or depth read.
uniform vec3 zenith : source_color = vec3(0.018, 0.027, 0.055);
uniform vec3 horizon : source_color = vec3(0.105, 0.15, 0.205);
uniform vec3 nadir : source_color = vec3(0.018, 0.027, 0.045);
uniform vec3 glow : source_color = vec3(0.16, 0.34, 0.38);
uniform float glow_gain : hint_range(0.0, 1.0) = 0.22;
// Cells per unit of direction (angular cell about 0.26 degrees) and the share
// of cells holding a star.
uniform float star_cells = 220.0;
uniform float star_density : hint_range(0.0, 0.02) = 0.0035;
uniform float star_gain : hint_range(0.0, 2.0) = 0.7;

float hash13(vec3 p) {
	p = fract(p * 0.1031);
	p += dot(p, p.zyx + 31.32);
	return fract((p.x + p.y) * p.z);
}

void sky() {
	vec3 d = normalize(EYEDIR);
	float h = d.y;
	vec3 col;
	if (h >= 0.0) {
		col = mix(horizon, zenith, pow(clamp(h, 0.0, 1.0), 0.45));
	} else {
		col = mix(horizon, nadir, pow(clamp(-h, 0.0, 1.0), 0.5));
	}
	col += glow * glow_gain * exp(-abs(h) * 14.0);
	if (h > -0.04) {
		vec3 g = d * star_cells;
		vec3 id = floor(g);
		float r = hash13(id);
		if (r > 1.0 - star_density) {
			vec3 jitter = vec3(hash13(id + 7.1), hash13(id + 13.7), hash13(id + 29.3)) - 0.5;
			float dist = length(g - (id + 0.5 + jitter * 0.5));
			float s = smoothstep(0.42, 0.0, dist);
			float fade = smoothstep(-0.04, 0.12, h);
			col += vec3(0.8, 0.9, 1.0) * s * star_gain * fade * (0.45 + 0.55 * hash13(id + 3.3));
		}
	}
	COLOR = col;
}
