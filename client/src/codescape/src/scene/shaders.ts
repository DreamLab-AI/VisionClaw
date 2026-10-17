/** GLSL for impostor nodes (shared by the colour pass and the id-buffer pick pass). */

export const NODE_VERTEX = /* glsl */ `
attribute vec3 iPos;
attribute vec3 iColor;
attribute vec4 iParams; // size, heat, alpha, flags
uniform float uScale;
uniform float uMinPx;
uniform float uViewportH;
uniform float uProjScale;
varying vec2 vUv;
varying vec3 vColor;
varying vec4 vParams;
varying float vRing;
void main() {
  float size = iParams.x * uScale;
  float flags = iParams.w;
  bool selected = mod(flags, 2.0) >= 1.0;
  bool hover = mod(floor(flags / 2.0), 2.0) >= 1.0;
  bool highlight = mod(floor(flags / 8.0), 2.0) >= 1.0;
  vRing = (selected || hover || highlight) ? 1.0 : 0.0;
  vec4 mv = modelViewMatrix * vec4(iPos, 1.0);
  // Keep tiny nodes visible: at least uMinPx on screen once their weight is non-zero.
  float px = size * uProjScale / max(1e-3, -mv.z);
  if (iParams.x > 0.0 && px < uMinPx) size *= uMinPx / max(px, 1e-4);
  float quad = size * (vRing > 0.5 ? 1.35 : 1.0) * (iParams.x > 0.0 ? 1.0 : 0.0);
  mv.xy += position.xy * quad;
  gl_Position = projectionMatrix * mv;
  vUv = position.xy * 2.0 * (vRing > 0.5 ? 1.35 : 1.0);
  vColor = iColor;
  vParams = iParams;
}
`;

export const NODE_FRAGMENT = /* glsl */ `
uniform vec3 uBg;
uniform float uTime;
varying vec2 vUv;
varying vec3 vColor;
varying vec4 vParams;
varying float vRing;
void main() {
  float r2 = dot(vUv, vUv);
  float heat = vParams.y;
  float alpha = vParams.z;
  float flags = vParams.w;
  bool selected = mod(flags, 2.0) >= 1.0;
  bool desat = mod(floor(flags / 4.0), 2.0) >= 1.0;
  bool highlight = mod(floor(flags / 8.0), 2.0) >= 1.0;
  if (r2 > 1.0) {
    // Selection / hover / highlight ring outside the sphere.
    float r = sqrt(r2);
    if (vRing < 0.5 || r > 1.32) discard;
    float band = smoothstep(1.12, 1.18, r) * (1.0 - smoothstep(1.26, 1.32, r));
    if (band < 0.05) discard;
    vec3 ringCol = selected ? vec3(1.0, 0.92, 0.55) : highlight ? vec3(0.55, 0.95, 1.0) : vec3(0.85);
    gl_FragColor = vec4(ringCol, 1.0);
    return;
  }
  vec3 n = vec3(vUv, sqrt(1.0 - r2));
  vec3 l = normalize(vec3(-0.45, 0.6, 0.65));
  float diff = max(dot(n, l), 0.0);
  float spec = pow(max(dot(reflect(-l, n), vec3(0.0, 0.0, 1.0)), 0.0), 28.0);
  float rim = pow(1.0 - n.z, 2.5);
  vec3 base = vColor;
  if (desat) {
    float lum = dot(base, vec3(0.299, 0.587, 0.114));
    base = mix(base, vec3(lum), 0.85);
  }
  vec3 col = base * (0.28 + 0.78 * diff) + vec3(spec) * 0.35 + base * rim * 0.35;
  // Heat: emissive glow pulsing toward warm white.
  float pulse = 0.75 + 0.25 * sin(uTime * 5.0);
  col += heat * pulse * vec3(1.0, 0.62, 0.25) * (0.6 + 0.8 * rim + 0.4 * (1.0 - r2));
  // Focus+context: composite against the background (opaque, no sorting needed).
  col = mix(uBg, col, alpha);
  gl_FragColor = vec4(col, 1.0);
}
`;

export const PICK_FRAGMENT = /* glsl */ `
varying vec2 vUv;
varying vec4 vParams;
varying float vRing;
varying float vPickId;
void main() {
  if (dot(vUv, vUv) > 1.0) discard;
  float id = vPickId;
  float r = mod(id, 256.0);
  float g = mod(floor(id / 256.0), 256.0);
  float b = floor(id / 65536.0);
  gl_FragColor = vec4(r / 255.0, g / 255.0, b / 255.0, 1.0);
}
`;

/** The pick pass reuses the node vertex shader and adds the instance id. */
export const PICK_VERTEX = NODE_VERTEX.replace('varying float vRing;', 'varying float vRing;\nvarying float vPickId;').replace(
  'vParams = iParams;',
  'vParams = iParams;\n  vPickId = float(gl_InstanceID + 1);',
);
