/**
 * MemoryExplorerPanel — HTML overlay for the live memory explorer.
 *
 * Explore tab: ask a question of memory, pick k and a namespace, switch the
 * route view, replay, tune the learning loop, read the HUD (hops, distance
 * evaluations, local recall against exact search, beam breadth, hints used,
 * build progress), see the sidecar's own results (click one to fly to it) and
 * a recall sparkline over recent queries.
 * Health tab: per-namespace coverage, the server's recall probe, sidecar and
 * embedder status, and how many live flashes landed on sampled rows.
 * Cinematic section (when enabled in settings): director, audio beat sync and
 * video export.
 *
 * View and learning changes are written to settings so they persist; the
 * cloud layer mirrors settings into the explorer store.
 */

import React, { useEffect, useId, useMemo, useState } from 'react';
import { GlassPanel } from '../../control-center/primitives/GlassPanel';
import { useSettingsStore } from '@/store/settingsStore';
import type { EmbeddingCloudSettings } from '../../settings/config/settings';
import { useMemoryCloudStore } from './memoryCloudInstance';
import { sidecarAgreement, describeAgreement, MIN_SPEED, MAX_SPEED } from './memoryCloudStore';
import { focusMemoryPoint } from '../cameraFocus';
import { ROUTE_PALETTE, TOTAL_DUR, GROW_DUR } from './routeMath';
import { startDirector, stopDirector } from './cinematicSession';
import { pickRecorderMime } from './recorder';
import { css, pct } from './panelStyles';
import { SoundSection, SpotifyChip, SpotifyPopover, useTapKey } from './SoundControls';
import type { TrajectoryView } from '../memoryTrajectory/types';
import type { MemoryCloudSearchMethod, MemoryCloudSidecarIssue } from './types';

const E = 'visualisation.embeddingCloud.';
const VIEWS: Array<{ id: TrajectoryView; label: string; hint: string }> = [
  { id: 'space', label: 'Space', hint: 'Route drawn through the cloud itself' },
  { id: 'canopy', label: 'Canopy', hint: 'Search tree fanned out from the entry point' },
  { id: 'tree', label: 'Tree', hint: 'Search tree by depth' },
  { id: 'hyper', label: 'Hyper', hint: 'Search tree in the Poincaré disk' },
];
const HEALTH_POLL_MS = 30_000;

const METHOD_LABEL: Record<MemoryCloudSearchMethod, { short: string; title: string }> = {
  hnsw: { short: 'HNSW', title: "Answered by the sidecar's HNSW index, as an agent's memory_search would be" },
  exact: {
    short: 'exact',
    title: 'Answered by an exact scan: namespace-restricted queries, or too few HNSW candidates survived the exclusions',
  },
};

/** Health tab wording for the sidecar's closed error categories (wire.rs `SidecarIssue`). */
export const SIDECAR_ISSUE_TEXT: Record<MemoryCloudSidecarIssue, { label: string; detail: string }> = {
  not_configured: {
    label: 'Not configured',
    detail: 'RUVECTOR_PG_CONNINFO is unset or invalid on the server, so the cloud cannot reach the sidecar.',
  },
  building: { label: 'Building', detail: 'The first snapshot is still building; this clears by itself.' },
  unreachable: {
    label: 'Unreachable',
    detail: 'The latest snapshot build could not reach or query the sidecar; the server log has the cause.',
  },
};

/** Whole seconds until `at`, ticking once a second; null when `at` is null. */
function useCountdown(at: number | null): number | null {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (at === null) return;
    setNow(Date.now());
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [at]);
  return at === null ? null : Math.max(0, Math.ceil((at - now) / 1000));
}

const fmtMs = (ms: number) => (ms < 1 ? `${(ms * 1000).toFixed(0)} µs` : `${ms.toFixed(ms < 10 ? 2 : 0)} ms`);
const recallColour = (r: number) => (r >= 0.9 ? ROUTE_PALETTE.mint : r >= 0.7 ? ROUTE_PALETTE.tip : ROUTE_PALETTE.miss);

/** Recall sparkline over recent queries (0..1 axis). */
export const RecallSparkline: React.FC<{ values: number[]; width?: number; height?: number }> = ({ values, width = 150, height = 28 }) => {
  if (values.length < 2) {
    return <span style={css.label}>{values.length ? `recall ${values[0].toFixed(2)}` : 'no queries yet'}</span>;
  }
  const step = (width - 4) / (values.length - 1);
  const y = (v: number) => height - 2 - v * (height - 4);
  const pts = values.map((v, i) => `${(2 + i * step).toFixed(1)},${y(v).toFixed(1)}`).join(' ');
  const lastV = values[values.length - 1];
  return (
    <svg width={width} height={height} role="img" aria-label={`Recall over the last ${values.length} queries, latest ${lastV.toFixed(2)}`}>
      <line x1={2} x2={width - 2} y1={y(0.9)} y2={y(0.9)} stroke="rgba(126,240,207,0.25)" strokeDasharray="2 3" />
      <polyline points={pts} fill="none" stroke={ROUTE_PALETTE.mint} strokeWidth={1.3} />
      <circle cx={2 + (values.length - 1) * step} cy={y(lastV)} r={2.2} fill={recallColour(lastV)} />
    </svg>
  );
};

const Stat: React.FC<{ label: string; value: React.ReactNode; title?: string }> = ({ label, value, title }) => (
  <div style={css.stat} title={title}>
    <span style={css.label}>{label}</span>
    <span style={{ ...css.mono, fontSize: 13 }}>{value}</span>
  </div>
);

/** Quiet countdown to the store's scheduled reload after a 503. */
const RetryNote: React.FC = () => {
  const retryAt = useMemoryCloudStore((s) => s.retryAt);
  const reason = useMemoryCloudStore((s) => s.error);
  const secs = useCountdown(retryAt);
  return (
    <div style={{ ...css.label, marginTop: 8 }} aria-live="polite">
      {secs === null ? 'Memory cloud unavailable.' : `Memory cloud unavailable; retrying in ${secs} s.`}
      {reason && <div style={css.mono}>{reason}</div>}
    </div>
  );
};

/** 429: the per-signer query budget is spent; a quiet note, not an error. */
const RateLimitNote: React.FC<{ retryAt: number | null }> = ({ retryAt }) => {
  const secs = useCountdown(retryAt);
  return (
    <div role="status" aria-label="Query rate limit" style={{ ...css.label, marginTop: 8, color: ROUTE_PALETTE.tip }}>
      Rate limited: the memory-cloud query budget is spent.{' '}
      {secs === null || secs === 0 ? 'You can retry now.' : `You can retry in ${secs} s.`}
    </div>
  );
};

const ExploreTab: React.FC<{ cfg: EmbeddingCloudSettings | undefined; setSetting: (k: string, v: unknown) => void }> = ({ cfg, setSetting }) => {
  const snapshot = useMemoryCloudStore((s) => s.snapshot);
  const status = useMemoryCloudStore((s) => s.status);
  const loadError = useMemoryCloudStore((s) => s.error);
  const buildProgress = useMemoryCloudStore((s) => s.buildProgress);
  const query = useMemoryCloudStore((s) => s.query);
  const view = useMemoryCloudStore((s) => s.view);
  const playback = useMemoryCloudStore((s) => s.playback);
  const recallHistory = useMemoryCloudStore((s) => s.recallHistory);
  const flashes = useMemoryCloudStore((s) => s.flashes);
  const [text, setText] = useState(query.text);
  const [k, setK] = useState(query.k);
  const [namespace, setNamespace] = useState<string>(query.namespace ?? '');
  const ids = useId();

  const run = query.run;
  const results = query.response?.sidecar.results ?? [];
  const agreement = useMemo(() => sidecarAgreement(results, run), [results, run]);
  const agreementText = describeAgreement(agreement);
  const method = query.response?.sidecar.method;
  const local = useMemo(() => new Set(run?.result.top ?? []), [run]);
  const n = snapshot?.count ?? 0;
  const busy = query.status === 'running';
  const canRun = !!snapshot && !busy && text.trim().length > 0;

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!canRun) return;
    void useMemoryCloudStore.getState().runQuery(text.trim(), { k, namespace: namespace || null });
  };

  const totalEl = TOTAL_DUR + GROW_DUR;

  return (
    <div>
      <form onSubmit={submit} aria-label="Query memory">
        <div style={css.row}>
          <input
            aria-label="Query"
            style={css.input}
            placeholder="Ask memory…"
            value={text}
            onChange={(e) => setText(e.target.value)}
          />
          <button type="submit" style={css.button(true)} disabled={!canRun}>
            {busy ? 'Searching…' : 'Search'}
          </button>
        </div>
        <div style={{ ...css.row, marginTop: 6 }}>
          <label htmlFor={`${ids}-k`} style={css.label}>k</label>
          <input
            id={`${ids}-k`}
            type="number"
            min={1}
            max={50}
            value={k}
            onChange={(e) => setK(Math.max(1, Math.min(50, Number(e.target.value) || 1)))}
            style={{ ...css.input, flex: '0 0 52px' }}
          />
          <label htmlFor={`${ids}-ns`} style={css.label}>namespace</label>
          <select id={`${ids}-ns`} value={namespace} onChange={(e) => setNamespace(e.target.value)} style={css.input}>
            <option value="">all sampled</option>
            {(snapshot?.namespaces ?? []).map((ns) => (
              <option key={ns} value={ns}>{ns}</option>
            ))}
          </select>
        </div>
      </form>

      {status === 'error' && <div role="alert" style={{ color: ROUTE_PALETTE.miss, marginTop: 8 }}>{loadError}</div>}
      {status === 'forbidden' && (
        <div style={{ ...css.label, marginTop: 8 }}>Memory cloud needs power-user access.</div>
      )}
      {status === 'unavailable' && <RetryNote />}
      {query.status === 'error' && <div role="alert" style={{ color: ROUTE_PALETTE.miss, marginTop: 8 }}>{query.error}</div>}
      {query.status === 'rate_limited' && <RateLimitNote retryAt={query.retryAt} />}
      {(status === 'building' || status === 'loading') && (
        <div style={{ marginTop: 8 }} aria-live="polite">
          <span style={css.label}>{status === 'loading' ? 'Loading sample…' : `Building local index ${pct(buildProgress)}`}</span>
          <div style={{ height: 3, background: 'rgba(255,255,255,0.08)', borderRadius: 2, marginTop: 3 }}>
            <div style={{ width: pct(status === 'loading' ? 0 : buildProgress), height: 3, background: ROUTE_PALETTE.root, borderRadius: 2 }} />
          </div>
        </div>
      )}

      <div style={css.section}>
        <div style={{ ...css.row, justifyContent: 'space-between' }} role="group" aria-label="Route view">
          {VIEWS.map((v) => (
            <button key={v.id} type="button" title={v.hint} aria-pressed={view === v.id} style={css.button(view === v.id)} onClick={() => setSetting('trajectoryView', v.id)}>
              {v.label}
            </button>
          ))}
        </div>
        <div style={{ ...css.row, marginTop: 8 }}>
          <button type="button" style={css.button()} disabled={!run} onClick={() => useMemoryCloudStore.getState().replay()}>Replay</button>
          <button type="button" style={css.button()} disabled={!run} onClick={() => useMemoryCloudStore.getState().setPlaying(!playback.playing)}>
            {playback.playing ? 'Pause' : 'Play'}
          </button>
          <button
            type="button"
            style={css.button()}
            disabled={!run}
            onClick={() => {
              setText('');
              useMemoryCloudStore.getState().clearQuery();
            }}
          >
            New
          </button>
          <input
            aria-label="Scrub route playback"
            type="range"
            min={0}
            max={totalEl}
            step={0.05}
            value={Math.min(totalEl, playback.el)}
            disabled={!run}
            onChange={(e) => useMemoryCloudStore.getState().seek(Number(e.target.value))}
            style={{ flex: 1, minWidth: 0 }}
          />
        </div>
        <div style={{ ...css.row, marginTop: 6 }}>
          <label htmlFor={`${ids}-speed`} style={css.label}>speed</label>
          <input
            id={`${ids}-speed`}
            type="range"
            min={MIN_SPEED}
            max={MAX_SPEED}
            step={0.25}
            value={cfg?.playbackSpeed ?? 1}
            onChange={(e) => setSetting('playbackSpeed', Number(e.target.value))}
            style={{ flex: 1 }}
          />
          <span style={css.mono}>{(cfg?.playbackSpeed ?? 1).toFixed(2)}×</span>
        </div>
      </div>

      {run && (
        <div style={css.section} aria-label="Search statistics">
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: 8 }}>
            <Stat label="hops" value={Math.max(0, run.tree.path.length - 1)} title="Hops on the route from the entry point to the best result" />
            <Stat label="distance evals" value={run.result.distanceEvals.toLocaleString('en-GB')} title={`Exact search would evaluate all ${n.toLocaleString('en-GB')}`} />
            <Stat label={`recall@${query.k}`} value={<span style={{ color: recallColour(run.recall) }}>{run.recall.toFixed(2)}</span>} title="Local HNSW top-k against exact search over the same sample" />
            <Stat label="ef / breadth" value={run.breadth} />
            <Stat label="hints used" value={run.hintsUsed.length} title="Remembered queries that seeded this search" />
            <Stat
              label="sidecar"
              value={query.response ? `${fmtMs(query.response.sidecar.tookMs)}${method ? ` · ${METHOD_LABEL[method]?.short ?? method}` : ''}` : '—'}
              title={method ? METHOD_LABEL[method]?.title : undefined}
            />
          </div>
          <div style={{ ...css.row, marginTop: 8, justifyContent: 'space-between' }}>
            <RecallSparkline values={recallHistory} />
            <span style={css.label}>{pct(n ? run.result.distanceEvals / n : 0)} of the sample evaluated</span>
          </div>
        </div>
      )}

      {query.response && (
        <div style={css.section}>
          <div style={{ fontWeight: 600 }}>Sidecar results</div>
          <div
            style={{ ...css.label, marginTop: 2, lineHeight: 1.45 }}
            title="Hits outside the sample have no point in the cloud, so only sampled hits are compared with the local route"
          >
            <div>{agreementText.coverage}</div>
            <div>{agreementText.agreement}</div>
          </div>
          <ol style={{ listStyle: 'none', padding: 0, margin: '6px 0 0' }} aria-label="Sidecar results">
            {results.map((h, i) => {
              const sampled = h.sampleIndex !== null;
              const agrees = sampled && local.has(h.sampleIndex as number);
              return (
                <li key={`${h.id}-${i}`}>
                  <button
                    type="button"
                    onClick={() => sampled && focusMemoryPoint(h.sampleIndex as number)}
                    disabled={!sampled}
                    title={sampled ? 'Fly to this memory' : 'Not in the sample, so it has no point in the cloud'}
                    style={{
                      display: 'block',
                      width: '100%',
                      textAlign: 'left',
                      background: 'transparent',
                      border: 'none',
                      borderLeft: `2px solid ${sampled ? ROUTE_PALETTE.sidecar : 'rgba(255,255,255,0.12)'}`,
                      color: 'inherit',
                      padding: '4px 0 4px 8px',
                      marginBottom: 4,
                      cursor: sampled ? 'pointer' : 'default',
                    }}
                  >
                    <div style={{ ...css.row, justifyContent: 'space-between' }}>
                      <span style={{ ...css.mono, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{h.key}</span>
                      <span style={css.mono}>{h.score.toFixed(3)}</span>
                    </div>
                    <div style={{ ...css.row, justifyContent: 'space-between' }}>
                      <span style={css.label}>{h.namespace}</span>
                      <span style={{ ...css.label, color: sampled ? (agrees ? ROUTE_PALETTE.mint : ROUTE_PALETTE.sidecar) : '#6b7386' }}>
                        {sampled ? (agrees ? 'in sample · local agrees' : 'in sample') : 'not sampled'}
                      </span>
                    </div>
                    {h.snippet && <div style={{ color: '#aeb5c6', marginTop: 2, overflowWrap: 'anywhere' }}>{h.snippet}</div>}
                  </button>
                </li>
              );
            })}
          </ol>
        </div>
      )}

      <LearningControls cfg={cfg} setSetting={setSetting} />

      <p style={{ ...css.label, marginTop: 10, lineHeight: 1.45 }}>
        Route drawn over a local index of the {n.toLocaleString('en-GB')}-point sample; sidecar results shown for comparison.
        {flashes.none > 0 && ` ${flashes.none.toLocaleString('en-GB')} live flash${flashes.none === 1 ? '' : 'es'} fell outside the sample.`}
      </p>
    </div>
  );
};

const LearningControls: React.FC<{ cfg: EmbeddingCloudSettings | undefined; setSetting: (k: string, v: unknown) => void }> = ({ cfg, setSetting }) => {
  const ids = useId();
  const enabled = cfg?.learningEnabled ?? true;
  const target = cfg?.learningTargetRecall ?? 0.9;
  const rate = cfg?.learningRate ?? 0.2;
  return (
    <div style={css.section} aria-label="Learning">
      <div style={{ ...css.row, justifyContent: 'space-between' }}>
        <label style={{ ...css.row, cursor: 'pointer' }}>
          <input type="checkbox" checked={enabled} onChange={(e) => setSetting('learningEnabled', e.target.checked)} />
          <span>Learn from queries</span>
        </label>
        <button type="button" style={css.button()} onClick={() => useMemoryCloudStore.getState().resetLearning()}>Reset learning</button>
      </div>
      <div style={{ ...css.row, marginTop: 6 }}>
        <label htmlFor={`${ids}-target`} style={{ ...css.label, width: 82 }}>target recall</label>
        <input id={`${ids}-target`} type="range" min={0.5} max={1} step={0.01} value={target} disabled={!enabled}
          onChange={(e) => setSetting('learningTargetRecall', Number(e.target.value))} style={{ flex: 1 }} />
        <span style={css.mono}>{target.toFixed(2)}</span>
      </div>
      <div style={{ ...css.row, marginTop: 4 }}>
        <label htmlFor={`${ids}-rate`} style={{ ...css.label, width: 82 }}>rate</label>
        <input id={`${ids}-rate`} type="range" min={0.01} max={1} step={0.01} value={rate} disabled={!enabled}
          onChange={(e) => setSetting('learningRate', Number(e.target.value))} style={{ flex: 1 }} />
        <span style={css.mono}>{rate.toFixed(2)}</span>
      </div>
    </div>
  );
};

const HealthTab: React.FC = () => {
  const health = useMemoryCloudStore((s) => s.health);
  const healthError = useMemoryCloudStore((s) => s.healthError);
  const snapshot = useMemoryCloudStore((s) => s.snapshot);
  const flashes = useMemoryCloudStore((s) => s.flashes);

  useEffect(() => {
    const st = useMemoryCloudStore.getState();
    void st.refreshHealth();
    const id = setInterval(() => void useMemoryCloudStore.getState().refreshHealth(), HEALTH_POLL_MS);
    return () => clearInterval(id);
  }, []);

  const probe = health?.recallProbe ?? null;
  const flashTotal = flashes.key + flashes.namespace + flashes.none;
  const ok = (b: boolean | undefined) => (
    <span style={{ color: b ? ROUTE_PALETTE.mint : ROUTE_PALETTE.miss }}>{b ? 'reachable' : 'unreachable'}</span>
  );

  return (
    <div>
      {healthError && <div role="alert" style={{ color: ROUTE_PALETTE.miss }}>{healthError}</div>}
      {!health && !healthError && <span style={css.label}>Checking…</span>}
      {health && (
        <>
          <div style={{ display: 'grid', gridTemplateColumns: 'auto 1fr', gap: '4px 10px' }}>
            <span style={css.label}>sidecar</span>
            <span>
              {ok(health.sidecar.reachable)}
              {health.sidecar.extensionVersion && <span style={css.mono}> · ruvector {health.sidecar.extensionVersion}</span>}
            </span>
            {health.sidecar.error && (
              <>
                <span style={css.label}>issue</span>
                <span aria-label="Sidecar issue" style={{ color: ROUTE_PALETTE.miss }}>
                  <strong>{SIDECAR_ISSUE_TEXT[health.sidecar.error]?.label ?? 'Unknown issue'}</strong>{' '}
                  <span style={css.mono}>({health.sidecar.error})</span>
                  {SIDECAR_ISSUE_TEXT[health.sidecar.error] && (
                    <div style={{ color: '#c9cfdc', marginTop: 2 }}>{SIDECAR_ISSUE_TEXT[health.sidecar.error].detail}</div>
                  )}
                </span>
              </>
            )}
            <span style={css.label}>embedder</span>
            <span>{ok(health.embedder.reachable)} <span style={css.mono}>· {health.embedder.model}</span></span>
            <span style={css.label}>snapshot</span>
            <span style={css.mono}>
              {health.snapshotId ?? '—'}
              {health.generatedAt ? ` · ${new Date(health.generatedAt).toLocaleString('en-GB')}` : ''}
            </span>
          </div>

          <div style={css.section} aria-label="Recall probe">
            <div style={{ fontWeight: 600, marginBottom: 4 }}>Sidecar recall probe</div>
            {probe ? (
              <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: 8 }}>
                <Stat label={`recall@${probe.k}`} value={<span style={{ color: recallColour(probe.recall) }}>{probe.recall.toFixed(3)}</span>} title={`Mean over ${probe.probes} probe queries: sidecar HNSW against an exact scan`} />
                <Stat label="index" value={fmtMs(probe.indexMs)} />
                <Stat label="exact" value={fmtMs(probe.exactMs)} />
                <span style={{ ...css.label, gridColumn: '1 / -1' }}>
                  {probe.probes} probes · measured {new Date(probe.measuredAt).toLocaleString('en-GB')}
                </span>
              </div>
            ) : (
              <span style={css.label}>No probe yet; the server runs one in the background.</span>
            )}
          </div>

          <div style={css.section}>
            <div style={{ fontWeight: 600, marginBottom: 4 }}>Namespaces</div>
            <table style={{ width: '100%', borderCollapse: 'collapse', ...css.mono }} aria-label="Namespace coverage">
              <thead>
                <tr style={{ color: '#8b93a7', textAlign: 'right' }}>
                  <th style={{ textAlign: 'left', fontWeight: 400 }}>namespace</th>
                  <th style={{ fontWeight: 400 }}>total</th>
                  <th style={{ fontWeight: 400 }}>embedded</th>
                  <th style={{ fontWeight: 400 }}>sampled</th>
                </tr>
              </thead>
              <tbody>
                {health.namespaces.map((r) => (
                  <tr key={r.namespace} style={{ textAlign: 'right' }}>
                    <td style={{ textAlign: 'left', overflow: 'hidden', textOverflow: 'ellipsis', maxWidth: 150 }}>{r.namespace}</td>
                    <td>{r.total.toLocaleString('en-GB')}</td>
                    <td>{r.embedded.toLocaleString('en-GB')}</td>
                    <td>{r.sampled.toLocaleString('en-GB')}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            {(snapshot?.excludedNamespaces.length ?? 0) > 0 && (
              <p style={{ ...css.label, marginTop: 6 }}>Withheld by server policy: {snapshot!.excludedNamespaces.join(', ')}</p>
            )}
          </div>
        </>
      )}

      <div style={css.section} aria-label="Live flashes">
        <div style={{ fontWeight: 600, marginBottom: 4 }}>Live memory flashes</div>
        <span style={css.mono}>
          {flashes.key} on sampled entries · {flashes.namespace} on namespace stand-ins · {flashes.none} unmatched
          {flashTotal > 0 && ` (${pct(flashes.none / flashTotal)} unmatched)`}
        </span>
      </div>
    </div>
  );
};

const CinematicSection: React.FC = () => {
  const cinematic = useMemoryCloudStore((s) => s.cinematic);
  const hasRun = useMemoryCloudStore((s) => !!s.query.run);
  const recorder = useMemo(() => pickRecorderMime(), []);

  return (
    <div style={css.section} aria-label="Cinematic">
      <div style={{ fontWeight: 600, marginBottom: 6 }}>Cinematic</div>
      <div style={css.row}>
        {cinematic.active ? (
          <button type="button" style={css.button(true)} onClick={stopDirector}>Stop</button>
        ) : (
          <button type="button" style={css.button()} disabled={!hasRun} onClick={() => startDirector(false)}>Play director</button>
        )}
        <button
          type="button"
          style={css.button()}
          disabled={!hasRun || cinematic.active || !recorder}
          title={recorder ? `Records ${recorder.ext.toUpperCase()} in real time` : 'This browser cannot record video'}
          onClick={() => startDirector(true)}
        >
          Export video
        </button>
        {cinematic.recording && (
          <button type="button" style={css.button()} onClick={stopDirector}>Cancel</button>
        )}
      </div>
      {cinematic.recording && (
        <div style={{ marginTop: 6 }} aria-live="polite">
          <span style={css.label}>Recording {pct(cinematic.exportProgress)}</span>
          <div style={{ height: 3, background: 'rgba(255,255,255,0.08)', borderRadius: 2, marginTop: 3 }}>
            <div style={{ width: pct(cinematic.exportProgress), height: 3, background: ROUTE_PALETTE.tip, borderRadius: 2 }} />
          </div>
        </div>
      )}
      {cinematic.exportError && <div role="alert" style={{ color: ROUTE_PALETTE.miss, marginTop: 6 }}>{cinematic.exportError}</div>}
      {cinematic.lastExport && (
        <div style={{ marginTop: 6 }}>
          <a href={cinematic.lastExport.url} download={cinematic.lastExport.name} style={{ color: ROUTE_PALETTE.mint }}>
            Download {cinematic.lastExport.name}
          </a>
        </div>
      )}

      <p style={{ ...css.label, marginTop: 6 }}>
        Beat sync follows the sound source above.{' '}
        {recorder ? `Video exports as ${recorder.ext.toUpperCase()}.` : 'Video export is unavailable in this browser.'}
      </p>
    </div>
  );
};

const MemoryExplorerPanel: React.FC = () => {
  const cfg = useSettingsStore((s) => s.settings?.visualisation?.embeddingCloud) as EmbeddingCloudSettings | undefined;
  const [tab, setTab] = useState<'explore' | 'health'>('explore');
  const [collapsed, setCollapsed] = useState(false);
  const [spotifyOpen, setSpotifyOpen] = useState(false);
  const spotifyId = useId();
  useTapKey();
  const setSetting = (key: string, value: unknown) => useSettingsStore.getState().set(`${E}${key}`, value);

  return (
    <GlassPanel elevation="overlay" style={css.panel} data-testid="memory-explorer-panel" aria-label="Memory explorer">
      <div style={{ ...css.row, justifyContent: 'space-between', marginBottom: collapsed ? 0 : 10 }}>
        <span style={{ fontWeight: 600, letterSpacing: 0.2 }}>
          <span style={{ color: ROUTE_PALETTE.mint }}>●</span> Memory explorer
        </span>
        <div style={css.row}>
        <SpotifyChip open={spotifyOpen} onToggle={() => setSpotifyOpen((o) => !o)} popoverId={spotifyId} />
        <div style={css.row} role="tablist" aria-label="Explorer sections">
          {!collapsed && (['explore', 'health'] as const).map((t) => (
            <button key={t} type="button" role="tab" aria-selected={tab === t} style={css.button(tab === t)} onClick={() => setTab(t)}>
              {t === 'explore' ? 'Explore' : 'Health'}
            </button>
          ))}
          <button type="button" style={css.button()} aria-expanded={!collapsed} aria-label={collapsed ? 'Expand explorer' : 'Collapse explorer'} onClick={() => setCollapsed(!collapsed)}>
            {collapsed ? '▸' : '▾'}
          </button>
        </div>
        </div>
      </div>
      <SpotifyPopover open={spotifyOpen} id={spotifyId} />
      {!collapsed && (
        <div role="tabpanel">
          {tab === 'explore' ? <ExploreTab cfg={cfg} setSetting={setSetting} /> : <HealthTab />}
          {tab === 'explore' && <SoundSection cinematicOn={!!cfg?.cinematic} />}
          {tab === 'explore' && cfg?.cinematic && <CinematicSection />}
        </div>
      )}
    </GlassPanel>
  );
};

export default MemoryExplorerPanel;
