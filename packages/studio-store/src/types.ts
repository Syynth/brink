/**
 * Local type definitions for studio-store.
 *
 * Types that will eventually come from @brink-lang/editor are defined here
 * as forward-compatible stubs until that package is created.
 */

// ── Element types ──────────────────────────────────────────────────────
//
// `ElementType`/`LineInfo` used to be duplicated here (a forward-compatible
// stub predating @brink-lang/editor). #368 deletes the duplicate — both are
// now re-exported from the real module (element-type.ts) via index.ts, which
// re-exports `ElementType` as `ElementTypeEnum` for historical reasons (kept
// for call-site compatibility).

// ── Key hints ────────────────────────────────────────────────────────

export interface KeyHint {
  key: string;
  hint: string;
}

// ── Document targets ─────────────────────────────────────────────────

export type TabTarget =
  | { kind: "file"; path: string }
  | { kind: "symbol"; path: string; name: string; start: number; end: number };

// ── Editor types (the real classes from @brink-lang/editor) ───────────

export type { DocumentSessions, ProjectSession, FileConflict } from "@brink-lang/editor";
