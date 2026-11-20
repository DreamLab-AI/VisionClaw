/**
 * Where HTML overlays inside the 3D scene are mounted.
 *
 * drei's <Html> mounts into `events.connected` when it is given no portal. The
 * GraphCanvas event guard can connect pointer events to the <canvas> itself (a
 * start-up race with the async WebGPU renderer), and children of a <canvas> are
 * fallback content the browser never draws, so every overlay silently vanished.
 * Overlays therefore always name their host: the element containing the canvas.
 */
import { useRef, type RefObject } from 'react';
import { useThree } from '@react-three/fiber';

export function overlayHost(canvas: Element | null | undefined): HTMLElement | null {
  const parent = canvas?.parentElement;
  return parent instanceof HTMLElement ? parent : null;
}

/** A ref to the overlay host, for drei `<Html portal={...}>`. */
export function useOverlayPortal(): RefObject<HTMLElement> {
  const canvas = useThree((s) => s.gl?.domElement) as Element | undefined;
  const ref = useRef<HTMLElement>(null!);
  const host = overlayHost(canvas);
  if (host) ref.current = host;
  return ref;
}
