import { describe, it, expect } from 'vitest';
import * as THREE from 'three';
import { createTextUniforms, createTslTextMaterial } from '../createTextMaterial';
import { overviewFaultNoticeText } from '../../../features/graph/overview/overviewLive';

function glyphGeometry(cap = 8): THREE.InstancedBufferGeometry {
  const g = new THREE.InstancedBufferGeometry();
  g.setIndex([0, 1, 2, 0, 2, 3]);
  g.setAttribute('position', new THREE.BufferAttribute(new Float32Array([0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 0]), 3));
  const sizes: Record<string, number> = { aLabelPos: 3, aLocalOffset: 2, aScale: 2, aUVRect: 4, aColor: 3, aOpacity: 1 };
  for (const [k, n] of Object.entries(sizes)) g.setAttribute(k, new THREE.InstancedBufferAttribute(new Float32Array(cap * n), n));
  return g;
}

describe('glyph labels (ADR-2186 D4)', () => {
  it('the TSL glyph material builds over the shared glyph layout', async () => {
    const tex = new THREE.Texture();
    const u = createTextUniforms(tex);
    const m = await createTslTextMaterial(u, glyphGeometry()) as THREE.Material & { isNodeMaterial?: boolean; vertexNode?: unknown; colorNode?: unknown };
    expect(m.isNodeMaterial).toBe(true);
    expect(m.vertexNode).toBeTruthy();
    expect(m.colorNode).toBeTruthy();
    expect(m.transparent).toBe(true);
    expect(m.depthWrite).toBe(false);
    m.dispose();
  });

  it('fresh uniforms per glyph mesh (the TSL material binds per geometry)', () => {
    const tex = new THREE.Texture();
    expect(createTextUniforms(tex)).not.toBe(createTextUniforms(tex));
  });

  it('a glyph fault is reported on the canvas notice, without the gem fallback clause', () => {
    expect(overviewFaultNoticeText({ labels: 'link failed' })).toBe('Overview labels failed on this GPU');
    expect(overviewFaultNoticeText({ labels: 'x', edges: 'y' })).toContain('drawing gems and glass tubes instead');
  });
});
