# Building a tool on `@brink-lang/editor`

For hosts that embed the ink editor **without** the `brink-studio` app. Everything of
substance from the #311 editor sweep lives in `@brink-lang/editor` (the CM6 editor,
published from `packages/ink-editor`) and `@brink-lang/web` (the wasm handle). `studio`
is just *one* consumer of these — your tool is on the same footing. This guide maps the
reusable seams, the per-feature host contract, and the thin studio wrappers to copy from.

## The two entry points

1. **`brinkStudio(options: BrinkStudioOptions): Extension`** (`extensions.ts`) — the editor
   itself. A CodeMirror 6 extension factory. You pass callbacks that bridge to `@brink-lang/web`;
   the editor supplies all the UI (completions, hover, inline rename, code-actions, folding, …).
   Features light up **only when you provide their callback** — omit `getCodeActions` and there's
   no code-actions menu, etc.
2. **`ProjectSession`** (`project-session.ts`) — the reusable session/file/dirty/conflict layer.
   Owns the wasm `EditorSessionHandle`, tracks dirty state, applies cross-file edits, and runs the
   external-conflict detection. This is the object your tool builds on; `studio` uses the same class.

You wire a `FileProvider` (implement the interface, or use the shipped `InMemoryFileProvider`) into
`ProjectSession`, then mount `brinkStudio(...)` per document.

## The host contract — `BrinkStudioOptions`

Provide only what you want. Each optional callback is the *bridge* to a `@brink-lang/web` op;
the editor owns the resulting UX.

| Option | Feature it enables | Backing `@brink-lang/web` call |
|---|---|---|
| `compile`, `getSemanticTokens`, `getTokenTypeNames` | required base | `compile`, `semantic_tokens`, … |
| `getCompletions` + `autoImport` | completions + **auto-import** (#312): out-of-scope completions tagged `from <file>`, accept inserts the `INCLUDE` | `completions_*`, `auto_import_include_doc` / `auto_import_apply_include_doc` → `AutoImportResult` |
| `getFoldingRanges` | folding incl. the **INCLUDE-block fold** (#313, `INCLUDE … (N files)`) | `folding_ranges` → `FoldRange[]` |
| `getCodeActions` | **code-actions** menu (#321); apply via the resolve op | `code_actions_*` + `resolve_code_action` |
| `prepareRename` + `renameSymbolAt` + `commitRename` (+ `onRenameBreakage`) | **inline rename** (#323) with the live **"⚠ breaks N"** badge (#324) and the inline breakage report | `prepare_rename` + `rename_symbol_at` → `StructuralResult` |
| `getHover`, `gotoDefinition`, `findReferences`, `getSignatureHelp`, `getInlayHints`, `getArgumentWidgets` | the corresponding LSP-style features | `hover`, `goto_definition`, `find_references*`, … |
| `onPlayFrom`, `onSymbolContextMenu`, `onNavigateToFile` | host hooks for your own chrome | — |
| `getGutterMarkers` (+ `onGutterMarkerClick`) | **host gutter markers** (#343): your own per-line gutter affordances (breakpoints, annotations, run/flag icons) | — (host-supplied data) |

Notes on the host gutter contract (#343):
- `getGutterMarkers(source, fromLine, toLine) => HostGutterMarker[]` returns markers
  (`{ line, className?, text?, title?, onClick? }`, `line` 1-based) for the inclusive line range.
  It is currently queried for the whole document; the range parameters let the contract narrow to
  the viewport later without an API change.
- Markers render in a **dedicated gutter slotted after (to the right of) the built-in
  play-from-here ▶ gutter**, so your affordances coordinate with the editor's own instead of
  needing a raw CM6 `gutter()`. Ordering is deterministic: by line, your array order within a line.
- Clicks fire the marker's own `onClick(line)` first, then the shared
  `onGutterMarkerClick(marker, line)`.
- The set recomputes on document changes. When it changes for *external* reasons (a breakpoint
  toggled in another panel), call the exported `refreshGutterMarkers(view)` (or dispatch
  `refreshGutterMarkersEffect`) to re-query.
- Composing extensions directly instead of using `brinkStudio`? The same feature is exported
  standalone as `hostGutterExtension(options)`.

Notes on the rename contract (the one with the most moving parts):
- `renameSymbolAt(offset, newName) => StructuralResult` is called **debounced on each keystroke** to
  drive the badge count (`result.introduced_diagnostics.length`, `result.safe`).
- `onRenameBreakage(result, ctx) => boolean` lets you **override how the breakage report is presented**
  (return whether to proceed). The editor's *default* is the inline report; studio uses this hook to
  route non-editor (binder/graph) renames to its own modal. Your tool decides its own rendering here.

## Per-feature: what you get vs. what you rebuild

| Feature | Reusable export (`@brink-lang/editor`) | You provide | Studio wrapper to reference (thin) |
|---|---|---|---|
| Find panel (#319) | `findPanel(options?)` extension | just add it to your editor's extensions | — (studio doesn't use it) |
| Inline markup (#367) | `inlineMarkup(rules)` extension + `rmmzAngleTagRule` preset; matches decorate as `brink-markup-<name>` + `data-*`, scoped to narrative content (never over ink syntax) | your `InlineMarkupRule[]` + host CSS for the classes | — (zero rules by default) |
| Fold INCLUDE block (#313) | `foldingExtension` + `getFoldingRanges` callback | the wasm folding call | — |
| Auto-import (#312) | completion tag + accept-side insert (wired via `brinkStudio`) | `getCompletions` + `autoImport` callbacks | — |
| Code-actions apply (#321) | menu + apply dispatch (via `getCodeActions`) | `getCodeActions` + the resolve op | — (not enabled in studio) |
| Inline rename + breaks-N (#323/#324) | the whole in-editor UX in `rename.ts` (input, badge, inline report) | rename callbacks above | binder/graph menus + modal `SymbolRenamePrompt` (only if you have non-editor rename entry points) |
| Merge view (#320) | `ConflictView` class + `ConflictViewOptions`; detection via `ProjectSession`/`FileChangeHub` | mount `ConflictView`; call the resolve methods | `studio-ui/ConflictMergeView.tsx` (~77 lines: React mount + store) |
| Editable search buffer (#322) | `SearchResultsBuffer` class; pure model `buildResultsRows` / `mapRowEditToSource`; `searchSources` engine | mount the buffer; feed it search results | `studio-ui/SearchResultsBufferView.tsx` (~85 lines) |
| Option identity / branch rails (#364) | `data-option-path` (full weave lineage, e.g. `"0.2.1"`) + `data-option` (innermost index) line attributes on choice/body lines, emitted automatically | nothing — style them from CSS (e.g. per-branch `border-left` rails colored from the path) | — (headless: brink emits identity, the host styles it) |

## The external-conflict flow (#320) — the one with real session API

Data-loss prevention lives in `ProjectSession` + `FileChangeHub`, not in any UI:
- Pass `onFileConflict: (conflict: FileConflict) => void` to `ProjectSession`. It fires when an
  external change arrives for a **dirty** buffer whose content differs — the editor buffer is
  **kept** (no clobber), and you're handed `{ path, disk, buffer, baseline }`.
- Query state: `dirtyPaths()`, `conflictedPaths()`, `hasConflict(path)`.
- Resolve: `resolveConflictUseDisk(path, disk)`, `resolveConflictKeepMine(path)`,
  `resolveConflictMerged(path, merged)`.
- Render: mount `ConflictView` (the self-contained 2-way `@codemirror/merge` banner + side-by-side
  surface) into a container, wiring its buttons to the three resolve methods.

## `ProjectSession` — the reusable surface (selected)

`addFile` · `closeFile` · `deleteFile` · `renameFile` · `getFiles` · `requestFile` ·
`applyEdit(path, newSource)` (the canonical cross-file edit seam — everything routes through it) ·
`compileProject` · `dirtyPaths` / `markAllSaved` / `save` · the conflict methods above ·
`refreshIncludes` · `initialize` / `destroy`.

## `DocumentSessions` view-state persistence (#347)

Per-tab cursor + scroll save/restore across app reloads:

- `viewState(docKey, groupId?) → { anchor, head, scrollTop } | null` — snapshot seam. Works for
  **every open tab**: mounted views read live; backgrounded tabs read their cached `EditorState`
  (selection) plus the scroll snapshotted at unmount. `groupId` addresses one `(docKey, groupId)`
  slot exactly (split views); without it, prefers focused → mounted → cached.
- `restoreViewState(docKey, snapshot, groupId?)` — applied on next mount (or immediately when
  mounted), no focus steal, offsets clamped so stale/corrupted snapshots degrade instead of
  throwing. Pass `groupId` to restore each pane of a split view independently.
- `revealAt(docKey, offset)` remains the "jump to a diagnostic" primitive (focus + center scroll).

Persistence loop: on quit, `viewState` every open tab into your session blob; on boot, reopen the
tab set, then `restoreViewState` each entry.

## `@brink-lang/web` structural ops (all return `StructuralResult`)

After #316 every mutating structural op returns one shape —
`StructuralResult { ok, path?, new_source?, cross_file_edits, safe, introduced_diagnostics }` — so you
gate them all the same way (show the breakage report when `!safe`, apply `cross_file_edits` on proceed):
`rename_symbol` / `rename_symbol_at` · `move_stitch` · `promote_stitch` · `demote_stitch` · `reorder_*`
· `delete_symbol` (#316) · `rename_dir` (#314) · `extract_to_knot` / `extract_to_function` (#315) ·
`resolve_code_action` (#321) · `find_references_at` / `references_to_symbol` (#317, document-agnostic).

## Boundary helpers (#369)

Small pure helpers every host needs at the editor ↔ host seam, published from
`@brink-lang/editor` so you don't carry shim copies:

- **`CompileResult` / `Diagnostic`** — re-exported from `@brink-lang/web`, so importing them
  through the editor gives the *same module identity* as importing `@brink-lang/web` directly.
  No structural `CompileResultLike` shims.
- **`sortDiagnostics(diagnostics)`** — the canonical **positional** sort: file path, then start
  offset, then errors before warnings (end offset and message as deterministic tiebreakers).
  Non-mutating. Note: presentation ORDER is a **host choice** layered on this canonical
  positional sort — re-group the sorted list however your UI wants (severity-first, per-file
  sections, …); the helper is the shared baseline, not a rendering policy.
- **`lineColAt(text, offset)`** — offset → 1-based `{ line, col }`, clamped to the text.
  UTF-16 offsets, matching `Diagnostic.start`/`end` and `editor.reveal` source spans.

(`docKeyFor` / `parseDocKey` are also published — see `document-sessions.ts`.)

## What is genuinely studio-only (you rebuild in your framework)

- **React mount wrappers** for the class-based UIs (`ConflictView`, `SearchResultsBuffer`). Thin —
  the studio versions are 77–85 lines; copy the shape, swap the framework.
- **State management**: studio uses Zustand slices (`conflict`, `search`, `symbol-menu`). You hold
  that state your own way.
- **App entry points**: binder/graph context menus, the modal rename prompt for non-editor surfaces.
  You have your own tool surface; the editor's in-canvas features don't need these.

Bottom line: you inherit every feature's substance from the editor + wasm packages and reimplement
only thin mount glue + your own state/entry-points.
