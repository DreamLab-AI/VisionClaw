/**
 * `/ws/positions`: binary V5 frames in, 21-byte 0x34 acks out. Frames go to the
 * FramePipeline (worker); the ack is sent once the worker reports the frame applied,
 * which is what lets the server pace partial frames (ADR-2141 semantics).
 */
import { decodeAgentActions, encodeAck, OPCODE_AGENT_ACTION, PROTOCOL_V3, PROTOCOL_V5 } from '../protocol';
import type { Applied, FramePipeline } from './FramePipeline';
import { ReconnectingSocket, type SocketStatus } from './ReconnectingSocket';

export interface PositionsHooks {
  onStatus(status: SocketStatus, attempts: number, detail?: string): void;
  onApplied(r: Applied): void;
  onAgentActions?(actions: ReturnType<typeof decodeAgentActions>): void;
}

export class PositionsSocket {
  private readonly sock: ReconnectingSocket;
  private received = 0;

  constructor(url: string, private readonly pipeline: FramePipeline, private readonly hooks: PositionsHooks) {
    this.sock = new ReconnectingSocket(url, {
      onStatus: hooks.onStatus,
      onMessage: (data) => this.onMessage(data),
    });
  }

  start(): void {
    this.sock.start();
  }

  stop(): void {
    this.sock.stop();
  }

  get framesReceived(): number {
    return this.received;
  }

  /** Called by the owner when the pipeline applied a frame. */
  ack(r: Applied): void {
    if (r.seq > 0) this.sock.send(encodeAck(r.seq, r.count, Date.now()));
    this.hooks.onApplied(r);
  }

  private onMessage(data: ArrayBuffer | string): void {
    if (typeof data === 'string' || data.byteLength === 0) return;
    const op = new Uint8Array(data, 0, 1)[0];
    if (op === PROTOCOL_V5 || op === PROTOCOL_V3) {
      this.received++;
      this.pipeline.push(data);
    } else if (op === OPCODE_AGENT_ACTION) {
      this.hooks.onAgentActions?.(decodeAgentActions(data));
    }
  }
}
