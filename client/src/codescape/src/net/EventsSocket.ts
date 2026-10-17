/** `/ws/events`: typed JSON ServerEvent in, ClientEvent out. */
import { parseServerEvent, type ClientEvent, type ServerEvent, type ServerEventType } from '../protocol';
import { ReconnectingSocket, type SocketStatus } from './ReconnectingSocket';

type Handler<K extends ServerEventType> = (e: Extract<ServerEvent, { type: K }>) => void;

export class EventsSocket {
  private readonly sock: ReconnectingSocket;
  private readonly handlers = new Map<string, Set<(e: ServerEvent) => void>>();
  private readonly any = new Set<(e: ServerEvent) => void>();
  private readonly queue: ClientEvent[] = [];

  constructor(url: string, onStatus: (s: SocketStatus, attempts: number, detail?: string) => void, private readonly hello: () => ClientEvent[]) {
    this.sock = new ReconnectingSocket(
      url,
      {
        onStatus,
        onOpen: () => {
          for (const e of this.hello()) this.sock.send(JSON.stringify(e));
          while (this.queue.length) this.sock.send(JSON.stringify(this.queue.shift()));
        },
        onMessage: (data) => {
          if (typeof data !== 'string') return;
          const e = parseServerEvent(data);
          if (!e) return;
          this.any.forEach((f) => f(e));
          this.handlers.get(e.type)?.forEach((f) => f(e));
        },
      },
      'arraybuffer',
    );
  }

  start(): void {
    this.sock.start();
  }

  stop(): void {
    this.sock.stop();
  }

  on<K extends ServerEventType>(type: K, fn: Handler<K>): () => void {
    let set = this.handlers.get(type);
    if (!set) this.handlers.set(type, (set = new Set()));
    const f = fn as (e: ServerEvent) => void;
    set.add(f);
    return () => set!.delete(f);
  }

  onAny(fn: (e: ServerEvent) => void): () => void {
    this.any.add(fn);
    return () => this.any.delete(fn);
  }

  /** Send now, or queue (bounded) until the socket opens. View/focus updates replace stale queued ones. */
  send(e: ClientEvent): void {
    if (this.sock.send(JSON.stringify(e))) return;
    if (e.type === 'view' || e.type === 'focus') {
      const k = this.queue.findIndex((q) => q.type === e.type);
      if (k >= 0) this.queue.splice(k, 1);
    }
    this.queue.push(e);
    if (this.queue.length > 64) this.queue.shift();
  }
}
