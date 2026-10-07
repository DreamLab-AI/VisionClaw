/**
 * Wire-shape contract: `fixtures/wire.json` mirrors the Rust structs in
 * `crates/visionclaw-memory-cloud/src/wire.rs`, whose
 * `client_fixture_round_trips` test deserialises and re-serialises the same
 * file. `WIRE_KEYS` (type-checked by `tsc -p .`) is exhaustive over the
 * TypeScript types; this test pins it to the fixture. A field added, dropped
 * or renamed on either side fails one of the three checks.
 */
import { describe, it, expect } from 'vitest';
import wire from './fixtures/wire.json';
import type { MemoryCloudHealth, MemoryCloudQueryResponse, MemoryCloudSnapshot } from '../types';
import { SIDECAR_ISSUES, SEARCH_METHODS } from '../types';
import { WIRE_KEYS as K } from '../wireContract';

const sorted = (o: object) => Object.keys(o).sort();

describe('memory-cloud wire contract (fixture shared with wire.rs)', () => {
  it('snapshot fields match the TypeScript contract', () => {
    const s = wire.snapshot as MemoryCloudSnapshot;
    expect(sorted(s)).toEqual(sorted(K.snapshot));
    expect(sorted(s.metadata[0])).toEqual(sorted(K.meta));
    expect(sorted(s.strata[0])).toEqual(sorted(K.stratum));
  });

  it('query request and response fields match, including sidecar.method', () => {
    expect(sorted(wire.queryRequest)).toEqual(sorted(K.queryRequest));
    const r = wire.queryResponse as MemoryCloudQueryResponse;
    expect(sorted(r)).toEqual(sorted(K.queryResponse));
    expect(sorted(r.query)).toEqual(sorted(K.queryEcho));
    expect(sorted(r.sidecar)).toEqual(sorted(K.sidecar));
    for (const h of r.sidecar.results) expect(sorted(h)).toEqual(sorted(K.hit));
    expect(SEARCH_METHODS).toContain(r.sidecar.method);
    expect(r.sidecar.results[1].sampleIndex).toBeNull();
  });

  it('health has a closed sidecar error category and no embedder url', () => {
    const h = wire.health as MemoryCloudHealth;
    expect(sorted(h)).toEqual(sorted(K.health));
    expect(sorted(h.sidecar)).toEqual(sorted(K.healthSidecar));
    expect(sorted(h.embedder)).toEqual(sorted(K.healthEmbedder));
    expect(h.embedder).not.toHaveProperty('url');
    expect(sorted(h.namespaces[0])).toEqual(sorted(K.healthNamespace));
    expect(sorted(h.recallProbe!)).toEqual(sorted(K.recallProbe));
    expect(SIDECAR_ISSUES).toContain(h.sidecar.error);
  });

  it('the runtime enum lists match the serde variants', () => {
    expect([...SEARCH_METHODS].sort()).toEqual(sorted(K.searchMethod));
    expect([...SIDECAR_ISSUES].sort()).toEqual(sorted(K.sidecarIssue));
  });
});
