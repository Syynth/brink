/**
 * The prose worker entry (#3491): the `brink-prose` wasm module, loaded and
 * run inside a Web Worker so a check never blocks the keystroke path.
 *
 * Measured before this existed: one check took **651 ms** on a real
 * 1,125-line file and **4.8 s** (p95) on the 8k-line perf fixture, all of it
 * on the main thread, 700 ms after the author stopped typing. The work is
 * genuinely O(document) — parse, tag, lint — so it cannot be made free; it
 * can only be moved somewhere that owns no input.
 *
 * Instantiated by `prose-checker.ts` through the bundler-standard
 * `new Worker(new URL(...), { type: "module" })` shape, the same one
 * `createSessionWorker()` uses for the session worker.
 *
 * The 6.5 MB module is imported **inside this worker**, dynamically, on the
 * first request — the same lazy posture the main-thread loader had. A host
 * that never checks prose downloads nothing, and the worker itself is a few
 * hundred bytes until then.
 *
 * A load failure is remembered rather than retried: the rejected promise
 * stays in `loading`, so every later request fails immediately instead of
 * re-fetching 6.5 MB for a session that is not going to work. The client
 * turns that into "no squiggles", which is what an embedder with no checker
 * registered already sees.
 */

import type { ProseLint } from "@brink-lang/editor";
import type { ProseWorkerRequest, ProseWorkerResponse } from "./prose-checker.js";

/** The wasm-pack glue's shape — the two members this worker calls. */
interface WasmProse {
  default: (input?: unknown) => Promise<unknown>;
  check_prose: (requestJson: string) => string;
}

/** The worker global, in the shape this file uses (see `session-worker.ts`:
 *  the DOM lib types `self` as a `Window`, so the scope is narrowed here
 *  rather than by pulling in a conflicting `webworker` lib reference). */
const scope = globalThis as unknown as {
  postMessage(data: unknown): void;
  addEventListener(type: "message", listener: (ev: { data: unknown }) => void): void;
};

let loading: Promise<WasmProse> | null = null;

function load(): Promise<WasmProse> {
  loading ??= (async () => {
    // Resolved through `packages/brink-studio`'s `file:` devDependency on
    // `crates/brink-prose/www/pkg`; `scripts/check-wasm-pkg.mjs` guards that
    // the link exists. Vite emits it as this worker's own chunk.
    const mod = (await import("brink-prose")) as unknown as WasmProse;
    await mod.default();
    return mod;
  })();
  return loading;
}

/** The lints out of a parsed response, or `[]` for any shape that is not one. */
function lintsOf(parsed: unknown): ProseLint[] {
  if (typeof parsed !== "object" || parsed === null) return [];
  const lints = (parsed as { lints?: unknown }).lints;
  return Array.isArray(lints) ? (lints as ProseLint[]) : [];
}

async function handle(message: ProseWorkerRequest): Promise<void> {
  try {
    const mod = await load();
    const parsed: unknown = JSON.parse(mod.check_prose(JSON.stringify(message.request)));
    scope.postMessage({
      id: message.id,
      lints: lintsOf(parsed),
    } satisfies ProseWorkerResponse);
  } catch (error) {
    scope.postMessage({
      id: message.id,
      error: error instanceof Error ? error.message : String(error),
    } satisfies ProseWorkerResponse);
  }
}

scope.addEventListener("message", (ev) => {
  void handle(ev.data as ProseWorkerRequest);
});
