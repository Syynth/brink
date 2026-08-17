/**
 * Minimal stand-in for `@brink-lang/web`, used ONLY by this package's own
 * unit suite via `vitest.config.ts`'s `resolve.alias` (issue #2794's
 * `ProjectSession.destroy()` regression test, `__tests__/
 * project-session-destroy.test.ts`) — see that config file's header for why
 * this suite otherwise avoids the real wasm-bindgen glue entirely, and why
 * this one exception is safe: `project-session.ts`'s module-level `import {
 * EditorSessionHandle } from "@brink-lang/web"` must resolve to SOMETHING
 * for that file to import at all, but every test that imports it supplies
 * `ProjectSessionOptions.session` itself, so the real class is never
 * constructed — this stub exists purely to satisfy module resolution, not
 * to be exercised.
 */
export class EditorSessionHandle {
  constructor() {
    throw new Error(
      "EditorSessionHandle stub (packages/ink-editor's own test suite): " +
        "every test here injects ProjectSessionOptions.session — this " +
        "constructor should never run. If a test needs the real wasm " +
        "session, it belongs in packages/brink-studio's suite instead.",
    );
  }
}
