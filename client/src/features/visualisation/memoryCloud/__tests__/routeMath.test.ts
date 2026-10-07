import { describe, it, expect } from 'vitest';
import {
  ease,
  clamp01,
  quadPoint,
  sampleRoute,
  resamplePolyline,
  pointAt,
  routeGradient,
  cometTail,
  beadScale,
  revealPhases,
  morphFade,
  shownCount,
  nodeGrowth,
  REVEAL_DUR,
  PATH_DUR,
  hexToRgb01,
  ROUTE_PALETTE,
  thinRejected,
  beatModulation,
} from '../routeMath';
import type { LayoutResult, Vec3 } from '../../memoryTrajectory/types';

const close = (a: number, b: number, eps = 1e-6) => expect(Math.abs(a - b)).toBeLessThan(eps);
const closeV = (a: Vec3, b: Vec3, eps = 1e-6) => a.forEach((v, i) => close(v, b[i], eps));

function layout(pos: Array<[number, Vec3]>, ctrl: Array<[number, Vec3]> = [], geo?: Array<[number, Vec3[]]>): LayoutResult {
  return {
    positions: new Map(pos),
    controls: new Map(ctrl),
    geodesics: geo ? new Map(geo) : undefined,
  };
}

describe('ease', () => {
  it('io is symmetric with fixed endpoints and ioInv inverts it', () => {
    close(ease.io(0), 0);
    close(ease.io(1), 1);
    close(ease.io(0.5), 0.5);
    for (const y of [0.05, 0.2, 0.5, 0.73, 0.99]) close(ease.io(ease.ioInv(y)), y, 1e-9);
  });
  it('out is a cubic ease-out', () => {
    close(ease.out(0), 0);
    close(ease.out(1), 1);
    expect(ease.out(0.5)).toBeGreaterThan(0.5);
  });
  it('back overshoots above 1 before settling at 1', () => {
    close(ease.back(0), 0, 1e-9);
    close(ease.back(1), 1);
    const peak = Math.max(...Array.from({ length: 99 }, (_, i) => ease.back((i + 1) / 100)));
    expect(peak).toBeGreaterThan(1.05);
  });
  it('clamp01 clamps', () => {
    expect(clamp01(-1)).toBe(0);
    expect(clamp01(2)).toBe(1);
    expect(clamp01(0.3)).toBe(0.3);
  });
});

describe('quadPoint', () => {
  it('evaluates a quadratic Bézier', () => {
    closeV(quadPoint([0, 0, 0], [1, 2, 0], [2, 0, 0], 0.5), [1, 1, 0]);
    closeV(quadPoint([0, 0, 0], [1, 2, 0], [2, 0, 0], 0), [0, 0, 0]);
    closeV(quadPoint([0, 0, 0], [1, 2, 0], [2, 0, 0], 1), [2, 0, 0]);
  });
});

describe('sampleRoute', () => {
  it('samples straight segments when there is no control point', () => {
    const r = sampleRoute([1, 2], layout([[1, [0, 0, 0]], [2, [16, 0, 0]]]), 16);
    expect(r.pts.length).toBe(17);
    expect(r.knots).toEqual([0, 16]);
    closeV(r.pts[8], [8, 0, 0]);
  });

  it('follows the Bézier control point of each edge (child id keys the control)', () => {
    const r = sampleRoute(
      [1, 2, 3],
      layout(
        [[1, [0, 0, 0]], [2, [2, 0, 0]], [3, [4, 0, 0]]],
        [[2, [1, 2, 0]], [3, [3, -2, 0]]],
      ),
      4,
    );
    expect(r.pts.length).toBe(9);
    expect(r.knots).toEqual([0, 4, 8]);
    closeV(r.pts[2], [1, 1, 0]);
    closeV(r.pts[6], [3, -1, 0]);
  });

  it('uses the hyperbolic geodesic polyline when the layout carries one', () => {
    const geo: Vec3[] = [[0, 0, 0], [1, 1, 0], [2, 0, 0]];
    const r = sampleRoute([1, 2], layout([[1, [0, 0, 0]], [2, [2, 0, 0]]], [], [[2, geo]]), 4);
    expect(r.pts.length).toBe(5);
    closeV(r.pts[0], [0, 0, 0]);
    closeV(r.pts[2], [1, 1, 0]);
    closeV(r.pts[4], [2, 0, 0]);
  });

  it('returns an empty route when a node has no position, or the path is one node', () => {
    expect(sampleRoute([1, 2], layout([[1, [0, 0, 0]]])).pts).toEqual([]);
    expect(sampleRoute([1], layout([[1, [0, 0, 0]]])).pts).toEqual([]);
  });
});

describe('resamplePolyline', () => {
  it('spaces points evenly by arc length', () => {
    const out = resamplePolyline([[0, 0, 0], [1, 0, 0], [4, 0, 0]], 4);
    expect(out.map((p) => p[0])).toEqual([0, 1, 2, 3, 4]);
  });
  it('handles a degenerate polyline', () => {
    const out = resamplePolyline([[1, 1, 1], [1, 1, 1]], 3);
    expect(out.length).toBe(4);
    out.forEach((p) => closeV(p, [1, 1, 1]));
  });
});

describe('pointAt', () => {
  it('interpolates at a fractional sample index and clamps at the ends', () => {
    const pts: Vec3[] = [[0, 0, 0], [2, 0, 0], [2, 2, 0]];
    closeV(pointAt(pts, 0.5), [1, 0, 0]);
    closeV(pointAt(pts, 1.5), [2, 1, 0]);
    closeV(pointAt(pts, 5), [2, 2, 0]);
    closeV(pointAt(pts, -1), [0, 0, 0]);
  });
});

describe('routeGradient', () => {
  it('runs root blue → white core at 0.42 → orange tip', () => {
    closeV(routeGradient(0), hexToRgb01(ROUTE_PALETTE.root));
    closeV(routeGradient(0.42), [1, 1, 1]);
    closeV(routeGradient(1), hexToRgb01(ROUTE_PALETTE.tip));
  });
  it('mixes linearly within each band', () => {
    const r = hexToRgb01(ROUTE_PALETTE.root);
    const mid = routeGradient(0.21);
    closeV(mid, [(r[0] + 1) / 2, (r[1] + 1) / 2, (r[2] + 1) / 2]);
  });
  it('hexToRgb01 parses #rrggbb', () => {
    closeV(hexToRgb01('#ff8000'), [1, 128 / 255, 0]);
  });
  it('the palette is the Explorer mint / blue / orange', () => {
    expect(ROUTE_PALETTE.mint).toBe('#7ef0cf');
    expect(ROUTE_PALETTE.root).toBe('#6f9bff');
    expect(ROUTE_PALETTE.tip).toBe('#ff7a3d');
  });
});

describe('cometTail', () => {
  const pts: Vec3[] = Array.from({ length: 101 }, (_, i) => [i, 0, 0] as Vec3);

  it('draws 18 segments ending at the head with fading alpha and width', () => {
    const segs = cometTail(pts, 80, 18);
    expect(segs.length).toBe(18);
    closeV(segs[0].b, [80, 0, 0]);
    for (let i = 1; i < segs.length; i++) {
      expect(segs[i].alpha).toBeLessThan(segs[i - 1].alpha);
      expect(segs[i].width).toBeLessThan(segs[i - 1].width);
      closeV(segs[i].b, segs[i - 1].a);
    }
    // tail length is 18 % of the route
    close(segs[0].b[0] - segs[17].a[0], 18, 1e-6);
  });

  it('stops at the start of the route', () => {
    const segs = cometTail(pts, 3, 18);
    expect(segs.length).toBeLessThanOrEqual(18);
    segs.forEach((s) => expect(s.a[0]).toBeGreaterThanOrEqual(0));
  });
});

describe('beadScale', () => {
  it('is zero before the head nears the knot, overshoots, then settles at 1', () => {
    expect(beadScale(0, 50)).toBe(0);
    expect(beadScale(39, 50)).toBe(0);
    expect(beadScale(50, 50)).toBeCloseTo(1, 6);
    const vals = Array.from({ length: 11 }, (_, i) => beadScale(40 + i, 50));
    expect(Math.max(...vals)).toBeGreaterThan(1);
  });
});

describe('reveal timeline', () => {
  it('reveals the tree over REVEAL_DUR then traces the path over PATH_DUR', () => {
    expect(revealPhases(0)).toMatchObject({ prog: 0, pathT: 0, done: false });
    const atTree = revealPhases(REVEAL_DUR);
    expect(atTree.prog).toBe(1);
    expect(atTree.pathT).toBe(0);
    const end = revealPhases(REVEAL_DUR + PATH_DUR);
    expect(end.pathT).toBe(1);
    expect(end.done).toBe(true);
  });

  it('jumps straight to the end under reduced motion', () => {
    expect(revealPhases(0, true)).toMatchObject({ prog: 1, pathT: 1, done: true });
  });

  it('shows the root immediately and every node by the end of the reveal', () => {
    expect(shownCount(0, 50)).toBe(1);
    expect(shownCount(1, 50)).toBe(50);
    expect(shownCount(0.5, 50)).toBe(25);
  });

  it('grows node k from the moment the eased reveal reaches it', () => {
    expect(nodeGrowth(0, 10, 0)).toBe(1);
    expect(nodeGrowth(5, 10, 0)).toBe(0);
    const appear = REVEAL_DUR * ease.ioInv(5 / 10);
    expect(nodeGrowth(5, 10, appear)).toBe(0);
    expect(nodeGrowth(5, 10, appear + 10)).toBe(1);
    expect(nodeGrowth(5, 10, appear + 0.1)).toBeGreaterThan(0);
  });
});

describe('thinRejected', () => {
  it('keeps every rejected node under the cap and a deterministic subset above it', () => {
    const nodes = Array.from({ length: 1000 }, (_, i) => ({ kept: i % 4 === 0, hash: (i * 0.6180339887) % 1 }));
    expect(thinRejected(nodes, 10_000).every((k, i) => k === true || nodes[i].kept)).toBe(true);
    const keep = thinRejected(nodes, 150);
    const rejectedShown = keep.filter((k, i) => k && !nodes[i].kept).length;
    expect(rejectedShown).toBeGreaterThan(100);
    expect(rejectedShown).toBeLessThan(200);
    nodes.forEach((n, i) => { if (n.kept) expect(keep[i]).toBe(true); });
    expect(thinRejected(nodes, 150)).toEqual(keep);
  });
});

describe('morphFade', () => {
  const base = { positions: new Map(), controls: new Map() };
  it('is 1 for a settled layout that carries no opacity map', () => {
    expect(morphFade(base, 3)).toBe(1);
  });
  it('reads the morph frame opacity, defaulting missing ids to 1', () => {
    const frame = { ...base, opacity: new Map([[3, 0.25]]) };
    expect(morphFade(frame, 3)).toBe(0.25);
    expect(morphFade(frame, 4)).toBe(1);
  });
  it('clamps to 0..1', () => {
    const frame = { ...base, opacity: new Map([[1, -0.2], [2, 1.4], [3, Number.NaN]]) };
    expect(morphFade(frame, 1)).toBe(0);
    expect(morphFade(frame, 2)).toBe(1);
    expect(morphFade(frame, 3)).toBe(1);
  });
});

describe('beatModulation (ADR-2107 reduced-motion parity with XR)', () => {
  const beat = { on: true, pulse: 0.8, bar: 0.8, phase: 0.1 };
  const idle = { on: false, pulse: 0, bar: 0, phase: 0 };

  it('drives glow, comet lead and size, root ring and pulse phase from the beat', () => {
    const m = beatModulation(beat, false);
    expect(m.glow).toBeCloseTo(1 + 0.2 * 0.8);
    expect(m.cometLead).toBeCloseTo(0.015 * 0.8);
    expect(m.cometScale).toBeCloseTo(1 + 0.35 * 0.8);
    expect(m.rootScale).toBeCloseTo(1 + 0.12 * 0.8);
    expect(m.pulsePhase).toBeCloseTo(0.1);
  });

  it('with no beat, everything is neutral and the pulse ring keeps its own clock', () => {
    expect(beatModulation(idle, false)).toEqual({ glow: 1, cometLead: 0, cometScale: 1, rootScale: null, pulsePhase: null });
  });

  it('under reduced motion the beat changes nothing on the route, glow, comet or rings', () => {
    const m = beatModulation(beat, true);
    expect(m).toEqual({ glow: 1, cometLead: 0, cometScale: 1, rootScale: 1, pulsePhase: null });
    // and it is the same whatever the beat is doing
    for (const pulse of [0, 0.3, 1]) {
      expect(beatModulation({ on: true, pulse, bar: pulse, phase: pulse }, true)).toEqual(m);
    }
  });
});
