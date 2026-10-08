import { describe, it, expect } from 'vitest';
import { sceneFitBounds, createSettleTrigger, cloudFitKey, CLOUD_SETTLE_MS } from '../sceneFitBounds';
import { robustBounds } from '../../../../utils/robustBounds';
import { separatedFrame, place, Vertex, type Vec3 } from '../../triLayout';
import { cloudPlacement } from '../../../visualisation/memoryCloud/cloudFrame';

// one graph body: 300 points, radius ~120 around a local centre
let s = 7;
const rnd = () => ((s = (s * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff) * 2 - 1;
const blob: Vec3[] = Array.from({ length: 300 }, () => [rnd() * 120, rnd() * 120, rnd() * 120] as Vec3);

function projected(): Float32Array {
  const f = separatedFrame();
  const out: number[] = [];
  for (const v of [Vertex.Knowledge, Vertex.Ontology]) for (const p of blob) out.push(...place(f, v, p));
  return new Float32Array(out);
}

const inside = (b: { centre: [number, number, number]; radius: number }, c: Vec3, r: number) =>
  Math.hypot(c[0] - b.centre[0], c[1] - b.centre[1], c[2] - b.centre[2]) + r <= b.radius + 1e-3;

const cloud = { bounds: { centre: [5, -3, 2] as [number, number, number], radius: 80 }, cloudScale: 5 };

describe('sceneFitBounds', () => {
  it('frames both graphs and the ×10 cloud behind them', () => {
    const pos = projected();
    const fit = sceneFitBounds(pos, pos.length / 3, cloud)!;
    const f = separatedFrame();
    const local = robustBounds(blob.flat(), blob.length)!;
    for (const v of [Vertex.Knowledge, Vertex.Ontology]) {
      expect(inside(fit, place(f, v, local.centre), local.radius), `graph ${v}`).toBe(true);
    }
    // the cloud: where cloudFrame puts it, at its placed radius
    const p = cloudPlacement(cloud.bounds, local, cloud.cloudScale);
    expect(inside(fit, p.position, p.scale * cloud.bounds.radius), 'cloud').toBe(true);
    // a graphs-only fit would leave the cloud out
    const old = robustBounds(pos, pos.length / 3)!;
    expect(inside(old, p.position, p.scale * cloud.bounds.radius)).toBe(false);
  });

  it('is tight: no bigger than the spheres need', () => {
    const pos = projected();
    const fit = sceneFitBounds(pos, pos.length / 3, cloud)!;
    const local = robustBounds(blob.flat(), blob.length)!;
    const p = cloudPlacement(cloud.bounds, local, cloud.cloudScale);
    const rc = p.scale * cloud.bounds.radius;
    // the farthest graph point and the cloud's far side bound the sphere
    const span = Math.hypot(...p.position) + rc + separatedFrame().radius + local.radius;
    expect(fit.radius).toBeLessThanOrEqual(span / 2 + 1);
  });

  it('without the cloud frames only the two graphs', () => {
    const pos = projected();
    const withCloud = sceneFitBounds(pos, pos.length / 3, cloud)!;
    const without = sceneFitBounds(pos, pos.length / 3, null)!;
    expect(without.radius).toBeLessThan(withCloud.radius);
    expect(without.centre[2]).toBeGreaterThan(withCloud.centre[2]); // the cloud is behind (−Z)
  });

  it('no positions: null', () => {
    expect(sceneFitBounds(new Float32Array(0), 0, cloud)).toBeNull();
  });
});

describe('cloudFitKey', () => {
  it('is empty while the cloud is off or unloaded, and names snapshot and scale otherwise', () => {
    expect(cloudFitKey(false, 'abc', 5)).toBe('');
    expect(cloudFitKey(true, null, 5)).toBe('');
    expect(cloudFitKey(true, 'abc', 5)).toBe('abc|5');
    expect(cloudFitKey(true, 'abc', 6)).not.toBe(cloudFitKey(true, 'abc', 5));
  });
});

describe('createSettleTrigger', () => {
  it('fires once after the value stops changing, never on every tick', () => {
    const t = createSettleTrigger(CLOUD_SETTLE_MS);
    expect(t.update(0, 0)).toBe(false); // initial value is the baseline, not a change
    let fired = 0;
    for (let i = 1; i <= 20; i++) if (t.update(i * 20, i * 50)) fired++;
    expect(fired).toBe(0);
    expect(t.update(400, 1000 + CLOUD_SETTLE_MS - 1)).toBe(false);
    expect(t.update(400, 1000 + CLOUD_SETTLE_MS)).toBe(true);
    expect(t.update(400, 5000)).toBe(false); // once per settle
    expect(t.update(0, 6000)).toBe(false);
    expect(t.update(0, 6000 + CLOUD_SETTLE_MS)).toBe(true);
  });

  it('a cloud snapshot arriving after the first fit refits once', () => {
    const t = createSettleTrigger(CLOUD_SETTLE_MS);
    expect(t.update(cloudFitKey(true, null, 5), 0)).toBe(false);
    expect(t.update(cloudFitKey(true, 'snap', 5), 100)).toBe(false);
    expect(t.update(cloudFitKey(true, 'snap', 5), 100 + CLOUD_SETTLE_MS)).toBe(true);
    expect(t.update(cloudFitKey(true, 'snap', 5), 5000)).toBe(false);
  });
});
