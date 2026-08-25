/**
 * The session worker entry (docs/editor-worker-spec.md §8, W4): boots its
 * own wasm module + `EditorSessionHandle` inside a Web Worker and runs
 * {@link SessionHostCore} — the same host semantics as `LocalTransport`,
 * behind a `postMessage` boundary instead of a microtask.
 *
 * Instantiated by `createSessionWorker()` (`worker-transport.ts`) via the
 * bundler-standard `new Worker(new URL(...), { type: "module" })` shape.
 *
 * Boot handshake: a `{ kind: "event", event: { type: "ready" } }` when
 * the wasm session is live, or `{ type: "bootError", message }` if init
 * failed (no wasm, fetch error) — the client tears the worker down and
 * falls back to the in-process road (spec §8: crash recovery is not
 * invisible in v1; the fallback is).
 *
 * Requests arriving before boot completes are queued behind the boot
 * promise — the mutation stream's ordering survives the boot window.
 */

import { EditorSessionHandle, initWasm } from "@brink-lang/web";
import type { SessionRequest } from "@brink/wasm-types";
import { setPerfEnabled } from "../perf/probe.js";
import { withPerfTiming } from "../perf/wasm-proxy.js";
import { SessionHostCore, type SessionServerLike } from "./session-host.js";

const scope = globalThis as unknown as {
  postMessage(data: unknown): void;
  onmessage: ((ev: { data: unknown }) => void) | null;
};

const boot: Promise<SessionHostCore> = (async () => {
  // Perf on by default in THIS realm (prod-perf ruling 2026-08-25): since
  // W5 the analysis cost lives here, invisible to the main thread's probe.
  // Both planes — the worker realm's JS probe (fed `wasm.<method>` spans by
  // the withPerfTiming wrap) and the session's wasm-internal counters —
  // report out through the host-level `hostPerfReport` query, and a host
  // that opted out sends `hostPerfSetEnabled(false)` right after mount.
  setPerfEnabled(true);
  await initWasm();
  const session = new EditorSessionHandle();
  (session as unknown as { setPerfEnabled?: (on: boolean) => void }).setPerfEnabled?.(true);
  return new SessionHostCore(withPerfTiming(session) as unknown as SessionServerLike, (response) =>
    scope.postMessage(response),
  );
})();

boot.then(
  () => scope.postMessage({ kind: "event", event: { type: "ready" } }),
  (error: unknown) =>
    scope.postMessage({
      kind: "event",
      event: {
        type: "bootError",
        message: error instanceof Error ? error.message : String(error),
      },
    }),
);

let drainScheduled = false;

scope.onmessage = (ev) => {
  void boot
    .then((core) => {
      core.accept(ev.data as SessionRequest);
      if (!drainScheduled) {
        drainScheduled = true;
        queueMicrotask(() => {
          drainScheduled = false;
          core.drain();
        });
      }
    })
    .catch(() => {
      // Boot failed — the bootError event already told the client to
      // tear this worker down; queued requests die with it.
    });
};
