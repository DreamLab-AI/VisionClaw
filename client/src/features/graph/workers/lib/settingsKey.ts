/**
 * Idempotency key for GraphWorker.updateSettings: identical pushes are skipped.
 *
 * It must cover EVERY setting the worker applies. It once covered physics only, so a
 * change to just the Smooth Movement (tweening) sliders returned early and never
 * reached the worker until a physics value also changed.
 */
export function settingsIdempotencyKey(
  physics: Record<string, unknown> | undefined,
  visionclawPhysics: Record<string, unknown> | null,
  tweening: Record<string, unknown> | undefined,
): string {
  return JSON.stringify({ gs: physics, vf: visionclawPhysics, tw: tweening });
}
