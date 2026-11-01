/**
 * The optional JSON payload of a 0x23 AGENT_ACTION event (ADR-2142, ADR-2135).
 *
 * One parser for every reader (transcript feed, registry, beam store). Producers send a JSON object
 * (`{intent, verification}` on action frames; `{intent, name, state}` on state frames) or a bare intent
 * string. A *state frame* is an event whose payload has a `state` key: presence plus lifecycle only, never
 * a beam, heat or drift credit.
 */
import type { AgentActionEvent } from './frameTypes';

export type AgentLifecycle = 'working' | 'done' | 'retired';

export interface ParsedActionPayload {
  intent?: string;
  verification?: string;
  name?: string;
  state?: AgentLifecycle;
}

const STATES: ReadonlySet<string> = new Set(['working', 'done', 'retired']);

/** Decode both payload shapes; anything undecodable yields `{}`. */
export function parseActionPayload(payload?: Uint8Array): ParsedActionPayload {
  if (!payload || payload.length === 0) return {};
  let text: string;
  try {
    text = new TextDecoder('utf-8', { fatal: false }).decode(payload).trim();
  } catch {
    return {};
  }
  if (!text) return {};

  if (text.startsWith('{')) {
    try {
      const obj = JSON.parse(text) as Record<string, unknown>;
      const str = (v: unknown) => (typeof v === 'string' ? v : undefined);
      const out: ParsedActionPayload = {
        intent: str(obj.intent) || undefined,
        verification: str(obj.verification) || undefined,
        name: str(obj.name) || undefined,
        state: typeof obj.state === 'string' && STATES.has(obj.state) ? (obj.state as AgentLifecycle) : undefined,
      };
      if (out.intent || out.verification || out.name || out.state) return out;
    } catch {
      // Not JSON after all: treat the whole string as intent.
    }
  }
  return { intent: text };
}

/** Whether the event is a state frame (its payload names a lifecycle `state`). */
export function isAgentStateFrame(event: Pick<AgentActionEvent, 'payload'>): boolean {
  const p = event.payload;
  if (!p || p.length === 0) return false;
  return parseActionPayload(p).state !== undefined;
}
