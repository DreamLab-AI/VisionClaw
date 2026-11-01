/**
 * Keeps a server-side drag hold alive while the pointer is still.
 *
 * The server releases a hold after DEFAULT_DRAG_TIMEOUT_MS (2000 ms) with no
 * `nodeDragUpdate` (ADR-2169). The desktop only sends updates on pointer move,
 * so a node held still with the mouse button down would be dropped. This
 * re-sends the current position every `intervalMs` until stopped.
 */
export const DRAG_HEARTBEAT_MS = 500;

export function startDragHeartbeat(
  send: () => void,
  intervalMs: number = DRAG_HEARTBEAT_MS
): () => void {
  const id = setInterval(send, intervalMs);
  return () => clearInterval(id);
}
