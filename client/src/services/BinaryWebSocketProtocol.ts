
import { createLogger } from '../utils/loggerConfig';

// Re-export everything so callers keep their existing import paths.
export * from './binaryProtocol/frameTypes';
export * from './binaryProtocol/agentMessages';
export * from './binaryProtocol/ssspVoice';
export * from './binaryProtocol/backpressure';

import {
  PROTOCOL_VERSION,
  SUPPORTED_HEADER_VERSIONS,
  MessageType,
  GraphTypeFlag,
  ControlFlags,
  MESSAGE_HEADER_SIZE,
  GRAPH_UPDATE_HEADER_SIZE,
  AGENT_POSITION_SIZE_V2,
  AGENT_STATE_SIZE_V2,
  AGENT_POSITION_SIZE,
  AGENT_STATE_SIZE,
} from './binaryProtocol/frameTypes';
import type {
  MessageHeader,
  AgentPositionUpdate,
  AgentStateData,
  SSSPData,
  VoiceChunk,
  AgentActionEvent,
  BroadcastAckData,
} from './binaryProtocol/frameTypes';
import {
  encodePositionPayload,
  decodePositionUpdates,
  encodeAgentStatePayload,
  decodeAgentState,
  decodeAgentAction,
  decodeAgentActions,
  encodeAgentActionPayload,
} from './binaryProtocol/agentMessages';
import {
  encodeSSSPPayload,
  decodeSSSPData,
  encodeVoiceChunkPayload,
  decodeVoiceChunk,
} from './binaryProtocol/ssspVoice';
import {
  encodeBroadcastAckPayload,
  decodeBroadcastAck,
} from './binaryProtocol/backpressure';

const logger = createLogger('BinaryWebSocketProtocol');

export class BinaryWebSocketProtocol {
  private static instance: BinaryWebSocketProtocol;
  private lastPositionUpdate: number = 0;
  private positionUpdateThrottle: number = 16;
  private metadataUpdateThrottle: number = 100;
  private isUserInteracting: boolean = false;
  private pendingPositionUpdates: AgentPositionUpdate[] = [];
  private static readonly MAX_PENDING_UPDATES = 1000;
  private voiceEnabled: boolean = false;

  private constructor() {}

  public static getInstance(): BinaryWebSocketProtocol {
    if (!BinaryWebSocketProtocol.instance) {
      BinaryWebSocketProtocol.instance = new BinaryWebSocketProtocol();
    }
    return BinaryWebSocketProtocol.instance;
  }

  public createMessage(type: MessageType, payload: ArrayBuffer, graphTypeFlag?: GraphTypeFlag): ArrayBuffer {
    const isGraphUpdate = type === MessageType.GRAPH_UPDATE;
    const headerSize = isGraphUpdate ? GRAPH_UPDATE_HEADER_SIZE : MESSAGE_HEADER_SIZE;

    const totalSize = headerSize + payload.byteLength;
    const buffer = new ArrayBuffer(totalSize);
    const view = new DataView(buffer);

    // Framed-message header: [1-byte type][1-byte version][4-byte payloadLength (uint32, LE)].
    // ADR-2078: this was commented "V4 header" while the very next line writes
    // PROTOCOL_VERSION, which is PROTOCOL_V3 = 3 — the header byte has never carried 4.
    // The version byte carries the POSITION protocol version, not a header format version.
    view.setUint8(0, type);
    view.setUint8(1, PROTOCOL_VERSION);
    view.setUint32(2, payload.byteLength, true);

    if (isGraphUpdate && graphTypeFlag !== undefined) {
      view.setUint8(6, graphTypeFlag);
    }

    new Uint8Array(buffer, headerSize).set(new Uint8Array(payload));

    return buffer;
  }

  public parseHeader(buffer: ArrayBuffer): MessageHeader | null {
    if (buffer.byteLength < MESSAGE_HEADER_SIZE) {
      logger.error('Buffer too small for message header');
      return null;
    }

    const view = new DataView(buffer);
    const type = view.getUint8(0) as MessageType;
    const header: MessageHeader = {
      type,
      version: view.getUint8(1),
      payloadLength: view.getUint32(2, true)
    };

    if (type === MessageType.GRAPH_UPDATE && buffer.byteLength >= GRAPH_UPDATE_HEADER_SIZE) {
      header.graphTypeFlag = view.getUint8(6) as GraphTypeFlag;
    }

    return header;
  }

  public extractPayload(buffer: ArrayBuffer, header?: MessageHeader): ArrayBuffer {
    const isGraphUpdate = header?.type === MessageType.GRAPH_UPDATE;
    const headerSize = isGraphUpdate ? GRAPH_UPDATE_HEADER_SIZE : MESSAGE_HEADER_SIZE;

    if (buffer.byteLength <= headerSize) {
      return new ArrayBuffer(0);
    }
    return buffer.slice(headerSize);
  }

  public encodePositionUpdates(updates: AgentPositionUpdate[]): ArrayBuffer | null {
    if (!this.isUserInteracting || updates.length === 0) {
      return null;
    }

    const now = performance.now();
    if (now - this.lastPositionUpdate < this.positionUpdateThrottle) {
      this.pendingPositionUpdates.push(...updates);
      // Bound pending updates to prevent unbounded memory growth
      if (this.pendingPositionUpdates.length > BinaryWebSocketProtocol.MAX_PENDING_UPDATES) {
        this.pendingPositionUpdates.splice(0, this.pendingPositionUpdates.length - BinaryWebSocketProtocol.MAX_PENDING_UPDATES);
      }
      return null;
    }

    const allUpdates = [...this.pendingPositionUpdates, ...updates];
    this.pendingPositionUpdates = [];
    this.lastPositionUpdate = now;

    const payload = new ArrayBuffer(allUpdates.length * AGENT_POSITION_SIZE_V2);
    encodePositionPayload(allUpdates, payload);

    return this.createMessage(MessageType.POSITION_UPDATE, payload);
  }

  public decodePositionUpdates(payload: ArrayBuffer): AgentPositionUpdate[] {
    return decodePositionUpdates(payload);
  }


  public encodeAgentState(agents: AgentStateData[]): ArrayBuffer {
    const payload = encodeAgentStatePayload(agents);
    return this.createMessage(MessageType.AGENT_STATE_FULL, payload);
  }

  public decodeAgentState(payload: ArrayBuffer): AgentStateData[] {
    return decodeAgentState(payload);
  }

  public encodeSSSPData(nodes: SSSPData[]): ArrayBuffer {
    return this.createMessage(MessageType.SSSP_DATA, encodeSSSPPayload(nodes));
  }

  public decodeSSSPData(payload: ArrayBuffer): SSSPData[] {
    return decodeSSSPData(payload);
  }

  public encodeControlBits(flags: ControlFlags): ArrayBuffer {
    const payload = new ArrayBuffer(1);
    new DataView(payload).setUint8(0, flags);
    return this.createMessage(MessageType.CONTROL_BITS, payload);
  }

  public decodeControlBits(payload: ArrayBuffer): ControlFlags {
    if (payload.byteLength < 1) {
      return 0 as ControlFlags;
    }
    return new DataView(payload).getUint8(0) as ControlFlags;
  }

  public encodeVoiceChunk(chunk: VoiceChunk): ArrayBuffer {
    return this.createMessage(MessageType.VOICE_CHUNK, encodeVoiceChunkPayload(chunk));
  }

  public decodeVoiceChunk(payload: ArrayBuffer): VoiceChunk | null {
    return decodeVoiceChunk(payload);
  }

  public setUserInteracting(interacting: boolean): ArrayBuffer | null {
    if (this.isUserInteracting && !interacting) {
      this.isUserInteracting = interacting;
      logger.debug(`User interaction state: ${interacting}`);
      return this.flushPositionUpdates();
    }
    this.isUserInteracting = interacting;
    logger.debug(`User interaction state: ${interacting}`);
    return null;
  }

  /**
   * Flush any pending position updates without checking isUserInteracting.
   * Used to drain accumulated updates when interaction ends, preventing
   * orphaned updates that would leave collaborators with stale positions.
   */
  public flushPositionUpdates(): ArrayBuffer | null {
    if (this.pendingPositionUpdates.length === 0) {
      return null;
    }
    const allUpdates = [...this.pendingPositionUpdates];
    this.pendingPositionUpdates = [];
    this.lastPositionUpdate = performance.now();

    const payload = new ArrayBuffer(allUpdates.length * AGENT_POSITION_SIZE_V2);
    encodePositionPayload(allUpdates, payload);
    return this.createMessage(MessageType.POSITION_UPDATE, payload);
  }

  public configureThrottling(positionMs: number, metadataMs: number): void {
    this.positionUpdateThrottle = positionMs;
    this.metadataUpdateThrottle = metadataMs;
    logger.info(`Throttling configured: position=${positionMs}ms, metadata=${metadataMs}ms`);
  }

  public setVoiceEnabled(enabled: boolean): void {
    this.voiceEnabled = enabled;
    logger.info(`Voice communication: ${enabled ? 'enabled' : 'disabled'}`);
  }

  /**
   * Create a broadcast acknowledgement message for backpressure flow control.
   * This should be sent after client processes a position update broadcast.
   *
   * @param sequenceId - Server broadcast sequence number (from position update header)
   * @param nodesReceived - Number of nodes successfully processed
   * @returns Binary message ready to send over WebSocket
   */
  public createBroadcastAck(sequenceId: number, nodesReceived: number): ArrayBuffer {
    // The server decodes an acknowledgement as `[0x34][20-byte payload]` with NO generic message
    // header (BinaryProtocol::decode_message). This used to go through createMessage(), whose
    // six-byte `[type][version][u32 length]` header shifted the payload, so the server read
    // garbage for the sequence and no acknowledgement ever matched a chunk.
    const payload = encodeBroadcastAckPayload(sequenceId, nodesReceived);
    const frame = new Uint8Array(1 + payload.byteLength);
    frame[0] = MessageType.BROADCAST_ACK;
    frame.set(new Uint8Array(payload), 1);
    return frame.buffer;
  }

  /**
   * Decode a broadcast acknowledgement from server (for server-sent acks if needed)
   */
  public decodeBroadcastAck(payload: ArrayBuffer): BroadcastAckData | null {
    return decodeBroadcastAck(payload);
  }

  /**
   * Decode an agent action event from binary payload.
   * Used to render ephemeral connections between agent and data nodes.
   *
   * @param payload - Binary payload (excluding message type byte)
   * @returns Decoded agent action event
   */
  public decodeAgentAction(payload: ArrayBuffer): AgentActionEvent | null {
    return decodeAgentAction(payload);
  }

  /**
   * Decode a batch of agent action events.
   * Wire format: [count: u16][event1_len: u16][event1_data]...
   *
   * @param payload - Binary payload (excluding message type byte)
   * @returns Array of decoded agent action events
   */
  public decodeAgentActions(payload: ArrayBuffer): AgentActionEvent[] {
    return decodeAgentActions(payload);
  }

  /**
   * Encode an agent action event for sending to server (if needed).
   * Primarily used for testing or client-initiated actions.
   *
   * @param event - Agent action event to encode
   * @returns Binary message ready to send
   */
  public encodeAgentAction(event: AgentActionEvent): ArrayBuffer {
    return this.createMessage(MessageType.AGENT_ACTION, encodeAgentActionPayload(event));
  }

  public calculateBandwidth(agentCount: number, updateRateHz: number): {
    positionOnly: number;
    fullState: number;
    withVoice: number;
  } {
    const positionBandwidth = agentCount * AGENT_POSITION_SIZE * updateRateHz;
    const stateBandwidth = agentCount * AGENT_STATE_SIZE * updateRateHz;
    const voiceBandwidth = this.voiceEnabled ? agentCount * 8000 : 0;

    return {
      positionOnly: positionBandwidth + MESSAGE_HEADER_SIZE * updateRateHz,
      fullState: stateBandwidth + MESSAGE_HEADER_SIZE * updateRateHz,
      withVoice: stateBandwidth + voiceBandwidth + MESSAGE_HEADER_SIZE * updateRateHz
    };
  }

  public validateMessage(buffer: ArrayBuffer): boolean {
    const header = this.parseHeader(buffer);
    if (!header) return false;

    if (!SUPPORTED_HEADER_VERSIONS.includes(header.version)) {
      logger.warn(`Unsupported framed-header version: ${header.version}. Supported: ${SUPPORTED_HEADER_VERSIONS.join(', ')}`);
      return false;
    }

    const expectedSize = MESSAGE_HEADER_SIZE + header.payloadLength;
    if (buffer.byteLength !== expectedSize) {
      logger.warn(`Message size mismatch: expected ${expectedSize}, got ${buffer.byteLength}`);
      return false;
    }

    return true;
  }
}

// Export singleton instance
export const binaryProtocol = BinaryWebSocketProtocol.getInstance();

// Export utility functions for bandwidth analysis
export function estimateDataSize(agentCount: number): {
  perUpdate: number;
  perSecondAt10Hz: number;
  perSecondAt60Hz: number;
  comparison: string;
} {
  const perUpdate = agentCount * AGENT_STATE_SIZE + MESSAGE_HEADER_SIZE;
  const perSecond10Hz = perUpdate * 10;
  const perSecond60Hz = perUpdate * 60;

  const jsonEstimate = agentCount * 200;
  const comparison = perUpdate < jsonEstimate
    ? `${Math.round((1 - perUpdate/jsonEstimate) * 100)}% smaller than JSON`
    : `${Math.round((perUpdate/jsonEstimate - 1) * 100)}% larger than JSON`;

  return {
    perUpdate,
    perSecondAt10Hz: perSecond10Hz,
    perSecondAt60Hz: perSecond60Hz,
    comparison
  };
}
