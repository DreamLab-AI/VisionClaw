// @ts-ignore - vitest types may not be available in all environments
import { describe, it, expect, beforeEach, vi } from 'vitest';

/**
 * ADR-2099 — agent-action (0x23) frame routing.
 *
 * These tests pin the invariant that made the old V4-headered `handleAgentAction`
 * branch dead code: `processBinaryData` peels the 0x23 tag and returns BEFORE
 * `parseHeader` is ever consulted, and `parseHeader` reads its type byte from
 * offset 0 — the same byte — so no agent-action frame can reach the type switch.
 * If someone reintroduces a framed decode path for 0x23, the extractPayload
 * assertions here fail.
 */

// --- Mock all external dependencies before importing the module under test ---

vi.mock('../../../utils/loggerConfig', () => ({
  createLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
  }),
  createErrorMetadata: vi.fn((e: unknown) => e),
}));

vi.mock('../../../utils/clientDebugState', () => ({
  debugState: {
    isEnabled: () => false,
    isDataDebugEnabled: () => false,
  },
}));

vi.mock('../../settingsStore', () => ({
  useSettingsStore: {
    getState: () => ({ settings: {} }),
  },
}));

vi.mock('../../../features/graph/managers/graphDataManager', () => ({
  graphDataManager: {
    getGraphType: () => 'knowledge',
    updateNodePositions: vi.fn().mockResolvedValue(undefined),
  },
}));

vi.mock('../../../features/analytics/store/nodeAnalyticsStore', () => ({
  nodeAnalyticsStore: { ingest: vi.fn() },
}));

vi.mock('../../transientBeamStore', () => ({
  pushTransientBeams: vi.fn(),
}));

vi.mock('../connectionManager', () => ({
  emit: vi.fn(),
  notifyBinaryMessageHandlers: vi.fn(),
}));

vi.mock('../../../utils/BatchQueue', () => ({
  NodePositionBatchQueue: class {},
  createWebSocketBatchProcessor: vi.fn(),
}));

vi.mock('../../../utils/validation', () => ({
  validateNodePositions: () => ({ valid: true, errors: [] }),
  createValidationMiddleware: vi.fn(),
}));

vi.mock('../../../services/BinaryWebSocketProtocol', () => ({
  // Mirrors the live tag space (frameTypes.ts). AGENT_ACTION is the server tag.
  MessageType: {
    GRAPH_UPDATE: 0x01,
    VOICE_DATA: 0x02,
    POSITION_UPDATE: 0x10,
    AGENT_POSITIONS: 0x11,
    AGENT_STATE_FULL: 0x20,
    AGENT_ACTION: 0x23,
    CONTROL_BITS: 0x30,
    BROADCAST_ACK: 0x34,
  },
  GraphTypeFlag: { KNOWLEDGE_GRAPH: 0, ONTOLOGY: 1 },
  binaryProtocol: {
    parseHeader: vi.fn(),
    extractPayload: vi.fn(),
    decodeAgentActions: vi.fn(() => []),
  },
}));

import { processBinaryData } from '../binaryProtocol';
import { binaryProtocol } from '../../../services/BinaryWebSocketProtocol';
import { emit } from '../connectionManager';
import { pushTransientBeams } from '../../transientBeamStore';

const AGENT_ACTION_TAG = 0x23;

/** A bare-tag agent-action frame: `[0x23][count:u16][…]`, padded to `byteLength`. */
function agentActionFrame(byteLength: number): ArrayBuffer {
  const buf = new ArrayBuffer(byteLength);
  new DataView(buf).setUint8(0, AGENT_ACTION_TAG);
  return buf;
}

const noopGet = () => ({ socket: null });
const noopSet = () => {};

describe('ADR-2099: agent-action 0x23 frame routing', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    (binaryProtocol.decodeAgentActions as ReturnType<typeof vi.fn>).mockReturnValue([]);
  });

  it('decodes a tagged 0x23 frame by peeling exactly one byte, never the framed header', async () => {
    const frame = agentActionFrame(64);

    await processBinaryData(frame, noopGet, noopSet);

    expect(binaryProtocol.decodeAgentActions).toHaveBeenCalledTimes(1);
    const payload = (binaryProtocol.decodeAgentActions as ReturnType<typeof vi.fn>).mock.calls[0][0];
    // 1-byte tag peeled — NOT the 6-byte MESSAGE_HEADER_SIZE.
    expect(payload.byteLength).toBe(frame.byteLength - 1);
  });

  it('never routes an agent-action frame through parseHeader/extractPayload', async () => {
    await processBinaryData(agentActionFrame(64), noopGet, noopSet);

    // The deleted `case MessageType.AGENT_ACTION` was the only extractPayload
    // caller for this tag; reaching either of these means the dead branch is back.
    expect(binaryProtocol.parseHeader).not.toHaveBeenCalled();
    expect(binaryProtocol.extractPayload).not.toHaveBeenCalled();
  });

  it('holds the no-framed-path invariant across every frame size', async () => {
    for (const size of [1, 17, 18, 19, 128, 4096]) {
      vi.clearAllMocks();
      await processBinaryData(agentActionFrame(size), noopGet, noopSet);
      expect(binaryProtocol.extractPayload).not.toHaveBeenCalled();
    }
  });

  it('fans decoded actions out to both sinks', async () => {
    const actions = [{ agent_id: 'a1', action_type: 1 }];
    (binaryProtocol.decodeAgentActions as ReturnType<typeof vi.fn>).mockReturnValue(actions);

    await processBinaryData(agentActionFrame(64), noopGet, noopSet);

    expect(emit).toHaveBeenCalledWith('agent-action', actions);
    expect(pushTransientBeams).toHaveBeenCalledWith(actions);
  });

  it('dispatches nothing for a runt frame below the minimum batch size', async () => {
    await processBinaryData(agentActionFrame(17), noopGet, noopSet);

    expect(binaryProtocol.decodeAgentActions).not.toHaveBeenCalled();
    expect(emit).not.toHaveBeenCalledWith('agent-action', expect.anything());
    expect(pushTransientBeams).not.toHaveBeenCalled();
  });

  it('emits nothing when the decoder yields an empty batch', async () => {
    (binaryProtocol.decodeAgentActions as ReturnType<typeof vi.fn>).mockReturnValue([]);

    await processBinaryData(agentActionFrame(64), noopGet, noopSet);

    expect(binaryProtocol.decodeAgentActions).toHaveBeenCalledTimes(1);
    expect(pushTransientBeams).not.toHaveBeenCalled();
  });

  it('fails closed on an unknown lead byte — never decoded as agent actions (ADR-2078)', async () => {
    const buf = new ArrayBuffer(64);
    new DataView(buf).setUint8(0, 0x99); // not V3, not V5, not a known tag
    (binaryProtocol.parseHeader as ReturnType<typeof vi.fn>).mockReturnValue(null);

    await processBinaryData(buf, noopGet, noopSet);

    expect(binaryProtocol.decodeAgentActions).not.toHaveBeenCalled();
    expect(pushTransientBeams).not.toHaveBeenCalled();
  });

  it('does not synthesise agent actions from an unsupported framed header version', async () => {
    const buf = new ArrayBuffer(64);
    new DataView(buf).setUint8(0, 0x01); // GRAPH_UPDATE tag, bogus version byte
    new DataView(buf).setUint8(1, 4);
    (binaryProtocol.parseHeader as ReturnType<typeof vi.fn>).mockReturnValue({
      type: 0x01,
      version: 4,
      payloadLength: 0,
    });
    (binaryProtocol.extractPayload as ReturnType<typeof vi.fn>).mockReturnValue(new ArrayBuffer(0));

    await processBinaryData(buf, noopGet, noopSet);

    expect(binaryProtocol.decodeAgentActions).not.toHaveBeenCalled();
  });
});
