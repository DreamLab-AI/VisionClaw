/**
 * Observe an element's size, failing open: where ResizeObserver is missing or
 * cannot be constructed (old engines, some test environments), the caller still
 * gets window-resize updates. Returns the cleanup.
 */
export function observeResize(el: Element, onResize: () => void): () => void {
  let ro: ResizeObserver | null = null;
  try {
    ro = typeof ResizeObserver === 'function' ? new ResizeObserver(() => onResize()) : null;
    ro?.observe(el);
  } catch {
    ro = null;
  }
  window.addEventListener('resize', onResize);
  return () => {
    ro?.disconnect();
    window.removeEventListener('resize', onResize);
  };
}
