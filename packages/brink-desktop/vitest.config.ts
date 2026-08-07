import { defineConfig } from "vitest/config";
import { resolve } from "path";

const wasmPkgPath = resolve(__dirname, "../../crates/brink-web/www/pkg");

// Node environment — no DOM needed. The Tauri IPC surfaces themselves
// (menu/window events, `invoke`) are not run headlessly here; only the
// awaitable-save seam (`quit.ts`) is unit-tested. See docs/decision-log.md
// "Desktop close: no dirty prompt; quit awaits the final save" (#2370) for
// why the actual quit path gets a manual-verification note instead.
//
// The alias map mirrors this package's own `vite.config.ts` DEV-MODE
// resolution (2026-08 review finding, #2409): without it, `@brink-lang/editor`
// / `@brink-lang/web` fall back to `dist/` builds that are neither
// git-tracked nor produced by `pnpm install` (unlike `@brink-lang/studio`,
// which brink-desktop's tests don't import directly), so `export-artifact.
// test.ts` — the PR's headline artifact-verification test — silently failed
// to resolve its entry point and ran zero tests behind a green-looking CI
// step ("1 failed | 2 passed" at the file level, "9 passed" at the test
// level). `brink-web` aliases to the REAL wasm-bindgen glue the Frontend CI
// job already builds (`crates/brink-web/www/pkg/brink_web.js`), never
// `brink-studio`'s `__mocks__/brink-web.ts` — that mock would make
// `export-artifact.test.ts` prove nothing about a real compiled artifact.
export default defineConfig({
  test: {
    environment: "node",
  },
  resolve: {
    alias: {
      "brink-web": resolve(wasmPkgPath, "brink_web.js"),
      "@brink-lang/web": resolve(__dirname, "../wasm/src/index.ts"),
      "@brink/wasm-types": resolve(__dirname, "../wasm-types/src/index.ts"),
      "@brink/ink-operations": resolve(__dirname, "../ink-operations/src/index.ts"),
      "@brink-lang/editor": resolve(__dirname, "../ink-editor/src/index.ts"),
    },
  },
});
