/**
 * In-process session transport (docs/editor-worker-spec.md §12, W1).
 *
 * Runs the full protocol — scheduler, ordering, coalescing, drops,
 * acks — against a session living on the same thread, dispatching in a
 * microtask so consumers experience the exact asynchrony the worker
 * transport (W4) has, with none of the worker. This is the strangler
 * substrate: consumers migrate to the async client against this
 * transport with zero behavior change, and W4 swaps the transport. The
 * host semantics live in {@link SessionHostCore}, SHARED with the
 * worker host — the two transports cannot drift.
 *
 * Every envelope is JSON round-tripped (`JSON.parse(JSON.stringify(…))`
 * plus a losslessness check) on the way in AND on the way out. That is
 * deliberate and load-bearing (spec §5.1/§5.4): a payload that would
 * mangle over postMessage-to-native byte streams — a `Map`, an
 * `undefined`-valued key, a class instance with methods — fails loudly
 * here, in every unit test, long before a real wire exists.
 */

import type { SessionRequest, SessionResponse } from "@brink/wasm-types";
import { SessionHostCore, type SessionServerLike } from "./session-host.js";
import type { SessionTransport } from "./transport.js";

export type { SessionServerLike } from "./session-host.js";

export class LocalTransport implements SessionTransport {
  private readonly core: SessionHostCore;
  private onResponse: ((response: SessionResponse) => void) | null = null;
  private drainScheduled = false;
  private closed = false;

  constructor(server: SessionServerLike) {
    this.core = new SessionHostCore(server, (response) => {
      this.onResponse?.(jsonRoundTrip(response, "response"));
    });
  }

  post(request: SessionRequest): void {
    if (this.closed) throw new Error("LocalTransport is closed");
    this.core.accept(jsonRoundTrip(request, "request"));
    if (!this.drainScheduled) {
      this.drainScheduled = true;
      queueMicrotask(() => {
        this.drainScheduled = false;
        this.core.drain();
      });
    }
  }

  setOnResponse(listener: (response: SessionResponse) => void): void {
    this.onResponse = listener;
  }

  close(): void {
    this.closed = true;
    this.onResponse = null;
    this.core.stop();
  }
}

/**
 * Serialize + parse, then verify losslessness. Throws `TypeError` on any
 * payload JSON cannot carry faithfully — the transport-level enforcement
 * of the spec's JSON-safety contract. Shared with `WorkerTransport`
 * (postMessage's structured clone is more permissive than JSON, and the
 * §5.4 native-transport contract is JSON — so the stricter check applies
 * on every transport).
 */
export function jsonRoundTrip<T>(value: T, label: string): T {
  const parsed: unknown = JSON.parse(JSON.stringify(value));
  if (!jsonEqual(value, parsed)) {
    throw new TypeError(
      `session ${label} is not JSON-safe (Map/Set, undefined-valued key, class instance, or NaN/Infinity)`,
    );
  }
  return parsed as T;
}

function jsonEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (typeof a !== typeof b) return false;
  if (typeof a !== "object" || a === null || b === null) return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  // A non-plain object (Map, Set, class instance) survives roundtrip only
  // as a plain object; comparing own enumerable string keys catches every
  // lossy case (Map -> {}, methods dropped, undefined-valued keys gone).
  if (Object.getPrototypeOf(a) !== Object.prototype && !Array.isArray(a)) return false;
  const ka = Object.keys(a as Record<string, unknown>);
  const kb = Object.keys(b as Record<string, unknown>);
  if (ka.length !== kb.length) return false;
  return ka.every((k) =>
    jsonEqual((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k]),
  );
}
