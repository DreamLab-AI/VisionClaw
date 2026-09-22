/**
 * Troika text-builder configuration — must run before the first <Text> mounts.
 *
 * This page is cross-origin isolated (COOP: same-origin + COEP: require-corp,
 * served by nginx for the SharedArrayBuffer physics pipeline). Under
 * `require-corp`, Chromium blocks `importScripts()` of blob: URLs inside
 * blob-created workers (crbug.com/1084951) — exactly how troika-worker-utils
 * boots its glyph-layout worker. The failure is invisible until the first
 * troika <Text> mounts (agent labels), then every label errors with
 * "worker module init function failed to rehydrate" and the canvas dies.
 *
 * Remedy per troika's own docs: build text on the main thread. Label counts on
 * the troika path are small (per-agent labels; bulk KG labels use the custom
 * InstancedLabels glyph system), so the main-thread cost is negligible.
 */
// troika-three-text ships no type declarations; the single call below is the
// entire surface we touch.
// @ts-ignore
import { configureTextBuilder } from 'troika-three-text';

if (typeof self !== 'undefined' && self.crossOriginIsolated) {
  configureTextBuilder({ useWorker: false });
}
