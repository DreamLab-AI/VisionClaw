import { describe, it, expect } from 'vitest';
import { sceneFitBounds, createSettleTrigger, SEPARATION_SETTLE_MS } from '../sceneFitBounds';
import { robustBounds } from '../../../../utils/robustBounds';
import { triangleFrame, place, Vertex, type Vec3 } from '../../triLayout';
import { cloudPlacement } from '../../../visualisation/memoryCloud/cloudFrame';

// one graph body: 300 points, radius ~120 around a local centre
let s = 7;
const rnd = () => ((s = (s * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff) * 2 - 1;
const blob: Vec3[] = Array.from({ length: 300 }, () => [rnd() * 120, rnd() * 120, rnd() * 120] as Vec3);

function projected(sep: number): Float32Array {
  const f = triangleFrame(sep);
  const out: number[] = [];
  for (const v of [Vertex.Knowledge, Vertex.Ontology]) for (const p of blob) out.push(...place(f, v, p));
  return new Float32Array(out);
}

const inside = (b: { centre: [number, number, number]; radius: number }, c: Vec3, r: number) =>
  Math.hypot(c[0] - b.centre[0], c[1] - b.centre[1], c[2] - b.centre[2]) + r <= b.radius + 1e-3;

const cloud = { bounds: { centre: [5, -3, 2] as [number, number, number], radius: 80 }, cloudScale: 5 };

describe('sceneFitBounds', () => {
  it('separation 0 is exactly the old graph fit', () => {
    const pos = projected(0);
    expect(sceneFitBounds(pos, pos.length / 3, 0, cloud)).toEqual(robustBounds(pos, pos.length / 3));
  });

  it('separated: frames both graphs and the cloud at its vertex', () => {
    const sep = 400;
    const pos = projected(sep);
    const fit = sceneFitBounds(pos, pos.length / 3, sep, cloud)!;
    const f = triangleFrame(sep);
    const local = robustBounds(blob.flat(), blob.length)!;
    for (const v of [Vertex.Knowledge, Vertex.Ontology]) {
      expect(inside(fit, place(f, v, local.centre), local.radius), `graph ${v}`).toBe(true);
    }
    // the cloud: where cloudFrame puts it, at its placed radius
    const p = cloudPlacement(cloud.bounds, local, cloud.cloudScale, sep);
    expect(inside(fit, p.position, p.scale * cloud.bounds.radius), 'cloud').toBe(true);
    // the old fit (graphs only) leaves the cloud out — the defect
    const old = robustBounds(pos, pos.length / 3)!;
    expect(inside(old, p.position, p.scale * cloud.bounds.radius)).toBe(false);
  });

  it('is tight: no bigger than the spheres need', () => {
    const sep = 300;
    const pos = projected(sep);
    const fit = sceneFitBounds(pos, pos.length / 3, sep, cloud)!;
    // circumradius + one body radius bounds any enclosing sphere about the centroid
    expect(fit.radius).toBeLessThan(triangleFrame(sep).radius + 2 * 220);
  });

  it('without the cloud frames only the two graphs', () => {
    const sep = 400;
    const pos = projected(sep);
    const withCloud = sceneFitBounds(pos, pos.length / 3, sep, cloud)!;
    const without = sceneFitBounds(pos, pos.length / 3, sep, null)!;
    expect(without.radius).toBeLessThan(withCloud.radius);
    expect(without.centre[2]).toBeGreaterThan(withCloud.centre[2]); // the cloud is behind (−Z)
  });

  it('no positions: null', () => {
    expect(sceneFitBounds(new Float32Array(0), 0, 200, cloud)).toBeNull();
  });
});

describe('createSettleTrigger', () => {
  it('fires once after the value stops changing, never on every tick', () => {
    const t = createSettleTrigger(SEPARATION_SETTLE_MS);
    expect(t.update(0, 0)).toBe(false); // initial value is the baseline, not a change
    let fired = 0;
    // a drag: a new value every 50 ms for a second
    for (let i = 1; i <= 20; i++) if (t.update(i * 20, i * 50)) fired++;
    expect(fired).toBe(0);
    expect(t.update(400, 1000 + SEPARATION_SETTLE_MS - 1)).toBe(false);
    expect(t.update(400, 1000 + SEPARATION_SETTLE_MS)).toBe(true);
    expect(t.update(400, 5000)).toBe(false); // once per settle
    expect(t.update(0, 6000)).toBe(false);
    expect(t.update(0, 6000 + SEPARATION_SETTLE_MS)).toBe(true);
  });
});
