import { describe, it, expect, beforeEach, vi } from 'vitest';
import * as THREE from 'three';

vi.mock('../../managers/dataManager/restClient', () => ({
  fetchStateAt: vi.fn(),
}));

import { fetchStateAt } from '../../managers/dataManager/restClient';
import { useTimelineStore, nodeTimelineStatus } from '../../../../store/timelineStore';
import { applyTimelineTint } from '../timelineTint';

const CAMERA = 'urn:ngm:class:camera';
const LENS = 'urn:ngm:class:lens';

const assertion = (subject: string) => ({
  subject,
  predicate: 'http://www.w3.org/2000/01/rdf-schema#subClassOf',
  object: 'urn:ngm:class:device',
  validFrom: '2026-03-01T09:00:00Z',
  validTo: null,
  generatedAt: '2026-03-01T09:00:00Z',
  activity: 'urn:visionclaw:execution:corpus-sync-20260301T090000Z',
  agent: 'did:nostr:' + 'a'.repeat(64),
});

describe('timeline overlay (ADR-2022 P3)', () => {
  beforeEach(() => {
    useTimelineStore.getState().reset();
    vi.mocked(fetchStateAt).mockReset();
  });

  it('is neutral for every node while inactive', () => {
    expect(nodeTimelineStatus(7, CAMERA)).toBe('neutral');
  });

  it('matches corpus class versions by the node IRI, not only the numeric id', async () => {
    vi.mocked(fetchStateAt).mockResolvedValueOnce([assertion(CAMERA), assertion(LENS)] as never);
    await useTimelineStore.getState().loadStateAt('2026-03-05T00:00:00Z');
    vi.mocked(fetchStateAt).mockResolvedValueOnce([assertion(CAMERA)] as never);
    await useTimelineStore.getState().loadStateAt('2026-03-12T00:00:00Z');

    expect(nodeTimelineStatus(7, CAMERA)).toBe('valid');
    expect(nodeTimelineStatus(8, LENS)).toBe('faded');
    expect(nodeTimelineStatus(9, 'urn:ngm:class:unversioned')).toBe('neutral');
    expect(nodeTimelineStatus(10)).toBe('neutral');
  });

  it('bumps version on every load so renderers repaint', async () => {
    vi.mocked(fetchStateAt).mockResolvedValue([assertion(CAMERA)] as never);
    const before = useTimelineStore.getState().version;
    await useTimelineStore.getState().loadStateAt('2026-03-05T00:00:00Z');
    expect(useTimelineStore.getState().version).toBeGreaterThan(before);
  });

  it('marks added and retracted subjects in diff mode', async () => {
    vi.mocked(fetchStateAt)
      .mockResolvedValueOnce([assertion(CAMERA)] as never)
      .mockResolvedValueOnce([assertion(LENS)] as never);
    await useTimelineStore.getState().loadDiff('2026-03-01T00:00:00Z', '2026-03-12T00:00:00Z');
    expect(nodeTimelineStatus(1, LENS)).toBe('added');
    expect(nodeTimelineStatus(2, CAMERA)).toBe('retracted');
  });

  it('tints colours by status', () => {
    const base = () => new THREE.Color(0.5, 0.5, 0.5);
    const faded = base(); applyTimelineTint(faded, 'faded');
    expect(faded.r).toBeCloseTo(0.1);
    const added = base(); applyTimelineTint(added, 'added');
    expect(added.g).toBeGreaterThan(added.r);
    const retracted = base(); applyTimelineTint(retracted, 'retracted');
    expect(retracted.r).toBeGreaterThan(retracted.g);
    const valid = base(); applyTimelineTint(valid, 'valid');
    expect(valid.equals(base())).toBe(true);
  });
});
