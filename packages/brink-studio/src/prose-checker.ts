/**
 * The studio's `ProseChecker` — the `brink-prose` wasm module behind a Web
 * Worker (#3209, moved off the main thread by #3491).
 *
 * **The `import()` is still the whole point.** `brink-prose` is 6.5 MB
 * gzipped, larger than the entire compiler, so it must not be in the main
 * bundle: a static import would put it there whether or not anyone ever
 * writes a sentence. It is now imported inside `prose-worker.ts`, on the
 * first check, and the worker itself is not even spawned until then — so an
 * embedder that never checks prose pays for neither.
 *
 * **The worker is the fix for #3491.** A check is O(document): parse, POS-tag
 * and lint the whole file. Measured on the main thread, that was 651 ms on a
 * real 1,125-line file and 4.8 s (p95) on the 8k-line perf fixture, landing
 * 700 ms after the author stopped typing — a freeze, not a delay. Moving it
 * does not make it faster; it makes it not the editor's problem. Latency
 * stays, jank goes.
 *
 * **Coalescing, because a superseded check is wasted work.** The worker
 * answers one request at a time, so a request that a newer edit has already
 * superseded is dropped before it is ever posted rather than queued behind
 * the one in flight. It cannot be un-run once the wasm call has started —
 * nothing can interrupt that — but it can be the last one that runs.
 *
 * Everything here is failure-tolerant by design. A checker that cannot load
 * is not an editor error — it is an editor without prose squiggles, which is
 * exactly what an embedder that never registered one gets. Where a worker
 * cannot exist at all (jsdom, a bundler that does not process the
 * `new URL(..., import.meta.url)` shape) the in-process loader below is used
 * instead: slow, but the same answers, and the same posture the session
 * worker takes for a no-`Worker` environment.
 */

import type { ProseChecker, ProseLint } from "@brink-lang/editor";

/** The request shape `ProseChecker.check` takes, named so the worker can
 *  carry it across `postMessage` without re-declaring it. */
export type ProseCheckRequest = Parameters<ProseChecker["check"]>[0];

/** Main thread → worker. `id` correlates the reply; there is at most one
 *  outstanding request, so it is a staleness guard rather than a queue key. */
export interface ProseWorkerRequest {
  id: number;
  request: ProseCheckRequest;
}

/** Worker → main thread. Exactly one of `lints` / `error` is present. */
export type ProseWorkerResponse =
  | { id: number; lints: ProseLint[] }
  | { id: number; error: string };

/** The slice of the DOM `Worker` interface this client needs — an interface
 *  so tests drive a loopback fake, the same shape `WorkerTransport` uses. */
export interface ProseWorkerLike {
  postMessage(data: unknown): void;
  addEventListener(type: "message", listener: (ev: { data: unknown }) => void): void;
  addEventListener(type: "error", listener: (ev: { message?: string }) => void): void;
  terminate(): void;
}

/** Rejection for a check a newer edit replaced before it ran.
 *
 *  A rejection rather than an empty result on purpose: the editor's plugin
 *  treats a rejection as "leave the previous squiggles standing" and an empty
 *  array as "there is nothing to show". The superseded check knows neither —
 *  the newer one will say — so it must not be the one that clears the set. */
export class ProseCheckSuperseded extends Error {
  constructor() {
    super("prose check superseded by a newer edit");
    this.name = "ProseCheckSuperseded";
  }
}

interface Pending {
  resolve: (lints: ProseLint[]) => void;
  reject: (error: Error) => void;
}

/**
 * A `ProseChecker` that answers over `postMessage`, with `fallback` for every
 * environment or failure where the worker cannot.
 *
 * `spawn` is called at most once, on the first check — keeping the lazy
 * posture the interface promises.
 */
export function createWorkerProseChecker(
  spawn: () => ProseWorkerLike | null,
  fallback: ProseChecker,
): ProseChecker {
  let worker: ProseWorkerLike | null = null;
  let spawned = false;
  /** The worker crashed or could not be created; every later check goes to
   *  the in-process road rather than to a dead port. */
  let broken = false;
  let nextId = 1;
  let inFlight: (Pending & { id: number }) | null = null;
  let queued: (Pending & { request: ProseCheckRequest }) | null = null;

  function fail(error: Error): void {
    const pending = [inFlight, queued];
    inFlight = null;
    queued = null;
    for (const p of pending) p?.reject(error);
  }

  function ensureWorker(): ProseWorkerLike | null {
    if (broken) return null;
    if (spawned) return worker;
    spawned = true;
    worker = spawn();
    if (worker === null) {
      broken = true;
      return null;
    }
    worker.addEventListener("message", (ev) => {
      const response = ev.data as ProseWorkerResponse;
      // A reply whose request was abandoned (the worker crashed and was
      // replaced, a duplicate) answers nobody. Dropping it is the whole
      // reason the id is on the wire.
      if (inFlight === null || inFlight.id !== response.id) return;
      const settled = inFlight;
      inFlight = null;
      if ("error" in response) settled.reject(new Error(response.error));
      else settled.resolve(response.lints);
      pump();
    });
    worker.addEventListener("error", (ev) => {
      broken = true;
      worker?.terminate();
      worker = null;
      console.warn("[prose] checker worker crashed; falling back in-process", ev.message);
      fail(new Error(ev.message ?? "prose worker crashed"));
    });
    return worker;
  }

  function pump(): void {
    if (inFlight !== null || queued === null || worker === null) return;
    const { request, resolve, reject } = queued;
    queued = null;
    const id = nextId++;
    inFlight = { id, resolve, reject };
    worker.postMessage({ id, request } satisfies ProseWorkerRequest);
  }

  return {
    check(request: ProseCheckRequest): Promise<ProseLint[]> {
      const live = ensureWorker();
      if (live === null) return fallback.check(request);
      return new Promise<ProseLint[]>((resolve, reject) => {
        // Coalesce: only the newest waiting request is worth running, and
        // the one it replaces is told so rather than left pending forever.
        queued?.reject(new ProseCheckSuperseded());
        queued = { request, resolve, reject };
        pump();
      });
    },
  };
}

/** The wasm-pack glue's shape — the two members the in-process road calls. */
interface WasmProse {
  default: (input?: unknown) => Promise<unknown>;
  check_prose: (requestJson: string) => string;
}

/** `null` while unloaded, a promise while loading. */
let loading: Promise<WasmProse | null> | null = null;
let failed = false;

async function loadProse(): Promise<WasmProse | null> {
  if (failed) return null;
  loading ??= (async () => {
    try {
      // Vite resolves this through packages/brink-studio's `file:`
      // devDependency on crates/brink-prose/www/pkg and emits it as its own
      // chunk. `scripts/check-wasm-pkg.mjs` guards that the link resolved —
      // a missing one is the #2479 failure, which reports as a
      // module-not-found here rather than anything about wasm.
      const mod = (await import("brink-prose")) as unknown as WasmProse;
      await mod.default();
      return mod;
    } catch (error) {
      // Remembered, not retried: re-fetching 6.5 MB on every debounce of a
      // session where the module is unavailable would be a real cost for
      // no chance of success.
      failed = true;
      console.warn("[prose] checker unavailable; prose checking is off", error);
      return null;
    }
  })();
  return loading;
}

/**
 * The in-process checker — the road for environments with no worker, and the
 * fallback when one crashes.
 *
 * Returns `[]` rather than throwing on any failure. The editor's plugin
 * treats a rejection as "leave the previous squiggles standing", which is
 * right for a transient fault and wrong for a permanent one — an empty
 * result is the honest answer for "this could not be checked".
 *
 * Exported for the tests that pin the fallback path; the studio's own
 * checker is {@link studioProseChecker}.
 */
export const inProcessProseChecker: ProseChecker = {
  async check(request): Promise<ProseLint[]> {
    const mod = await loadProse();
    if (mod === null) return [];
    try {
      const parsed: unknown = JSON.parse(mod.check_prose(JSON.stringify(request)));
      if (
        typeof parsed !== "object" ||
        parsed === null ||
        !Array.isArray((parsed as { lints?: unknown }).lints)
      ) {
        return [];
      }
      return (parsed as { lints: ProseLint[] }).lints;
    } catch (error) {
      console.warn("[prose] check failed", error);
      return [];
    }
  },
};

/**
 * Spawn the prose worker, or `null` where it cannot run: no `Worker` global
 * (jsdom/node), or a bundler that did not process the
 * `new URL(..., import.meta.url)` worker pattern. Both land on the
 * in-process road — the same arrangement `createSessionWorker()` has.
 */
export function createProseWorker(): ProseWorkerLike | null {
  if (typeof Worker === "undefined") return null;
  try {
    return new Worker(new URL("./prose-worker.ts", import.meta.url), { type: "module" });
  } catch {
    return null;
  }
}

/** The studio's checker. */
export const studioProseChecker: ProseChecker = createWorkerProseChecker(
  createProseWorker,
  inProcessProseChecker,
);

/** Whether the in-process module has been loaded — for the status surface
 *  and tests. The worker road loads its own copy inside the worker, where
 *  this realm cannot see it. */
export function proseCheckerLoaded(): boolean {
  return loading !== null && !failed;
}
