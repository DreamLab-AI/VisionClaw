shader_type spatial;
// A stationary metre grid gives scale without moving the viewer or the world.
// Derivative antialiasing avoids high-frequency shimmer in either XR eye.
render_mode unshaded, cull_disabled, shadows_disabled, depth_draw_never;
uniform vec3 floor_color : source_color = vec3(0.018, 0.027, 0.045);
uniform vec3 horizon_color : source_color = vec3(0.105, 0.15, 0.205);
uniform vec3 grid_color : source_color = vec3(0.16, 0.29, 0.34);
uniform vec3 accent_color : source_color = vec3(0.24, 0.58, 0.61);
varying vec2 ground_position;

float grid_line(vec2 p, float spacing) {
    vec2 cell = p / spacing;
    vec2 width = max(fwidth(cell), vec2(0.0001));
    vec2 distance_to_line = abs(fract(cell - 0.5) - 0.5) / width;
    return 1.0 - min(min(distance_to_line.x, distance_to_line.y), 1.0);
}

void vertex() {
    ground_position = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xz;
}

void fragment() {
    float distance_from_origin = length(ground_position);
    float fade = 1.0 - smoothstep(2.8, 8.0, distance_from_origin);
    float major = grid_line(ground_position, 1.0) * 0.20;
    float minor = grid_line(ground_position, 0.25) * 0.035;
    vec2 graph_center = ground_position - vec2(0.0, -1.6);
    float radius = length(graph_center);
    float aa = max(fwidth(radius), 0.001);
    float orbit = 1.0 - smoothstep(0.008, 0.008 + aa, abs(radius - 1.55));
    float inner = 1.0 - smoothstep(0.004, 0.004 + aa, abs(radius - 1.45));
    float pool = exp(-radius * radius * 0.25);
    // Transparent reference marks never hide nodes moved below the ground plane.
    ALBEDO = mix(grid_color, accent_color, max(orbit, inner));
    ALPHA = clamp((max(major, minor) + orbit * 0.30 + inner * 0.08 + pool * 0.012) * fade, 0.0, 0.35);
}
