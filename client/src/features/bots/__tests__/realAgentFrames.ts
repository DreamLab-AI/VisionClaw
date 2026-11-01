/**
 * The shared real-agent fixture (ADR-2142 / ADR-2135): `tests/fixtures/agents/real-agent-frames.v1.json`
 * at the repo root, written from the server's own transform (`RosterFeed` + `encode_agent_actions`), so the
 * desktop decodes exactly the bytes the server produces. Never hand-write hex here; a missing file is an error.
 *
 * `REAL_AGENT_FRAMES` points the loader at another file of the same shape (local runs before the server
 * fixture lands only).
 */
import { binaryProtocol } from '../../../services/BinaryWebSocketProtocol';
import type { AgentActionEvent } from '../../../services/binaryProtocol/frameTypes';

type Fs = typeof import('fs');
type PathMod = typeof import('path');
const getBuiltin = (process as unknown as { getBuiltinModule(id: string): unknown }).getBuiltinModule;

export interface FixtureEvent {
  sourceAgentId: number;
  targetNodeId: number;
  actionType: number;
  timestamp: number;
  durationMs: number;
  payload: Record<string, unknown>;
}
export interface FixtureFrame {
  name: 'spawn' | 'keepalive' | 'done' | 'retired';
  hex: string;
  events: FixtureEvent[];
}

let cached: FixtureFrame[] | null = null;

export function loadRealAgentFrames(): FixtureFrame[] {
  if (cached) return cached;
  const { readFileSync, existsSync } = getBuiltin('fs') as Fs;
  const { resolve } = getBuiltin('path') as PathMod;
  const file = process.env.REAL_AGENT_FRAMES ?? resolve(__dirname, '../../../../../tests/fixtures/agents/real-agent-frames.v1.json');
  if (!existsSync(file)) throw new Error(`shared fixture missing: ${file} (the server writes it; do not invent hex)`);
  const doc = JSON.parse(readFileSync(file, 'utf8')) as { schema: string; frames: FixtureFrame[] };
  if (doc.schema !== 'visionclaw.real-agent-frames.v1') throw new Error(`unexpected fixture schema ${doc.schema}`);
  cached = doc.frames;
  return cached;
}

export function hexToBuffer(hex: string): ArrayBuffer {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < bytes.length; i++) bytes[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return bytes.buffer;
}

/** Decode a fixture frame with the real client decoder. */
export function decodeFrame(name: FixtureFrame['name']): AgentActionEvent[] {
  const frame = loadRealAgentFrames().find(f => f.name === name);
  if (!frame) throw new Error(`fixture has no ${name} frame`);
  return binaryProtocol.decodeAgentActions(hexToBuffer(frame.hex));
}

/** The decoded events of `name` with the payload re-parsed from its bytes, for comparing with `events`. */
export function parsedPayloads(events: AgentActionEvent[]): Array<Record<string, unknown>> {
  return events.map(e => JSON.parse(new TextDecoder().decode(e.payload)) as Record<string, unknown>);
}
