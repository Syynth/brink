/**
 * Real (non-mocked) wasm bootstrap for tests that need to prove a genuine
 * compiled artifact plays back correctly (#2391, "Export Story (.inkb)").
 *
 * `@brink-lang/studio`'s test suite aliases the low-level `brink-web`
 * bindings to a hand-written JS mock (see `packages/brink-studio/src/
 * __mocks__/brink-web.ts`) — fine for UI-level tests, but useless for
 * proving an exported `.inkb` file is a real, playable artifact. This
 * package's `vitest.config.ts` instead aliases `@brink-lang/web` to
 * workspace source and `brink-web` directly to the wasm-bindgen glue the
 * Frontend CI job already builds (`crates/brink-web/www/pkg/brink_web.js`),
 * mirroring this package's own `vite.config.ts` dev-mode resolution
 * (2026-08 review finding, #2409) — so this file reads the matching
 * `.wasm` binary straight out of that same `pkg/` output, never
 * `packages/wasm/dist`, which is neither git-tracked nor produced by
 * `pnpm install` alone.
 *
 * `@brink-lang/web`'s default `initWasm()` calls `fetch(new URL(...,
 * import.meta.url))`, which resolves to a `file:` URL — Node's built-in
 * fetch (undici) rejects that scheme ("not implemented... yet..."). Instead
 * we read the `.wasm` bytes ourselves and hand `initWasm` an
 * already-compiled `WebAssembly.Module`; the wasm-bindgen glue accepts that
 * directly (`WebAssembly.instantiate(module, imports)`), skipping `fetch`
 * entirely.
 */

import { readFileSync } from "node:fs";
import { initWasm } from "@brink-lang/web";

let initialized: Promise<void> | null = null;

/** Idempotent: safe to call from every test file that needs the real wasm — `@brink-lang/web`'s own double-init guard makes repeat calls a no-op. */
export function initRealWasmOnce(): Promise<void> {
  initialized ??= (async () => {
    const bytes = readFileSync(
      new URL("../../../../crates/brink-web/www/pkg/brink_web_bg.wasm", import.meta.url),
    );
    const wasmModule = await WebAssembly.compile(bytes);
    await initWasm(wasmModule);
  })();
  return initialized;
}
