import { describe, it, expect } from 'vitest';
import { describePhysicsState, parsePhysicsState } from '../physicsState';

const live = { isSettled: false, stableFrameCount: 0, kineticEnergy: 0.207, telemetryLive: true, physicsRunning: true };

describe('parsePhysicsState', () => {
  it('reads the flat camelCase endpoint body', () => {
    const p = parsePhysicsState({ settlementState: { isSettled: false, stableFrameCount: 3, kineticEnergy: 0.207 }, telemetryLive: true, physicsRunning: true });
    expect(p).toEqual({ isSettled: false, stableFrameCount: 3, kineticEnergy: 0.207, telemetryLive: true, physicsRunning: true });
  });

  it('accepts an enveloped body and rejects garbage', () => {
    expect(parsePhysicsState({ data: { settlementState: { isSettled: true, stableFrameCount: 9, kineticEnergy: 0 }, telemetryLive: true, physicsRunning: false } })?.isSettled).toBe(true);
    expect(parsePhysicsState(null)).toBeNull();
    expect(parsePhysicsState({ nope: 1 })).toBeNull();
  });
});

describe('describePhysicsState', () => {
  it('says settling, with the measured kinetic energy, while the GPU is moving nodes', () => {
    const d = describePhysicsState(live);
    expect(d.label).toBe('Settling');
    expect(d.detail).toContain('0.21');
    expect(d.tone).toBe('moving');
  });

  it('says settled once the server says so', () => {
    expect(describePhysicsState({ ...live, isSettled: true, kineticEnergy: 0 }).label).toBe('Settled');
  });

  it('never presents a fallback as a measurement: no GPU telemetry is its own state', () => {
    const d = describePhysicsState({ ...live, telemetryLive: false, isSettled: true });
    expect(d.label).toBe('GPU not reporting');
    expect(d.tone).toBe('unknown');
  });

  it('says paused when the loop is stopped but telemetry exists', () => {
    expect(describePhysicsState({ ...live, physicsRunning: false, isSettled: false }).label).toBe('Paused');
  });

  it('is explicit when the endpoint itself is unreachable', () => {
    const d = describePhysicsState(null);
    expect(d.label).toBe('Unavailable');
    expect(d.tone).toBe('unknown');
  });
});
