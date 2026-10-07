import { describe, it, expect } from 'vitest';
import { compileDirector, INTRO_DUR, OUTRO_DUR } from '../director';
import { TOTAL_DUR, REVEAL_DUR, dist3 } from '../routeMath';
import type { Vec3 } from '../../memoryTrajectory/types';

const route: Vec3[] = Array.from({ length: 65 }, (_, i) => [i * 10, Math.sin(i / 6) * 40, Math.cos(i / 9) * 30] as Vec3);
const R = 300;

describe('compileDirector', () => {
  it('runs intro, reveal and trace at the playback speed, then the outro', () => {
    for (const speed of [0.5, 1, 2]) {
      const d = compileDirector({ route, radius: R, speed });
      expect(d.duration).toBeCloseTo(INTRO_DUR + TOTAL_DUR / speed + OUTRO_DUR, 6);
      expect(d.playbackAt(0)).toBe(0);
      expect(d.playbackAt(INTRO_DUR)).toBe(0);
      expect(d.playbackAt(INTRO_DUR + REVEAL_DUR / speed)).toBeCloseTo(REVEAL_DUR, 6);
      expect(d.playbackAt(d.duration)).toBeGreaterThanOrEqual(TOTAL_DUR);
    }
  });

  it('playback time never runs backwards', () => {
    const d = compileDirector({ route, radius: R, speed: 1 });
    let prev = -1;
    for (let t = 0; t <= d.duration; t += 0.05) {
      const el = d.playbackAt(t);
      expect(el).toBeGreaterThanOrEqual(prev);
      prev = el;
    }
  });

  it('opens on the route centroid and ends framing the answer', () => {
    const d = compileDirector({ route, radius: R, speed: 1 });
    const c = route.reduce<Vec3>((a, p) => [a[0] + p[0] / route.length, a[1] + p[1] / route.length, a[2] + p[2] / route.length], [0, 0, 0]);
    expect(dist3(d.poseAt(0).target, c)).toBeLessThan(1e-6);
    const end = d.poseAt(d.duration);
    expect(dist3(end.target, route[route.length - 1])).toBeLessThan(1e-6);
    expect(dist3(end.position, end.target)).toBeGreaterThan(R * 0.2);
  });

  it('flies along the route during the trace, looking ahead of the comet', () => {
    const d = compileDirector({ route, radius: R, speed: 1 });
    const mid = INTRO_DUR + REVEAL_DUR + 1.4;
    const pose = d.poseAt(mid);
    const nearest = Math.min(...route.map((p) => dist3(p, pose.target)));
    expect(nearest).toBeLessThan(15);
    expect(dist3(pose.position, pose.target)).toBeLessThan(R);
  });

  it('moves the camera continuously: no frame-to-frame jumps', () => {
    for (const speed of [1, 3]) {
      const d = compileDirector({ route, radius: R, speed });
      const dt = 1 / 60;
      const steps: number[] = [];
      for (let t = dt; t <= d.duration; t += dt) {
        steps.push(dist3(d.poseAt(t).position, d.poseAt(t - dt).position));
      }
      const max = Math.max(...steps);
      expect(max).toBeLessThan(R * 0.12);
    }
  });

  it('orbits a focus point when there is no route', () => {
    const d = compileDirector({ route: [], radius: R, speed: 1, focus: [5, 5, 5] });
    const a = d.poseAt(0.5);
    const b = d.poseAt(d.duration - 0.1);
    expect(a.target).toEqual([5, 5, 5]);
    expect(dist3(a.position, b.position)).toBeGreaterThan(1);
  });

  it('holds still under reduced motion', () => {
    const d = compileDirector({ route, radius: R, speed: 1, reducedMotion: true });
    expect(d.poseAt(0)).toEqual(d.poseAt(d.duration / 2));
    expect(d.playbackAt(0)).toBeGreaterThanOrEqual(TOTAL_DUR);
  });
});
