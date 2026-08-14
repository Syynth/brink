import { defineConfig } from "vitest/config";
import { desktopAliases } from "./alias-map";

// Node environment BY DEFAULT — most of this suite needs no DOM. See
// docs/decision-log.md "Desktop close: no dirty prompt; quit awaits the
// final save" (#2370) for why the actual quit path gets a
// manual-verification note instead of a headless one.
//
// The default is `node` rather than `jsdom` because only a minority of
// files need a DOM, and jsdom costs startup time on every other file. A
// file that does need one opts in per-file with a
// `// @vitest-environment jsdom` pragma rather than flipping the package
// default — `autosave-reopen.test.ts` (#2486) is the first, because it
// drives `main.tsx`'s real `openProject`/`closeProject` (Tauri IPC,
// `mountStudio` and the file provider all mocked) and those read
// `document`. So the Tauri IPC surfaces ARE now exercised headlessly under
// mocks in that one file; `quit.ts`'s awaitable-save seam is no longer the
// only thing unit-tested here.
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
