/** State frames (ADR-2142 / ADR-2135, real swarm agents): presence plus state, no beam, no heat, no drift credit. */
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { useAgentTargetStore } from '../agentTargetStore';
import { useTransientBeamStore } from '../transientBeamStore';
import { AgentActionType, type AgentActionEvent } from '../../services/binaryProtocol/frameTypes';
import { decodeFrame } from '../../features/bots/__tests__/realAgentFrames';

const store = () => useAgentTargetStore.getState();
const enc = (o: object) => new TextEncoder().encode(JSON.stringify(o));
const stateFrame = (src: number, state: string, target = 0, name = 'agent x'): AgentActionEvent => ({
  sourceAgentId: src, targetNodeId: target, actionType: AgentActionType.Query, timestamp: 0, durationMs: 0, payload: enc({ intent: 'i', name, state }),
});
const action = (src: number, target: number): AgentActionEvent => ({
  sourceAgentId: src, targetNodeId: target, actionType: AgentActionType.Update, timestamp: 0, durationMs: 1500, payload: enc({ intent: 'edit' }),
});
const ID = 0x82000007;

beforeEach(() => { vi.useFakeTimers({ toFake: ['Date'] }); vi.setSystemTime(50_000); store().clear(); useTransientBeamStore.getState().clear(); });

describe('registry state', () => {
  it('a working state frame registers the agent with its name, and adds a target only when it names one', () => {
    store().pushActions([stateFrame(ID, 'working')]);
    expect(store().agents.get(ID)).toMatchObject({ state: 'working', name: 'agent x' });
    expect(store().targets.has(ID)).toBe(false);
    store().pushActions([stateFrame(ID, 'working', 42)]);
    expect(store().targets.get(ID)).toBe(42);
  });

  it('done is explicit, and any ordinary action clears it', () => {
    store().pushActions([stateFrame(ID, 'done', 5)]);
    expect(store().agents.get(ID)?.state).toBe('done');
    store().pushActions([action(ID, 6)]);
    expect(store().agents.get(ID)?.state).toBe('working');
  });

  it('retired removes everything at once; retire(id) does the same', () => {
    store().pushActions([stateFrame(ID, 'working', 5)]);
    store().pushActions([stateFrame(ID, 'retired')]);
    for (const m of [store().agents, store().targets, store().seenAt]) expect(m.has(ID)).toBe(false);
    store().pushActions([action(ID, 5)]);
    store().retire(ID);
    for (const m of [store().agents, store().targets, store().seenAt]) expect(m.has(ID)).toBe(false);
  });

  it('the flagged and the bare form of an id are one agent', () => {
    store().pushActions([action(1003, 5), action((0x80000000 | 1003) >>> 0, 6)]);
    expect(store().seenAt.size).toBe(1);
  });

  it('entries past the 30 s presence window are pruned on the next push', () => {
    store().pushActions([action(ID, 5)]);
    vi.setSystemTime(50_000 + 31_000);
    store().pushActions([action(ID + 1, 5)]);
    expect(store().seenAt.has(ID)).toBe(false);
    expect(store().seenAt.has(ID + 1)).toBe(true);
  });

  it('the real fixture frames drive it: spawn registers, done marks done, retired removes', () => {
    store().pushActions(decodeFrame('spawn'));
    const id = decodeFrame('spawn')[0].sourceAgentId;
    expect(store().agents.get(id)).toMatchObject({ state: 'working', name: 'agent task-1' });
    store().pushActions(decodeFrame('done'));
    expect(store().agents.get(id)?.state).toBe('done');
    store().pushActions(decodeFrame('retired'));
    expect(store().agents.has(id)).toBe(false);
  });
});

describe('transientBeamStore skips state frames', () => {
  it('admits ordinary actions and drops state frames in the same batch', () => {
    useTransientBeamStore.getState().pushBeams([stateFrame(ID, 'working', 9), action(ID, 9), stateFrame(ID, 'done', 9)]);
    expect(useTransientBeamStore.getState().beams).toHaveLength(1);
  });
});
