/**
 * Group 8 — System & Developer (id `system`, hotkey 8, 17 fields).
 * Renderer toggle (Effects tab), authentication + network (System tab), and the
 * debug logging switches (Developer tab). `broadcastFps` is a server physics
 * setting (physics bucket) shown under Network because it sets the per-client
 * position-stream bandwidth, not the simulation. `rendererInfo` is a readonly readout of
 * `rendererCapabilities`; `nostr` is the NIP-07 login button bound to `auth.nostr`.
 */
import type { GroupData, RegistryField } from '../types';

const D = 'system.debug.';

const fields: RegistryField[] = [
  // Renderer
  { key: 'webgpuRenderer', subgroup: 'Renderer', label: 'WebGPU Renderer', type: 'action-button', action: 'toggle-webgpu', description: 'Switch between WebGPU (TSL materials) and WebGL renderer. Page reloads on change.' },
  { key: 'rendererInfo', subgroup: 'Renderer', label: 'Renderer Info', type: 'readonly', path: 'rendererCapabilities', description: 'Active renderer backend and GPU info' },
  // Authentication
  { key: 'nostr', subgroup: 'Authentication', label: 'Nostr Login', type: 'nostr-button', path: 'auth.nostr', description: 'Connect with Nostr' },
  { key: 'authEnabled', subgroup: 'Authentication', label: 'Auth Enabled', type: 'toggle', path: 'auth.enabled', description: 'Enable authentication' },
  { key: 'authRequired', subgroup: 'Authentication', label: 'Auth Required', type: 'toggle', path: 'auth.required', description: 'Require authentication' },
  // Network
  { key: 'customBackendURL', subgroup: 'Network', label: 'Custom Backend URL', type: 'text', path: 'system.customBackendUrl', description: 'Override backend URL' },
  { key: 'broadcastFps', subgroup: 'Network', label: 'Position Broadcast Rate (Hz)', type: 'slider', min: 1, max: 60, step: 1, path: 'visualisation.graphs.knowledge.physics.broadcastFps', description: 'How often the server sends every client a full position snapshot (default 8 Hz; nodes tween in between). Applies live, to all clients. Cost is linear in the rate, ~0.49 MB per snapshot on the ~9.5k-node graph: ~3.9 MB/s per client at 8 Hz, ~12 MB/s per client at 25 Hz.' },
  // Debug Logging
  { key: 'enableDebug', subgroup: 'Debug Logging', label: 'Debug Mode', type: 'toggle', path: `${D}enabled`, description: 'Enable debug mode' },
  { key: 'enableDataDebug', subgroup: 'Debug Logging', label: 'Data Debug', type: 'toggle', path: `${D}enableDataDebug`, description: 'Log data operations' },
  { key: 'enableWebsocketDebug', subgroup: 'Debug Logging', label: 'WebSocket Debug', type: 'toggle', path: `${D}enableWebsocketDebug`, description: 'Log WebSocket traffic' },
  { key: 'logBinaryHeaders', subgroup: 'Debug Logging', label: 'Log Binary Headers', type: 'toggle', path: `${D}logBinaryHeaders`, description: 'Log binary message headers' },
  { key: 'logFullJson', subgroup: 'Debug Logging', label: 'Log Full JSON', type: 'toggle', path: `${D}logFullJson`, description: 'Log complete JSON payloads' },
  { key: 'enablePhysicsDebug', subgroup: 'Debug Logging', label: 'Physics Debug', type: 'toggle', path: `${D}enablePhysicsDebug`, description: 'Physics visualization' },
  { key: 'enableNodeDebug', subgroup: 'Debug Logging', label: 'Node Debug', type: 'toggle', path: `${D}enableNodeDebug`, description: 'Node state logging' },
  { key: 'enableShaderDebug', subgroup: 'Debug Logging', label: 'Shader Debug', type: 'toggle', path: `${D}enableShaderDebug`, description: 'Shader debugging' },
  { key: 'enableMatrixDebug', subgroup: 'Debug Logging', label: 'Matrix Debug', type: 'toggle', path: `${D}enableMatrixDebug`, description: 'Matrix transformations' },
  { key: 'enablePerformanceDebug', subgroup: 'Debug Logging', label: 'Performance Debug', type: 'toggle', path: `${D}enablePerformanceDebug`, description: 'Performance metrics' },
];

export const system: GroupData = {
  id: 'system',
  label: 'System & Developer',
  description: 'Renderer backend, authentication, backend URL, and debug logging switches.',
  hotkey: '8',
  loadPaths: ['auth', 'system', 'visualisation.graphs.knowledge.physics'],
  fields,
};
