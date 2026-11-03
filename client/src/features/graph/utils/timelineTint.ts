import type * as THREE from 'three';
import type { TimelineNodeStatus } from '../../../store/timelineStore';

/** Target colours for the diff states (sRGB-ish, linear-safe). */
const ADDED = { r: 0.2, g: 0.85, b: 0.35 };
const RETRACTED = { r: 0.9, g: 0.25, b: 0.2 };
/** How far a diff state pulls the base colour toward its target. */
const DIFF_MIX = 0.7;
/** Brightness kept by a node that is versioned but not valid at the instant. */
const FADED_KEEP = 0.2;

/**
 * Apply the bi-temporal timeline overlay (ADR-2022 P3) to a node's base
 * colour in place: 'faded' dims toward black, 'added' and 'retracted' pull
 * toward green and red, 'valid' and 'neutral' leave it unchanged.
 */
export function applyTimelineTint(c: THREE.Color, status: TimelineNodeStatus): void {
  switch (status) {
    case 'faded':
      c.setRGB(c.r * FADED_KEEP, c.g * FADED_KEEP, c.b * FADED_KEEP);
      return;
    case 'added':
      c.setRGB(
        c.r + (ADDED.r - c.r) * DIFF_MIX,
        c.g + (ADDED.g - c.g) * DIFF_MIX,
        c.b + (ADDED.b - c.b) * DIFF_MIX,
      );
      return;
    case 'retracted':
      c.setRGB(
        c.r + (RETRACTED.r - c.r) * DIFF_MIX,
        c.g + (RETRACTED.g - c.g) * DIFF_MIX,
        c.b + (RETRACTED.b - c.b) * DIFF_MIX,
      );
      return;
    default:
      return;
  }
}
