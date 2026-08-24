/**
 * Wasm-boundary timing (measure-first ruling, 2026-08-24).
 *
 * Every wasm query in the editor stack — DocHandle methods, ProjectSession
 * calls, studio panel pulls — routes through the one shared
 * `EditorSessionHandle`, so wrapping that instance times the ENTIRE wasm
 * boundary from a single choke point with zero changes inside
 * `@brink-lang/web`. Spans are named `wasm.<method>`.
 *
 * The wrapper is a Proxy rather than a subclass because the handle's method
 * set is wide (~110 calls) and evolves with the wasm surface; a Proxy stays
 * complete by construction. Wrapped methods are cached per name so property
 * access after the first costs a Map hit, not an allocation.
 */

import { isPerfEnabled, perfRecord } from "./probe.js";

/**
 * Wrap `session` so every method call records a `wasm.<method>` span while
 * the probe is enabled. Identity-sensitive uses are safe: the proxy forwards
 * every non-function property untouched and calls methods with the original
 * receiver, so internal state lives where it always did.
 */
export function withPerfTiming<T extends object>(session: T): T {
  const wrapped = new Map<PropertyKey, unknown>();
  return new Proxy(session, {
    get(target, prop, receiver) {
      const value = Reflect.get(target, prop, receiver);
      if (typeof value !== "function") return value;
      let fn = wrapped.get(prop);
      if (fn === undefined) {
        const name = `wasm.${String(prop)}`;
        fn = function (this: unknown, ...args: unknown[]): unknown {
          // Resolved at CALL time, not captured at first access: tests spy
          // on session methods (vi.spyOn) and hosts may monkey-patch — a
          // cached function reference would silently bypass both.
          const current = Reflect.get(target, prop) as (...a: unknown[]) => unknown;
          if (!isPerfEnabled()) return current.apply(target, args);
          const start = performance.now();
          try {
            return current.apply(target, args);
          } finally {
            perfRecord(name, start, performance.now() - start);
          }
        };
        wrapped.set(prop, fn);
      }
      return fn;
    },
  });
}
