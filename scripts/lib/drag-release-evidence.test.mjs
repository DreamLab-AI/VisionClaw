import { test } from 'node:test';
import assert from 'node:assert/strict';
import { releaseEvidence } from './drag-release-evidence.mjs';
const f = (t, seq, x) => ({ t, seq: BigInt(seq), positions: new Map([[1, [x, 0, 0]]]) });
test('silent permanently pinned observer cannot pass release', () => {
  assert.equal(releaseEvidence([f(1, 1, 0)], 10, 1n, [1], 1000, .05, 500).settleMs, null);
});
test('one postrelease jump and stale/out-of-order frames cannot prove release', () => {
  const out = releaseEvidence([f(11, 3, 150), f(12, 2, 0), f(13, 3, 0)], 10, 1n, [1], 1000, .05, 500);
  assert.equal(out.travel[1], 0);
  assert.equal(out.settleMs, null);
});
test('ordered postrelease movement followed by quiet is evidence', () => {
  const out = releaseEvidence([f(11, 2, 150), f(20, 3, 140)], 10, 1n, [1], 1000, .05, 500);
  assert.equal(out.travel[1], 10);
  assert.equal(out.settleMs, 10);
});
