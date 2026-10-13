/**
 * Turn a registry path under `visualisation.graphs.<graph>.physics.` into the partial
 * document the physics PUT expects. Segments below `.physics.` nest, so
 * `...physics.semantic.dag.levelAttraction` becomes `{ semantic: { dag: { levelAttraction } } }`
 * and the server (which merges nested objects deeply) changes only that field. Using just the
 * last path segment, as this once did, wrote a key no server field has.
 */
import { setNestedFromDotPath } from './schemaMappings';

export function physicsPatchEntry(path: string, value: unknown): Record<string, unknown> | null {
  const marker = '.physics.';
  const at = path.indexOf(marker);
  if (!path.startsWith('visualisation.graphs.') || at < 0) return null;
  const relative = path.slice(at + marker.length);
  if (!relative) return null;
  const patch: Record<string, unknown> = {};
  setNestedFromDotPath(patch, relative, value);
  return patch;
}
