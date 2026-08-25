/**
 * In-process session transport (docs/editor-worker-spec.md §12, W1).
 *
 * Runs the full protocol — scheduler, ordering, coalescing, drops,
 * acks — against a session living on the same thread, dispatching in a
 * microtask so consumers experience the exact asynchrony the worker
 * transport (W4) will have, with none of the worker. This is the
 * strangler substrate: consumers migrate to the async client against
 * this transport with zero behavior change, and W4 swaps the transport.
 *
 * Every envelope is JSON round-tripped (`JSON.parse(JSON.stringify(…))`
 * plus a losslessness check) on the way in AND on the way out. That is
 * deliberate and load-bearing (spec §5.1/§5.4): a payload that would
 * mangle over postMessage-to-native byte streams — a `Map`, an
 * `undefined`-valued key, a class instance with methods — fails loudly
 * here, in every unit test, long before a real wire exists.
 */

import type {
  DocumentId,
  SessionEditSpan,
  SessionRequest,
  SessionResponse,
} from "@brink/wasm-types";
import { AdmissionScheduler } from "./scheduler.js";
import type { SessionTransport } from "./transport.js";

/**
 * The server-side surface the transport dispatches to. Structurally a
 * subset of `EditorSessionHandle` (`@brink-lang/web`): the two mutation
 * entry points are named; every `query`/`config`/`files` method is
 * dispatched dynamically by name against the same object.
 */
export interface SessionServerLike {
  updateDocument(doc: DocumentId, source: string): unknown;
  applyEditsDocument?(doc: DocumentId, edits: readonly SessionEditSpan[]): boolean;
  configEpoch?(): number;
}

export class LocalTransport implements SessionTransport {
  private readonly scheduler = new AdmissionScheduler();
  private onResponse: ((response: SessionResponse) => void) | null = null;
  private drainScheduled = false;
  private closed = false;

  constructor(private readonly server: SessionServerLike) {}

  post(request: SessionRequest): void {
    if (this.closed) throw new Error("LocalTransport is closed");
    this.scheduler.enqueue(jsonRoundTrip(request, "request"));
    if (!this.drainScheduled) {
      this.drainScheduled = true;
      queueMicrotask(() => {
        this.drainScheduled = false;
        this.drain();
      });
    }
  }

  setOnResponse(listener: (response: SessionResponse) => void): void {
    this.onResponse = listener;
  }

  close(): void {
    this.closed = true;
    this.onResponse = null;
  }

  private drain(): void {
    for (;;) {
      if (this.closed) return;
      const action = this.scheduler.nextAction();
      if (action === null) return;
      if (action.kind === "drop") {
        this.respond({
          kind: "error",
          id: action.request.id,
          message: `dropped:${action.reason}`,
        });
        continue;
      }
      this.dispatch(action.request);
    }
  }

  private dispatch(request: SessionRequest): void {
    switch (request.kind) {
      case "edit": {
        const applied = this.server.applyEditsDocument?.(request.doc, request.edits) ?? false;
        this.respond({
          kind: "ack",
          doc: request.doc,
          docVersion: request.docVersion,
          applied,
        });
        return;
      }
      case "push": {
        // `updateDocument` returns a change spec on an applied push and
        // null on a refused one (read-only target, unknown handle).
        const spec = this.server.updateDocument(request.doc, request.source);
        this.respond({
          kind: "ack",
          doc: request.doc,
          docVersion: request.docVersion,
          applied: spec !== null,
        });
        return;
      }
      case "config":
      case "files": {
        // Fire-and-forget mutations. A returned value (e.g. config
        // warnings from `applyProjectConfig`) or a thrown error flows
        // back as an event — mutations have no request id to answer.
        const { method, args } = request.op;
        try {
          const value = callByName(this.server, method, args);
          if (value !== undefined) {
            this.respond({ kind: "event", event: { type: "mutationResult", method, value } });
          }
        } catch (error) {
          this.respond({
            kind: "event",
            event: { type: "mutationError", method, message: describe(error) },
          });
        }
        return;
      }
      case "query": {
        try {
          const value = callByName(this.server, request.method, request.args);
          this.respond({
            kind: "result",
            id: request.id,
            ...(request.doc !== undefined &&
            this.scheduler.latestDocVersion(request.doc) !== undefined
              ? { docVersion: this.scheduler.latestDocVersion(request.doc) }
              : {}),
            configEpoch: this.server.configEpoch?.() ?? 0,
            value: value ?? null,
          });
        } catch (error) {
          this.respond({ kind: "error", id: request.id, message: describe(error) });
        }
        return;
      }
      case "cancel":
        // Fully handled inside the scheduler at enqueue time.
        return;
    }
  }

  private respond(response: SessionResponse): void {
    this.onResponse?.(jsonRoundTrip(response, "response"));
  }
}

function callByName(server: SessionServerLike, method: string, args: unknown[]): unknown {
  const fn = (server as unknown as Record<string, unknown>)[method];
  if (typeof fn !== "function") {
    throw new Error(`unknown session method: ${method}`);
  }
  return (fn as (...a: unknown[]) => unknown).apply(server, args);
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Serialize + parse, then verify losslessness. Throws `TypeError` on any
 * payload JSON cannot carry faithfully — the transport-level enforcement
 * of the spec's JSON-safety contract.
 */
function jsonRoundTrip<T>(value: T, label: string): T {
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
