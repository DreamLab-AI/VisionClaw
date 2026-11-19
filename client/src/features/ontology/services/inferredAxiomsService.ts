/**
 * Inferred-axioms read-model — the reasoning-report data the InferencePanel
 * surfaces, and the inferred-edge set the renderer differentiates.
 *
 * Source of truth is the server-side inferred named graph
 * `urn:ngm:graph:ontology:inferred` (ADR-099 D3/D4). The Rust reasoner
 * (whelk EL) materialises inferred axioms with provenance; this client only
 * READS them. If the backend contract is not live yet, every call returns a
 * cleanly-typed empty result — we never fabricate inferred triples.
 *
 * Expected backend contract (handed to the Rust agents — see report):
 *   GET /api/ontology/inferred
 *   Response (application/json):
 *     {
 *       "namedGraph": "urn:ngm:graph:ontology:inferred",
 *       "runId": "whelk-2026-06-05T...",        // prov:wasGeneratedBy (optional)
 *       "triples": [
 *         {
 *           "subject":   "vc:domain/slug",      // canonical IRI
 *           "predicate": "rdfs:subClassOf",
 *           "object":    "vc:domain/other",
 *           "sourceNodeId": "123",              // optional GPU node id (string)
 *           "targetNodeId": "456",              // optional GPU node id (string)
 *           "justification": "subsumption",      // class of inference
 *           "confidence": 1.0                    // optional 0..1
 *         }
 *       ]
 *     }
 *   Non-200 or missing endpoint → treated as "no inferred data yet" (empty,
 *   not an error toast), so the UI degrades gracefully before the backend lands.
 */

import { unifiedApiClient } from '@/services/api/UnifiedApiClient';
import { createLogger } from '@/utils/loggerConfig';

const logger = createLogger('inferredAxiomsService');

/** The inferred named graph IRI (ADR-099 D3). */
export const INFERRED_NAMED_GRAPH = 'urn:ngm:graph:ontology:inferred';

/** Canonical endpoint path the backend reasoning agent must implement.
 *  Relative to the unifiedApiClient base ('/api'); a leading '/api' here doubles to 404. */
export const INFERRED_ENDPOINT = '/ontology/inferred';

/** A single inferred triple with provenance, as surfaced in the report. */
export interface InferredTriple {
  subject: string;
  predicate: string;
  object: string;
  /** GPU node id of the subject, when resolvable (string-coerced u32). */
  sourceNodeId?: string;
  /** GPU node id of the object, when resolvable (string-coerced u32). */
  targetNodeId?: string;
  /** Inference justification class (e.g. "subsumption", "equivalence"). */
  justification?: string;
  /** Reasoner confidence 0..1, when provided. */
  confidence?: number;
}

/** The reasoning report read-model the InferencePanel renders. */
export interface ReasoningReport {
  namedGraph: string;
  runId?: string;
  triples: InferredTriple[];
  /** True number of inferred axioms on the server (not the size of what was fetched). */
  count: number;
  /** True when fewer axioms were fetched than `count` (page cap reached). */
  partial?: boolean;
}

/** The empty report — used as the typed loading / not-yet-materialised state. */
export const EMPTY_REASONING_REPORT: ReasoningReport = {
  namedGraph: INFERRED_NAMED_GRAPH,
  triples: [],
  count: 0,
};

/** A raw entry from the backend. Two shapes are tolerated:
 *  (a) flat   `{subject, predicate, object, ...}` (the originally-specified contract), or
 *  (b) reified `{s, p, o}` provenance triples where each inferred axiom is a
 *      `urn:ngm:axiom:HASH` node carrying ngm:subject/ngm:object/ngm:axiomType
 *      (the format the Rust whelk materialiser actually emits — ADR-099 D3). */
interface RawEntry extends Partial<InferredTriple> {
  s?: string;
  p?: string;
  o?: string;
}

interface RawInferredResponse {
  namedGraph?: string;
  runId?: string;
  triples?: RawEntry[];
  /** Axioms in the whole (caller-visible) inferred graph. */
  total?: number;
  /** More axioms exist beyond this page. */
  truncated?: boolean;
}

/** Axioms requested per page. */
export const INFERRED_PAGE_SIZE = 500;
/** Page ceiling: bounds a pathological graph at INFERRED_PAGE_SIZE x this many axioms. */
export const INFERRED_MAX_PAGES = 40;

const NGM = 'https://narrativegoldmine.com/ns/v1#';

/** OWL axiom-type literal → predicate IRI for the reconstructed inferred edge. */
function axiomTypeToPredicate(axiomType?: string): string {
  switch (axiomType) {
    case 'SubClassOf':
      return 'rdfs:subClassOf';
    case 'EquivalentClass':
    case 'EquivalentClasses':
      return 'owl:equivalentClass';
    case 'DisjointWith':
    case 'DisjointClasses':
      return 'owl:disjointWith';
    default:
      return axiomType || 'rdfs:subClassOf';
  }
}

/** True when the payload is the reified provenance graph (s/p/o entries). */
function isReified(entries: RawEntry[]): boolean {
  return entries.some((e) => typeof e.s === 'string' && typeof e.p === 'string' && typeof e.o === 'string');
}

/** Collapse reified provenance triples into one InferredTriple per axiom node.
 *  Each `urn:ngm:axiom:*` subject accrues its ngm:subject / ngm:object /
 *  ngm:axiomType / ngm:derivation / ngm:confidence into a single edge. */
function reconstructFromReified(entries: RawEntry[]): InferredTriple[] {
  const byAxiom = new Map<string, Record<string, string>>();
  for (const e of entries) {
    if (typeof e.s !== 'string' || typeof e.p !== 'string' || typeof e.o !== 'string') continue;
    let bucket = byAxiom.get(e.s);
    if (!bucket) {
      bucket = {};
      byAxiom.set(e.s, bucket);
    }
    bucket[e.p] = e.o;
  }

  const out: InferredTriple[] = [];
  for (const bucket of byAxiom.values()) {
    const subject = bucket[`${NGM}subject`];
    const object = bucket[`${NGM}object`];
    if (!subject || !object) continue; // not an axiom-shaped node (e.g. the run node)
    const confidence = bucket[`${NGM}confidence`];
    out.push({
      subject,
      object,
      predicate: axiomTypeToPredicate(bucket[`${NGM}axiomType`]),
      justification: bucket[`${NGM}derivation`] || 'inferred',
      confidence: confidence != null && confidence !== '' ? Number(confidence) : undefined,
    });
  }
  return out;
}

/** Map the originally-specified flat `{subject,predicate,object}` shape. */
function mapFlat(entries: RawEntry[]): InferredTriple[] {
  return entries
    .filter((t) => typeof t.subject === 'string' && typeof t.predicate === 'string' && typeof t.object === 'string')
    .map((t) => ({
      subject: String(t.subject),
      predicate: String(t.predicate),
      object: String(t.object),
      sourceNodeId: t.sourceNodeId != null ? String(t.sourceNodeId) : undefined,
      targetNodeId: t.targetNodeId != null ? String(t.targetNodeId) : undefined,
      justification: t.justification,
      confidence: typeof t.confidence === 'number' ? t.confidence : undefined,
    }));
}

/** Fetch one page; throws when the endpoint is absent. */
async function fetchPage(offset: number): Promise<RawInferredResponse> {
  const response = await unifiedApiClient.get<RawInferredResponse>(
    `${INFERRED_ENDPOINT}?offset=${offset}&limit=${INFERRED_PAGE_SIZE}`,
    { timeout: 15000 },
  );
  // unifiedApiClient unwraps the HTTP body into `.data`. Some endpoints wrap a
  // second time ({ data: {...} }); tolerate both without assuming either.
  const body = response.data as RawInferredResponse & { data?: RawInferredResponse };
  return body?.data ?? body ?? {};
}

/**
 * Fetch the materialised inferred axioms, page by page, until the server's
 * `total` is reached or INFERRED_MAX_PAGES. `count` is the server's total;
 * `partial` says the page ceiling stopped the fetch short. Returns
 * EMPTY_REASONING_REPORT (not a throw) when the endpoint is absent or returns
 * non-200 — the report simply shows "no inferred axioms yet".
 */
export async function fetchReasoningReport(): Promise<ReasoningReport> {
  try {
    const rawEntries: RawEntry[] = [];
    let first: RawInferredResponse | undefined;
    let total: number | undefined;
    let more = true;
    for (let page = 0; more && page < INFERRED_MAX_PAGES; page++) {
      // `offset` counts axioms (the server pages whole axiom records).
      const data = await fetchPage(page * INFERRED_PAGE_SIZE);
      first ??= data;
      if (typeof data.total === 'number') total = data.total;
      rawEntries.push(...(Array.isArray(data.triples) ? data.triples : []));
      more = data.truncated === true;
    }

    // The whelk materialiser emits reified provenance triples; collapse them to
    // one edge per inferred axiom so the count reflects axioms, not raw triples.
    const triples: InferredTriple[] = isReified(rawEntries)
      ? reconstructFromReified(rawEntries)
      : mapFlat(rawEntries);

    const count = Math.max(total ?? triples.length, triples.length);
    logger.info(`Reasoning report: ${triples.length} of ${count} inferred axioms (${rawEntries.length} raw triples)`);
    return {
      namedGraph: first?.namedGraph || INFERRED_NAMED_GRAPH,
      runId: first?.runId,
      triples,
      count,
      partial: triples.length < count,
    };
  } catch (err: any) {
    // The inferred endpoint may not exist yet (WS-2 Rust work pending). Degrade
    // to an empty report rather than surfacing a noisy error — see file header.
    logger.debug('Inferred-axioms endpoint unavailable; returning empty report:', err?.message);
    return EMPTY_REASONING_REPORT;
  }
}
