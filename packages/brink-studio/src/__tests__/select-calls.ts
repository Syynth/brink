/**
 * The canonical id list for §7.7.1's text-input select-call invariant
 * (issue #2542).
 *
 * `docs/studio-shell-spec.md` §7.7.1 rule 2 — "never `select()` text the user
 * typed" — was, until this file, enforced by exactly one vitest suite
 * (`search-view-focus.test.tsx`), which only exercises `SearchView`. A new
 * `.select()`/`.setSelectionRange()` call site anywhere else in `studio-ui`,
 * `studio-shell`, or `ink-editor` shipped with zero signal — the exact shape
 * of the `inline-name-input.ts` violator that #2548 fixed.
 *
 * This is the allowlist half of that fix, sibling to `save-paths.ts`'s role
 * for the confirm→retire sweep: `select-call-enrolment.test.ts` scans
 * production source for every real `.select()` (zero-argument only — see
 * that file's header for why) / `.setSelectionRange(` call site — plus, since
 * #2571, the sibling spellings that reach the same end state (a
 * `.selectionStart`/`.selectionEnd` write, `execCommand`, and the
 * Selection/Range API) — and requires
 * a `SELECT-INVARIANT` marker comment above each one naming an id from this
 * list, so a brand-new call site fails loudly until a human names it here
 * and states why it satisfies (or is exempt from) the invariant.
 *
 * Adding a new call site therefore means: add its id here, and put a
 * `// SELECT-INVARIANT <id>: <justification>` marker directly above the call
 * (or `// SELECT-INVARIANT-EXEMPT <id>: <reason>` if the call is provably
 * unrelated to a seeded text input — e.g. a same-named but different API).
 *
 * It lives in its own module rather than being exported from the test file
 * for the same reason `save-paths.ts` does: importing a `*.test.ts` file
 * re-registers its `describe`/`it` blocks in the importer (measured in PR
 * #2510 — guarded workspace-wide by `no-test-file-imports.test.ts`, #2516).
 */
export const SELECT_CALL_IDS = [
  "Binder.renameInput.preSelectBasename",
  "Binder.newFileInput.cursorToEnd",
  "SymbolRenamePrompt.select",
  "SearchView.select",
  "InlineNameInput.select",
] as const;

/** The id of a call site the §7.7.1 select-call enrolment guard tracks. */
export type SelectCallId = (typeof SELECT_CALL_IDS)[number];
