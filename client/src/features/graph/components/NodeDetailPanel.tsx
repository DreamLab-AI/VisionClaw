import React, { useState, useEffect, useCallback } from 'react';
import { graphDataManager, type GraphData, type Node } from '../managers/graphDataManager';
import { useSettingsStore } from '../../../store/settingsStore';
import { nodePageUrl, nodePageSlug } from '../utils/pageLinks';
import { createLogger } from '../../../utils/loggerConfig';

const logger = createLogger('NodeDetailPanel');

/** Reference to a related page: shared shape across subClassOf / relationships. */
interface PageRef { id?: string; label?: string; slug?: string }

/**
 * Subset of the narrativegoldmine `api/pages/<slug>.json` card the panel renders.
 * The site is the source-of-truth page store; this mirrors the fields we surface.
 */
interface SourcePageCard {
  title?: string;
  definition?: string;
  domain?: string;
  maturity?: string;
  subClassOf?: PageRef[];
  relationships?: Record<string, PageRef[]>;
  backlinks?: PageRef[];
  wikilinks?: PageRef[];
}

const NGM_PAGES_BASE = 'https://narrativegoldmine.com/api/pages/';

/**
 * Assertion-version attribution (ADR-049) threaded into the node-selected event
 * by GraphManager/useGraphSelection. The panel renders it only — it never
 * fetches. `signatureValid` reflects native-envelope signature verification;
 * the agent identity is the authenticated principal (did:nostr), never a
 * client-chosen field.
 */
export interface NodeAttribution {
  didNostr: string;
  activityUrn: string;
  generatedAtTime: string;
  signatureValid: boolean;
}

export interface NodeSelectionDetail {
  nodeId: string;
  label: string;
  metadata?: Record<string, any>;
  connectionCount: number;
  neighbors: Array<{ id: string; label: string }>;
  attribution?: NodeAttribution;
}

/**
 * Slide-in panel that displays details for the currently selected graph node.
 * Listens for 'visionclaw:node-selected' custom events dispatched by GraphManager.
 */
export interface NodeDetailPanelProps {
  /**
   * Render inside the control centre's right-hand drawer (ADR-2189 §3): no fixed
   * positioning or slide-in, no own close button (the drawer owns closing, Esc
   * and focus). Standalone (the default) keeps the legacy slide-in panel.
   */
  embedded?: boolean;
}

export const NodeDetailPanel: React.FC<NodeDetailPanelProps> = ({ embedded = false }) => {
  const [detail, setDetail] = useState<NodeSelectionDetail | null>(null);
  const [visible, setVisible] = useState(false);
  // W-G phase-1 client-only flag (default-off). When off, the attribution
  // section renders nothing even if the payload carries attribution.
  const showAttribution = useSettingsStore(
    s => s.settings?.provenance?.showAttribution ?? false
  );

  // Graph↔source panel: the narrativegoldmine page card for the selected node.
  const [sourceCard, setSourceCard] = useState<SourcePageCard | null>(null);
  const [sourceStatus, setSourceStatus] = useState<'idle' | 'loading' | 'error' | 'missing' | 'ready'>('idle');

  const handleNodeSelected = useCallback((event: Event) => {
    const customEvent = event as CustomEvent<NodeSelectionDetail | null>;
    const payload = customEvent.detail;
    if (payload) {
      // Not every producer sends the full shape: an agent-marker selection
      // (bots/agentSelection.ts) has no neighbors or connectionCount.
      setDetail({
        ...payload,
        neighbors: Array.isArray(payload.neighbors) ? payload.neighbors : [],
        connectionCount: typeof payload.connectionCount === 'number' ? payload.connectionCount : 0,
      });
      setVisible(true);
    } else {
      setVisible(false);
    }
  }, []);

  // Fetch the source page card whenever the selection changes. Keyed on nodeId so
  // it refetches per selection. Fail-open: any error → 'error'/'missing', never
  // throws into the panel. An in-flight guard drops stale responses.
  useEffect(() => {
    if (!detail) { setSourceCard(null); setSourceStatus('idle'); return; }
    const slug = nodePageSlug(detail as Parameters<typeof nodePageSlug>[0]);
    if (!slug) { setSourceCard(null); setSourceStatus('missing'); return; }

    let cancelled = false;
    setSourceCard(null);
    setSourceStatus('loading');
    const url = `${NGM_PAGES_BASE}${encodeURIComponent(slug)}.json`;
    fetch(url, { headers: { Accept: 'application/json' } })
      .then(res => {
        if (res.status === 404) { if (!cancelled) setSourceStatus('missing'); return null; }
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        return res.json();
      })
      .then((json: SourcePageCard | null) => {
        if (cancelled || json === null) return;
        setSourceCard(json);
        setSourceStatus('ready');
      })
      .catch(err => {
        if (cancelled) return;
        logger.warn(`Source page fetch failed for "${slug}":`, err);
        setSourceStatus('error');
      });

    return () => { cancelled = true; };
  }, [detail]);

  useEffect(() => {
    window.addEventListener('visionclaw:node-selected', handleNodeSelected);
    return () => {
      window.removeEventListener('visionclaw:node-selected', handleNodeSelected);
    };
  }, [handleNodeSelected]);

  const handleClose = useCallback(() => {
    setVisible(false);
    // Dispatch deselection so GraphManager clears highlight edges
    window.dispatchEvent(new CustomEvent('visionclaw:node-deselect'));
  }, []);

  const handleNeighborClick = useCallback((neighborId: string) => {
    // Dispatch a search event to fly to the neighbor and select it
    window.dispatchEvent(new CustomEvent('visionclaw:search', {
      detail: { query: '', nodeId: neighborId },
    }));
  }, []);

  const handleOpenFullPage = useCallback(() => {
    if (!detail) return;
    // Slug-first resolution against the path-routed site (see pageLinks.ts);
    // the legacy #/page/<Title> hash route no longer resolves.
    const url = nodePageUrl(detail as Parameters<typeof nodePageUrl>[0]);
    if (url) {
      window.open(url, '_blank', 'noopener,noreferrer');
    }
  }, [detail]);

  if (!detail) return null;

  const contentPreview = extractContentPreview(detail.metadata);

  return (
    <div
      role={embedded ? undefined : 'complementary'}
      aria-label={embedded ? undefined : 'Node details'}
      data-testid="node-detail-panel"
      style={embedded ? {
        display: 'flex',
        flexDirection: 'column',
        minHeight: 0,
        height: '100%',
        color: '#e0e0e0',
        fontFamily: '"Inter", "Segoe UI", sans-serif',
        fontSize: 13,
        overflow: 'hidden',
      } : {
        position: 'fixed',
        top: 0,
        right: visible ? 0 : -340,
        width: 320,
        height: '100vh',
        backgroundColor: 'rgba(10, 10, 30, 0.92)',
        backdropFilter: 'blur(12px)',
        borderLeft: '1px solid rgba(255, 255, 255, 0.1)',
        color: '#e0e0e0',
        fontFamily: '"Inter", "Segoe UI", sans-serif',
        fontSize: 13,
        zIndex: 1100,
        transition: 'right 0.25s ease-out',
        display: 'flex',
        flexDirection: 'column',
        overflow: 'hidden',
        pointerEvents: 'auto',
      }}
    >
      {/* Header */}
      <div style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        padding: '16px 16px 12px',
        borderBottom: '1px solid rgba(255, 255, 255, 0.08)',
      }}>
        <h2 style={{
          margin: 0,
          fontSize: 15,
          fontWeight: 600,
          color: '#ffffff',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
          flex: 1,
          marginRight: 8,
        }}>
          {detail.label}
        </h2>
        {!embedded && (
        <button
          onClick={handleClose}
          aria-label="Close node details"
          style={{
            background: 'rgba(255, 255, 255, 0.06)',
            border: '1px solid rgba(255, 255, 255, 0.12)',
            borderRadius: 4,
            color: '#aaa',
            cursor: 'pointer',
            fontSize: 16,
            width: 28,
            height: 28,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            flexShrink: 0,
          }}
        >
          x
        </button>
        )}
      </div>

      {/* Content */}
      <div style={{ flex: 1, overflowY: 'auto', padding: '12px 16px' }}>
        {/* Metadata badges */}
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', marginBottom: 12 }}>
          <Badge label="Connections" value={String(detail.connectionCount)} />
          {detail.metadata?.type && (
            <Badge label="Type" value={detail.metadata.type} />
          )}
          {detail.metadata?.domain && (
            <Badge label="Domain" value={detail.metadata.domain} />
          )}
        </div>

        {/* Attribution (ADR-049 assertion-version provenance) */}
        {showAttribution && detail.attribution && (
          <div style={{ marginBottom: 14 }}>
            <h3 style={{
              margin: '0 0 8px',
              fontSize: 12,
              fontWeight: 600,
              textTransform: 'uppercase',
              letterSpacing: 0.5,
              color: '#888',
            }}>
              Attribution
            </h3>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
              <Badge label="Agent" value={truncateMiddle(detail.attribution.didNostr, 28)} />
              <Badge label="Activity" value={truncateMiddle(detail.attribution.activityUrn, 28)} />
              <Badge
                label="Generated"
                value={formatTimestamp(detail.attribution.generatedAtTime)}
              />
              <span style={{
                display: 'inline-flex',
                alignItems: 'center',
                gap: 6,
                alignSelf: 'flex-start',
                padding: '3px 10px',
                borderRadius: 999,
                fontSize: 11,
                fontWeight: 600,
                border: `1px solid ${detail.attribution.signatureValid ? 'rgba(78, 205, 120, 0.5)' : 'rgba(230, 180, 60, 0.5)'}`,
                backgroundColor: detail.attribution.signatureValid ? 'rgba(78, 205, 120, 0.15)' : 'rgba(230, 180, 60, 0.15)',
                color: detail.attribution.signatureValid ? '#6fdca0' : '#e6c24a',
              }}>
                {detail.attribution.signatureValid ? 'Signature verified' : 'Signature unverified'}
              </span>
            </div>
          </div>
        )}

        {/* Content preview */}
        {contentPreview && (
          <div style={{
            backgroundColor: 'rgba(255, 255, 255, 0.04)',
            borderRadius: 6,
            padding: '10px 12px',
            marginBottom: 14,
            lineHeight: 1.5,
            fontSize: 12,
            color: '#c0c0c0',
          }}>
            {contentPreview}
          </div>
        )}

        {/* Graph↔source panel — narrativegoldmine page card for this node */}
        <SourceSection status={sourceStatus} card={sourceCard} />

        {/* Neighbors */}
        {detail.neighbors.length > 0 && (
          <div>
            <h3 style={{
              margin: '0 0 8px',
              fontSize: 12,
              fontWeight: 600,
              textTransform: 'uppercase',
              letterSpacing: 0.5,
              color: '#888',
            }}>
              Connected Nodes ({detail.neighbors.length})
            </h3>
            <ul style={{ listStyle: 'none', padding: 0, margin: 0 }}>
              {detail.neighbors.slice(0, 30).map(n => (
                <li key={n.id}>
                  <button
                    onClick={() => handleNeighborClick(n.id)}
                    style={{
                      display: 'block',
                      width: '100%',
                      textAlign: 'left',
                      background: 'transparent',
                      border: 'none',
                      borderBottom: '1px solid rgba(255, 255, 255, 0.04)',
                      color: '#8ecfff',
                      cursor: 'pointer',
                      padding: '6px 4px',
                      fontSize: 12,
                      fontFamily: 'inherit',
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                      whiteSpace: 'nowrap',
                    }}
                  >
                    {n.label}
                  </button>
                </li>
              ))}
              {detail.neighbors.length > 30 && (
                <li style={{ padding: '6px 4px', color: '#666', fontSize: 11 }}>
                  ...and {detail.neighbors.length - 30} more
                </li>
              )}
            </ul>
          </div>
        )}
      </div>

      {/* Footer: Open full page */}
      <div style={{
        padding: '12px 16px',
        borderTop: '1px solid rgba(255, 255, 255, 0.08)',
      }}>
        <button
          onClick={handleOpenFullPage}
          style={{
            width: '100%',
            padding: '8px 12px',
            backgroundColor: 'rgba(78, 205, 196, 0.15)',
            border: '1px solid rgba(78, 205, 196, 0.3)',
            borderRadius: 6,
            color: '#4ECDC4',
            cursor: 'pointer',
            fontSize: 13,
            fontFamily: 'inherit',
            fontWeight: 500,
          }}
        >
          Open full page
        </button>
      </div>
    </div>
  );
};

const sectionHeadingStyle: React.CSSProperties = {
  margin: '0 0 8px',
  fontSize: 12,
  fontWeight: 600,
  textTransform: 'uppercase',
  letterSpacing: 0.5,
  color: '#888',
};

/** Fly to a related page's node by label (reuses the search focus contract). */
function focusByLabel(label?: string) {
  if (!label) return;
  window.dispatchEvent(new CustomEvent('visionclaw:search', { detail: { query: label } }));
}

/**
 * Renders the narrativegoldmine source card synced to the current selection:
 * definition, ontology parents (subClassOf), typed relationships, and links.
 * Related entities are clickable — they refocus the graph on the matching node.
 */
const SourceSection: React.FC<{ status: string; card: SourcePageCard | null }> = ({ status, card }) => {
  if (status === 'loading') {
    return <div style={{ marginBottom: 14, color: '#888', fontSize: 12 }}>Loading source page…</div>;
  }
  if (status === 'missing') {
    return <div style={{ marginBottom: 14, color: '#777', fontSize: 12 }}>No source page for this node.</div>;
  }
  if (status === 'error') {
    return <div style={{ marginBottom: 14, color: '#e6a24a', fontSize: 12 }}>Source page unavailable.</div>;
  }
  if (status !== 'ready' || !card) return null;

  const relationships = card.relationships || {};
  const relEntries = Object.entries(relationships).filter(([, v]) => Array.isArray(v) && v.length > 0);
  const links = (card.backlinks || []).concat(card.wikilinks || []);

  return (
    <div style={{ marginBottom: 14 }}>
      <h3 style={sectionHeadingStyle}>Source Page</h3>

      {(card.domain || card.maturity) && (
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', marginBottom: 10 }}>
          {card.domain && <Badge label="Domain" value={card.domain} />}
          {card.maturity && <Badge label="Maturity" value={card.maturity} />}
        </div>
      )}

      {card.definition && (
        <div style={{
          backgroundColor: 'rgba(78, 205, 196, 0.06)',
          borderLeft: '2px solid rgba(78, 205, 196, 0.4)',
          borderRadius: 4,
          padding: '8px 10px',
          marginBottom: 12,
          lineHeight: 1.5,
          fontSize: 12,
          color: '#cfe6e3',
        }}>
          {card.definition}
        </div>
      )}

      {card.subClassOf && card.subClassOf.length > 0 && (
        <div style={{ marginBottom: 10 }}>
          <div style={{ color: '#777', fontSize: 11, marginBottom: 4 }}>Subclass of</div>
          <RefChips refs={card.subClassOf} />
        </div>
      )}

      {relEntries.length > 0 && (
        <div style={{ marginBottom: 10 }}>
          <div style={{ color: '#777', fontSize: 11, marginBottom: 6 }}>Relationships</div>
          {relEntries.map(([predicate, refs]) => (
            <div key={predicate} style={{ marginBottom: 6 }}>
              <span style={{ color: '#6fdca0', fontSize: 11, marginRight: 6 }}>{prettifyPredicate(predicate)}</span>
              <RefChips refs={refs} />
            </div>
          ))}
        </div>
      )}

      {links.length > 0 && (
        <div>
          <div style={{ color: '#777', fontSize: 11, marginBottom: 4 }}>Links ({links.length})</div>
          <RefChips refs={links.slice(0, 24)} />
          {links.length > 24 && (
            <span style={{ color: '#666', fontSize: 11 }}> …and {links.length - 24} more</span>
          )}
        </div>
      )}
    </div>
  );
};

/** Clickable chips for related page refs; clicking refocuses the graph. */
const RefChips: React.FC<{ refs: PageRef[] }> = ({ refs }) => (
  <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
    {refs.map((r, i) => (
      <button
        key={`${r.id ?? r.slug ?? r.label ?? i}`}
        onClick={() => focusByLabel(r.label)}
        title={`Focus "${r.label ?? ''}"`}
        style={{
          background: 'rgba(142, 207, 255, 0.08)',
          border: '1px solid rgba(142, 207, 255, 0.2)',
          borderRadius: 4,
          color: '#8ecfff',
          cursor: 'pointer',
          padding: '3px 8px',
          fontSize: 11,
          fontFamily: 'inherit',
          maxWidth: 160,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}
      >
        {r.label ?? r.slug ?? r.id}
      </button>
    ))}
  </div>
);

/** camelCase predicate → spaced words ("dependsOn" → "depends on"). */
function prettifyPredicate(p: string): string {
  return p.replace(/([a-z])([A-Z])/g, '$1 $2').replace(/[_-]+/g, ' ').toLowerCase();
}

const Badge: React.FC<{ label: string; value: string }> = ({ label, value }) => (
  <span style={{
    display: 'inline-flex',
    alignItems: 'center',
    gap: 4,
    padding: '3px 8px',
    backgroundColor: 'rgba(255, 255, 255, 0.06)',
    borderRadius: 4,
    fontSize: 11,
    color: '#bbb',
  }}>
    <span style={{ color: '#777' }}>{label}:</span>
    <span style={{ color: '#ddd' }}>{value}</span>
  </span>
);

/** Middle-ellipsis a long mono identifier (did / URN) to `max` chars. */
function truncateMiddle(value: string, max: number): string {
  if (!value || value.length <= max) return value;
  const keep = Math.max(4, Math.floor((max - 1) / 2));
  return `${value.slice(0, keep)}…${value.slice(-keep)}`;
}

/** Render an ISO prov:generatedAtTime as a locale string; fall back to raw. */
function formatTimestamp(iso: string): string {
  const d = new Date(iso);
  return isNaN(d.getTime()) ? iso : d.toLocaleString();
}

function extractContentPreview(metadata?: Record<string, any>): string | null {
  if (!metadata) return null;
  const content = metadata.content || metadata.description || metadata.summary
    || metadata.body || metadata.text || metadata.excerpt;
  if (!content || typeof content !== 'string') return null;
  return content.length > 300 ? content.slice(0, 300) + '...' : content;
}

export default NodeDetailPanel;
