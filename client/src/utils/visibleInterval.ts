/**
 * `setInterval` that does not tick while the tab is hidden, and fires once on
 * return to the tab. Each tick of a signed poll costs one NIP-98 signature (one
 * extension prompt with a NIP-07 signer), so polling a tab nobody is looking at
 * is pure cost. Returns a stop function.
 */
export function visibleInterval(fn: () => void, ms: number): () => void {
  const id = window.setInterval(() => {
    if (!document.hidden) fn();
  }, ms);
  const onVisible = () => {
    if (!document.hidden) fn();
  };
  document.addEventListener('visibilitychange', onVisible);
  return () => {
    window.clearInterval(id);
    document.removeEventListener('visibilitychange', onVisible);
  };
}
