/**
 * useDockCompact — fold the dock's dials into one menu when the dock does not fit.
 *
 * The dock body is capped at the viewport width (control-center.css
 * `.cc-dock-body`). When its content overflows that cap, `compact` turns on and
 * MacroBar renders the five dials behind a single "Dials" button. It turns off
 * again only when the viewport is clearly wider than the width at which the
 * overflow happened (hysteresis), so the dock does not flicker at the boundary.
 * At 1024 px the old dock started 74 px off-screen (audit finding 4); this makes
 * that state impossible rather than tuning one breakpoint.
 */
import { useLayoutEffect, useRef, useState } from 'react';
import { observeResize } from '../primitives/observeResize';

const HYSTERESIS_PX = 48;

export function useDockCompact<T extends HTMLElement>(): { ref: React.RefObject<T | null>; compact: boolean } {
  const ref = useRef<T>(null);
  const [compact, setCompact] = useState(false);
  const overflowAtRef = useRef<number | null>(null);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || typeof window === 'undefined') return;
    // The element whose width is capped: the dock body (GlassPanel), else the bar itself.
    const box = (el.closest('.cc-dock-body') as HTMLElement | null) ?? el;

    const check = () => {
      const overflowing = box.scrollWidth > box.clientWidth + 1;
      if (!compact && overflowing) {
        overflowAtRef.current = window.innerWidth;
        setCompact(true);
      } else if (compact && overflowAtRef.current !== null && window.innerWidth > overflowAtRef.current + HYSTERESIS_PX) {
        overflowAtRef.current = null;
        setCompact(false);
      }
    };

    check();
    return observeResize(box, check);
  }, [compact]);

  return { ref, compact };
}
