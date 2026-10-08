import { describe, it, expect } from 'vitest';
import * as THREE from 'three';
import {
  sceneFitBounds,
  createSettleTrigger,
  cloudFitKey,
  CLOUD_SETTLE_MS,
  unoccludedRect,
  fitPose,
  type CloudFitInput,
  type Rect,
} from '../sceneFitBounds';
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
    // the farthest graph point and the cloud's far side bound the sphere;
    // when the closer cloud encloses both graphs (ADR-2135, 2026-10-08) the
    // cloud's own sphere is the bound
    const span = Math.hypot(...p.position) + rc + separatedFrame().radius + local.radius;
    expect(fit.radius).toBeLessThanOrEqual(Math.max(span / 2, rc) + 1);
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

// ── fit inside the unoccluded viewport (operator report 2026-10-08) ──
// Live numbers from the desktop at 1440×900: the explorer panel's rect, the
// control dock's, and the snapshot's robust cloud bounds (5–95% box).
const VIEW = { width: 1440, height: 900 };
const PANEL: Rect = { left: 1058, top: 14, right: 1426, bottom: 478 };
const DOCK: Rect = { left: 163, top: 750, right: 1277, bottom: 845 };
const LIVE_CLOUD = { centre: [8.75, 5.69, -0.81] as Vec3, half: [81.26, 64.25, 40.04] as Vec3 };
const liveCloud: CloudFitInput = {
  bounds: { centre: LIVE_CLOUD.centre, radius: Math.hypot(...LIVE_CLOUD.half) },
  cloudScale: 5,
};

/** pixel position of a world point through a camera posed as `pose` */
function projector(pose: { position: Vec3; target: Vec3 }) {
  const cam = new THREE.PerspectiveCamera(75, VIEW.width / VIEW.height, 0.1, 1e6);
  cam.position.set(...pose.position);
  cam.lookAt(new THREE.Vector3(...pose.target));
  cam.updateMatrixWorld();
  return (p: Vec3) => {
    const v = new THREE.Vector3(...p).project(cam);
    return { x: ((v.x + 1) / 2) * VIEW.width, y: ((1 - v.y) / 2) * VIEW.height, front: v.z < 1 };
  };
}

/** the 8 corners of the cloud's robust box where cloudFrame places it, at a yaw of the outer group */
function cloudCorners(yaw: number): Vec3[] {
  const local = robustBounds(blob.flat(), blob.length)!;
  const p = cloudPlacement(liveCloud.bounds, local, liveCloud.cloudScale);
  const out: Vec3[] = [];
  for (const sx of [-1, 1]) for (const sy of [-1, 1]) for (const sz of [-1, 1]) {
    const c: Vec3 = [sx * LIVE_CLOUD.half[0], sy * LIVE_CLOUD.half[1], sz * LIVE_CLOUD.half[2]];
    const cs = Math.cos(yaw), sn = Math.sin(yaw);
    const r: Vec3 = [c[0] * cs + c[2] * sn, c[1], -c[0] * sn + c[2] * cs];
    out.push([p.position[0] + p.scale * r[0], p.position[1] + p.scale * r[1], p.position[2] + p.scale * r[2]]);
  }
  return out;
}

const within = (q: { x: number; y: number; front: boolean }, r: Rect) =>
  q.front && q.x >= r.left - 0.5 && q.x <= r.right + 0.5 && q.y >= r.top - 0.5 && q.y <= r.bottom + 0.5;

describe('unoccludedRect', () => {
  it('is the whole view with nothing over it', () => {
    expect(unoccludedRect(VIEW, [])).toEqual({ left: 0, top: 0, right: 1440, bottom: 900 });
  });
  it('is the largest free rectangle beside the explorer panel and above the dock', () => {
    expect(unoccludedRect(VIEW, [PANEL, DOCK])).toEqual({ left: 0, top: 0, right: 1058, bottom: 750 });
  });
  it('ignores occluders outside the view and empty ones', () => {
    const r = unoccludedRect(VIEW, [{ left: 2000, top: 0, right: 2100, bottom: 50 }, { left: 10, top: 10, right: 10, bottom: 400 }]);
    expect(r).toEqual({ left: 0, top: 0, right: 1440, bottom: 900 });
  });
});

describe('fitPose', () => {
  it('lands both graphs and the memory cloud inside the unoccluded viewport, at every cloud yaw', () => {
    const pos = projected();
    const fit = sceneFitBounds(pos, pos.length / 3, liveCloud)!;
    const free = unoccludedRect(VIEW, [PANEL, DOCK]);
    const proj = projector(fitPose(fit, { fovDeg: 75, aspect: VIEW.width / VIEW.height }, VIEW, free));
    for (let k = 0; k < 8; k++) {
      for (const c of cloudCorners((k * Math.PI) / 4)) expect(within(proj(c), free), `cloud corner ${c}`).toBe(true);
    }
    const f = separatedFrame();
    for (const v of [Vertex.Knowledge, Vertex.Ontology]) for (const p of blob) {
      expect(within(proj(place(f, v, p)), free), `graph ${v}`).toBe(true);
    }
  });

  it('frames tightly: the cloud spans a large share of the free rectangle, not a speck', () => {
    const pos = projected();
    const fit = sceneFitBounds(pos, pos.length / 3, liveCloud)!;
    const free = unoccludedRect(VIEW, [PANEL, DOCK]);
    const proj = projector(fitPose(fit, { fovDeg: 75, aspect: VIEW.width / VIEW.height }, VIEW, free));
    const xs = cloudCorners(0).map((c) => proj(c).x);
    expect(Math.max(...xs) - Math.min(...xs)).toBeGreaterThan(0.4 * (free.right - free.left));
  });

  it('the old centred whole-view fit put the cloud under the explorer panel', () => {
    const pos = projected();
    const fit = sceneFitBounds(pos, pos.length / 3, liveCloud)!;
    const whole = { left: 0, top: 0, right: VIEW.width, bottom: VIEW.height };
    const proj = projector(fitPose(fit, { fovDeg: 75, aspect: VIEW.width / VIEW.height }, VIEW, whole));
    const free = unoccludedRect(VIEW, [PANEL, DOCK]);
    const all = [0, 1, 2, 3].flatMap((k) => cloudCorners((k * Math.PI) / 2));
    expect(all.some((c) => !within(proj(c), free))).toBe(true);
  });

  it('keeps the elevated three-quarter view: camera above and in front of the target', () => {
    const pos = projected();
    const fit = sceneFitBounds(pos, pos.length / 3, liveCloud)!;
    const pose = fitPose(fit, { fovDeg: 75, aspect: 1.6 }, VIEW, unoccludedRect(VIEW, [PANEL]));
    const d: Vec3 = [pose.position[0] - pose.target[0], pose.position[1] - pose.target[1], pose.position[2] - pose.target[2]];
    expect(d[1] / d[2]).toBeCloseTo(0.3, 5);
    expect(d[2]).toBeGreaterThan(fit.radius);
  });
});
