/**
 * Web Worker session transport (docs/editor-worker-spec.md §12, W4): the
 * same `SessionTransport` contract as `LocalTransport`, carried over
 * `postMessage` to the session worker (`session-worker.ts`).
 *
 * Every request is JSON round-tripped before posting — structured clone
 * is more permissive than JSON, and the §5.4 native-transport contract
 * is JSON, so the stricter check applies here too (identical to the
 * in-process transport's enforcement).
 */

import type { SessionRequest, SessionResponse } from "@brink/wasm-types";
import { jsonRoundTrip } from "./local-transport.js";
import type { SessionTransport } from "./transport.js";

/** The slice of the DOM `Worker` interface the transport needs — an
 *  interface so tests can drive a loopback fake. */
export interface WorkerLike {
  postMessage(data: unknown): void;
  terminate(): void;
  addEventListener(type: "message", listener: (ev: { data: unknown }) => void): void;
  addEventListener(type: "error", listener: (ev: { message?: string }) => void): void;
}

export class WorkerTransport implements SessionTransport {
  private onResponse: ((response: SessionResponse) => void) | null = null;
  private closed = false;

  constructor(
    private readonly worker: WorkerLike,
    options?: {
      /** The worker crashed (script error) — the owner should close this
       *  transport and fall back to the in-process road. */
      onCrash?: (message: string) => void;
    },
  ) {
    worker.addEventListener("message", (ev) => {
      if (!this.closed) this.onResponse?.(ev.data as SessionResponse);
    });
    worker.addEventListener("error", (ev) => {
      options?.onCrash?.(ev.message ?? "session worker crashed");
    });
  }

  post(request: SessionRequest): void {
    if (this.closed) throw new Error("WorkerTransport is closed");
    this.worker.postMessage(jsonRoundTrip(request, "request"));
  }

  setOnResponse(listener: (response: SessionResponse) => void): void {
    this.onResponse = listener;
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.onResponse = null;
    this.worker.terminate();
  }
}

/**
 * Spawn the session worker, or `null` where it cannot run: no `Worker`
 * global (jsdom/node), or a bundler that did not process the
 * `new URL(..., import.meta.url)` worker pattern (the tsup LIBRARY build
 * ships this file untransformed — vite-built hosts, which every current
 * consumer is, handle it; other bundlers get the in-process fallback
 * until a host-supplied worker factory lands with the W5 flip).
 */
export function createSessionWorker(): Worker | null {
  if (typeof Worker === "undefined") return null;
  try {
    return new Worker(new URL("./session-worker.ts", import.meta.url), { type: "module" });
  } catch {
    return null;
  }
}
