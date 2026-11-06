/**
 * Build the node table's columns and its eligible-row mask (ADR-2184 D1).
 *
 * Pure and worker-safe: it imports only the table schema, the analytics layout
 * and the node-filter predicates. `nodeTable.worker.ts` runs it off the main
 * thread; `tableBuilder.ts` falls back to calling it directly.
 */
import type { Node as GraphNode } from '../managers/graphDataManager';
import { buildNodeColumns, type NodeColumns } from './tableSchema';
import { isRemovedByScope, resolveNodeFilter, type NodeFilterInput } from './nodeFilter';

export interface TableInput {
  nodes: readonly GraphNode[];
  /** id → render index (defaults to the order of `nodes`, which is how the shared index is built). */
  indexMap?: ReadonlyMap<string, number>;
  /** Render-index-aligned analytics buffer (stride 5). */
  analytics?: Float32Array | null;
  /** Node-filter settings: their population scope decides which rows count. */
  nodeFilter?: NodeFilterInput | null;
}

export interface BuiltTable {
  table: NodeColumns;
  /** 1 where the row is delivered and in the render set (population scope applied). */
  eligible: Uint8Array;
}

export function buildTable(input: TableInput): BuiltTable {
  const indexMap = input.indexMap ?? new Map(input.nodes.map((n, i) => [String(n.id), i]));
  const table = buildNodeColumns(input.nodes, { indexMap, analytics: input.analytics ?? null });
  const eligible = new Uint8Array(table.rowCount);
  const scope = resolveNodeFilter(input.nodeFilter);
  for (const node of input.nodes) {
    const i = indexMap.get(String(node.id));
    if (i !== undefined && i < eligible.length && !isRemovedByScope(node, scope)) eligible[i] = 1;
  }
  return { table, eligible };
}

/**
 * The fields of a node the table reads (id, label, metadata, and the legacy
 * top-level type fallbacks of the population scope): what crosses to the worker.
 */
export function leanNode(n: GraphNode): GraphNode {
  const raw = n as unknown as { type?: string; nodeType?: string };
  const out: Record<string, unknown> = { id: n.id, label: n.label, metadata: n.metadata };
  if (raw.type !== undefined) out.type = raw.type;
  if (raw.nodeType !== undefined) out.nodeType = raw.nodeType;
  return out as unknown as GraphNode;
}

/** The ArrayBuffers of a built table, for a zero-copy transfer. */
export function transferablesOf(built: BuiltTable): ArrayBuffer[] {
  const out = new Set<ArrayBuffer>([built.table.present.buffer as ArrayBuffer, built.eligible.buffer as ArrayBuffer]);
  for (const c of Object.values(built.table.columns)) {
    if (c.type === 'categorical') out.add(c.codes.buffer as ArrayBuffer);
    else if (c.type !== 'text') out.add(c.values.buffer as ArrayBuffer);
  }
  return [...out];
}
