# Search result cards — spec + build plan

Ruled 2026-08-24 (decision-log: "Search results: stable snapshot, per-match
editable cards with context" + addendum). Design canvas: `.design/search-results/`
(three artboards: Main = text search, References, Replace), approved by the
maintainer ("yeah, this all looks good").

## The surface

One results surface serves text search AND Find References (the context-menu
spec's open question, ruled): a vertical list of **per-match cards** inside
the Search tool window, replacing the single editable results buffer
(#322's "Zed design D" — superseded by this spec).

Each card:

- **Header row**: `file:line` (mono, accent) · containing knot/stitch ·
  mode-specific badges · reveal `↗` (dispatches `editor.reveal`, same as
  today) · collapse chevron (leftmost).
- **Body**: an individual small **editable buffer** showing the match line
  with its context window — default **1 line above, 2 below**, tunable via
  the `context 1↑ 2↓ ▾` knob in the summary row. Edits write through to the
  source via the existing apply-edits seam. Full syntax highlighting.
- **Collapsed**: header only, with a truncated one-line match preview inline;
  `⌃⌃` / `⌄⌄` in the summary row collapse/expand all. Collapse state spans
  modes.

### The frozen snapshot (the core semantic)

Once a search (or references query) populates the panel, the result set is a
**snapshot**: edits — from cards or from regular editors — NEVER remove or
re-filter rows. A card whose match no longer matches the query is badged
`edited` and stays. Only two things replace the snapshot: running a new
search (typing in the query field) or the explicit **`↻` refresh** in the
summary row / references header.

Positions inside the snapshot are **edit-mapped**: every match span (and the
references declaration anchor) maps through subsequent document changes, so
write-through stays correct, stale detection works, and references-refresh
re-resolves from the declaration's *current* position (the original click
offset goes stale).

### References mode

- Mode header replaces the query summary: `REFERENCES <symbol> · N in M
  files` + `↻` + `✕` (✕ = `clearReferences`, returns to query mode without
  re-running the typed query).
- The **declaration card is pinned first** with an accent border + `decl`
  badge.
- Each site carries a **kind-of-use badge**: `call` / `divert` / `read` /
  `write`. (Needs a small wasm addition — see PR E.)
- Typing in the query field exits references mode (already implemented).

### Replace mode

Typing replace text turns every still-matching card into a **preview**: old
text struck red, new text green, display-only. The previews ARE the
confirmation — the old arm-then-confirm step is removed.

- Per-card **Accept** applies that one replacement → card shows `✓ replaced`.
- Per-card **skip** excludes it (badge `skipped`, `undo skip` available).
- **Accept all (N)** in the summary applies every *pending* card — skipped
  and `edited`-stale cards are excluded, each badged with why.
- Hand-editing a card exits its preview (it becomes `edited`).

### Performance (ruled "if it's not too slow")

- **Virtualized list**: only visible cards are real CM instances (minimal
  extension set: highlight decorations + write-through; no basicSetup).
  Off-screen cards render as static HTML.
- **Per-file semantic-token cache**: ONE `getSemanticTokens(source)` call
  per file-with-results, memoized on the snapshot; cards (live and static)
  slice their lines from it. Edits re-tokenize only the edited file,
  debounced. Never per-card wasm calls.

## What already exists (the foundation, on branch `feat/references-in-search`)

- `locationsToSearchResult` (ink-editor `project-search.ts`): Locations →
  `ProjectSearchResult` (grouped, line previews).
- Search slice (`studio-store/slices/search.ts`): `searchMode`
  (`query`/`references`), `showReferences`, `clearReferences`,
  `searchRevealSeq` (SearchCommands reacts with `ensureToolWindowOpen` —
  the layout store lives in the shell, unreachable from the slice).
- Editor routing: `findReferencesAt` (references.ts) sends `(symbol,
  locations)` to `onShowReferences` when wired (menu item + ⇧⌥F); in-view
  highlight remains only as a no-host fallback. Threaded through
  `brinkStudio` options and `DocumentSessions` callbacks; mount wires it to
  the slice.
- SearchView: references-mode chip (baseline UI, superseded by this spec's
  mode header) + the mount-time live-search debounce is guarded so opening
  the panel to show references doesn't clobber them (only an actual
  query/options change exits references mode) — keep this guard.

## PR stack

Stack base: `feat/references-in-search` (open it as the first PR — it is
functional on its own). Each subsequent PR branches from the previous.

1. **PR A — plumbing (exists)**: everything under "What already exists" +
   this spec + the design canvas files.
2. **PR B — snapshot model + edit mapping** (studio-store, no UI):
   `SearchSnapshot` replacing raw `ProjectSearchResult` in the slice:
   per-match tracked spans mapped through document edits (subscribe to the
   documents/compile seam the replace path already uses), `stale`/`edited`
   flags, context-lines setting (default 1/2, persisted per session),
   `refreshSnapshot()` (re-run query; references re-resolve from the mapped
   decl anchor), collapse-state map. Pure-logic tests for mapping and
   staleness.
3. **PR C — card list view**: virtualized card list in SearchView
   (replaces `SearchResultsBufferView` usage for both modes; keep the old
   component until D lands, then delete), per-card CM editors (visible) +
   static HTML (off-screen) sharing the per-file token cache, headers
   (file:line, knot via outline lookup — reuse the TODO panel's
   `containerAt` approach), collapse + all-buttons, reveal, `edited`
   badges, context knob.
4. **PR D — replace previews**: pending/skip/accept/accept-all/replaced
   states per the Replace artboard; remove the arm-confirm flow and the old
   results buffer component.
5. **PR E — references dressing**: decl-pinned card + kind-of-use badges.
   Wasm addition: `find_references` variant returning per-location kind
   (`decl`/`call`/`divert`/`read`/`write`) — the resolution data exists in
   the analyzer's `ResolvedRef`/span kinds; expose through
   `brink-web/src/editor/navigation.rs` + wasm-types + the mock. References
   `↻` re-resolve from the mapped decl anchor.

Gates per PR (from CLAUDE.md's table): studio suite needs the wasm pkg
built; ink-editor has its own suite; wasm-observable changes (PR E, and PR A
already has one) need a `@brink-lang/web` changeset; every studio-side PR
needs a `@brink-lang/studio` changeset (editing an existing changeset
counts). Run `cargo clippy --workspace --all-targets -- -D warnings` for any
`.rs` change — doc-comment backticks and `too_many_lines` are the usual
traps.

## Session facts a fresh context needs

- The playground dev server is launch.json entry `studio-openflow` (port
  5186); it serves THIS worktree — verify live there, and remember vite
  serves whatever branch the worktree has checked out.
- The desktop releases 0.2.0–0.2.2 shipped via
  `scripts/release-desktop-local.sh` (CI's macOS codesign hang is
  unresolved; probe proved Apple reachable — see the workflow comments).
  The script derives the version from the tag and must be invoked from a
  checkout that has it (main has it now).
- Context-menu matrix state: headers/identity/text/structural rows +
  Tab-indent all merged (#3053, #3054, #3059, #3060, #3061); externals
  rename behind the E190 Force gate; remaining rows: weave ops, tag actions
  (parked on #474), grayed-vs-hidden ruling.
