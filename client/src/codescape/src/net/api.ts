/** REST surface of codescape-server (contracts §codescape-server). */
import type { GraphSnapshot, QueryResult, Tier } from '../protocol';

export interface Health {
  engine: string;
  repo: string;
  revision: number;
  nodes: number;
  edges: number;
  settled: boolean;
  version: string;
}

export interface SearchHit {
  id: number;
  label: string;
  score: number;
  kind?: string;
  path?: string | null;
}

async function json<T>(res: Response): Promise<T> {
  if (!res.ok) {
    const body = await res.text().catch(() => '');
    throw new Error(`${res.status} ${res.statusText}${body ? `: ${body.slice(0, 200)}` : ''}`);
  }
  return (await res.json()) as T;
}

export class Api {
  constructor(private readonly base: string) {}

  health(signal?: AbortSignal): Promise<Health> {
    return fetch(`${this.base}/api/health`, { signal }).then((r) => json<Health>(r));
  }

  snapshot(tier: Tier, signal?: AbortSignal): Promise<GraphSnapshot> {
    return fetch(`${this.base}/api/graph/snapshot?tier=${tier}`, { signal }).then((r) => json<GraphSnapshot>(r));
  }

  /** Search; accepts either `[{id,label,score}]` or `[[id, score]]` server shapes. */
  async search(q: string, limit = 20, signal?: AbortSignal): Promise<SearchHit[]> {
    const raw = await fetch(`${this.base}/api/graph/search?q=${encodeURIComponent(q)}&limit=${limit}`, { signal }).then((r) => json<unknown>(r));
    const list = Array.isArray(raw) ? raw : Array.isArray((raw as { results?: unknown }).results) ? (raw as { results: unknown[] }).results : [];
    return list.map((h): SearchHit => {
      if (Array.isArray(h)) return { id: Number(h[0]), score: Number(h[1] ?? 0), label: String(h[2] ?? h[0]) };
      const o = h as Partial<SearchHit> & { node?: number };
      return { id: Number(o.id ?? o.node), label: String(o.label ?? o.id), score: Number(o.score ?? 0), kind: o.kind, path: o.path };
    });
  }

  query(text: string, signal?: AbortSignal): Promise<QueryResult> {
    return fetch(`${this.base}/api/graph/query`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ text }),
      signal,
    }).then((r) => json<QueryResult>(r));
  }
}
