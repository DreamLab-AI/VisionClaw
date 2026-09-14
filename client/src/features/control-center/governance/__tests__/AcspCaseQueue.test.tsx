// REC-2 / D3 (PRD-023 WP-4): the case queue renders the inbox, shows the ambient
// open-case count, and decides through the WS-9 operator route.

import React from 'react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/react';

const { getDataSpy, postSpy } = vi.hoisted(() => ({
  getDataSpy: vi.fn(),
  postSpy: vi.fn(),
}));

vi.mock('../../../../services/api/UnifiedApiClient', () => ({
  unifiedApiClient: {
    getData: (...args: unknown[]) => getDataSpy(...args),
    post: (...args: unknown[]) => postSpy(...args),
  },
}));

vi.mock('../../../../store/websocketStore', () => ({
  webSocketService: {
    onMessage: () => () => {},
  },
}));

import { AcspCaseQueue } from '../AcspCaseQueue';

beforeEach(() => {
  cleanup();
  getDataSpy.mockReset();
  postSpy.mockReset();
});

describe('AcspCaseQueue (D3)', () => {
  it('renders open-case count and decides via the operator route', async () => {
    getDataSpy.mockResolvedValue({
      cases: [
        {
          id: 'case-7',
          category: 'knowledge_enrichment',
          status: 'pending',
          metadata: { target_path: 'pages/foo.md', content: 'proposed body' },
        },
      ],
      total: 1,
    });
    postSpy.mockResolvedValue({ data: { success: true } });

    render(<AcspCaseQueue />);

    // Ambient indicator shows one open case.
    const indicator = await screen.findByTestId('acsp-indicator');
    await waitFor(() => expect(indicator).toHaveTextContent('1'));

    // Expand and approve the pending case.
    fireEvent.click(indicator);
    const approve = await screen.findByText('Approve');
    fireEvent.click(approve);

    await waitFor(() => expect(postSpy).toHaveBeenCalled());
    const [url, body] = postSpy.mock.calls[0];
    expect(url).toBe('/broker/cases/case-7/decide');
    expect(body).toMatchObject({ outcome: 'approve' });
  });
});

// ---------------------------------------------------------------------------
// FR2.3 / FR2.4 / FR6.5 (EXP-AC-002, EXP-AC-006): the card must make a NON-
// VACUOUS decision possible — the proposal in view, the human's own words
// published, the agent's self-assessment kept below the controls where it
// cannot anchor the judgment, and the oldest case surfaced first.
// ---------------------------------------------------------------------------

const CRITICAL_CASE = {
  id: 'case-crit',
  category: 'knowledge_enrichment',
  status: 'pending',
  created_at_ms: 1_000,
  metadata: {
    target_path: 'pages/critical.md',
    content: 'the proposed body',
    risk_tier: 'critical',
    confidence: 0.82,
    proposal_urn: 'urn:visionclaw:kg:aaaa:enrichment-proposal:case-crit',
    reasoning_summary: 'Frontier concept with 12 axiom references.',
    reasoning_hash: 'sha256-deadbeef',
    provenance: { model: 'qwen3.8-27B', source_excerpt: 'the seed sentence' },
  },
};

describe('AcspCaseQueue — non-vacuous decision surface (EXP-AC-002)', () => {
  const expand = async () => {
    render(<AcspCaseQueue />);
    fireEvent.click(await screen.findByTestId('acsp-indicator'));
  };

  it('renders the full proposal payload, URNs, reasoning and provenance', async () => {
    getDataSpy.mockResolvedValue({ cases: [CRITICAL_CASE], total: 1 });
    await expand();

    const payload = await screen.findByTestId('acsp-case-payload');
    // Pretty-printed and complete — not a clipped preview.
    expect(payload.textContent).toContain('"content": "the proposed body"');
    expect(payload.textContent).toContain('"target_path": "pages/critical.md"');
    expect(payload.className).not.toMatch(/line-clamp/);

    expect(screen.getByTestId('acsp-case-proposal-urn')).toHaveTextContent(
      'urn:visionclaw:kg:aaaa:enrichment-proposal:case-crit',
    );
    expect(screen.getByTestId('acsp-case-reasoning-hash')).toHaveTextContent('sha256-deadbeef');
    expect(screen.getByTestId('acsp-case-reasoning')).toHaveTextContent(
      'Frontier concept with 12 axiom references.',
    );
    const prov = screen.getByTestId('acsp-case-provenance');
    expect(prov).toHaveTextContent('qwen3.8-27B');
    expect(prov).toHaveTextContent('the seed sentence');
  });

  it('renders no provenance and no confidence when the agent produced none', async () => {
    getDataSpy.mockResolvedValue({
      cases: [{ id: 'c-bare', status: 'pending', metadata: { target_path: 'p.md' } }],
      total: 1,
    });
    await expand();
    await screen.findByTestId('acsp-case-payload');
    expect(screen.queryByTestId('acsp-case-provenance')).toBeNull();
    expect(screen.queryByTestId('acsp-case-confidence')).toBeNull();
  });

  it('keeps the agent self-assessment BELOW the decision controls', async () => {
    getDataSpy.mockResolvedValue({ cases: [CRITICAL_CASE], total: 1 });
    await expand();

    const card = await screen.findByTestId('acsp-case');
    const controls = screen.getByTestId('acsp-case-controls');
    const selfAssessment = screen.getByTestId('acsp-case-self-assessment');
    const order = [...card.querySelectorAll('[data-testid]')];
    expect(order.indexOf(controls)).toBeLessThan(order.indexOf(selfAssessment));
    expect(selfAssessment).toHaveTextContent('critical');
    expect(screen.getByTestId('acsp-case-confidence')).toHaveTextContent('0.82');
  });

  it('disables a critical decision until 20 characters of rationale are typed', async () => {
    getDataSpy.mockResolvedValue({ cases: [CRITICAL_CASE], total: 1 });
    postSpy.mockResolvedValue({ data: { success: true } });
    await expand();

    const approve = (await screen.findByTestId('acsp-approve')) as HTMLButtonElement;
    expect(approve.disabled).toBe(true);

    const rationale = screen.getByTestId('acsp-rationale');
    fireEvent.change(rationale, { target: { value: 'too short' } });
    expect((screen.getByTestId('acsp-approve') as HTMLButtonElement).disabled).toBe(true);

    const typed = 'Checked the axioms; the draft definition matches the corpus.';
    fireEvent.change(rationale, { target: { value: typed } });
    expect((screen.getByTestId('acsp-approve') as HTMLButtonElement).disabled).toBe(false);

    fireEvent.click(screen.getByTestId('acsp-approve'));
    await waitFor(() => expect(postSpy).toHaveBeenCalled());
    const [, body] = postSpy.mock.calls[0];
    // Byte-for-byte the human's own words — nothing the UI authored.
    expect(body.reasoning).toBe(typed);
    expect(body.outcome).toBe('approve');
  });

  it('never publishes a fabricated rationale on an untiered case', async () => {
    getDataSpy.mockResolvedValue({
      cases: [{ id: 'c-low', status: 'pending', metadata: { target_path: 'p.md' } }],
      total: 1,
    });
    postSpy.mockResolvedValue({ data: { success: true } });
    await expand();

    fireEvent.click(await screen.findByTestId('acsp-reject'));
    await waitFor(() => expect(postSpy).toHaveBeenCalled());
    const [, body] = postSpy.mock.calls[0];
    expect(body.reasoning).toBeUndefined();
  });
});

describe('AcspCaseQueue — ageing and ordering (EXP-AC-006)', () => {
  it('shows the oldest case first and labels its age', async () => {
    const now = Date.now();
    getDataSpy.mockResolvedValue({
      cases: [
        { id: 'newer', status: 'pending', created_at_ms: now - 60 * 60 * 1000, metadata: {} },
        { id: 'oldest', status: 'pending', created_at_ms: now - 4 * 24 * 60 * 60 * 1000, metadata: {} },
      ],
      total: 2,
    });
    render(<AcspCaseQueue />);
    fireEvent.click(await screen.findByTestId('acsp-indicator'));

    const cards = await screen.findAllByTestId('acsp-case');
    expect(cards[0].textContent).toContain('oldest');
    expect(screen.getAllByTestId('acsp-case-age')[0]).toHaveTextContent('4d');
  });
});
