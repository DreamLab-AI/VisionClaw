/**
 * Dev-only guard for `performance.measure`.
 *
 * React 19 dev builds (Performance Tracks) call `performance.measure(name,
 * { start, end, detail: { devtools: { properties } } })` and the browser
 * structured-clones `detail`. A large props diff makes that throw
 * `DataCloneError: ... out of memory`, which surfaces as an uncaught error from
 * inside the reconciler. The guard retries without `detail` and never lets the
 * call throw; only DevTools track metadata is lost.
 *
 * It cannot undo the allocation made by building the diff before the call, so
 * components must still avoid bulk props (see useEdgePointsHolder).
 */

type MeasureFn = (this: unknown, ...args: unknown[]) => unknown;

interface MeasureHost {
  measure: MeasureFn;
}

const INSTALLED = Symbol.for('visionclaw.measureGuard');

/** Wrap `host.measure` in place. Idempotent. Returns true when it installed. */
export function installMeasureGuard(host: MeasureHost): boolean {
  const marked = host as unknown as Record<symbol, unknown>;
  if (marked[INSTALLED]) return false;
  const original = host.measure;
  if (typeof original !== 'function') return false;

  host.measure = function guardedMeasure(this: unknown, ...args: unknown[]): unknown {
    try {
      return original.apply(this ?? host, args);
    } catch (firstError) {
      const opts = args[1];
      if (opts && typeof opts === 'object' && 'detail' in (opts as object)) {
        const { detail: _detail, ...rest } = opts as Record<string, unknown>;
        try {
          return original.apply(this ?? host, [args[0], rest, ...args.slice(2)]);
        } catch {
          return undefined;
        }
      }
      void firstError;
      return undefined;
    }
  };
  marked[INSTALLED] = true;
  return true;
}

if (import.meta.env.DEV && typeof performance !== 'undefined') {
  installMeasureGuard(performance as unknown as MeasureHost);
}
