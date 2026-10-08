import { describe, it, expect } from 'vitest';
import { cloudPlacement, discSpritePixels, createRouteFramer, cloudPointSize, DEFAULT_CLOUD_SCALE, graphBoundsFor } from '../cloudFrame';
import { separatedFrame, memoryCentre, place, Vertex, LIVE_GRAPH_RADIUS, MEMORY_BODY_SCALE } from '../../../graph/triLayout';
import { robustBounds } from '@/utils/robustBounds';

const cloud = { centre: [-10, 20, -15] as [number, number, number], radius: 120 };
const graph = { centre: [90, -3, -14] as [number, number, number], radius: 300 };

const dist = (a: readonly number[], b: readonly number[]) => Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);

describe('cloudPlacement', () => {
  it('sits the cloud behind the graphs on the memory ray, its dense core at the pivot', () => {
    const p = cloudPlacement(cloud, graph, DEFAULT_CLOUD_SCALE);
    const m = memoryCentre(separatedFrame(), graph.radius, p.scale * cloud.radius);
    p.position.forEach((x, k) => expect(x).toBeCloseTo(graph.centre[k] + m[k], 4));
    expect(p.position[2]).toBeLessThan(graph.centre[2] + separatedFrame().vertices[Vertex.Memory][2]);
    expect(p.offset).toEqual([10, -20, 15]);
  });

  it('at the default scale the cloud is MEMORY_BODY_SCALE graphs wide', () => {
    const p = cloudPlacement(cloud, graph, DEFAULT_CLOUD_SCALE);
    expect(MEMORY_BODY_SCALE).toBe(10);
    expect(p.scale * cloud.radius).toBeCloseTo(MEMORY_BODY_SCALE * graph.radius);
  });

  it('cloudScale stays a linear multiplier relative to that fit', () => {
    const p = cloudPlacement(cloud, graph, DEFAULT_CLOUD_SCALE * 2);
    expect(p.scale * cloud.radius).toBeCloseTo(2 * MEMORY_BODY_SCALE * graph.radius);
  });

  it('never overlaps either graph, whatever the graph size', () => {
    const f = separatedFrame();
    for (const r of [20, 93, 152, 300]) {
      const g = { centre: [0, 0, 0] as [number, number, number], radius: r };
      for (const k of [DEFAULT_CLOUD_SCALE, DEFAULT_CLOUD_SCALE * 2]) {
        const p = cloudPlacement(cloud, g, k);
        const rc = p.scale * cloud.radius;
        for (const v of [Vertex.Knowledge, Vertex.Ontology]) {
          expect(dist(p.position, f.vertices[v]), `r=${r} k=${k}`).toBeGreaterThanOrEqual(rc + r);
        }
      }
    }
  });

  it('without a graph keeps the cloudScale size rule and clears a live-sized graph', () => {
    const p = cloudPlacement(cloud, null, 5);
    expect(p.scale).toBe(5 * MEMORY_BODY_SCALE);
    const m = memoryCentre(separatedFrame(), LIVE_GRAPH_RADIUS, p.scale * cloud.radius);
    p.position.forEach((x, k) => expect(x).toBeCloseTo(m[k], 4));
    expect(p.offset).toEqual([10, -20, 15]);
    // no cloud yet: the memory vertex itself
    const bare = cloudPlacement(null, null, 5);
    bare.position.forEach((x, k) => expect(x).toBeCloseTo(separatedFrame().vertices[Vertex.Memory][k], 4));
  });

  it('guards a bad scale setting', () => {
    expect(cloudPlacement(cloud, graph, 0).scale).toBeGreaterThan(0);
    expect(cloudPlacement(cloud, graph, NaN).scale).toBeCloseTo((MEMORY_BODY_SCALE * graph.radius) / cloud.radius);
  });
});

describe('graphBoundsFor', () => {
  // a 200-node blob of radius ~150 around the origin
  const blob: [number, number, number][] = [];
  let s = 1;
  const rnd = () => ((s = (s * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff) * 2 - 1;
  for (let i = 0; i < 200; i++) blob.push([rnd() * 150, rnd() * 150, rnd() * 150]);

  it('measures one graph, not the whole triangle', () => {
    const f = separatedFrame();
    const flat: number[] = [];
    for (const v of [Vertex.Knowledge, Vertex.Ontology]) for (const p of blob) flat.push(...place(f, v, p));
    const local = robustBounds(blob.flat(), blob.length)!;
    const folded = graphBoundsFor(flat, flat.length / 3)!;
    const raw = robustBounds(flat, flat.length / 3)!;
    expect(folded.radius).toBeCloseTo(local.radius, 0);
    expect(raw.radius).toBeGreaterThan(1.5 * local.radius);
  });
});

describe('discSpritePixels', () => {
  it('is a white disc: opaque centre, transparent corners, so vertex colours pass through', () => {
    const size = 32;
    const px = discSpritePixels(size);
    expect(px.length).toBe(size * size * 4);
    const at = (x: number, y: number) => px.subarray((y * size + x) * 4, (y * size + x) * 4 + 4);
    expect(Array.from(at(16, 16))).toEqual([255, 255, 255, 255]);
    expect(at(0, 0)[3]).toBe(0);
    expect(at(31, 31)[3]).toBe(0);
    expect(at(0, 16)[3]).toBeLessThan(64);
    for (let i = 0; i < px.length; i += 4) expect(px[i]).toBe(255);
  });

  it('is round: alpha depends only on the distance from the centre', () => {
    const size = 32;
    const px = discSpritePixels(size);
    const a = (x: number, y: number) => px[(y * size + x) * 4 + 3];
    expect(a(16, 4)).toBe(a(4, 16));
    // centre is 15.5: (16,4) mirrors to (15,27) and transposes to (27,15)
    expect(a(16, 4)).toBe(a(15, 27));
    expect(a(16, 4)).toBe(a(27, 15));
    // a square sprite would be opaque at the corner of the inscribed square
    expect(a(4, 4)).toBe(0);
  });
});

describe('createRouteFramer', () => {
  it('frames once per query, only after the new route has been published', () => {
    const f = createRouteFramer();
    // query 1 completes while the channel still holds the previous route (seq 4)
    expect(f.update({ querySeq: 1, morphSeq: 0, hasRun: true, routeSeq: 4, routeLength: 6 })).toBe(false);
    // TrajectoryLayer publishes the new route
    expect(f.update({ querySeq: 1, morphSeq: 0, hasRun: true, routeSeq: 5, routeLength: 6 })).toBe(true);
    // later frames do not reframe, so the user can orbit freely
    expect(f.update({ querySeq: 1, morphSeq: 0, hasRun: true, routeSeq: 5, routeLength: 6 })).toBe(false);
    expect(f.update({ querySeq: 1, morphSeq: 0, hasRun: true, routeSeq: 6, routeLength: 6 })).toBe(false);
  });

  it('reframes after a view change once the morphed route settles', () => {
    const f = createRouteFramer();
    f.update({ querySeq: 1, morphSeq: 0, hasRun: true, routeSeq: 0, routeLength: 6 });
    expect(f.update({ querySeq: 1, morphSeq: 0, hasRun: true, routeSeq: 1, routeLength: 6 })).toBe(true);
    expect(f.update({ querySeq: 1, morphSeq: 1, hasRun: true, routeSeq: 1, routeLength: 6 })).toBe(false);
    expect(f.update({ querySeq: 1, morphSeq: 1, hasRun: true, routeSeq: 2, routeLength: 6 })).toBe(true);
  });

  it('frames when the route is published in the same frame the query lands (layer runs before the rig)', () => {
    const f = createRouteFramer();
    // idle frames: channel at seq 7, no run
    expect(f.update({ querySeq: 0, morphSeq: 0, hasRun: false, routeSeq: 7, routeLength: 0 })).toBe(false);
    // the query lands and TrajectoryLayer has already published (seq 8) before the rig looks
    expect(f.update({ querySeq: 1, morphSeq: 0, hasRun: true, routeSeq: 8, routeLength: 6 })).toBe(true);
    expect(f.update({ querySeq: 1, morphSeq: 0, hasRun: true, routeSeq: 8, routeLength: 6 })).toBe(false);
  });

  it('never frames without a run or with a route under two points', () => {
    const f = createRouteFramer();
    expect(f.update({ querySeq: 1, morphSeq: 0, hasRun: false, routeSeq: 0, routeLength: 0 })).toBe(false);
    expect(f.update({ querySeq: 1, morphSeq: 0, hasRun: false, routeSeq: 1, routeLength: 0 })).toBe(false);
    const g = createRouteFramer();
    g.update({ querySeq: 2, morphSeq: 0, hasRun: true, routeSeq: 0, routeLength: 1 });
    expect(g.update({ querySeq: 2, morphSeq: 0, hasRun: true, routeSeq: 1, routeLength: 1 })).toBe(false);
  });
});

describe('cloudPointSize', () => {
  it('keeps the pointSize setting at its legacy look: points scale with the cloud', () => {
    // the old fixed placement was ×5 (the default cloudScale)
    expect(cloudPointSize(7.5, DEFAULT_CLOUD_SCALE)).toBeCloseTo(7.5);
    expect(cloudPointSize(7.5, DEFAULT_CLOUD_SCALE / 5)).toBeCloseTo(1.5);
    expect(cloudPointSize(15, 2)).toBeCloseTo(2 * cloudPointSize(7.5, 2));
  });
  it('never collapses a point below a visible floor', () => {
    expect(cloudPointSize(7.5, 0.001)).toBeGreaterThanOrEqual(0.5);
  });
});
