/** WebSocket with exponential-backoff reconnect and status callbacks. */
import { backoffDelay } from './config';

export type SocketStatus = 'connecting' | 'open' | 'closed' | 'error';

export interface SocketHooks {
  onStatus?(status: SocketStatus, attempts: number, detail?: string): void;
  onOpen?(ws: WebSocket): void;
  onMessage(data: ArrayBuffer | string): void;
}

export class ReconnectingSocket {
  private ws: WebSocket | null = null;
  private attempts = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private stopped = false;

  constructor(
    readonly url: string,
    private readonly hooks: SocketHooks,
    private readonly binaryType: BinaryType = 'arraybuffer',
  ) {}

  start(): void {
    this.stopped = false;
    this.connect();
  }

  stop(): void {
    this.stopped = true;
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    const ws = this.ws;
    this.ws = null;
    if (ws && ws.readyState <= WebSocket.OPEN) ws.close(1000, 'client stop');
  }

  get open(): boolean {
    return this.ws?.readyState === WebSocket.OPEN;
  }

  send(data: string | ArrayBufferLike | ArrayBufferView): boolean {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return false;
    this.ws.send(data as ArrayBuffer);
    return true;
  }

  private connect(): void {
    if (this.stopped) return;
    this.hooks.onStatus?.('connecting', this.attempts);
    let ws: WebSocket;
    try {
      ws = new WebSocket(this.url);
    } catch (e) {
      this.hooks.onStatus?.('error', this.attempts, String(e));
      this.schedule();
      return;
    }
    ws.binaryType = this.binaryType;
    this.ws = ws;
    ws.onopen = () => {
      this.attempts = 0;
      this.hooks.onStatus?.('open', 0);
      this.hooks.onOpen?.(ws);
    };
    ws.onmessage = (ev) => this.hooks.onMessage(ev.data as ArrayBuffer | string);
    ws.onerror = () => this.hooks.onStatus?.('error', this.attempts, 'socket error');
    ws.onclose = (ev) => {
      if (this.ws !== ws) return;
      this.ws = null;
      this.hooks.onStatus?.('closed', this.attempts, ev.reason || `code ${ev.code}`);
      this.schedule();
    };
  }

  private schedule(): void {
    if (this.stopped) return;
    const delay = backoffDelay(this.attempts++);
    this.timer = setTimeout(() => this.connect(), delay);
  }
}
