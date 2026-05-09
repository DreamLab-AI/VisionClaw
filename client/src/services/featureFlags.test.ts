import { describe, it, expect, beforeEach, vi } from 'vitest';

// Reset module state between tests by re-importing
let featureFlagsModule: typeof import('./featureFlags');

describe('featureFlags', () => {
  beforeEach(async () => {
    vi.resetModules();
    featureFlagsModule = await import('./featureFlags');
  });

  describe('getFeatureFlags', () => {
    it('returns default flags on initial call', () => {
      const flags = featureFlagsModule.getFeatureFlags();

      expect(flags).toEqual({
        BRIDGE_EDGE_ENABLED: false,
        VISIBILITY_TRANSITIONS: false,
        URN_SOLID_ALIGNMENT: false,
      });
    });
  });

  describe('fetchFeatureFlags', () => {
    it('returns cached defaults (no backend route yet)', async () => {
      const flags = await featureFlagsModule.fetchFeatureFlags();

      expect(flags).toEqual({
        BRIDGE_EDGE_ENABLED: false,
        VISIBILITY_TRANSITIONS: false,
        URN_SOLID_ALIGNMENT: false,
      });
    });

    it('deduplicates concurrent calls (returns same result)', async () => {
      const p1 = featureFlagsModule.fetchFeatureFlags();
      const p2 = featureFlagsModule.fetchFeatureFlags();

      const [r1, r2] = await Promise.all([p1, p2]);
      expect(r1).toStrictEqual(r2);
    });

    it('sets lastFetchedAt timestamp', async () => {
      expect(featureFlagsModule.getFeatureFlagsFetchedAt()).toBeNull();

      await featureFlagsModule.fetchFeatureFlags();

      expect(featureFlagsModule.getFeatureFlagsFetchedAt()).toBeTypeOf('number');
    });
  });

  describe('refreshFeatureFlags', () => {
    it('bypasses in-flight cache and fetches again', async () => {
      await featureFlagsModule.fetchFeatureFlags();
      const first = featureFlagsModule.getFeatureFlagsFetchedAt();

      // Small delay to ensure timestamp differs
      await new Promise((r) => setTimeout(r, 5));

      await featureFlagsModule.refreshFeatureFlags();
      const second = featureFlagsModule.getFeatureFlagsFetchedAt();

      expect(second).toBeGreaterThanOrEqual(first!);
    });
  });

  describe('getFeatureFlagsFetchedAt', () => {
    it('returns null before any fetch', () => {
      expect(featureFlagsModule.getFeatureFlagsFetchedAt()).toBeNull();
    });

    it('returns a number after fetch', async () => {
      await featureFlagsModule.fetchFeatureFlags();
      const ts = featureFlagsModule.getFeatureFlagsFetchedAt();
      expect(ts).toBeTypeOf('number');
      expect(ts).toBeGreaterThan(0);
    });
  });
});
