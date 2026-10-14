/**
 * Group 8 — System & Developer (id `system`, hotkey 8, 17 fields).
 * Renderer toggle (Effects tab), authentication + network (System tab), and the
 * debug logging switches (Developer tab). the `stream*` settings are server physics
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
  { key: 'streamChunkNodes', subgroup: 'Network', label: 'Stream Chunk Size (nodes)', type: 'slider', min: 100, max: 20000, step: 100, path: 'visualisation.graphs.knowledge.physics.streamChunkNodes', description: 'Most nodes in one position chunk. Each client is sent the nodes that changed since it last acknowledged, most-moved first, in chunks of this size; a new client\'s initial sync is simply every node in chunks. ~52 bytes per node (default 2500, about 130 KB). Applies live, to all clients' },
  { key: 'streamMoveThreshold', subgroup: 'Network', label: 'Stream Move Threshold', type: 'slider', min: 0, max: 100, step: 0.05, path: 'visualisation.graphs.knowledge.physics.streamMoveThreshold', description: 'A node is sent again only once it is this far (layout units) from what the client last acknowledged. 0 sends any change. Lower is smoother and costs bandwidth (measured per client: 0.25 ~1 MB/s with visible steps, 0.05 ~4.7 MB/s, 0.01 ~7 MB/s). Default 0.05. Slow drift accumulates until it crosses this, so nothing is lost' },
  { key: 'streamAckTimeoutMs', subgroup: 'Network', label: 'Stream Ack Timeout (ms)', type: 'slider', min: 20, max: 10000, step: 10, path: 'visualisation.graphs.knowledge.physics.streamAckTimeoutMs', description: 'How long a chunk may go unacknowledged before it is rebuilt and sent again. A lost frame or a client that never acknowledges costs this once; a healthy client never waits for it' },
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
