/**
 * The studio's `ProseChecker` — a lazy loader around the `brink-prose` wasm
 * module (#3209).
 *
 * **The `import()` is the whole point.** `brink-prose` is 6.5 MB gzipped,
 * larger than the entire compiler, so it must not be in the main bundle: a
 * static import would put it there whether or not anyone ever writes a
 * sentence. Vite code-splits a dynamic import into its own chunk, fetched on
 * the first check and never before.
 *
 * Everything here is failure-tolerant by design. A checker that cannot load
 * is not an editor error — it is an editor without prose squiggles, which is
 * exactly what an embedder that never registered one gets. So a failed load
 * is remembered and never retried, rather than re-fetching 6.5 MB on every
 * keystroke of a session that is not going to work.
 */

import type { ProseChecker, ProseLint } from "@brink-lang/editor";

type WasmProse = {
  default: (input?: unknown) => Promise<unknown>;
  check_prose: (requestJson: string) => string;
};

/** `null` while unloaded, a promise while loading, `false` once it failed. */
let loading: Promise<WasmProse | null> | null = null;
let failed = false;

async function loadProse(): Promise<WasmProse | null> {
  if (failed) return null;
  if (loading === null) {
    loading = (async () => {
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
  }
  return loading;
}

/**
 * The studio's checker.
 *
 * Returns `[]` rather than throwing on any failure. The editor's plugin
 * treats a rejection as "leave the previous squiggles standing", which is
 * right for a transient fault and wrong for a permanent one — an empty
 * result is the honest answer for "this could not be checked".
 */
export const studioProseChecker: ProseChecker = {
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

/** Whether the module has been loaded — for the status surface and tests. */
export function proseCheckerLoaded(): boolean {
  return loading !== null && !failed;
}
