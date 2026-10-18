/**
 * Turn a six-axis SpacePilot sample into an orbit-camera delta. Pure maths,
 * unit-tested; `useSpacePilotMotion` applies the result to the camera each
 * frame from inside `CameraRig`.
 *
 * Model: the puck orbits the camera around the OrbitControls target, with the
 * same hand feel as VisionClaw's own SpacePilot camera mode so a user moving
 * between the two apps keeps their muscle memory:
 * - tilt sideways (ry) and twist (rz) both orbit left/right (azimuth),
 * - tilt forward/back (rx) orbits up/down (elevation),
 * - push (tz, positive = push away from you) dollies IN, scaled by distance,
 * - slide (tx, ty) pans the target in the camera plane, scaled by distance,
 * - roll about the view axis is not used: OrbitControls keeps the horizon.
 *
 * Inputs are the raw device integers after the driver's sign convention
 * (`parseReport` negates rx and ry like VisionClaw's SpaceDriverService), so
 * the direction table in `__tests__/spacePilot.test.ts` is the contract both
 * apps share. Output is per second; the caller multiplies by frame time.
 */

export interface MotionConfig {
  /** Raw value treated as full deflection (350 fits a SpacePilot; 512 a SpaceMouse). */
  fullScale: number;
  /** Fraction of full deflection ignored around centre. */
  deadzone: number;
  /** Radians per second at full tilt/twist. */
  rotateRate: number;
  /** Fraction of the current distance per second at full push. */
  dollyRate: number;
  /** Fraction of the current distance per second at full slide. */
  panRate: number;
  /** Response curve exponent: 1 linear, 2 gentle centre and fast edges. */
  curve: number;
  invert: { tx: boolean; ty: boolean; tz: boolean; rx: boolean; ry: boolean; rz: boolean };
}

export const DEFAULT_MOTION: MotionConfig = {
  fullScale: 350,
  deadzone: 0.06,
  rotateRate: 1.6,
  dollyRate: 1.4,
  panRate: 1.1,
  curve: 1.6,
  invert: { tx: false, ty: false, tz: false, rx: false, ry: false, rz: false },
};

export interface CameraDelta {
  /** Azimuth change in radians (positive = camera orbits to the left of the target). */
  azimuth: number;
  /** Elevation change in radians (positive = camera orbits up). */
  elevation: number;
  /** Multiplicative distance factor (below 1 = closer). */
  dollyFactor: number;
  /** Pan in camera-plane units of current distance: right and up. */
  panRight: number;
  panUp: number;
}

/** Normalise a raw axis to [-1, 1] with deadzone and response curve. */
export function shape(raw: number, cfg: MotionConfig): number {
  if (!Number.isFinite(raw) || cfg.fullScale <= 0) return 0;
  const v = Math.max(-1, Math.min(1, raw / cfg.fullScale));
  const a = Math.abs(v);
  if (a <= cfg.deadzone) return 0;
  // Re-map (deadzone..1] onto (0..1] so motion starts from zero at the edge.
  const t = (a - cfg.deadzone) / (1 - cfg.deadzone);
  return Math.sign(v) * Math.pow(t, cfg.curve);
}

export type Sample = { tx: number; ty: number; tz: number; rx: number; ry: number; rz: number };

export function cameraDelta(s: Sample, dt: number, cfg: MotionConfig = DEFAULT_MOTION): CameraDelta {
  const sgn = (k: keyof MotionConfig['invert']) => (cfg.invert[k] ? -1 : 1);
  const tx = shape(s.tx, cfg) * sgn('tx');
  const ty = shape(s.ty, cfg) * sgn('ty');
  const tz = shape(s.tz, cfg) * sgn('tz');
  const rx = shape(s.rx, cfg) * sgn('rx');
  const ry = shape(s.ry, cfg) * sgn('ry');
  const rz = shape(s.rz, cfg) * sgn('rz');
  const step = Math.max(0, Math.min(0.1, dt)); // clamp a stalled frame
  // VisionClaw: theta -= ry (sideways tilt orbits), phi += rx, camera moves by
  // -tz (push = forward), pan by +tx / +ty. Twist is added to azimuth so the
  // puck's natural "turn" works too; both agree in sign.
  const yaw = Math.max(-1, Math.min(1, ry + rz));
  return {
    azimuth: -yaw * cfg.rotateRate * step,
    elevation: rx * cfg.rotateRate * step,
    dollyFactor: Math.exp(-tz * cfg.dollyRate * step),
    panRight: tx * cfg.panRate * step,
    panUp: ty * cfg.panRate * step,
  };
}

/** `true` when the sample is at rest after shaping (no camera work needed). */
export function isIdle(s: Sample, cfg: MotionConfig = DEFAULT_MOTION): boolean {
  return [s.tx, s.ty, s.tz, s.rx, s.ry, s.rz].every((v) => shape(v, cfg) === 0);
}
