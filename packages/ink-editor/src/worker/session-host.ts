/**
 * SessionHostCore (docs/editor-worker-spec.md §6, W4) — the server side
 * of the session protocol: the admission scheduler plus the dispatch of
 * every request kind against a session facade.
 *
 * Extracted from `LocalTransport` so the in-process transport (W1) and
 * the Web Worker host (`session-worker.ts`, W4) run the IDENTICAL
 * semantics by construction — the worker is the same core behind a
 * `postMessage` boundary instead of a microtask.
 */

import type {
  DocumentId,
  SessionEditSpan,
  SessionRequest,
  SessionResponse,
} from "@brink/wasm-types";
import { AdmissionScheduler } from "./scheduler.js";
import {
  isPerfEnabled,
  perfReport,
  perfReset,
  setPerfEnabled,
  type PerfReport,
} from "../perf/probe.js";

/**
 * The perf state of the realm HOSTING the session — answered by the
 * host-level `hostPerfReport` query (prod-perf ruling 2026-08-25). In
 * worker mode this is the worker realm, where all analysis actually runs
 * since W5; on the in-process road it is the main realm (the same probe
 * the HUD reads directly, so hosts skip the query there).
 */
export interface HostPerfBundle {
  enabled: boolean;
  /** The hosting realm's JS probe report (`wasm.<method>` spans etc.). */
  probe: PerfReport;
  /** The session's wasm-internal counters, when the facade exposes them. */
  wasmCounters: Record<string, { count: number; totalMs: number; maxMs: number }> | null;
}

/**
 * The server-side surface the host dispatches to. Structurally a subset
 * of `EditorSessionHandle` (`@brink-lang/web`): the two mutation entry
 * points are named; every `query`/`config`/`files` method is dispatched
 * dynamically by name against the same object.
 */
export interface SessionServerLike {
  updateDocument(doc: DocumentId, source: string): unknown;
  applyEditsDocument?(doc: DocumentId, edits: readonly SessionEditSpan[]): boolean;
  configEpoch?(): number;
}

export class SessionHostCore {
  private readonly scheduler = new AdmissionScheduler();
  private stopped = false;

  constructor(
    private readonly server: SessionServerLike,
    private readonly respond: (response: SessionResponse) => void,
  ) {}

  accept(request: SessionRequest): void {
    this.scheduler.enqueue(request);
  }

  stop(): void {
    this.stopped = true;
  }

  /** Run everything currently admitted (mutations → interactive →
   *  background, with policy drops answered). Safe to call repeatedly. */
  drain(): void {
    for (;;) {
      if (this.stopped) return;
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
          const host = this.hostCall(request.method, request.args);
          const value = host.handled
            ? host.value
            : callByName(this.server, request.method, request.args);
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

  /**
   * Host-level queries (prod-perf ruling 2026-08-25): methods answered by
   * the HOST REALM itself rather than dispatched onto the session facade —
   * the only road to a worker realm's own probe. Checked before the
   * dynamic dispatch so a facade can never shadow them.
   */
  private hostCall(method: string, args: unknown[]): { handled: boolean; value?: unknown } {
    const facade = this.server as unknown as {
      setPerfEnabled?: (on: boolean) => void;
      getPerfCounters?: () => HostPerfBundle["wasmCounters"];
      resetPerfCounters?: () => void;
    };
    switch (method) {
      case "hostPerfReport": {
        const bundle: HostPerfBundle = {
          enabled: isPerfEnabled(),
          probe: perfReport(),
          wasmCounters: facade.getPerfCounters?.() ?? null,
        };
        return { handled: true, value: bundle };
      }
      case "hostPerfReset":
        perfReset();
        facade.resetPerfCounters?.();
        return { handled: true, value: true };
      case "hostPerfSetEnabled": {
        const on = args[0] === true;
        setPerfEnabled(on);
        facade.setPerfEnabled?.(on);
        return { handled: true, value: on };
      }
      default:
        return { handled: false };
    }
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
