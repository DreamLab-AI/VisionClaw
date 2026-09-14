// REC-2 / D3 (PRD-023 WP-4): pure logic for the control-centre broker case
// queue and the ambient ACSP indicator.
//
// Renderer-free and DOM-free so the wire parsing (broker:new_case /
// broker:case_decided) and the open-case bookkeeping are unit-testable without a
// live socket. The stateful consumer that fetches `/api/broker/inbox`, wires the
// socket and calls the decide route is `useBrokerCaseQueue`.

/**
 * Generation provenance the proposing agent recorded for a proposal. Every
 * member is optional and is rendered ONLY when present: EXP-AC-002's honesty
 * rule is that absence renders as absence, never as a default (the D7 rule).
 */
export interface CaseProvenance {
  /** The model that generated the proposal, when one is recorded. */
  model?: string;
  /** The source text the proposal was drawn from, when recorded. */
  sourceExcerpt?: string;
  /** The generator's own confidence — present only when a model produced one. */
  confidence?: number;
}

/** A broker case as the control centre renders it. */
export interface CaseView {
  id: string;
  title: string;
  category: string;
  status: 'pending' | 'claimed' | 'decided';
  targetPath?: string;
  content?: string;
  proposedBy?: string;
  /**
   * Case-row creation instant (epoch ms), from the inbox row's `created_at_ms`.
   * FR6.5: the queue sorts oldest first and shows age, so a stalled case is
   * visible rather than silently buried. `undefined` when the row carries no
   * timestamp — the queue then sorts it last rather than inventing an age.
   */
  createdAt?: number;
  /** PROV-O proposal URN (owner-scoped, content-addressed) when attributed. */
  proposalUrn?: string;
  /** The agent's reasoning trailer, as the agent wrote it. */
  reasoningSummary?: string;
  /** Content hash of the agent's reasoning, for tamper-evident review. */
  reasoningHash?: string;
  /**
   * The complete proposal payload as served, rendered pretty-printed and
   * unclipped so the reviewer judges the actual proposed change (EXP-AC-002:
   * a decision taken without the proposal in view is vacuous verification).
   */
  proposal?: unknown;
  /** The agent's SELF-DECLARED risk tier. Telemetry; gates the rationale. */
  tier?: string;
  /** The agent's SELF-ASSESSED confidence. Present only when the agent produced one. */
  confidence?: number;
  /** Generation provenance, when the proposal recorded any. */
  provenance?: CaseProvenance;
}

/** The two broker events P0 (`services/broker_events.rs`) publishes. */
export type BrokerCaseEvent =
  | { kind: 'new_case'; caseId: string; title: string; category: string }
  | { kind: 'case_decided'; caseId: string; action: string; decisionId?: string };

interface RawWsMessage {
  type?: string;
  channel?: string;
  payload?: Record<string, unknown>;
}

function payStr(p: Record<string, unknown> | undefined, key: string): string | undefined {
  const v = p?.[key];
  return typeof v === 'string' && v.length > 0 ? v : undefined;
}

/**
 * Parse an inbound WS text message into a broker case event, or `null` when the
 * message is anything else (the same socket multiplexes every frame, so the
 * queue must ignore non-broker traffic).
 */
export function parseBrokerEvent(message: unknown): BrokerCaseEvent | null {
  if (!message || typeof message !== 'object') return null;
  const m = message as RawWsMessage;
  const caseId = payStr(m.payload, 'caseId');
  if (!caseId) return null;

  if (m.type === 'broker:new_case') {
    return {
      kind: 'new_case',
      caseId,
      title: payStr(m.payload, 'title') ?? caseId,
      category: payStr(m.payload, 'category') ?? 'knowledge_enrichment',
    };
  }
  if (m.type === 'broker:case_decided') {
    return {
      kind: 'case_decided',
      caseId,
      action: payStr(m.payload, 'action') ?? 'decided',
      decisionId: payStr(m.payload, 'decisionId'),
    };
  }
  return null;
}

/** The subset of an `/api/broker/inbox` case object the queue consumes. */
export interface InboxCase {
  id: string;
  category?: string;
  status?: string;
  metadata?: Record<string, unknown> | null;
  /** Case-row creation instant in epoch ms (`created_at_ms` on the wire). */
  created_at_ms?: number;
}

function normaliseStatus(s: string | undefined): CaseView['status'] {
  if (s === 'claimed') return 'claimed';
  if (s === 'decided') return 'decided';
  return 'pending';
}

/**
 * A finite number, or `undefined`. Used for every numeric field the agent MAY
 * have produced: a missing or non-finite value stays absent rather than
 * collapsing to a default the agent never authored (EXP-AC-002 counter-example
 * "confidence: 0.5 appearing for a case where no model produced a confidence").
 */
function num(v: unknown): number | undefined {
  return typeof v === 'number' && Number.isFinite(v) ? v : undefined;
}

/** Project the `provenance` sub-bag, or `undefined` when it carries nothing. */
function toProvenance(v: unknown): CaseProvenance | undefined {
  if (!v || typeof v !== 'object') return undefined;
  const p = v as Record<string, unknown>;
  const str = (k: string) => (typeof p[k] === 'string' && (p[k] as string).length > 0 ? (p[k] as string) : undefined);
  const out: CaseProvenance = {
    model: str('model'),
    sourceExcerpt: str('source_excerpt') ?? str('sourceExcerpt'),
    confidence: num(p.confidence),
  };
  // Absence renders as absence: an all-empty provenance bag is no provenance.
  return out.model === undefined && out.sourceExcerpt === undefined && out.confidence === undefined
    ? undefined
    : out;
}

/** Project an inbox case into the render shape. */
export function toCaseView(c: InboxCase): CaseView {
  const meta = (c.metadata ?? {}) as Record<string, unknown>;
  const str = (k: string) => {
    const v = meta[k];
    return typeof v === 'string' && v.length > 0 ? v : undefined;
  };
  const targetPath = str('target_path');
  return {
    id: c.id,
    title: targetPath ?? c.id,
    category: c.category ?? 'knowledge_enrichment',
    status: normaliseStatus(c.status),
    targetPath,
    content: str('content'),
    proposedBy: str('proposed_by') ?? str('agent_did'),
    createdAt: num(c.created_at_ms),
    proposalUrn: str('proposal_urn'),
    reasoningSummary: str('reasoning_summary'),
    reasoningHash: str('reasoning_hash'),
    // The whole metadata bag IS the proposal payload the bridge serves; keep it
    // intact so the card can render it in full rather than a curated subset.
    proposal: c.metadata ?? undefined,
    tier: str('risk_tier') ?? str('tier'),
    confidence: num(meta.confidence),
    provenance: toProvenance(meta.provenance),
  };
}

// ---------------------------------------------------------------------------
// FR2.3 rationale gate (EXP-AC-002)
// ---------------------------------------------------------------------------

/**
 * Minimum length of a human rationale on a tier that requires one. Short enough
 * that a real sentence clears it, long enough that a keystroke does not.
 */
export const MIN_RATIONALE_CHARS = 20;

/**
 * Does this effective tier require a typed human rationale? `high` and
 * `critical` do; everything else — including an unlabelled case — does not.
 */
export function rationaleRequired(tier: string | undefined): boolean {
  const t = tier?.trim().toLowerCase();
  return t === 'high' || t === 'critical';
}

/**
 * May a decision be published? On a rationale-requiring tier, only once the
 * human has typed at least [`MIN_RATIONALE_CHARS`] non-whitespace-padded
 * characters. The text is never supplied by the UI — it is published verbatim.
 */
export function canPublishDecision(tier: string | undefined, rationale: string): boolean {
  if (!rationaleRequired(tier)) return true;
  return rationale.trim().length >= MIN_RATIONALE_CHARS;
}

// ---------------------------------------------------------------------------
// FR6.5 ageing and ordering (EXP-AC-006)
// ---------------------------------------------------------------------------

/**
 * Oldest case first, so the longest-waiting judgment is the one the reviewer
 * sees. Cases with no recorded `createdAt` sort last (an unknown age is not an
 * infinite one). Returns a NEW array; the input is untouched.
 */
export function sortOldestFirst(views: readonly CaseView[]): CaseView[] {
  return [...views].sort((a, b) => {
    if (a.createdAt === undefined && b.createdAt === undefined) return 0;
    if (a.createdAt === undefined) return 1;
    if (b.createdAt === undefined) return -1;
    return a.createdAt - b.createdAt;
  });
}

const MINUTE_MS = 60 * 1000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;

/**
 * A compact age label in the largest whole unit (`4d`, `3h`, `5m`), or
 * `just now` under a minute. `undefined` when the case carries no creation
 * instant — the card then shows no age rather than a fabricated one.
 */
export function caseAgeLabel(createdAt: number | undefined, now: number): string | undefined {
  if (createdAt === undefined) return undefined;
  const age = Math.max(0, now - createdAt);
  if (age >= DAY_MS) return `${Math.floor(age / DAY_MS)}d`;
  if (age >= HOUR_MS) return `${Math.floor(age / HOUR_MS)}h`;
  if (age >= MINUTE_MS) return `${Math.floor(age / MINUTE_MS)}m`;
  return 'just now';
}

/**
 * The open (not-yet-decided) case ids from an inbox snapshot — the count the
 * ambient ACSP indicator shows.
 */
export function openCaseIds(cases: InboxCase[]): Set<string> {
  const open = new Set<string>();
  for (const c of cases) {
    if (normaliseStatus(c.status) !== 'decided') open.add(c.id);
  }
  return open;
}

/**
 * Fold a broker event into the open-case set: a new case opens, a decided case
 * closes. Returns a NEW set (never mutates the input) so React state updates
 * stay referentially honest.
 */
export function applyBrokerEvent(openIds: ReadonlySet<string>, event: BrokerCaseEvent): Set<string> {
  const next = new Set(openIds);
  if (event.kind === 'new_case') next.add(event.caseId);
  else if (event.kind === 'case_decided') next.delete(event.caseId);
  return next;
}
