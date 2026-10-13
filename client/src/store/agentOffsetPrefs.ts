/**
 * Per-viewer preference: whether agent nodes animate away from their server position.
 *
 * Agents are drawn with a client-side offset (utils/agentNudge.ts: converge on the work
 * target, drift outward when idle). The XR client applies its own, different offset. That
 * is a deliberate visual layer, but it must be visible and switchable, so one toggle exists
 * in each interface with the same default (on). Off means agents are drawn exactly at the
 * server position, the authoritative one.
 *
 * Storage is a per-viewer convenience only: every access is guarded, and the preference
 * works for the session when storage is unavailable.
 */

const KEY = 'vc.agentOffsets';
type Listener = () => void;

function load(): boolean {
  try {
    return localStorage.getItem(KEY) !== 'off';
  } catch {
    return true;
  }
}

let enabled = load();
const listeners = new Set<Listener>();

export function agentOffsetsEnabled(): boolean {
  return enabled;
}

export function setAgentOffsetsEnabled(value: boolean): void {
  if (value === enabled) return;
  enabled = value;
  try {
    localStorage.setItem(KEY, value ? 'on' : 'off');
  } catch {
    /* storage unavailable: session-only */
  }
  listeners.forEach((cb) => cb());
}

export function subscribeAgentOffsets(cb: Listener): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

/** Re-read storage; tests simulate a page load with this. */
export function __resetAgentOffsetPrefsForTest(): void {
  enabled = load();
  listeners.clear();
}
