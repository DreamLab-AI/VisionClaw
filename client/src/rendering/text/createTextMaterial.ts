import * as THREE from 'three';

/** Live glyph uniforms: write `.value`, never replace the object. */
export interface TextUniforms {
  uAtlas: { value: THREE.Texture };
  uCamRight: { value: THREE.Vector3 };
  uCamUp: { value: THREE.Vector3 };
}

/**
 * The glyph material (ADR-2186 D4): instanced billboard quads sampling the
 * glyph atlas, in TSL, which WebGPURenderer compiles for either backend.
 * Instance attributes: aLabelPos (world position + Y offset), aLocalOffset,
 * aScale, aUVRect (atlas u, v, w, h), aColor, aOpacity. Async because
 * three/webgpu and three/tsl load on demand. Not cached: a node material binds
 * to its geometry's attribute objects.
 */
export async function createTslTextMaterial(
  uniforms: TextUniforms,
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
export function createTextUniforms(atlas: THREE.Texture): TextUniforms {
  return {
    uAtlas: { value: atlas },
    uCamRight: { value: new THREE.Vector3(1, 0, 0) },
    uCamUp: { value: new THREE.Vector3(0, 1, 0) },
  };
}
