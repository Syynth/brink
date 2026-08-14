import { defineConfig } from "vitest/config";
import { desktopAliases } from "./alias-map";

// Node environment — no DOM needed. The Tauri IPC surfaces themselves
// (menu/window events, `invoke`) are not run headlessly here; only the
// awaitable-save seam (`quit.ts`) is unit-tested. See docs/decision-log.md
// "Desktop close: no dirty prompt; quit awaits the final save" (#2370) for
// why the actual quit path gets a manual-verification note instead.
//
// The alias map is imported from `./alias-map.ts`, which this config shares
// with `vite.config.ts` (#2418). It used to be a hand-copied SUBSET of the
// vite map (2026-08 review finding, #2409): without an entry,
// `@brink-lang/editor` / `@brink-lang/web` fall back to `dist/` builds that
// are neither git-tracked nor produced by `pnpm install`, so
// `export-artifact.test.ts` — that PR's headline artifact-verification
// test — silently failed to resolve its entry point and ran zero tests
// behind a green-looking CI step ("1 failed | 2 passed" at the file level,
// "9 passed" at the test level). Sharing one map removes the class: an
// alias added for the app is an alias the suite resolves too.
// `brink-web` aliases to the REAL wasm-bindgen glue the Frontend CI job
// already builds (`crates/brink-web/www/pkg/brink_web.js`), never
// `brink-studio`'s `__mocks__/brink-web.ts` — that mock would make
// `export-artifact.test.ts` prove nothing about a real compiled artifact.
export default defineConfig({
  test: {
    environment: "node",
  },
  resolve: {
    alias: desktopAliases(__dirname),
  },
});
