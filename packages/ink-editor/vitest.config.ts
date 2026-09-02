import { defineConfig } from "vitest/config";
import { resolve } from "node:path";

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
// Deliberately NO wasm alias for the general case, unlike the studio's
// config (which repoints `brink-web` at a jsdom mock via
// `studioTestWasmAliases`). This package's runtime code only ever
// *type*-imports from `@brink/wasm-types` (erased at build time, so it needs
// no runtime resolution) and reaches `@brink-lang/web` only through modules
// this suite's tests should avoid importing (`document-sessions.ts`,
// `project-session.ts`, and the package's own `index.ts` barrel, which
// re-exports both) — those pull in the real wasm-bindgen glue and are
// exactly what makes the studio suite slow to start. Test files here should
// import their subject directly from its source module (e.g.
// `../inline-name-input.js`, `../rename.js`), never through `./index.js` or
// `@brink-lang/editor`, so this suite needs no built `crates/brink-web/
// www/pkg` at all AT TEST-RUN TIME; the workspace install still does
// (#2479).
//
// ONE exception (#2794): `__tests__/project-session-destroy.test.ts` tests
// `ProjectSession.destroy()`'s handling of a gated call still waiting on its
// `scheduleIdleWork` yield — a fix that lives IN `project-session.ts`, so
// that test has no choice but to import it directly. It still needs no real
// wasm: every test in that file supplies its own stub
// `ProjectSessionOptions.session`, so `new EditorSessionHandle()` (the only
// thing that would actually touch wasm) is never reached — but
// `project-session.ts`'s own `import { EditorSessionHandle } from
// "@brink-lang/web"` still has to resolve to SOMETHING for the module to
// load at all, and that package's real entry point needs a `tsup` build
// this suite does not run (unlike the studio's alias, which points at
// SOURCE, `@brink-lang/web`'s own source imports the real wasm-pack glue).
// The alias below repoints it at a local, do-nothing stub instead
// (`src/__mocks__/brink-web-stub.ts`) — narrower than reaching for the
// studio's mock (a different package, and a heavier one than one class
// this suite never constructs).
export default defineConfig({
  test: {
    environment: "jsdom",
    exclude: ["node_modules/**", "dist/**"],
  },
  resolve: {
    alias: {
      "@brink-lang/web": resolve(__dirname, "src/__mocks__/brink-web-stub.ts"),
      // The dialect core moved to its own pure-TS package (#3393); this
      // suite resolves it to SOURCE, like the studio's alias map does.
      "@brink-lang/dialect": resolve(__dirname, "../dialect/src/index.ts"),
    },
  },
});
