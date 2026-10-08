import { describe, it, expect } from 'vitest';
import { createCloudSprites } from '../cloudSprites';

const positions = new Float32Array([0, 0, 0, 1, 2, 3, -4, 5, 6]);
const colours = new Float32Array([1, 0, 0, 0, 1, 0, 0, 0, 1]);

describe('createCloudSprites (WebGPU path)', () => {
  it('draws every row as one instanced, size-attenuated disc sprite', async () => {
    const s = await createCloudSprites(positions, colours, 3);
    const obj = s.object as unknown as { isSprite: boolean; count: number; frustumCulled: boolean };
    expect(obj.isSprite).toBe(true);
    expect(obj.count).toBe(3);
    expect(obj.frustumCulled).toBe(false); // one quad's bounds say nothing about 30k instances
    const m = s.material as unknown as Record<string, unknown>;
    expect(m.isPointsNodeMaterial).toBe(true);
    expect(m.sizeAttenuation).toBe(true);
    expect(m.transparent).toBe(true);
    expect(m.depthWrite).toBe(false);
    expect(m.positionNode).toBeTruthy();
    expect(m.colorNode).toBeTruthy();
    expect(m.opacityNode).toBeTruthy(); // the disc's soft edge, not a texture sampled at a point UV
    s.dispose();
  });

  it('shares the colour buffer, so focus dimming written there reaches the sprites', async () => {
    const s = await createCloudSprites(positions, colours, 3);
    expect(s.colourArray).toBe(colours);
    const v = s.colourVersion();
    s.markColoursDirty();
    expect(s.colourVersion()).toBeGreaterThan(v);
    s.dispose();
  });

  it('follows the draw range, size and opacity', async () => {
    const s = await createCloudSprites(positions, colours, 3);
    s.setCount(2);
    expect((s.object as unknown as { count: number }).count).toBe(2);
    s.setCount(99);
    expect((s.object as unknown as { count: number }).count).toBe(3);
    s.setSize(12.5);
    s.setOpacity(0.4);
    expect((s.material as unknown as { size: number }).size).toBe(12.5);
    expect((s.material as unknown as { opacity: number }).opacity).toBe(0.4);
    s.dispose();
  });

  it('never raycasts: hover picking stays on the hidden points object', async () => {
    const s = await createCloudSprites(positions, colours, 3);
    const hits: unknown[] = [];
    (s.object as unknown as { raycast: (r: unknown, h: unknown[]) => void }).raycast({}, hits);
    expect(hits).toEqual([]);
    s.dispose();
  });
});
