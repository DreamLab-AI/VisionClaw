/**
 * Server physics state for the status flyout, from GET /api/graph/settlement.
 *
 * This is the GPU actor's own settlement telemetry, shown next to the inferred
 * "Layout" readout (frame-throughput based, see useLayoutMotion). Both are kept: the
 * server number is the authority, the inferred one proves frames are really arriving.
 * Nothing is presented as a measurement unless the GPU actor answered
 * (`telemetryLive`), and "unreachable" is its own state rather than a guess.
 */

export interface PhysicsState {
  isSettled: boolean;
  stableFrameCount: number;
  kineticEnergy: number;
  /** false when the GPU actor did not answer: the numbers are run-state fallbacks. */
  telemetryLive: boolean;
  physicsRunning: boolean;
}

export type PhysicsTone = 'moving' | 'settled' | 'paused' | 'unknown';

export interface PhysicsDescription {
  label: string;
  detail: string;
  tone: PhysicsTone;
}

const num = (v: unknown, fallback = 0): number => (typeof v === 'number' && Number.isFinite(v) ? v : fallback);

export function parsePhysicsState(raw: unknown): PhysicsState | null {
  const body = (raw as { data?: unknown } | null)?.data ?? raw;
  const b = body as { settlementState?: Record<string, unknown>; telemetryLive?: unknown; physicsRunning?: unknown } | null;
  const s = b?.settlementState;
  if (!s || typeof s !== 'object') return null;
  return {
    isSettled: s.isSettled === true,
    stableFrameCount: num(s.stableFrameCount),
    kineticEnergy: num(s.kineticEnergy),
    telemetryLive: b?.telemetryLive === true,
    physicsRunning: b?.physicsRunning === true,
  };
}

export function describePhysicsState(p: PhysicsState | null): PhysicsDescription {
  if (!p) return { label: 'Unavailable', detail: 'physics state endpoint unreachable', tone: 'unknown' };
  if (!p.telemetryLive) {
    return { label: 'GPU not reporting', detail: 'no telemetry from the GPU actor', tone: 'unknown' };
  }
  if (!p.physicsRunning) return { label: 'Paused', detail: 'physics loop stopped', tone: 'paused' };
  if (p.isSettled) return { label: 'Settled', detail: `${p.stableFrameCount} stable frames`, tone: 'settled' };
  return { label: 'Settling', detail: `KE ${p.kineticEnergy.toFixed(2)}`, tone: 'moving' };
}
