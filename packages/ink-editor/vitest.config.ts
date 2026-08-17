import { defineConfig } from "vitest/config";

// ink-editor's own unit suite (#2559) — until this file, `@brink-lang/editor`
// had NO test script and ZERO .test.ts(x) files under this package; its only
// coverage was `packages/brink-studio/src/__tests__/*` reaching in through
// the studio's alias map. That made a published package's regressions gated
// by a *different* published package's runner: see #2559 and the ruling
// recorded on it ("editor should have its own test suite yeah").
//
// jsdom, matching `packages/brink-studio/vitest.config.ts`: this package is a
// CodeMirror 6 / DOM package (widgets, inputs, `document.createElement`), so
// most of what's worth unit-testing here needs a `document`.
//
// Deliberately NO wasm alias, unlike the studio's config (which repoints
// `brink-web` at a jsdom mock via `studioTestWasmAliases`). This package's
// runtime code only ever *type*-imports from `@brink/wasm-types` (erased at
// build time, so it needs no runtime resolution) and reaches
// `@brink-lang/web` only through modules this suite's tests should avoid
// importing (`document-sessions.ts`, `project-session.ts`, and the package's
// own `index.ts` barrel, which re-exports both) — those pull in the real
// wasm-bindgen glue and are exactly what makes the studio suite slow to
// start. Test files here should import their subject directly from its
// source module (e.g. `../inline-name-input.js`, `../rename.js`), never
// through `./index.js` or `@brink-lang/editor`, so this suite never needs a
// built `crates/brink-web/www/pkg` at all. If a future test genuinely needs
// the wasm-backed surface, add the alias then and say why in this comment —
// don't add it speculatively.
export default defineConfig({
  test: {
    environment: "jsdom",
    exclude: ["node_modules/**", "dist/**"],
  },
});
