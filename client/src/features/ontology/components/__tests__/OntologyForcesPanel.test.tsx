/**
 * ADR-2192: the Ontology tab's master switch and strength slider are the stored physics keys
 * `ontologyConstraintsEnabled` / `ontologyConstraintStrength`, written through the settings
 * store like every other physics value. They used to be panel-local state posting to
 * /api/ontology-physics/{enable,disable,weights}, which nothing stored and the GPU actor's
 * cached copy undid.
 */
import React from 'react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';

const P = 'visualisation.graphs.knowledge.physics.';
const stored: Record<string, unknown> = {};
const setters: Record<string, ReturnType<typeof vi.fn>> = {};

vi.mock('@/features/control-center/hooks/useSettingField', () => ({
  useSettingField: (path: string) => {
    setters[path] ??= vi.fn();
    return [stored[path], setters[path]];
  },
}));
vi.mock('../../hooks/useConstraintStats', () => ({
  useConstraintStats: () => ({
    stats: {
      activeConstraints: 18000,
      totalConstraints: 18000,
      axiomsProcessed: 104489,
      constraintEvaluationCount: 1,
      lastUpdateTimeMs: 0,
      gpuFailureCount: 0,
      cpuFallbackCount: 0,
    },
    refresh: vi.fn(),
  }),
}));
vi.mock('../../store/useInferredEdgesStore', () => ({
  useInferredEdgesStore: (sel: (s: { report: { count: number }; refresh: () => void }) => unknown) =>
    sel({ report: { count: 0 }, refresh: vi.fn() }),
}));
const resync = vi.fn().mockResolvedValue(true);
vi.mock('../../services/ontologyPhysicsService', () => ({ resyncReasoning: () => resync() }));

import { OntologyForcesPanel } from '../OntologyForcesPanel';

// The Radix slider constructs a ResizeObserver; the shared test setup stubs it as a function.
class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.ResizeObserver = ResizeObserverStub as unknown as typeof ResizeObserver;

describe('OntologyForcesPanel', () => {
  beforeEach(() => {
    for (const k of Object.keys(stored)) delete stored[k];
    for (const k of Object.keys(setters)) delete setters[k];
  });

  it('shows the stored defaults (on, 60%) before settings load', () => {
    render(<OntologyForcesPanel />);
    expect(screen.getByRole('switch').getAttribute('data-state')).toBe('checked');
    expect(screen.getByText('60%')).toBeTruthy();
  });

  it('reflects the stored values and writes the switch to the stored key', () => {
    stored[`${P}ontologyConstraintsEnabled`] = true;
    stored[`${P}ontologyConstraintStrength`] = 0.25;
    render(<OntologyForcesPanel />);
    expect(screen.getByText('25%')).toBeTruthy();
    fireEvent.click(screen.getByRole('switch'));
    expect(setters[`${P}ontologyConstraintsEnabled`]).toHaveBeenCalledWith(false);
  });

  it('hides the strength slider while the stored switch is off', () => {
    stored[`${P}ontologyConstraintsEnabled`] = false;
    render(<OntologyForcesPanel />);
    expect(screen.getByRole('switch').getAttribute('data-state')).toBe('unchecked');
    expect(screen.queryByText(/Global strength/)).toBeNull();
  });
});
