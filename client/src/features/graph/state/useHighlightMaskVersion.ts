/**
 * React binding for the highlight mask (ADR-2183). Re-renders the caller when the
 * mask recomposes; read the bytes with `mask.getMask()` (in a frame loop, prefer
 * comparing `mask.version` over re-rendering).
 */
import { useSyncExternalStore } from 'react';
import { highlightMask, type HighlightMask } from './highlightMask';

export function useHighlightMaskVersion(mask: HighlightMask = highlightMask): number {
  return useSyncExternalStore(
    (onChange) => mask.subscribe(onChange),
    () => mask.version,
    () => mask.version,
  );
}
