/**
 * @deprecated DORMANT SERVICE -- checkAndApplyGNNPhysics() is exported but
 * never imported or called from any component, hook, or render loop in the
 * codebase. The settings toggle (qualityGates.gnnPhysics) exists but is
 * unconnected. 52 lines of unused glue code. Consider removing alongside
 * gnnPhysics.ts in the next dead-code cleanup pass.  Audited 2026-05-09.
 *
 * GNN Physics Connector
 *
 * Connects the GNN physics module to VisionFlow's settings system.
 * Listens for gnnPhysics/ruvectorEnabled quality gate changes and
 * triggers GNN computation when appropriate.
 */

import { computeGNNWeights, applyGNNWeightsToPhysics } from './gnnPhysics';
import { useSettingsStore } from '../../../store/settingsStore';
import { createLogger } from '../../../utils/loggerConfig';

const logger = createLogger('GNNPhysicsConnector');

let lastComputeTime = 0;
const MIN_COMPUTE_INTERVAL = 5000; // Minimum 5s between GNN computations

/**
 * Check if GNN physics should be active and trigger computation if needed.
 * Called from the render loop or settings change handler.
 */
export async function checkAndApplyGNNPhysics(
  nodes: Array<{ id: string; position?: { x: number; y: number; z: number } }>,
  edges: Array<{ source: string; target: string }>,
): Promise<void> {
  const settings = useSettingsStore.getState().settings;
  const gnnEnabled = settings?.qualityGates?.gnnPhysics;
  const ruvectorEnabled = settings?.qualityGates?.ruvectorEnabled;

  if (!gnnEnabled) return;

  const now = Date.now();
  if (now - lastComputeTime < MIN_COMPUTE_INTERVAL) return;
  lastComputeTime = now;

  if (nodes.length === 0 || edges.length === 0) return;

  try {
    const result = computeGNNWeights(nodes, edges, {
      useHNSW: !!ruvectorEnabled,
    });

    // Determine base URL
    const customUrl = settings?.system?.customBackendUrl;
    const baseUrl = customUrl || window.location.origin;

    await applyGNNWeightsToPhysics(result, baseUrl);
  } catch (err) {
    logger.warn('GNN physics computation failed:', err);
  }
}
