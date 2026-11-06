/**
 * Integration flags — pure data, so the registry (macros.ts) can read them
 * without importing the renderer. integrationSlots.ts re-exports them; flipping a
 * flag is part of the same one-file integration change.
 */

/** gv-render honours requestFit(target) (ADR-2180): enables the Fit scope menu. */
export const FIT_TARGETS_SUPPORTED = false;

/**
 * gv-render reads visualisation.graphView.labels.budget (ADR-2186). Until then the
 * Labels dial drives today's label controls (draw distance and size), so it is
 * never a dead control.
 */
export const LABEL_BUDGET_SUPPORTED = false;
