import { describe, it, expect, beforeEach, vi } from 'vitest';
import * as THREE from 'three';

vi.mock('../../managers/dataManager/restClient', () => ({
  fetchStateAtSubjects: vi.fn(),
}));

import { fetchStateAtSubjects as fetchStateAt } from '../../managers/dataManager/restClient';
import { useTimelineStore, nodeTimelineStatus } from '../../../../store/timelineStore';
import { applyTimelineTint } from '../timelineTint';

const CAMERA = 'urn:ngm:class:camera';
const LENS = 'urn:ngm:class:lens';

const valid = (...subjects: string[]) => ({ subjects, earliestValidFrom: '2026-03-01T09:00:00Z' });

describe('timeline overlay (ADR-2022 P3)', () => {
  beforeEach(() => {
    useTimelineStore.getState().reset();
    vi.mocked(fetchStateAt).mockReset();
  });

  it('is neutral for every node while inactive', () => {
    expect(nodeTimelineStatus(7, CAMERA)).toBe('neutral');
  });

  it('matches corpus class versions by the node IRI, not only the numeric id', async () => {
    vi.mocked(fetchStateAt).mockResolvedValueOnce(valid(CAMERA, LENS));
    await useTimelineStore.getState().loadStateAt('2026-03-05T00:00:00Z');
    vi.mocked(fetchStateAt).mockResolvedValueOnce(valid(CAMERA));
    await useTimelineStore.getState().loadStateAt('2026-03-12T00:00:00Z');

    expect(nodeTimelineStatus(7, CAMERA)).toBe('valid');
    expect(nodeTimelineStatus(8, LENS)).toBe('faded');
    expect(nodeTimelineStatus(9, 'urn:ngm:class:unversioned')).toBe('neutral');
    expect(nodeTimelineStatus(10)).toBe('neutral');
  });

  it('widens the slider domain to the earliest validFrom', async () => {
    vi.mocked(fetchStateAt).mockResolvedValueOnce({ subjects: [CAMERA], earliestValidFrom: '2020-01-01T00:00:00Z' });
    await useTimelineStore.getState().loadStateAt('2026-03-05T00:00:00Z');
    expect(useTimelineStore.getState().domainMinMs).toBe(Date.parse('2020-01-01T00:00:00Z'));
  });

  it('bumps version on every load so renderers repaint', async () => {
    vi.mocked(fetchStateAt).mockResolvedValue(valid(CAMERA));
    const before = useTimelineStore.getState().version;
    await useTimelineStore.getState().loadStateAt('2026-03-05T00:00:00Z');
    expect(useTimelineStore.getState().version).toBeGreaterThan(before);
  });

  it('marks added and retracted subjects in diff mode', async () => {
    vi.mocked(fetchStateAt)
      .mockResolvedValueOnce(valid(CAMERA))
      .mockResolvedValueOnce(valid(LENS));
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
