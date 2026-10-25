/**
 * Deterministic settings-patch assembly for the Jev option (VC-06-S1-01).
 *
 * Jev only *selects* the target setting. Everything numeric lives here, in
 * code: value parsing, direction words, clamping to the registry range and
 * step snapping. The result is a reviewable patch (path, current → proposed);
 * the user applies it through the existing store/authenticated path.
 * Protected settings are never offered and never patched.
 */
import type { SettingCandidate } from './jevClient';

export interface PatchField {
  path?: string;
  label: string;
  type: string;
  min?: number;
  max?: number;
  step?: number;
  options?: string[];
  description?: string;
}

export interface SettingsPatch {
  path: string;
  label: string;
  current: unknown;
  proposed: number | boolean | string;
  /** True when clamping changed the requested number. */
  clamped: boolean;
}

export type PatchResult =
  | { ok: true; patch: SettingsPatch }
  | { ok: false; reason: 'protected' | 'unsupported-type' | 'missing-value' | 'bad-value' };

const PROTECTED = /(secret|token|password|passwd|apikey|api_key|credential|privatekey|private_key|nsec|auth|nostr|pubkey|session)/i;

/** Mirrors the server's `is_protected_path`. */
export function isProtectedPath(path: string): boolean {
  return PROTECTED.test(path);
}

const UP = /\b(more|increase|bigger|larger|higher|raise|stronger|greater|up)\b/i;
const DOWN = /\b(less|decrease|smaller|lower|reduce|weaker|fewer|down)\b/i;
const ON = /\b(on|enable|enabled|show|turn on|yes|true|display)\b/i;
const OFF = /\b(off|disable|disabled|hide|turn off|no|false)\b/i;

function tokens(s: string): string[] {
  return s.toLowerCase().split(/[^a-z0-9]+/).filter((t) => t.length > 2);
}

/** Lexically shortlist registry fields for a command (≤ `limit`), excluding protected paths. */
export function shortlistSettings(
  command: string,
  fields: Array<PatchField & { current?: unknown }>,
  limit = 40,
): SettingCandidate[] {
  const q = new Set(tokens(command));
  const scored = fields
    .filter((f) => f.path && !isProtectedPath(f.path) && f.type !== 'action-button' && f.type !== 'readonly' && f.type !== 'nostr-button')
    .map((f) => {
      const hay = tokens(`${f.label} ${f.path} ${f.description ?? ''}`);
      let s = 0;
      for (const t of hay) if (q.has(t)) s += 1;
      return { f, s };
    })
    // Keep a stable remainder so a paraphrase with no lexical overlap still has candidates.
    .sort((a, b) => b.s - a.s);
  return scored.slice(0, limit).map(({ f }) => ({
    path: f.path as string,
    label: f.label,
    type: f.type,
    min: f.min,
    max: f.max,
    current: typeof f.current === 'string' ? f.current.slice(0, 64) : f.current,
    description: f.description,
  }));
}

/** First number in the command, honouring a trailing `%`. */
export function parseNumber(command: string): { value: number; percent: boolean } | null {
  const m = command.match(/(-?\d+(?:\.\d+)?)\s*(%)?/);
  if (!m) return null;
  const value = Number(m[1]);
  if (!Number.isFinite(value)) return null;
  return { value, percent: !!m[2] };
}

function snap(v: number, step?: number): number {
  if (!step || step <= 0) return v;
  const snapped = Math.round(v / step) * step;
  return Number(snapped.toFixed(10));
}

/**
 * Assemble a patch for `field` from the command. Pure and deterministic; the
 * only model input is *which* field.
 */
export function buildPatch(command: string, field: PatchField, current: unknown): PatchResult {
  const path = field.path;
  if (!path || isProtectedPath(path)) return { ok: false, reason: 'protected' };

  if (field.type === 'toggle') {
    const wantOn = ON.test(command);
    const wantOff = OFF.test(command);
    if (wantOn === wantOff) return { ok: false, reason: 'missing-value' };
    return { ok: true, patch: { path, label: field.label, current, proposed: wantOn, clamped: false } };
  }

  if (field.type === 'slider') {
    const cur = typeof current === 'number' ? current : Number(current);
    const num = parseNumber(command);
    let target: number;
    if (num) {
      target = num.percent && field.max !== undefined ? (num.value / 100) * field.max : num.value;
    } else if (Number.isFinite(cur) && (UP.test(command) !== DOWN.test(command))) {
      const factor = UP.test(command) ? 1.25 : 0.8;
      const stepBump = field.step && field.step > 0 ? field.step : 0;
      target = cur * factor;
      if (stepBump && Math.abs(target - cur) < stepBump) target = cur + (UP.test(command) ? stepBump : -stepBump);
    } else {
      return { ok: false, reason: 'missing-value' };
    }
    if (!Number.isFinite(target)) return { ok: false, reason: 'bad-value' };
    const lo = field.min ?? -Infinity;
    const hi = field.max ?? Infinity;
    const clampedV = Math.min(hi, Math.max(lo, target));
    const proposed = snap(clampedV, field.step);
    const final = Math.min(hi, Math.max(lo, proposed));
    return { ok: true, patch: { path, label: field.label, current, proposed: final, clamped: final !== target } };
  }

  if (field.type === 'select' && field.options?.length) {
    const lower = command.toLowerCase();
    const hits = field.options.filter((o) => lower.includes(o.toLowerCase()));
    if (hits.length !== 1) return { ok: false, reason: 'missing-value' };
    return { ok: true, patch: { path, label: field.label, current, proposed: hits[0], clamped: false } };
  }

  return { ok: false, reason: 'unsupported-type' };
}
