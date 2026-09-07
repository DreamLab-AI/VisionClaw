shader_type spatial;
// One depth-tested world-space focus reticle, never a full-screen overlay.
render_mode unshaded, blend_mix, cull_disabled, depth_draw_never, shadows_disabled;
uniform vec4 focus_color : source_color = vec4(0.40, 0.90, 0.94, 0.9);
void fragment() {
    vec2 p = UV * 2.0 - 1.0;
    float r = length(p);
    float aa = max(fwidth(r), 0.006);
    float ring = (1.0 - smoothstep(0.018, 0.018 + aa, abs(r - 0.77)));
    // Four short sight marks reinforce targeting without an animated spinner.
    float cardinal = 1.0 - smoothstep(0.022, 0.022 + aa, min(abs(p.x), abs(p.y)));
    cardinal *= smoothstep(0.80, 0.84, r) * (1.0 - smoothstep(0.94, 0.98, r));
    ALBEDO = focus_color.rgb;
    ALPHA = max(ring, cardinal) * focus_color.a;
}
