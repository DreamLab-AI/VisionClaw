/**
 * Emits registry/settings-manifest.json from the settings registry.
 *
 * Run: `npm run gen:manifest` (ts-node, scripts/tsconfig.emit.json + scripts/package.json
 * force CommonJS so the app's `type:module` + bundler moduleResolution don't apply here).
 * The import graph is lucide/React-free by construction — see registry/manifest.ts.
 *
 * The committed JSON is consumed by the browser-automation coverage phase to assert
 * every testid is present + interactive once its group is open. Regenerate after any
 * registry change; a CI freshness check should diff this against a fresh emit.
 */
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { buildManifest, buildIa } from '../src/features/control-center/registry/manifest';

const OUT = resolve(__dirname, '../src/features/control-center/registry/settings-manifest.json');
// ADR-2189 §2: one IA document, two committed copies (desktop + headset). The
// registry parity test asserts both equal a fresh buildIa().
const IA_OUTS = [
  resolve(__dirname, '../src/features/control-center/registry/ia.json'),
  resolve(__dirname, '../../xr-client/data/ia.json'),
];

const manifest = buildManifest();
writeFileSync(OUT, `${JSON.stringify(manifest, null, 2)}\n`, 'utf8');
const ia = `${JSON.stringify(buildIa(), null, 2)}\n`;
for (const out of IA_OUTS) writeFileSync(out, ia, 'utf8');

console.log(
  `[gen:manifest] wrote ${manifest.count} settings across ${manifest.groups.length} groups ` +
    `(+${manifest.panels.length} panels) → ${OUT}; ia.json → ${IA_OUTS.join(', ')}`,
);
