/**
 * OntologyForcesPanel — the intuitive control surface for the GPU-resident
 * ontology-constraint forces (PRD-018 / ADR-098). This is the client's single
 * place to *see* and *steer* the semantic physics that the CUDA live-kernel is
 * solving:
 *
 *   - live readout of active constraints, axioms processed, inferred edges and
 *     GPU health (failures / CPU fallbacks),
 *   - master enable/disable and a global strength slider: the stored physics keys
 *     `ontologyConstraintsEnabled` / `ontologyConstraintStrength` (ADR-2192), the same
 *     values as Layout › Forces › Communities & constraints; the server removes the
 *     constraints from the GPU when off and re-applies them when on,
 *   - a "Re-sync reasoning" action that re-runs the full ingest→reason→dispatch
 *     pipeline server-side (POST /api/admin/sync).
 *
 * It performs NO layout and NO solving client-side — every control mutates the
 * GPU-resident engine and reads back its live stats (binding constraint A:
 * all solving GPU-resident). The previous per-group sliders mutated only local
 * Zustand state and drove nothing; this panel replaces that dead surface with
 * controls wired to the real engine.
 */

import React, { useCallback, useEffect, useState } from 'react';
import { Card } from '@/features/design-system/components/Card';
import { Switch } from '@/features/design-system/components/Switch';
import { Slider } from '@/features/design-system/components/Slider';
import { Label } from '@/features/design-system/components/Label';
import { Badge } from '@/features/design-system/components/Badge';
import { Button } from '@/features/design-system/components/Button';
import { Separator } from '@/features/design-system/components/Separator';
import { RefreshCw, Zap, GitBranch, Sigma, Network, AlertTriangle, Cpu, ArrowRightLeft, Minus, Magnet } from 'lucide-react';
import { useConstraintStats } from '../hooks/useConstraintStats';
import { useInferredEdgesStore } from '../store/useInferredEdgesStore';
import { resyncReasoning } from '../services/ontologyPhysicsService';
import { useSettingField } from '@/features/control-center/hooks/useSettingField';
import { useSettingsStore } from '@/store/settingsStore';

const PHYSICS = 'visualisation.graphs.knowledge.physics.';
/** The stored defaults (crates/visionclaw-domain physics_config.rs), shown until settings load. */
const DEFAULT_ENABLED = true;
const DEFAULT_STRENGTH = 0.6;
/** Physics settings load lazily (per control-center section); this panel needs these two. */
export const ONTOLOGY_POLICY_PATHS = [
  `${PHYSICS}ontologyConstraintsEnabled`,
  `${PHYSICS}ontologyConstraintStrength`,
];

/** Semantic relation → GPU force legend (mirrors ontology_constraint_mapper). */
const FORCE_LEGEND: Array<{ icon: React.ReactNode; relation: string; force: string; color: string }> = [
  { icon: <Magnet className="w-3.5 h-3.5" />, relation: 'SubClassOf · hasPart · partOf', force: 'Attract', color: '#22c55e' },
  { icon: <ArrowRightLeft className="w-3.5 h-3.5" />, relation: 'EquivalentClass · sameAs', force: 'Colocate', color: '#3b82f6' },
  { icon: <Minus className="w-3.5 h-3.5" />, relation: 'DisjointWith', force: 'Separate', color: '#ef4444' },
];

export function OntologyForcesPanel() {
  const { stats, refresh: refreshStats } = useConstraintStats(5000);
  const inferredCount = useInferredEdgesStore(s => s.report.count);
  const refreshInferred = useInferredEdgesStore(s => s.refresh);

  const hasForces = stats.activeConstraints > 0;
  // ADR-2192: one stored definition. Writes go through the settings store's physics PUT,
  // which the server applies to the constraint actor; there is no local copy to drift.
  const [storedEnabled, setStoredEnabled] = useSettingField<boolean>(`${PHYSICS}ontologyConstraintsEnabled`);
  const [storedStrength, setStoredStrength] = useSettingField<number>(`${PHYSICS}ontologyConstraintStrength`);
  // Hydrate the stored values even if the Layout section was never opened, so the panel
  // shows (and a first toggle starts from) what is stored, not the defaults.
  useEffect(() => {
    void useSettingsStore
      .getState()
      .ensureLoaded(ONTOLOGY_POLICY_PATHS)
      .catch(() => {
        /* fail-open: the row stays on the default until the store answers */
      });
  }, []);
  const enabled = storedEnabled ?? DEFAULT_ENABLED;
  const strength = storedStrength ?? DEFAULT_STRENGTH;
  const [resyncState, setResyncState] = useState<'idle' | 'running' | 'done' | 'error'>('idle');

  const onToggle = useCallback((next: boolean) => {
    setStoredEnabled(next);
    // The constraint count follows once the store has saved and the server applied it.
    window.setTimeout(() => refreshStats(), 1500);
  }, [setStoredEnabled, refreshStats]);

  const onStrengthChange = useCallback((value: number) => {
    setStoredStrength(value);
  }, [setStoredStrength]);

  const onResync = useCallback(async () => {
    setResyncState('running');
    const ok = await resyncReasoning();
    if (ok) {
      setResyncState('done');
      // The pipeline re-dispatches constraints + re-materialises inferred axioms;
      // pull both fresh after a short settle so the readouts reflect the re-run.
      window.setTimeout(() => { refreshStats(); void refreshInferred(); }, 1500);
      window.setTimeout(() => setResyncState('idle'), 4000);
    } else {
      setResyncState('error');
      window.setTimeout(() => setResyncState('idle'), 4000);
    }
  }, [refreshStats, refreshInferred]);

  return (
    <div className="space-y-4" role="region" aria-label="Ontology forces">
      {/* Master control */}
      <Card className={`p-4 transition-all ${enabled ? 'border-amber-500' : 'border-gray-700'}`}>
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <Switch
              checked={enabled}
              onCheckedChange={onToggle}
              id="ontology-forces-enabled"
            />
            <div>
              <Label htmlFor="ontology-forces-enabled" className="text-base font-medium flex items-center gap-2">
                <Zap className="w-4 h-4 text-amber-400" />
                Ontology Forces
              </Label>
              <p className="text-xs text-gray-400">
                GPU-resident semantic layout from OWL axioms
              </p>
            </div>
          </div>
          <Badge variant={hasForces ? 'default' : 'secondary'}>
            {stats.activeConstraints.toLocaleString()} live
          </Badge>
        </div>

        {enabled && (
          <div className="mt-4 space-y-2">
            <div className="flex items-center justify-between">
              <Label htmlFor="ontology-force-strength" className="text-sm">Global strength</Label>
              <span className="text-sm font-medium text-gray-300">{(strength * 100).toFixed(0)}%</span>
            </div>
            <Slider
              id="ontology-force-strength"
              min={0}
              max={1}
              step={0.05}
              value={[strength]}
              onValueChange={(v) => onStrengthChange(v[0])}
              className="w-full"
            />
          </div>
        )}
      </Card>

      {/* Live engine readout */}
      <div className="grid grid-cols-2 gap-2">
        <StatTile icon={<Zap className="w-4 h-4" />} label="Active constraints" value={stats.activeConstraints} accent="#22c55e" />
        <StatTile icon={<Sigma className="w-4 h-4" />} label="Axioms processed" value={stats.axiomsProcessed} accent="#F2C14E" />
        <StatTile icon={<Network className="w-4 h-4" />} label="Inferred edges" value={inferredCount} accent="#FBBF24" />
        <StatTile icon={<Cpu className="w-4 h-4" />} label="GPU evaluations" value={stats.constraintEvaluationCount} accent="#60a5fa" />
      </div>

      {(stats.gpuFailureCount > 0 || stats.cpuFallbackCount > 0) && (
        <Card className="p-3 border-amber-600/40">
          <div className="flex items-center gap-2 text-xs text-amber-400">
            <AlertTriangle className="w-4 h-4" />
            <span>
              GPU health: {stats.gpuFailureCount} failure(s), {stats.cpuFallbackCount} CPU fallback(s)
            </span>
          </div>
        </Card>
      )}

      <Separator />

      {/* Force legend */}
      <div>
        <p className="text-xs font-medium text-gray-400 mb-2 flex items-center gap-1.5">
          <GitBranch className="w-3.5 h-3.5" /> How relations become forces
        </p>
        <div className="space-y-1.5">
          {FORCE_LEGEND.map((row) => (
            <div key={row.force} className="flex items-center gap-2 text-xs">
              <span style={{ color: row.color }} className="flex items-center">{row.icon}</span>
              <span className="text-gray-300 flex-1">{row.relation}</span>
              <Badge variant="secondary" style={{ color: row.color }}>{row.force}</Badge>
            </div>
          ))}
        </div>
      </div>

      <Separator />

      {/* Re-sync pipeline */}
      <div className="flex items-center justify-between gap-3">
        <div className="flex-1">
          <p className="text-sm font-medium text-gray-200">Re-sync reasoning</p>
          <p className="text-xs text-gray-400">
            Re-run ingest → Whelk inference → GPU constraint dispatch
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={onResync}
          disabled={resyncState === 'running'}
          aria-label="Re-sync ontology reasoning"
        >
          <RefreshCw className={`w-4 h-4 mr-1.5 ${resyncState === 'running' ? 'animate-spin' : ''}`} />
          {resyncState === 'running' ? 'Syncing…'
            : resyncState === 'done' ? 'Done'
            : resyncState === 'error' ? 'Failed'
            : 'Re-sync'}
        </Button>
      </div>
    </div>
  );
}

const StatTile: React.FC<{ icon: React.ReactNode; label: string; value: number; accent: string }> = ({
  icon, label, value, accent,
}) => (
  <Card className="p-3">
    <div className="flex items-center gap-2" style={{ color: accent }}>
      {icon}
      <span className="text-lg font-bold tabular-nums">{value.toLocaleString()}</span>
    </div>
    <p className="text-xs text-gray-400 mt-0.5">{label}</p>
  </Card>
);

export default OntologyForcesPanel;
