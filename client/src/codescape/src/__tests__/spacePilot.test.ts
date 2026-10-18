import { describe, expect, it } from 'vitest';
import * as THREE from 'three';
import { parseReport, SpacePilotDevice } from '../input/spacePilot';
import { cameraDelta, DEFAULT_MOTION, isIdle, shape } from '../input/spaceMotion';
import { applyDelta } from '../input/SpacePilotRig';

function i16(...vals: number[]): DataView {
  const b = new ArrayBuffer(vals.length * 2);
  const v = new DataView(b);
  vals.forEach((x, i) => v.setInt16(i * 2, x, true));
  return v;
}

describe('SpacePilot report parsing', () => {
  it('decodes a 6-byte translation report', () => {
    expect(parseReport(1, i16(10, -20, 30))).toEqual({ axes: { tx: 10, ty: -20, tz: 30 } });
  });
  it('decodes a combined 12-byte 6-axis report with the driver sign convention', () => {
    const r = parseReport(1, i16(1, 2, 3, 4, 5, 6));
    expect(r.axes).toEqual({ tx: 1, ty: 2, tz: 3, rx: -4, ry: -5, rz: 6 });
  });
  it('decodes rotation and button reports; ignores unknown ids', () => {
    expect(parseReport(2, i16(7, 8, 9)).axes).toEqual({ rx: -7, ry: -8, rz: 9 });
    const b = new DataView(new ArrayBuffer(2));
    b.setUint16(0, 0b101, true);
    expect(parseReport(3, b)).toEqual({ buttons: 5 });
    expect(parseReport(9, i16(1))).toEqual({});
  });
  it('tolerates short payloads', () => {
    expect(parseReport(1, i16(5)).axes).toEqual({ tx: 5, ty: 0, tz: 0 });
    expect(parseReport(3, new DataView(new ArrayBuffer(0)))).toEqual({ buttons: 0 });
  });
});

describe('SpacePilotDevice ingest', () => {
  it('merges axes across reports and fires rising-edge buttons once', () => {
    const d = new SpacePilotDevice();
    const pressed: number[] = [];
    d.onButton = (i) => pressed.push(i);
    d.ingest(1, i16(100, 0, 0));
    d.ingest(2, i16(0, 0, 50));
    expect(d.sample).toMatchObject({ tx: 100, rz: 50 });
    const b = new DataView(new ArrayBuffer(2));
    b.setUint16(0, 0b11, true);
    d.ingest(3, b);
    d.ingest(3, b); // held, no new edge
    b.setUint16(0, 0b10, true);
    d.ingest(3, b); // button 1 released
    b.setUint16(0, 0b11, true);
    d.ingest(3, b); // button 1 pressed again
    expect(pressed).toEqual([1, 2, 1]);
  });
});

describe('camera motion mapping', () => {
  it('applies deadzone, clamps and a monotonic curve', () => {
    expect(shape(0, DEFAULT_MOTION)).toBe(0);
    expect(shape(10, DEFAULT_MOTION)).toBe(0);
    expect(shape(350, DEFAULT_MOTION)).toBeCloseTo(1, 6);
    expect(shape(-9999, DEFAULT_MOTION)).toBeCloseTo(-1, 6);
    const a = shape(100, DEFAULT_MOTION);
    const b = shape(200, DEFAULT_MOTION);
    expect(a).toBeGreaterThan(0);
    expect(b).toBeGreaterThan(a);
  });

  const rest = { tx: 0, ty: 0, tz: 0, rx: 0, ry: 0, rz: 0 };

  it('is idle at rest and ignores nothing but the deadzone', () => {
    expect(isIdle(rest)).toBe(true);
    expect(isIdle({ ...rest, ry: 350 })).toBe(false);
  });

  /**
   * Direction contract shared with VisionClaw's SpacePilotController (camera
   * mode): the same puck gesture must move the camera the same way in both
   * apps. Values after the driver's sign convention (rx, ry negated).
   *   VisionClaw: theta -= ry; phi += rx; camera += -tz forward; pan +tx, +ty.
   */
  const table: Array<[string, Partial<typeof rest>, (d: ReturnType<typeof cameraDelta>) => boolean]> = [
    ['sideways tilt +ry orbits left (theta decreases)', { ry: 350 }, (d) => d.azimuth < 0],
    ['twist +rz orbits the same way as +ry', { rz: 350 }, (d) => d.azimuth < 0],
    ['forward tilt +rx orbits up (phi increases)', { rx: 350 }, (d) => d.elevation > 0],
    ['push +tz moves closer', { tz: 350 }, (d) => d.dollyFactor < 1],
    ['pull -tz moves away', { tz: -350 }, (d) => d.dollyFactor > 1],
    ['slide right +tx pans right', { tx: 350 }, (d) => d.panRight > 0],
    ['slide up +ty pans up', { ty: 350 }, (d) => d.panUp > 0],
  ];
  for (const [name, input, check] of table) {
    it(name, () => {
      expect(check(cameraDelta({ ...rest, ...input }, 0.1))).toBe(true);
    });
  }

  it('clamps a stalled frame and combines yaw sources', () => {
    const push = cameraDelta({ ...rest, tz: 350 }, 1);
    expect(push.dollyFactor).toBeCloseTo(Math.exp(-DEFAULT_MOTION.dollyRate * 0.1), 6);
    const both = cameraDelta({ ...rest, ry: 350, rz: 350 }, 0.1);
    const one = cameraDelta({ ...rest, ry: 350 }, 0.1);
    // Yaw is clamped to full deflection, so tilt plus twist is no faster than either.
    expect(both.azimuth).toBeCloseTo(one.azimuth, 6);
  });

  it('honours axis inversion', () => {
    const cfg = { ...DEFAULT_MOTION, invert: { ...DEFAULT_MOTION.invert, tz: true } };
    const d = cameraDelta({ ...rest, tz: 350 }, 0.1, cfg);
    expect(d.dollyFactor).toBeGreaterThan(1);
  });
});

describe('applyDelta on an orbit camera', () => {
  it('keeps the camera looking at the target and respects distance limits', () => {
    const cam = new THREE.PerspectiveCamera();
    cam.position.set(0, 0, 10);
    const c = { target: new THREE.Vector3(0, 0, 0), minDistance: 2, maxDistance: 20 };
    applyDelta(cam, c, { azimuth: Math.PI / 2, elevation: 0, dollyFactor: 0.5, panRight: 0, panUp: 0 });
    expect(cam.position.distanceTo(c.target)).toBeCloseTo(5, 5);
    expect(Math.abs(cam.position.x)).toBeCloseTo(5, 5);
    const dir = new THREE.Vector3(0, 0, -1).applyQuaternion(cam.quaternion);
    expect(dir.dot(c.target.clone().sub(cam.position).normalize())).toBeCloseTo(1, 5);
    applyDelta(cam, c, { azimuth: 0, elevation: 0, dollyFactor: 0.01, panRight: 0, panUp: 0 });
    expect(cam.position.distanceTo(c.target)).toBeCloseTo(2, 5);
  });
  it('pans the target in the camera plane by a fraction of distance', () => {
    const cam = new THREE.PerspectiveCamera();
    cam.position.set(0, 0, 10);
    cam.lookAt(0, 0, 0);
    const c = { target: new THREE.Vector3(0, 0, 0), minDistance: 0, maxDistance: 0 };
    applyDelta(cam, c, { azimuth: 0, elevation: 0, dollyFactor: 1, panRight: 0.1, panUp: 0 });
    expect(c.target.x).toBeCloseTo(1, 5);
    expect(cam.position.x).toBeCloseTo(1, 5);
  });
});
