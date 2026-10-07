/**
 * Cinematic director: a camera timeline over one query route.
 *
 *   intro    wide orbit of the route while the scene settles (playback held at 0)
 *   reveal   the orbit tightens while the search tree grows
 *   trace    fly-along behind the comet, looking ahead of it
 *   outro    slow orbit around the answer
 *
 * Shots are pure functions of director time; seams are cross-faded with a
 * smoothstep so position and target stay continuous. Modelled on the RuVector
 * Explorer director (`startDirector` / `compileTour20`,
 * https://github.com/ruvnet/RuVector, docs/explorer, MIT licence), reduced to
 * the single-route tour the memory explorer needs.
 */

import type { Vec3 } from '../memoryTrajectory/types';
import { PATH_DUR, REVEAL_DUR, TOTAL_DUR, clamp01, ease, lerp3, pointAt } from './routeMath';

export const INTRO_DUR = 2.5;
export const OUTRO_DUR = 4;
const MAX_BLEND = 1.2;
const ORBIT_RATE = 0.3; // rad/s
const ELEVATION = 0.42; // rad

export interface CameraPose {
  position: Vec3;
  target: Vec3;
}

export interface DirectorInput {
  /** route polyline in world coordinates, root first */
  route: Vec3[];
  /** framing radius in world units (roughly the route's extent) */
  radius: number;
  /** playback speed; the reveal and trace run 1 / speed as long */
  speed: number;
  /** orbit centre when there is no route */
  focus?: Vec3;
  reducedMotion?: boolean;
}

export interface Director {
  duration: number;
  /** normalised playback time (the `el` of the reveal clock) at director time t */
  playbackAt(t: number): number;
  poseAt(t: number): CameraPose;
}

const smoothstep = (x: number) => {
  const t = clamp01(x);
  return t * t * (3 - 2 * t);
};

const add = (a: Vec3, b: Vec3): Vec3 => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
const sub = (a: Vec3, b: Vec3): Vec3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const scale = (a: Vec3, s: number): Vec3 => [a[0] * s, a[1] * s, a[2] * s];
const norm = (a: Vec3): Vec3 => {
  const l = Math.hypot(a[0], a[1], a[2]);
  return l > 1e-9 ? scale(a, 1 / l) : [1, 0, 0];
};
const cross = (a: Vec3, b: Vec3): Vec3 => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];

const mixPose = (a: CameraPose, b: CameraPose, s: number): CameraPose => ({
  position: lerp3(a.position, b.position, s),
  target: lerp3(a.target, b.target, s),
});

function orbit(centre: Vec3, r: number, theta: number): CameraPose {
  const c = Math.cos(ELEVATION);
  return {
    position: [centre[0] + r * c * Math.cos(theta), centre[1] + r * Math.sin(ELEVATION), centre[2] + r * c * Math.sin(theta)],
    target: [...centre] as Vec3,
  };
}

export function compileDirector(input: DirectorInput): Director {
  const speed = Math.max(0.05, input.speed);
  const R = Math.max(1e-3, input.radius);
  const route = input.route;
  const hasRoute = route.length >= 2;
  const revealLen = REVEAL_DUR / speed;
  const traceLen = PATH_DUR / speed;
  const tTrace = INTRO_DUR + revealLen;
  const tOutro = tTrace + traceLen;
  const duration = tOutro + OUTRO_DUR;

  const centroid: Vec3 = hasRoute
    ? route.reduce<Vec3>((a, p) => add(a, scale(p, 1 / route.length)), [0, 0, 0])
    : input.focus ?? [0, 0, 0];
  const tip: Vec3 = hasRoute ? route[route.length - 1] : centroid;
  const chord = hasRoute ? sub(tip, route[0]) : ([1, 0, 0] as Vec3);
  const theta0 = Math.atan2(chord[2], chord[0]) + Math.PI / 2;
  const last = route.length - 1;

  const playbackAt = (t: number): number => {
    if (input.reducedMotion) return TOTAL_DUR;
    if (t <= INTRO_DUR) return 0;
    return (t - INTRO_DUR) * speed;
  };

  // shot A: intro + reveal orbit around the centroid, tightening
  const shotOrbit = (t: number): CameraPose => {
    const r = R * (2.2 - 0.9 * smoothstep(t / tTrace));
    return orbit(centroid, r, theta0 + ORBIT_RATE * t);
  };

  // shot B: behind the comet head, looking ahead
  const shotFly = (t: number): CameraPose => {
    const u = clamp01((t - tTrace) / traceLen);
    const head = ease.io(u) * last;
    const span = Math.max(2, last * 0.12);
    const behind = pointAt(route, head - span);
    const ahead = pointAt(route, head + span * 0.6);
    const dir = norm(sub(ahead, behind));
    const side = norm(cross(dir, [0, 1, 0]));
    const position = add(add(behind, scale([0, 1, 0], R * 0.18)), scale(side, R * 0.12));
    return { position, target: ahead };
  };

  // shot C: outro orbit around the answer
  const shotOutro = (t: number): CameraPose => orbit(tip, R * 0.55, theta0 + ORBIT_RATE * t);

  const blend = Math.min(MAX_BLEND, 0.9 * traceLen, 0.9 * OUTRO_DUR);

  const poseAt = (tIn: number): CameraPose => {
    if (input.reducedMotion) return shotOrbit(0);
    const t = Math.max(0, Math.min(duration, tIn));
    if (!hasRoute) return orbit(centroid, R * 1.6, theta0 + ORBIT_RATE * t);
    const h = blend / 2;
    if (t < tTrace - h) return shotOrbit(t);
    if (t <= tTrace + h) return mixPose(shotOrbit(t), shotFly(t), smoothstep((t - (tTrace - h)) / blend));
    if (t < tOutro - h) return shotFly(t);
    if (t <= tOutro + h) return mixPose(shotFly(t), shotOutro(t), smoothstep((t - (tOutro - h)) / blend));
    return shotOutro(t);
  };

  return { duration, playbackAt, poseAt };
}
