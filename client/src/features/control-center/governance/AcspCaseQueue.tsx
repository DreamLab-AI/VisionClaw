// REC-2 / D3 (PRD-023 WP-4): the control-centre broker case queue + the ambient
// ACSP indicator.
//
// An always-mounted glass pill shows the open-case count (the ambient ACSP
// indicator, driven by the broker:new_case / broker:case_decided WS events);
// clicking it expands the pending-judgment queue, each case decidable through
// the WS-9 operator decide route. A decided case round-trips
// `broker:new_case → broker:case_decided`, which fires `CANARY-VC-REC2-CASE`
// server-side.
//
// PRD-augmentation-conditions FR2.3/FR2.4/FR6.5 (EXP-AC-002, EXP-AC-006) — this
// card exists to make a NON-VACUOUS decision possible, so its layout is load-
// bearing, not cosmetic:
//
//   * the full proposal payload renders pretty-printed and UNCLIPPED, with the
//     proposal URN, the reasoning hash and the agent's reasoning, so the human
//     judges the actual proposed change rather than its title;
//   * the human types their own rationale and THAT TEXT is what the decision
//     carries, byte for byte. The UI authors nothing. The former
//     `operator ${outcome} via control centre` template is deleted: a rationale
//     the human did not write is a fabricated judgement (DDD invariant 1);
//   * on a `high`/`critical` tier the decision is disabled until the rationale
//     reaches `MIN_RATIONALE_CHARS`;
//   * the agent's SELF-ASSESSMENT (declared tier, confidence) renders BELOW the
//     controls so it cannot anchor the reviewer before they have read the
//     proposal, and each field renders only when the agent actually produced it;
//   * the queue sorts oldest first and badges each case's age, so a stalled case
//     is visible rather than silently buried.

import React, { useMemo, useState } from 'react';
import { Scale } from 'lucide-react';
import { GlassPanel } from '../primitives/GlassPanel';
import { useBrokerCaseQueue } from './useBrokerCaseQueue';
import {
  MIN_RATIONALE_CHARS,
  canPublishDecision,
  caseAgeLabel,
  rationaleRequired,
  sortOldestFirst,
  type CaseView,
} from './brokerCaseQueue';

/** Pretty-print the proposal payload for review. Never truncated. */
function formatPayload(proposal: unknown): string {
  if (proposal === undefined || proposal === null) return '{}';
  try {
    return JSON.stringify(proposal, null, 2);
  } catch {
    return String(proposal);
  }
}

interface CaseCardProps {
  view: CaseView;
  busy: boolean;
  now: number;
  onDecide: (caseId: string, outcome: string, rationale: string) => void;
}

const CaseCard: React.FC<CaseCardProps> = ({ view, busy, now, onDecide }) => {
  // The human's own words. Starts empty and is never seeded by the UI.
  const [rationale, setRationale] = useState('');
  const required = rationaleRequired(view.tier);
  const canPublish = canPublishDecision(view.tier, rationale);
  const age = caseAgeLabel(view.createdAt, now);

  return (
    <li key={view.id} className="rounded bg-white/5 p-2" data-testid="acsp-case">
      <div className="flex items-start justify-between gap-2">
        <div className="text-[11px] font-medium break-words" title={view.title}>
          {view.title}
        </div>
        {age && (
          <span
            data-testid="acsp-case-age"
            title="Time since this case was opened"
            className="shrink-0 text-[10px] px-1 rounded bg-amber-500/15 text-amber-400"
          >
            {age}
          </span>
        )}
      </div>
      <div className="text-[10px] text-muted-foreground mb-1">
        {view.category} · {view.status}
        {view.proposedBy ? ` · ${view.proposedBy.slice(0, 18)}` : ''}
      </div>

      {/* --- the proposed change, in full ---------------------------------- */}
      <pre
        data-testid="acsp-case-payload"
        className="text-[10px] text-muted-foreground bg-black/20 rounded p-1 mb-1 overflow-x-auto whitespace-pre-wrap break-words"
      >
        {formatPayload(view.proposal)}
      </pre>
      {view.proposalUrn && (
        <div data-testid="acsp-case-proposal-urn" className="text-[10px] text-muted-foreground break-all">
          Proposal: {view.proposalUrn}
        </div>
      )}
      {view.reasoningSummary && (
        <div data-testid="acsp-case-reasoning" className="text-[10px] text-muted-foreground mt-1">
          Agent reasoning: {view.reasoningSummary}
        </div>
      )}
      {view.reasoningHash && (
        <div data-testid="acsp-case-reasoning-hash" className="text-[10px] text-muted-foreground break-all">
          Reasoning hash: {view.reasoningHash}
        </div>
      )}
      {view.provenance && (
        <div data-testid="acsp-case-provenance" className="text-[10px] text-muted-foreground mt-1">
          {view.provenance.model && <div>Model: {view.provenance.model}</div>}
          {view.provenance.sourceExcerpt && <div>Source: {view.provenance.sourceExcerpt}</div>}
          {view.provenance.confidence !== undefined && (
            <div>Generation confidence: {view.provenance.confidence}</div>
          )}
        </div>
      )}

      {/* --- the human's rationale, then the controls ----------------------- */}
      <label className="block mt-2 text-[10px] text-muted-foreground" htmlFor={`rationale-${view.id}`}>
        Your rationale{required ? ` (required, ${MIN_RATIONALE_CHARS}+ characters)` : ' (optional)'}
      </label>
      <textarea
        id={`rationale-${view.id}`}
        data-testid="acsp-rationale"
        value={rationale}
        onChange={(e) => setRationale(e.target.value)}
        rows={2}
        placeholder="Why this decision, in your own words"
        className="w-full mt-1 mb-2 text-[10px] rounded bg-black/20 p-1 text-foreground placeholder:text-muted-foreground"
      />

      <div className="flex gap-2" data-testid="acsp-case-controls">
        <button
          type="button"
          data-testid="acsp-approve"
          disabled={busy || !canPublish}
          onClick={() => onDecide(view.id, 'approve', rationale)}
          className="flex-1 text-[10px] px-2 py-1 rounded bg-emerald-500/15 text-emerald-400 hover:bg-emerald-500/25 disabled:opacity-50"
        >
          Approve
        </button>
        <button
          type="button"
          data-testid="acsp-reject"
          disabled={busy || !canPublish}
          onClick={() => onDecide(view.id, 'reject', rationale)}
          className="flex-1 text-[10px] px-2 py-1 rounded bg-red-500/15 text-red-400 hover:bg-red-500/25 disabled:opacity-50"
        >
          Reject
        </button>
      </div>

      {/* --- the agent's SELF-ASSESSMENT, below the controls ---------------- */}
      {(view.tier || view.confidence !== undefined) && (
        <div data-testid="acsp-case-self-assessment" className="mt-2 text-[10px] text-muted-foreground">
          Agent self-assessment:{view.tier ? ` tier ${view.tier}` : ''}
          {view.confidence !== undefined && (
            <span data-testid="acsp-case-confidence"> · confidence {view.confidence}</span>
          )}
        </div>
      )}
    </li>
  );
};

export const AcspCaseQueue: React.FC = () => {
  const { cases, openCount, loading, decide } = useBrokerCaseQueue();
  const [expanded, setExpanded] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);

  // Oldest first: the longest-waiting judgment is the one the reviewer sees.
  const pending = useMemo(
    () => sortOldestFirst(cases.filter((c) => c.status !== 'decided')),
    [cases],
  );
  const now = Date.now();

  const onDecide = async (caseId: string, outcome: string, rationale: string) => {
    setBusyId(caseId);
    try {
      // The human's text verbatim, or nothing at all — never a template.
      const typed = rationale.trim();
      await decide(caseId, outcome, typed.length > 0 ? rationale : undefined);
    } finally {
      setBusyId(null);
    }
  };

  return (
    <div className="fixed bottom-20 left-4 z-40 flex flex-col items-start gap-2" style={{ pointerEvents: 'auto' }}>
      {expanded && (
        <GlassPanel
          elevation="overlay"
          data-testid="acsp-case-queue"
          role="region"
          aria-label="Broker case queue"
          className="w-80 max-h-[60vh] overflow-y-auto p-3 text-foreground"
        >
          <div className="flex items-center justify-between mb-2">
            <span className="text-sm font-semibold">Governance Queue</span>
            <button
              type="button"
              aria-label="Close case queue"
              onClick={() => setExpanded(false)}
              className="text-xs text-muted-foreground hover:text-foreground"
            >
              ✕
            </button>
          </div>

          {loading ? (
            <p className="text-xs text-muted-foreground">Loading cases…</p>
          ) : pending.length === 0 ? (
            <p className="text-xs text-muted-foreground">No pending judgments.</p>
          ) : (
            <ul className="space-y-2">
              {pending.map((c) => (
                <CaseCard key={c.id} view={c} busy={busyId === c.id} now={now} onDecide={onDecide} />
              ))}
            </ul>
          )}
        </GlassPanel>
      )}

      <button
        type="button"
        data-testid="acsp-indicator"
        aria-label={`Governance queue — ${openCount} open case${openCount === 1 ? '' : 's'}`}
        aria-expanded={expanded}
        onClick={() => setExpanded((v) => !v)}
        className="cc-glass flex items-center gap-1.5 px-3 py-1.5 rounded-full text-xs text-foreground hover:text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <Scale size={13} aria-hidden="true" />
        ACSP
        <span
          className="inline-flex items-center justify-center min-w-[18px] h-[18px] px-1 rounded-full text-[10px]"
          style={{
            background: openCount > 0 ? 'rgba(245,158,11,0.2)' : 'rgba(107,114,128,0.2)',
            color: openCount > 0 ? '#f59e0b' : '#9ca3af',
          }}
        >
          {openCount}
        </span>
      </button>
    </div>
  );
};

AcspCaseQueue.displayName = 'AcspCaseQueue';

export default AcspCaseQueue;
