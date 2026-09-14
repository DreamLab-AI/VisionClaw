// REC-2 / D3 (PRD-023 WP-4): broker event parsing + open-case bookkeeping.

import { describe, it, expect } from 'vitest';
import {
  applyBrokerEvent,
  openCaseIds,
  parseBrokerEvent,
  toCaseView,
  sortOldestFirst,
  caseAgeLabel,
  rationaleRequired,
  canPublishDecision,
  MIN_RATIONALE_CHARS,
  type InboxCase,
} from '../brokerCaseQueue';

describe('parseBrokerEvent', () => {
  it('parses a broker:new_case frame', () => {
    const ev = parseBrokerEvent({
      type: 'broker:new_case',
      channel: 'inbox',
      payload: { caseId: 'case-7', title: 'Elevate concept', category: 'knowledge_enrichment' },
    });
    expect(ev).toEqual({ kind: 'new_case', caseId: 'case-7', title: 'Elevate concept', category: 'knowledge_enrichment' });
  });

  it('parses a broker:case_decided frame', () => {
    const ev = parseBrokerEvent({
      type: 'broker:case_decided',
      channel: 'case:case-7',
      payload: { caseId: 'case-7', decisionId: 'dec-1', action: 'approve' },
    });
    expect(ev).toEqual({ kind: 'case_decided', caseId: 'case-7', action: 'approve', decisionId: 'dec-1' });
  });

  it('ignores non-broker multiplexed frames', () => {
    expect(parseBrokerEvent({ type: 'initialGraphLoad', nodes: [] })).toBeNull();
    expect(parseBrokerEvent({ type: 'broker:new_case', payload: {} })).toBeNull(); // no caseId
    expect(parseBrokerEvent(null)).toBeNull();
    expect(parseBrokerEvent('not-an-object')).toBeNull();
  });
});

describe('open-case bookkeeping', () => {
  const inbox: InboxCase[] = [
    { id: 'c1', status: 'pending' },
    { id: 'c2', status: 'claimed' },
    { id: 'c3', status: 'decided' },
  ];

  it('counts only not-yet-decided cases as open', () => {
    const open = openCaseIds(inbox);
    expect([...open].sort()).toEqual(['c1', 'c2']);
  });

  it('a new_case opens and a case_decided closes, without mutating the input set', () => {
    const start = openCaseIds(inbox); // {c1, c2}
    const afterNew = applyBrokerEvent(start, { kind: 'new_case', caseId: 'c9', title: 'x', category: 'y' });
    expect(afterNew.has('c9')).toBe(true);
    expect(start.has('c9')).toBe(false); // input untouched

    const afterDecided = applyBrokerEvent(afterNew, { kind: 'case_decided', caseId: 'c1', action: 'approve' });
    expect(afterDecided.has('c1')).toBe(false);
    expect(afterNew.has('c1')).toBe(true); // previous set untouched
  });
});

describe('toCaseView', () => {
  it('projects an inbox case, titling by target_path', () => {
    const view = toCaseView({
      id: 'case-7',
      category: 'knowledge_enrichment',
      status: 'pending',
      metadata: { target_path: 'pages/foo.md', content: 'body', proposed_by: 'did:nostr:aaaa' },
    });
    expect(view).toEqual({
      id: 'case-7',
      title: 'pages/foo.md',
      category: 'knowledge_enrichment',
      status: 'pending',
      targetPath: 'pages/foo.md',
      content: 'body',
      proposedBy: 'did:nostr:aaaa',
      // FR2.3: the whole payload rides along so the card can render it in full.
      proposal: { target_path: 'pages/foo.md', content: 'body', proposed_by: 'did:nostr:aaaa' },
    });
  });

  it('falls back to the id as title and normalises unknown status to pending', () => {
    const view = toCaseView({ id: 'case-9', metadata: {} });
    expect(view.title).toBe('case-9');
    expect(view.status).toBe('pending');
  });
});

// ---------------------------------------------------------------------------
// FR2.3 / FR6.5 (EXP-AC-002, EXP-AC-006): the case view carries everything the
// reviewer needs to judge — the full proposal payload, its provenance, its age —
// and the queue puts the oldest case first.
// ---------------------------------------------------------------------------

describe('toCaseView — non-vacuous decision surface (EXP-AC-002)', () => {
  it('carries the full proposal payload, URNs, reasoning and provenance', () => {
    const view = toCaseView({
      id: 'case-7',
      category: 'knowledge_enrichment',
      status: 'pending',
      created_at_ms: 1_700_000_000_000,
      metadata: {
        target_path: 'pages/foo.md',
        content: 'proposed body',
        proposal_urn: 'urn:visionclaw:kg:aaaa:enrichment-proposal:case-7',
        reasoning_summary: 'Frontier concept with 12 axiom references.',
        reasoning_hash: 'sha256-deadbeef',
        risk_tier: 'high',
        provenance: { model: 'qwen3.8-27B', source_excerpt: 'the seed sentence', confidence: 0.82 },
      },
    });
    expect(view.createdAt).toBe(1_700_000_000_000);
    expect(view.proposalUrn).toBe('urn:visionclaw:kg:aaaa:enrichment-proposal:case-7');
    expect(view.reasoningSummary).toBe('Frontier concept with 12 axiom references.');
    expect(view.reasoningHash).toBe('sha256-deadbeef');
    expect(view.tier).toBe('high');
    expect(view.provenance).toEqual({ model: 'qwen3.8-27B', sourceExcerpt: 'the seed sentence', confidence: 0.82 });
    // The whole metadata bag is retained so the payload renders in full.
    expect((view.proposal as Record<string, unknown>).content).toBe('proposed body');
  });

  it('renders absence as absence — no confidence is synthesised', () => {
    const view = toCaseView({ id: 'case-9', metadata: { target_path: 'pages/bar.md' } });
    expect(view.confidence).toBeUndefined();
    expect(view.provenance).toBeUndefined();
    expect(view.createdAt).toBeUndefined();
  });

  it('keeps an agent confidence only when the agent actually produced one', () => {
    const view = toCaseView({ id: 'c', metadata: { confidence: 0.3 } });
    expect(view.confidence).toBe(0.3);
  });
});

describe('rationale gate (EXP-AC-002)', () => {
  it('requires a rationale on high and critical tiers only', () => {
    expect(rationaleRequired('high')).toBe(true);
    expect(rationaleRequired('critical')).toBe(true);
    expect(rationaleRequired('medium')).toBe(false);
    expect(rationaleRequired('low')).toBe(false);
    expect(rationaleRequired(undefined)).toBe(false);
  });

  it('blocks publishing a high-tier decision until the rationale reaches 20 characters', () => {
    expect(MIN_RATIONALE_CHARS).toBe(20);
    expect(canPublishDecision('critical', '')).toBe(false);
    expect(canPublishDecision('critical', 'too short')).toBe(false);
    expect(canPublishDecision('critical', 'x'.repeat(19))).toBe(false);
    expect(canPublishDecision('critical', 'x'.repeat(20))).toBe(true);
    // Untiered/low cases may be decided without one.
    expect(canPublishDecision('low', '')).toBe(true);
    expect(canPublishDecision(undefined, '')).toBe(true);
  });

  it('counts trimmed characters, so whitespace cannot satisfy the gate', () => {
    expect(canPublishDecision('high', `${' '.repeat(30)}`)).toBe(false);
    expect(canPublishDecision('high', `  ${'y'.repeat(20)}  `)).toBe(true);
  });
});

describe('queue ordering and ageing (EXP-AC-006)', () => {
  it('sorts oldest first, with undated cases last', () => {
    const views = [
      toCaseView({ id: 'newer', created_at_ms: 3_000, metadata: {} }),
      toCaseView({ id: 'undated', metadata: {} }),
      toCaseView({ id: 'oldest', created_at_ms: 1_000, metadata: {} }),
      toCaseView({ id: 'middle', created_at_ms: 2_000, metadata: {} }),
    ];
    expect(sortOldestFirst(views).map((v) => v.id)).toEqual(['oldest', 'middle', 'newer', 'undated']);
  });

  it('does not mutate the input array', () => {
    const views = [toCaseView({ id: 'b', created_at_ms: 2 }), toCaseView({ id: 'a', created_at_ms: 1 })];
    sortOldestFirst(views);
    expect(views.map((v) => v.id)).toEqual(['b', 'a']);
  });

  it('labels case age in the largest whole unit', () => {
    const now = 10 * 24 * 60 * 60 * 1000;
    expect(caseAgeLabel(now - 30 * 1000, now)).toBe('just now');
    expect(caseAgeLabel(now - 5 * 60 * 1000, now)).toBe('5m');
    expect(caseAgeLabel(now - 3 * 60 * 60 * 1000, now)).toBe('3h');
    expect(caseAgeLabel(now - 4 * 24 * 60 * 60 * 1000, now)).toBe('4d');
    expect(caseAgeLabel(undefined, now)).toBeUndefined();
  });
});
