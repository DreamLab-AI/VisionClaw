/**
 * Selective bloom layer (ADR-2182 D4).
 *
 * Under `visualisation.graphView.nodes.bloomScope = 'emphasis'` (the default)
 * GemPostProcessing blooms only what is on this layer: the emphasis halo
 * sprites (selected / hovered / search hits / focus neighbours) and the
 * selection's glass edges. Objects keep layer 0 as well, so the main pass
 * still draws them; the bloom pass renders from a camera that sees this layer
 * alone, so the memory cloud and the rest of the scene never feed the bloom.
 */
import type * as THREE from 'three';

export const BLOOM_LAYER = 11;

/** Put `obj` on the bloom layer (in addition to its current layers). */
export function enableBloom(obj: THREE.Object3D, on = true): void {
  if (on) obj.layers.enable(BLOOM_LAYER);
  else obj.layers.disable(BLOOM_LAYER);
}
