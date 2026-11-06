import * as THREE from 'three';

export interface TextMaterialResult {
  material: THREE.ShaderMaterial;
  uniforms: {
    uAtlas: { value: THREE.Texture };
    uCamRight: { value: THREE.Vector3 };
    uCamUp: { value: THREE.Vector3 };
  };
}

const vertexShader = /* glsl */ `
  // Per-instance attributes
  attribute vec3 aLabelPos;    // node world position + Y offset
  attribute vec2 aLocalOffset; // glyph offset within label
  attribute vec2 aScale;       // glyph quad dimensions
  attribute vec4 aUVRect;      // atlas UV rectangle (u, v, w, h)
  attribute vec3 aColor;       // per-glyph color
  attribute float aOpacity;    // distance fade opacity

  // Camera basis vectors for billboarding
  uniform vec3 uCamRight;
  uniform vec3 uCamUp;

  varying vec2 vUV;
  varying vec3 vColor;
  varying float vOpacity;

  void main() {
    // Compute UV from base quad position (0..1 range)
    vUV = vec2(
      aUVRect.x + position.x * aUVRect.z,
      aUVRect.y + (1.0 - position.y) * aUVRect.w
    );
    vColor = aColor;
    vOpacity = aOpacity;

    // Billboard: orient quad to face camera using camera basis vectors
    vec3 localOffset = uCamRight * (aLocalOffset.x + position.x * aScale.x)
                     + uCamUp * (aLocalOffset.y + position.y * aScale.y);

    vec3 worldPos = aLabelPos + localOffset;

    gl_Position = projectionMatrix * viewMatrix * vec4(worldPos, 1.0);
  }
`;

const fragmentShader = /* glsl */ `
  uniform sampler2D uAtlas;

  varying vec2 vUV;
  varying vec3 vColor;
  varying float vOpacity;

  void main() {
    vec4 texSample = texture2D(uAtlas, vUV);
    float alpha = texSample.r * vOpacity;
    if (alpha < 0.01) discard;
    gl_FragColor = vec4(vColor * texSample.r, alpha);
  }
`;

let _cached: TextMaterialResult | null = null;

export function createTextMaterial(atlas: THREE.Texture): TextMaterialResult {
  if (_cached) return _cached;

  const uniforms = {
    uAtlas: { value: atlas },
    uCamRight: { value: new THREE.Vector3(1, 0, 0) },
    uCamUp: { value: new THREE.Vector3(0, 1, 0) },
  };

  const material = new THREE.ShaderMaterial({
    uniforms,
    vertexShader,
    fragmentShader,
    transparent: true,
    depthWrite: false,
    depthTest: true,
    side: THREE.DoubleSide,
    blending: THREE.NormalBlending,
  });

  _cached = { material, uniforms };
  return _cached;
}

/**
 * WebGPU dialect (TSL) of the glyph material (ADR-2186 D4): the same
 * instanced-quad layout, billboard maths and atlas sampling as the GLSL above,
 * reading the same uniform objects, so InstancedLabels draws one glyph path on
 * both renderers. Async because three/webgpu and three/tsl load on demand.
 * Not cached: a node material binds to its geometry's attribute objects.
 */
export async function createTslTextMaterial(
  uniforms: TextMaterialResult['uniforms'],
  geometry: THREE.InstancedBufferGeometry,
): Promise<THREE.Material> {
  const W = await import('three/webgpu');
  // eslint-disable-next-line @typescript-eslint/no-explicit-any -- TSL node builder types have no stable public typing
  const T = (await import('three/tsl')) as any;
  const {
    Fn, float, vec2, vec3, vec4, uniform, instancedBufferAttribute, positionGeometry,
    cameraViewMatrix, cameraProjectionMatrix, varyingProperty, Discard, If,
  } = T;
  const atlas = T.texture(uniforms.uAtlas.value);
  const camRight = uniform(new THREE.Vector3(1, 0, 0)).onRenderUpdate(() => uniforms.uCamRight.value);
  const camUp = uniform(new THREE.Vector3(0, 1, 0)).onRenderUpdate(() => uniforms.uCamUp.value);
  const attr = (name: string) => instancedBufferAttribute(geometry.getAttribute(name));
  const aLabelPos = attr('aLabelPos');
  const aLocalOffset = attr('aLocalOffset');
  const aScale = attr('aScale');
  const aUVRect = attr('aUVRect');
  const aColor = attr('aColor');
  const aOpacity = attr('aOpacity');
  const vUV = varyingProperty('vec2', 'vGlyphUV');
  const vColor = varyingProperty('vec3', 'vGlyphColor');
  const vOpacity = varyingProperty('float', 'vGlyphOpacity');

  const vertex = Fn(() => {
    const p = positionGeometry;
    vUV.assign(vec2(aUVRect.x.add(p.x.mul(aUVRect.z)), aUVRect.y.add(float(1).sub(p.y).mul(aUVRect.w))));
    vColor.assign(vec3(aColor));
    vOpacity.assign(aOpacity);
    const local = vec3(camRight).mul(aLocalOffset.x.add(p.x.mul(aScale.x)))
      .add(vec3(camUp).mul(aLocalOffset.y.add(p.y.mul(aScale.y))));
    return cameraProjectionMatrix.mul(cameraViewMatrix).mul(vec4(vec3(aLabelPos).add(local), 1));
  })();

  const fragment = Fn(() => {
    const s = atlas.sample(vUV).r;
    const alpha = s.mul(vOpacity);
    If(alpha.lessThan(0.01), () => { Discard(); });
    return vec4(vColor.mul(s), alpha);
  })();

  const material = new W.MeshBasicNodeMaterial({
    transparent: true,
    depthWrite: false,
    depthTest: true,
    side: THREE.DoubleSide,
    blending: THREE.NormalBlending,
  });
  material.name = 'GlyphNodeMaterial';
  (material as unknown as { vertexNode: unknown }).vertexNode = vertex;
  (material as unknown as { colorNode: unknown }).colorNode = fragment;
  return material as unknown as THREE.Material;
}

/** Fresh uniform objects for one glyph mesh (the TSL path is per geometry). */
export function createTextUniforms(atlas: THREE.Texture): TextMaterialResult['uniforms'] {
  return {
    uAtlas: { value: atlas },
    uCamRight: { value: new THREE.Vector3(1, 0, 0) },
    uCamUp: { value: new THREE.Vector3(0, 1, 0) },
  };
}
