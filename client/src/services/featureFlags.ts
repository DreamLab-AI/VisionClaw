/**
 * Feature flags (ADR-049 / ADR-051).
 *
 * Fetches server-side feature flags from `GET /api/features` and exposes a
 * typed React hook + synchronous accessor. Gates Sprint-3 UI surfaces:
 *
 *   - BRIDGE_EDGE_ENABLED    - render bridge promotion filaments + broker inbox
 *   - VISIBILITY_TRANSITIONS - enable publish/unpublish controls + tombstones
 *   - URN_SOLID_ALIGNMENT    - show pod URLs + Solid-backed metadata
 *
 * Flags are cached in module scope and fetched once per session; a manual
 * `refreshFeatureFlags()` is exposed for tests and debug tooling.
 */

import { useEffect, useState } from 'react';

export type FeatureFlagKey =
  | 'BRIDGE_EDGE_ENABLED'
  | 'VISIBILITY_TRANSITIONS'
  | 'URN_SOLID_ALIGNMENT';

export type FeatureFlags = Record<FeatureFlagKey, boolean>;

const DEFAULT_FLAGS: FeatureFlags = {
  BRIDGE_EDGE_ENABLED: false,
  VISIBILITY_TRANSITIONS: false,
  URN_SOLID_ALIGNMENT: false,
};

let cached: FeatureFlags = { ...DEFAULT_FLAGS };
let inflight: Promise<FeatureFlags> | null = null;
let lastFetchedAt: number | null = null;
const listeners = new Set<(flags: FeatureFlags) => void>();

function notify(): void {
  listeners.forEach((l) => {
    try {
      l(cached);
    } catch {
      // Swallow listener errors - flag notifications must not break the UI.
    }
  });
}

/**
 * Fetch flags from the backend, cache, and notify subscribers.
 * Subsequent concurrent callers share the in-flight promise.
 *
 * Backend endpoint: GET /api/analytics/feature-flags (analytics flag set).
 * The enterprise flags (BRIDGE_EDGE_ENABLED etc.) don't have a backend
 * route yet — return defaults synchronously to avoid a 404 that wastes a
 * browser connection slot during initial page load.
 */
export async function fetchFeatureFlags(): Promise<FeatureFlags> {
  if (inflight) return inflight;
  inflight = (async () => {
    cached = { ...DEFAULT_FLAGS };
    lastFetchedAt = Date.now();
    notify();
    return cached;
  })();
  return inflight;
}

/** Force a refresh, bypassing the in-flight cache. */
export async function refreshFeatureFlags(): Promise<FeatureFlags> {
  inflight = null;
  return fetchFeatureFlags();
}

/** Synchronous accessor - returns the last cached value (or defaults). */
export function getFeatureFlags(): FeatureFlags {
  return cached;
}

/** Timestamp of the last successful fetch, or null. */
export function getFeatureFlagsFetchedAt(): number | null {
  return lastFetchedAt;
}

/**
 * React hook that returns the live flags, fetching once on mount and
 * re-rendering when the cache changes.
 */
export function useFeatureFlags(): FeatureFlags {
  const [flags, setFlags] = useState<FeatureFlags>(cached);

  useEffect(() => {
    let active = true;
    const onChange = (next: FeatureFlags) => {
      if (active) setFlags(next);
    };
    listeners.add(onChange);
    if (lastFetchedAt === null) {
      void fetchFeatureFlags().then((f) => {
        if (active) setFlags(f);
      });
    }
    return () => {
      active = false;
      listeners.delete(onChange);
    };
  }, []);

  return flags;
}

/** Convenience: one-flag hook. */
export function useFeatureFlag(key: FeatureFlagKey): boolean {
  const flags = useFeatureFlags();
  return flags[key];
}
