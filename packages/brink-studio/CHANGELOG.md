# @brink-lang/studio

## 0.15.0

### Minor Changes

- 8f30776: `[project] entry` now has a real schema slot and wins over a host's `entryFile` argument (issue
  #2331, ruled 2026-08-07 "`[project] entry` beats `mountStudio`'s `entryFile`").

  - `brink_project_config::ProjectConfig` gains `entry: Option<String>`, validated the same way as
    `conventions` (any non-empty string; existence/resolution is left to the consuming mount, kept
    dependency-free per #1234).
  - `EditorSession` (`@brink-lang/web`) tracks the discovered file's `[project] entry` and exposes it
    via the new `configured_entry()`/`EditorSessionHandle.getConfiguredEntry()` — `null` when no
    `brink.toml` was found, or one was found that doesn't set `entry`.
  - `ProjectSession` (`@brink-lang/editor`) now owns entry-file precedence: after
    `discoverProjectConfig` runs, a discovered `entry` that resolves to a real file in the session
    supersedes the constructor's `entryFile` argument (for both `compileProject()` and
    `getEntryFile()`); an `entry` that does NOT resolve to a real file falls back to the current
    `entryFile` and is reported through the existing `onProjectConfigWarnings` channel — no new
    warning channel invented. The `entryFile` constructor option is now only the configless fallback
    (and the seed path `brink.toml` discovery walks up from).
  - `mountStudio` (`@brink-lang/studio`) opens the initial tab from `project.getEntryFile()` (read
    after `initialize()`, so any config supersession has already happened) instead of its raw
    `entryFile` option.
  - `packages/brink-desktop`'s `resolveEntryFile` regex peek at `[project] entry` is deleted (not
    merely unused) — it shrinks to the plain configless-fallback chain, since `ProjectSession` now
    supersedes its guess whenever `brink.toml` sets a valid `entry`. Not independently versioned
    (`@brink/desktop` is private).

  The embedded playground's `?fixture=native` project (`packages/brink-studio/src/main.tsx`'s
  `NATIVE_FIXTURE`) already sets `entry = "story.brink"`, agreeing with the `entryFile` argument
  `main.tsx` passes for that fixture — this change is a no-op there by construction, not by luck; a
  test asserts the agreement holds (`config wins` test in `project-config-application.test.ts`
  additionally exercises a real mismatch to prove supersession, not just agreement).

- 9fc8665: Binder "Library" section for mounted `std/` files (issue #2343, part 2 of #2306's ruling "Mounted stdlib
  presents as a read-only library node" — part 3, session-level read-only enforcement, shipped separately
  in #2342). `list_files`/`project_outline`/`story_graph` (`@brink-lang/web`) switch from **excluding**
  mounted stdlib files entirely (#2231's phantom-row fix) to **listing them flagged** (`mounted: boolean` on
  `ProjectFile`/`FileOutline`/`StoryGraphNode`) — dropping the exclusion without adding a consumer that
  renders the flag would reintroduce the exact phantom-row bug #2231/#2303 fixed, so this ships both
  together. `EditorSession::remove_file` (`@brink-lang/web`) and `rename_file` now refuse a mounted path
  (the delete/rename route gap #2343's review found: previously unreachable only because `list_files`
  excluded the mount from the Binder) — `remove_file` gains a `boolean` return (previously `void`).

  `@brink-lang/studio`'s Binder renders a visually distinct, collapsed-by-default "Library" section below
  the project's own file tree: browsable (expand/collapse a folder tree, click/double-click to open a file
  read-only) but with no drag, rename, delete, or new-file affordances. `@brink/studio-store`'s search slice
  (internal) excludes mounted files from `runSearch`'s candidate list — "Excluded from save-all and
  search/replace" per the ruling — and `ProjectSession.markAllSaved` (`@brink-lang/editor`) does the same for
  `file.saveAll`. The binder slice's `applyMoveResult`/`undo` now surface an `applyEdit` refusal (a structural
  move or undo landing on a mounted path) as a "skipped N read-only file(s)" warning instead of a silent
  no-op behind a success toast.

  A mounted file's CM6 view (`@brink-lang/editor`'s `DocumentSessions`) is now genuinely non-editable —
  `EditorState.readOnly` + `EditorView.editable.of(false)`, the same pattern `conflict-view.ts` uses for its
  "ON DISK" pane — rather than relying solely on the wasm-layer write refusal to make a keystroke silently
  revert. `ProjectSession` gains a public `isReadOnly(path)` query for this. Navigation (goto-def/hover) into
  a mounted file lands in the same read-only view via the existing open-file path — no special-casing needed.

- ce263f8: External deletion of an open file: keep the view, mark orphaned; ⌘S
  recreates (issue #2371, 2026-08-07 decision). `mountStudio`'s
  `onExternalFileChange` used to skip deletions entirely; it now calls
  `DocumentSessions.markOrphaned`, which never touches the kept editor buffer
  (no refresh, no auto-close) and recreates the file in the wasm session from
  that buffer so IDE queries and a later save keep working. `FileChangeHub`
  gains an `orphaned` path set (`isOrphaned`/`orphanedPaths`, mirroring the
  existing `conflicted` tracking) — set by `noteOrphanRecreated` once a kept
  buffer is confirmed to survive a deletion (`applyExternal(path, null)`
  alone does not flag it, so a headless deletion with no open view never gets
  permanently badged), cleared by a canonical save (`markSaved`, or a
  write-through `flush()`) or by the path reappearing on disk. New
  `ProjectSession.recreateOrphaned` (the
  provider is deliberately not notified until a real save, so recreation
  stays gated on ⌘S even for a provider whose `onFileChanged` is itself the
  persistence step) and `isOrphaned`/`orphanedPaths` pass-throughs. New
  `StudioApi.getOrphanedFiles()`, mirroring `getDirtyFiles()`, for a host to
  render an orphaned-tab badge.
- 2303f00: `StudioApi` gains `getStoryBytes()` — the latest successful compile's story
  bytes, or `null` when the latest compile failed (issue #2391, "Export Story
  (.inkb)"). Pull-on-demand, like `getFiles()`/`getDirtyFiles()`: bytes are
  big and change on every compile, so they stay out of `StudioPublicState`. A
  host drives `dispatch("compile.run")` first (the same surface the Player's
  Run button uses) to get a fresh compile, then reads this to get the
  artifact. Purely additive — no existing `StudioApi` behavior changes.
- 733e3ec: `FileProvider.renameFile` now receives the moved file's rewritten source
  (issue #2425).

  The rename op folds the moved file's own outbound `INCLUDE` rewrites into
  `new_source`, but the atomic-rename branch of `ProjectSession.renameFile`
  passed only the two paths on — so a host whose `renameFile` moves bytes
  (a real filesystem rename) kept the pre-rewrite text in storage, while the
  `createFile` + `deleteFile` fallback branch had always written the rewritten
  source. The optional third parameter, `newContent`, closes that gap:

  ```ts
  renameFile?(oldPath: string, newPath: string, newContent?: string): Promise<void>;
  ```

  It is optional and additive — an existing implementation declaring only
  `(oldPath, newPath)` still satisfies the interface and behaves exactly as
  before. `InMemoryFileProvider` now stores `newContent` when supplied.

  `@brink-lang/studio` re-exports `FileProvider` through `mountStudio`'s
  `MountStudioOptions.provider` (`packages/brink-studio/src/mount.tsx`), so an
  embedder that supplies its own provider and implements `renameFile` is
  affected by the new third argument: it can keep taking two parameters and
  see no change, or add the third to persist the rewritten source the way
  `InMemoryFileProvider` and `TauriFileProvider` now do.

- e69d48c: **Breaking:** `ProjectSession.renameFile` now resolves `Promise<RenameFileResult>`
  instead of `Promise<string[]>` — a consumer doing `(await project.renameFile(a,
b)).length` or iterating the resolved value directly will break at runtime.
  `packages/ink-editor/src/index.ts` also gains two new exported types,
  `RenameFileResult` and `RenameDirResult`.

  Surface the rename/move breakage gate at the Binder's rename call sites (issue #2918).

  `ProjectSession.renameFile`/`renameDir` (`@brink-lang/editor`) run the same
  safe-by-default breakage gate every other structural op does (#316): the
  wasm `rename_file`/`rename_dir` ops already compute `safe` and
  `introduced_diagnostics` correctly. But both methods used to resolve with
  only the bare data a caller needed to apply the move (a referrer path list,
  or `{ moved, referrers }`) — discarding the breakage-gate verdict entirely.
  A move that broke a reference (a divert pointing at the renamed file, for
  example) applied exactly like a clean one, with nothing anywhere telling the
  user.

  `renameFile` now resolves with `{ referrers, safe, introducedDiagnostics }`;
  `renameDir` with `{ moved, referrers, safe, introducedDiagnostics }`. The
  Binder's `applyRename`/`applyDirRename` (`studio-store`'s binder slice,
  bundled into `@brink-lang/studio`) thread the verdict through to the same
  `_notify` channel PR #2916 used for a refused move: a `safe: false` result
  now raises a `warning`-severity "breaks N reference(s)" notification instead
  of the unconditional `info` toast every rename got before. This is the
  notification FLOOR, not a preflight gate — the move still applies (the undo
  entry still gets pushed) exactly as it did before; the user is now told
  about the breakage rather than discovering it later. The fuller "will break
  N references" preflight/confirm pattern (#324) exists for the editor's
  inline symbol rename, on a dedicated widget the Binder's type-a-new-name
  tree rename has no analog of — building one is out of this fix's scope; see
  issue #2918 for the follow-up.

- 18da64e: Overlay persistence for embedding hosts (the celeris file model, 2026-08-07
  decision): `FileChangeHub`/`ProjectSession`/`mountStudio` gain an
  `egressPersists: false` contract under which `onFilesChanged` delivery feeds
  a backup ring rather than counting as persistence — dirty then means
  "diverges from the last canonical save" and only the save commands clear it
  (an undo back to the saved text drops to clean). New `OverlayPersistence`
  coordinator in `@brink-lang/editor`: routes egress batches to a
  host-provided `BackupSink` (ring bounds are sink-owned), owns canonical
  `save`/`saveAll` (write + re-baseline, rejected writes stay dirty for
  retry), and an autosave scheduler where an autosave tick IS `saveAll` —
  one save path, one artifact class. The default (`egressPersists` absent)
  is byte-identical to the previous write-through behavior.

### Patch Changes

- d9a83d3: `brink.toml` is no longer inert (issue #2324). `EditorSessionHandle.applyProjectConfig`/`discoverProjectConfig`
  (#1005, #1414) were exposed and unit-tested but nothing outside test files ever called either, so every
  `[project]`/`[lints]` key in a mounted project's `brink.toml` was silently ignored end to end.

  `ProjectSession` (`@brink-lang/editor`) now calls `discoverProjectConfig` — chosen over `applyProjectConfig`
  because it walks the session's own already-loaded documents, so no host-specific directory-walk/read code is
  needed — once during `initialize()` (before the first analysis) and again whenever a `brink.toml` anywhere in
  the session is created, edited, renamed into/out of, or externally rewritten. A new optional
  `ProjectSessionOptions.onProjectConfigWarnings` callback forwards the unrecognized-key/lint-code warnings from
  each call.

  `mountStudio` (`@brink-lang/studio`) wires that callback into the Output tool window, so a typo'd or
  unrecognized `brink.toml` key is now visible instead of silently dropped. `[project] entry` is one such key:
  `brink_project_config::ProjectConfig` has no field for it at all (verified against
  `crates/internal/brink-project-config/src/lib.rs`), so it always reports as an unrecognized key — `mountStudio`'s
  explicit `entryFile` argument remains the only thing that decides the compiled entry file; there was nothing at
  the wasm-session layer for it to conflict with.

  **Review-finding fix:** `discoverProjectConfig` throws on malformed TOML or a recognized key with an
  invalid value, and that throw was unhandled — a typo'd `brink.toml` aborted `mountStudio` entirely (no
  editor to fix the file in), or, once mounted, threw out of every subsequent keystroke's debounced
  `notifyFileChanged`/`applyEdit` call. `ProjectSession.applyProjectConfig` now catches the throw at its
  single call site and reports it through a new optional `ProjectSessionOptions.onProjectConfigError`
  callback instead of rethrowing; `mountStudio` wires it into the same Output channel as the warnings.

- 184c655: `file.save`/`file.saveAll` no longer re-baseline a path against content a
  host write never persisted (issue #2426). Both commands snapshot the
  content they're about to write and, once the host save resolves, only mark
  a path clean if its current session content still matches the snapshot. A
  path edited while its host write was still in flight stays dirty and
  surfaces a "…changed while saving — still unsaved" warning instead of a
  false "Saved" notice; `file.saveAll`'s "Saved N files" count and
  `api.getDirtyFiles()`/`StudioPublicState.dirtyFiles` reflect only the
  verified subset.
- 255cf53: `file.save`/`file.saveAll` no longer raise a false "…changed while saving —
  still unsaved" warning when a `requestSave` queued behind another in-flight
  write legitimately catches up to a later edit and persists it (issue
  #2435). The #2426 mid-write guard's pre-save content comparison couldn't
  tell a queued write's legitimate catch-up apart from a genuine mid-write
  divergence; a path whose content moved on since the pre-save snapshot is
  now re-checked against the provider's actual written content
  (`ProjectSession.readProviderFile`) before being treated as stale — a
  genuine divergence still fails that check and stays dirty, unchanged from
  #2426's behaviour.
- 9045be7: The knot/stitch rename prompt (`SymbolRenamePrompt`, issue #2511) now seeds its name field
  synchronously at mount instead of from a `requestAnimationFrame` callback. Previously the field
  mounted empty and was filled a frame later, so between mount and that frame it was visible,
  enabled and editable but blank — and anything typed during that window was overwritten when the
  frame ran. The field is uncontrolled and the confirm path reads `input.value`, so a clobbered
  rename degraded to `name === currentName`: the prompt closed as if the user had accepted the
  existing name, silently performing no rename at all. Typing into the prompt the instant it opens
  now keeps what you typed.
- 23f4091: A knot/stitch rename that fails now tells you why (issue #2528). `performSymbolRename`
  returned the rename op's error — "symbol not found" when the knot was edited away between
  opening the context menu and confirming, "file not loaded", "cannot rename this symbol" —
  and `SymbolRenamePrompt` closed on it exactly as it closed on success. Nothing else read
  that error, so a failed rename was indistinguishable from a successful one: the prompt
  disappeared and nothing was renamed. The failure now raises an error-severity notification
  tagged `binder`, the same surface the file rename's failure path already uses and the same
  source tag the rename's own success toast carries.
- 9e27c91: A refused reorder/move/promote/demote now tells you why, instead of vanishing
  (issue #2544). `dispatchSymbolAction`'s seven structural-op branches
  (`reorderStitch`, `reorderKnot`, `reorderStitches`, `reorderKnots`,
  `moveStitch`, `promoteStitch`, `demoteKnot`) ended with
  `if (result.ok && result.path) { await applyMoveResult(...) }` and no `else`
  — a refused `StructuralResult` (`ok: false`) applied nothing, correctly, but
  also raised nothing, so the user had no way to tell a refusal apart from a
  no-op. Each refusal now raises an error-severity notification tagged
  `binder`, through the same `notifyStructuralRefusal` helper the rename
  surfaces already use (#2528/#2543) — one reporting contract, not a second
  style.

  `performSymbolRename`'s `!session` early return had the same gap in a worse
  shape: it returned `{ applied: false, diagnostics: [] }` with neither
  `applied` nor `error` set, so `SymbolRenamePrompt` fell into its breakage-
  report branch with an EMPTY report — rendering "would break 0 places" with a
  live **Force rename** button whose retry hit the same branch again. It now
  sets `error` too, so the prompt closes and the same error notification
  fires, instead of asserting a rename is unsafe when in truth no session was
  ever bound.

  Refused ops still push no undo entry (unchanged — nothing was written).

- 72bfb5b: Collapsed fold placeholders now render as chips rather than loose glyphs (issue #2546).
  `folding.ts` applies `brink-fold-pill`, its `-machinery`/`-narrative` kind classes,
  `-icon`, `-summary`, `-count` and `brink-fold-decl-icon`, and none of them had a rule in
  any stylesheet or CM6 theme in the workspace — so a collapsed machinery or narrative run
  rendered as a bare `⚙`/`❞` followed by summary text and a count, reading as stray
  characters spliced into the line. The studio's `editor.css` now skins all seven from the
  existing semantic tokens (both the latte and mocha themes), in the same inline-chip
  language as `.brink-host-chip`/`.brink-value-chip`, with the summary as the pill's only
  elastic part so the icon and count are never pushed out of view. The decl fold's kind
  glyph is tinted from the same `--bs-symbol-*` tokens the binder's outline icons use.
- fbdb3fb: Wire the Binder's folder rename to the atomic `rename_dir` op (issue #2587).

  The Binder's folder-rename action (`renameFolder`, `packages/studio-store/src/slices/binder.ts`,
  bundled into `@brink-lang/studio`) looped a per-file `renameFile` call over
  every file under the folder — the exact pattern `rename_dir` (#314) was built
  to replace, because a per-file loop computes each file's cross-file INCLUDE
  edits independently, against whatever has already moved, rather than against
  one pre-move snapshot. Concretely: a folder move that only changes the
  directory prefix (every moved file keeps its own basename) left an outside
  referrer's `INCLUDE` pointing at the old, now-nonexistent path, because a
  same-basename rename never triggers the per-file op's basename-keyed
  cross-file rewrite.

  `ProjectSession` (`@brink-lang/editor`) gains `renameDir`, the directory
  analog of `renameFile`: it calls the atomic wasm `rename_dir` op (unused by
  any TS caller since #314 landed), applies every moved file's content plus
  the outside referrers' rewrites from that one snapshot, and writes each
  moved file through the provider (a provider write is inherently per-file —
  the atomicity guarantee lives in the edit computation, not in these writes).
  Deferred off the paint path via the same `deferGatedCall` yield `renameFile`
  uses (#2776), since `rename_dir` runs the identical breakage gate.

  `renameFolder` now calls `project.renameDir` instead of looping
  `applyRename`. All-or-nothing failure semantics (a deliberate change from
  the old loop's silently-skip-a-collision-and-move-the-rest behavior): a
  partial directory move can only be computed by falling back to per-file
  INCLUDE rewriting for the files that "succeed," which is exactly the
  inconsistency #314 exists to prevent, so a collision now refuses the whole
  move with one error notification and nothing moves. Undo gets a new
  `rename-dir` entry kind that re-applies `renameDir` with the prefixes
  swapped, so undoing a folder move gets the same single-snapshot consistency
  guarantee the forward move does, instead of falling back to a per-file undo
  loop.

- 2b3a869: Add a mechanical guard (`packages/brink-studio/src/__tests__/dismiss-registry-enrolment.test.ts`)
  that a new dismissable surface enrols in the "Escape dismisses every
  registered transient surface" safety net (#279, PR #2760), so a future
  surface can no longer ship its own `document`-level `keydown`/`pointerdown`
  dismiss listener without a `registerDismissible()` call and silently fall
  back into the unescapable-menu failure mode #279 was filed for (issue
  #2766).

  The scan covers both independent, uncoordinated registries
  (`packages/studio-shell/src/dismiss-registry.ts` and
  `packages/ink-editor/src/dismiss-registry.ts`) from one test file, checking
  each package's listeners against its own registry. `packages/studio-shell`'s
  three Escape-cancels-a-gesture handlers (tab drag, strip-icon drag,
  maximize restore — `tab-drag.ts`, `strip-drag.ts`, `regions.tsx`) are marked
  `DISMISS-NET-EXEMPT` with a reason: they manage transient interaction/layout
  state, not a floating menu/popover/modal surface, so they are out of this
  net's scope by design.

- 0f1a4ff: Two structural gaps in the paint-path-defer family (issue #2794, found by
  #2788's adversarial re-review — "the enrolment family's gap, not this PR's").

  `ProjectSession` (`@brink-lang/editor`): a gated call deferred via
  `scheduleIdleWork` (today, `renameFile`) could outlive `destroy()` — an
  unmount landing inside the deferral's idle window let the scheduled callback
  fire anyway and call into a wasm handle `destroy()` had already freed. This
  was contained (the throw surfaced as an ordinary error notification through
  `applyRename`'s existing `catch`), not unreachable, but containment is not a
  fix. `deferGatedCall` (replacing a bare `scheduleIdleWork` await) now
  tracks its idle handle and rejects the caller's `await` — instead of
  resolving into a freed session — if `destroy()` runs first; `destroy()`
  cancels every still-pending handle and rejects its caller before freeing the
  wasm handle. One guard, meant to cover every gated call this class defers,
  present or future — including `runGatedStructuralOp`'s symbol-menu ops
  (`moveStitch`/`promoteStitch`/`demoteKnot`, in `@brink/studio-ui`), which a
  follow-up review found still deferred through their own independent
  `scheduleIdleWork` yield outside this guard; `deferGatedCall` is public for
  exactly this reuse.

  `structuralOpPending` (`@brink/studio-store`, bundled into
  `@brink-lang/studio`): two independent fire-and-forget writers
  (`runGatedStructuralOp` for symbol-menu ops, `applyRename` for Binder
  rename/move) both cleared this status-bar pending indicator unconditionally
  in a `finally`. An overlapping Binder drag-move and symbol-menu op could
  erase each other's still-live indicator, whichever settled last winning
  regardless of which op was actually still running. `SymbolMenuSlice` gains
  `clearStructuralOpPending(description)` — a compare-and-clear that only nulls
  the field when the live value still equals the description the clearing
  call itself set — and both writers now clear through it instead of an
  unconditional clear. `setStructuralOpPending` is narrowed to take only
  `string` (no caller ever passed `null`), so a future regression back to the
  unconditional shape fails typecheck instead of relying on review attention.

- 0534e33: Clicking a Binder entry for a file that is already open in another editor
  group, while a different group is maximized, now un-maximizes so the
  revealed group actually paints (issue #2797). Previously the reveal moved
  focus to a group the editor area was not rendering, so the click appeared
  to do nothing.
- 0e385d1: Reopening the story player after its tab was closed now restores the two-up
  split (issue #280) instead of dropping the player into the focused group.
- 9555c8c: Clicking a Binder entry for a file that is not yet open anywhere, while a
  different editor group is maximized, now un-maximizes so the newly opened
  tab actually paints (issue #2826). Previously this new-tab case moved focus
  to a group the editor area was not rendering, so the click appeared to do
  nothing — PR #2817 fixed the same symptom for the already-open-file reveal
  case only.
- 0d4e6ca: Dismiss-net enrolment guard follow-ups (issue #2846, following PR #2838 / #2766).
  `dismiss-registry-enrolment.test.ts`'s scan widened from `document`-only to
  `document`/`window`/`ownerDocument` targets and `keydown`/`keyup`/`pointerdown`
  events — `dismiss-registry.ts` itself attaches its net listener on `window`, so
  "attach the way the registry does" was the single most plausible unguarded
  next-surface shape and previously evaded the scan entirely. Widening surfaced
  that call site in both `studio-shell`'s and `ink-editor`'s `dismiss-registry.ts`,
  which now each carry a `DISMISS-NET-EXEMPT` marker (that call site _is_ the net,
  not a surface enrolling into it) backed by a new `dismiss-registry-net-listener.test.ts`
  in each package.

  Every `DISMISS-NET-EXEMPT` marker in the workspace — the pre-existing three
  (`tab-drag.ts`, `strip-drag.ts`, `regions.tsx`) plus `ElementDropdown.tsx` plus
  the two new net-listener ones — now carries a dedicated behavioural test
  proving its specific claim against the real production module, matching the
  `SAVE-PATH` precedent's "proven, not just present" bar
  (`docs/studio-shell-spec.md` §7.7.1).

  Also fixed: `scanListenerSites` used to skip `//`-prefixed lines only, so a
  block comment (e.g. JSDoc) quoting the listener shape in prose counted as a
  real, unmarkable call site; block-comment spans are now blanked (line numbers
  preserved) before the scan runs.

  No runtime behavior changes — this is test-infrastructure and doc-comment only.

- 658e7a6: The knot/stitch rename prompt (`SymbolRenamePrompt`, the "Rename…" context-menu surface) no
  longer runs its collision analysis synchronously on the paint path (issue #696). Previously a
  confirmed rename (Enter) or a forced override ran `performSymbolRename`'s wasm call inline in the
  same frame as the triggering event, with no yield point of its own — under CPU contention this
  could block the whole page (including a real user's next interaction) for however long the
  analysis took, with zero visual feedback until it finished.

  #722 fixed this exact defect for the sibling inline (F2) rename widget by committing a pending
  state synchronously (so it paints before the heavy call runs) and deferring the call itself to
  the next idle slot; it never reached this modal prompt. The prompt now takes the same discipline
  — a `.brink-rename-pending` indicator ("Checking for conflicts…") appears immediately on Enter or
  Force, and the actual analysis runs afterward via the same `scheduleIdleWork` helper. This is also
  what stabilizes the long-flaky `e2e/symbol-rename.spec.ts` "a colliding rename shows the breakage
  report; Force overrides" test (PR #714's timeout-only fix reduced but never eliminated the
  recurrence): the pending indicator gives that test — and `symbol-rename-prompt-pending.test.tsx`'s
  deterministic, fake-timer-driven ordering check — a real signal to wait on instead of a race
  against an unbounded synchronous call.

- af35e74: `mountStudio`'s returned `StudioHandle` now exposes `entryFile`: the
  project's EFFECTIVE entry file (`ProjectSession.getEntryFile()`'s result,
  with `[project] entry` precedence already applied per issue #2331), read
  once `initialize()` has run. A host that needs to act on "the file the
  editor actually treats as the entry" (batch tooling, an export command)
  should read this instead of echoing back its own `MountStudioOptions.entryFile`
  argument — that argument is only the fallback for a configless project, and
  a host using it directly could silently disagree with the editor for any
  project whose `brink.toml` names a different entry (2026-08 review finding
  on brink-desktop's `exportXliff`, #2392).
- 70e9e48: Inline (F2) rename no longer reports a REFUSED rename as a success (#2543).

  The editor's inline rename commits through `applyComputedRename` →
  `applyMoveResult`, and neither checked `result.ok`. A rename the op refuses —
  "cannot rename this symbol" when the cursor is not on a declaration, "file not
  loaded" when the file went away mid-rename — therefore reached the apply seam,
  which pushed an undo entry, raised the confirming **info** toast ("Rename X to
  Y") with an Undo button, and re-keyed the symbol's open tab, all for an edit
  that never happened.

  `isSafeRename` cannot catch this: a refusal carries `safe: true` with no
  introduced diagnostics, so the editor's inline gate reads it as safe and
  commits. `safe` describes the breakage of edits that were actually computed;
  `ok` is the field that says whether the operation happened. The guard is now on
  `ok`:

  - `applyComputedRename` refuses an `ok: false` result and raises an
    error-severity `binder` notification carrying the op's own reason
    ("Rename hello failed: cannot rename this symbol") — the same channel the
    modal prompt's refusal path uses (#2528).
  - `applyMoveResult` refuses an `ok: false` result at the seam, so no caller can
    turn a refusal into a toast plus an undo entry.

  Successful renames are unchanged: edits apply, one informational toast with
  Undo, symbol tab re-keyed.

  Per-op refusal reporting for the remaining structural ops is tracked separately
  in #2544 and is not changed here.

- Updated dependencies [3b94ac6]
- Updated dependencies [462f61b]
- Updated dependencies [87fe945]
- Updated dependencies [f7e54e3]
- Updated dependencies [e3ae45a]
- Updated dependencies [f36faf9]
- Updated dependencies [f71aa3d]
- Updated dependencies [ae7b829]
- Updated dependencies [5a95959]
- Updated dependencies [39f3801]
- Updated dependencies [bb503cc]
- Updated dependencies [aeebad7]
- Updated dependencies [4fd4658]
- Updated dependencies [640d1d1]
- Updated dependencies [3ddd90e]
- Updated dependencies [f87adc2]
- Updated dependencies [199c822]
- Updated dependencies [8add320]
- Updated dependencies [1d5c985]
- Updated dependencies [1ef7797]
- Updated dependencies [319f9dc]
- Updated dependencies [257e7a9]
- Updated dependencies [c852cbe]
- Updated dependencies [cf076d5]
- Updated dependencies [2f0b5cf]
- Updated dependencies [4bae57f]
- Updated dependencies [9b1d832]
- Updated dependencies [ec58199]
- Updated dependencies [9586408]
- Updated dependencies [b5fcf8e]
- Updated dependencies [c074d71]
- Updated dependencies [7239301]
- Updated dependencies [74b8586]
- Updated dependencies [ff8794e]
- Updated dependencies [5a7c18e]
- Updated dependencies [2ccae0b]
- Updated dependencies [269fc6f]
- Updated dependencies [cb56346]
- Updated dependencies [2df4377]
- Updated dependencies [3b18503]
- Updated dependencies [0dcdd10]
- Updated dependencies [51d243b]
- Updated dependencies [137c169]
- Updated dependencies [e839fa9]
- Updated dependencies [529bc3f]
- Updated dependencies [72b978c]
- Updated dependencies [741ac65]
- Updated dependencies [5680e1e]
- Updated dependencies [b6fdef9]
- Updated dependencies [916837b]
- Updated dependencies [d27382f]
- Updated dependencies [cd70ad8]
- Updated dependencies [8531452]
- Updated dependencies [cbc6683]
- Updated dependencies [d7994d5]
- Updated dependencies [867e75c]
- Updated dependencies [faf45f2]
- Updated dependencies [b8e3246]
- Updated dependencies [80ede86]
- Updated dependencies [db2a6fa]
- Updated dependencies [f285bec]
- Updated dependencies [7545fdf]
- Updated dependencies [6262d13]
- Updated dependencies [ef2973c]
- Updated dependencies [fd10f7a]
- Updated dependencies [52fb2d3]
- Updated dependencies [b895c4f]
- Updated dependencies [4de4d3f]
- Updated dependencies [ad09a98]
- Updated dependencies [98a1ae6]
- Updated dependencies [9dec659]
- Updated dependencies [d22cef5]
- Updated dependencies [11cdf95]
- Updated dependencies [38db35c]
- Updated dependencies [186546c]
- Updated dependencies [2ae8fc9]
- Updated dependencies [63bc2a3]
- Updated dependencies [276bf6c]
- Updated dependencies [cc52b83]
- Updated dependencies [96173a5]
- Updated dependencies [39124bb]
- Updated dependencies [acc6b0b]
- Updated dependencies [73b12c8]
- Updated dependencies [e5d78d1]
- Updated dependencies [7182df5]
- Updated dependencies [a5d1b37]
- Updated dependencies [67bf45d]
- Updated dependencies [f58b1f6]
- Updated dependencies [ad9d451]
- Updated dependencies [aef14d6]
- Updated dependencies [5ee89a8]
- Updated dependencies [b615f7d]
- Updated dependencies [cc34968]
- Updated dependencies [34f740a]
- Updated dependencies [c41b0c7]
- Updated dependencies [874c40b]
- Updated dependencies [0c9db81]
- Updated dependencies [65f96b0]
- Updated dependencies [e4fb577]
- Updated dependencies [7e8d3a2]
- Updated dependencies [b308544]
- Updated dependencies [fbd074e]
- Updated dependencies [e4fc530]
- Updated dependencies [666edaf]
- Updated dependencies [0de4a8f]
- Updated dependencies [a9cdbf8]
- Updated dependencies [1e91561]
- Updated dependencies [bdeecb2]
- Updated dependencies [cb874b5]
- Updated dependencies [f766b2a]
- Updated dependencies [af56482]
- Updated dependencies [4917db1]
- Updated dependencies [78cfd24]
- Updated dependencies [b1122e3]
- Updated dependencies [6cd41cc]
- Updated dependencies [18dffa4]
- Updated dependencies [025c865]
- Updated dependencies [689f1f7]
- Updated dependencies [d7fb30e]
- Updated dependencies [55976d2]
- Updated dependencies [029512d]
- Updated dependencies [405be81]
- Updated dependencies [9e89eb2]
- Updated dependencies [12b5302]
- Updated dependencies [0b94925]
- Updated dependencies [96998ef]
- Updated dependencies [25e3742]
- Updated dependencies [533daf9]
- Updated dependencies [62e63ba]
- Updated dependencies [3436d7f]
- Updated dependencies [96eb265]
- Updated dependencies [70a1385]
- Updated dependencies [7915095]
- Updated dependencies [f73db83]
- Updated dependencies [c2d0c9f]
- Updated dependencies [f59a88c]
- Updated dependencies [16a548e]
- Updated dependencies [bee5bdb]
- Updated dependencies [220957f]
- Updated dependencies [3316a25]
- Updated dependencies [80735d8]
- Updated dependencies [6453c13]
- Updated dependencies [470cef5]
- Updated dependencies [0d28d28]
- Updated dependencies [ea92b07]
- Updated dependencies [ae3eece]
- Updated dependencies [a6863e3]
- Updated dependencies [1104a9f]
- Updated dependencies [9243ec0]
- Updated dependencies [f07284d]
- Updated dependencies [a6d86e5]
- Updated dependencies [3dd7936]
- Updated dependencies [f81379d]
- Updated dependencies [19e6cbb]
- Updated dependencies [fa52c61]
- Updated dependencies [21a40e8]
- Updated dependencies [8f0f38b]
- Updated dependencies [22bac8a]
- Updated dependencies [329560b]
- Updated dependencies [b42e3e5]
- Updated dependencies [c1ed5cd]
- Updated dependencies [540d094]
- Updated dependencies [90e0989]
- Updated dependencies [217ba82]
- Updated dependencies [4c6c8a5]
- Updated dependencies [20ab18e]
- Updated dependencies [1adefcc]
- Updated dependencies [814276c]
- Updated dependencies [e976041]
- Updated dependencies [c1be12d]
- Updated dependencies [260a94a]
- Updated dependencies [2a4b311]
- Updated dependencies [422d968]
- Updated dependencies [881726e]
- Updated dependencies [9c211d5]
- Updated dependencies [a4f14ba]
- Updated dependencies [92eb241]
- Updated dependencies [a7556a5]
- Updated dependencies [ef4d386]
- Updated dependencies [e44f1fa]
- Updated dependencies [b2b1ad5]
- Updated dependencies [f5395de]
- Updated dependencies [c3ac050]
- Updated dependencies [0d17b32]
- Updated dependencies [60b83cd]
- Updated dependencies [736e8d4]
- Updated dependencies [4dcafc9]
- Updated dependencies [06cacc4]
- Updated dependencies [50c1107]
- Updated dependencies [52e6809]
- Updated dependencies [22540ca]
- Updated dependencies [d64cefc]
- Updated dependencies [a5e5896]
- Updated dependencies [115bb40]
- Updated dependencies [f958d24]
- Updated dependencies [8632205]
- Updated dependencies [231bb5f]
- Updated dependencies [9fac670]
- Updated dependencies [f628345]
- Updated dependencies [4a1dee1]
- Updated dependencies [4bfcdab]
- Updated dependencies [78b4c2d]
- Updated dependencies [309c00c]
- Updated dependencies [19e18be]
- Updated dependencies [aa26464]
- Updated dependencies [31155ad]
- Updated dependencies [a64d78e]
- Updated dependencies [9943755]
- Updated dependencies [c91926b]
- Updated dependencies [f6838e2]
- Updated dependencies [d120ecb]
- Updated dependencies [5fabf50]
- Updated dependencies [8e6427a]
- Updated dependencies [9c8d51a]
- Updated dependencies [e5b980d]
- Updated dependencies [cf57b22]
- Updated dependencies [546ded5]
- Updated dependencies [3bbd8d9]
- Updated dependencies [56ce7bf]
- Updated dependencies [4a664ec]
- Updated dependencies [c025a9f]
- Updated dependencies [85cb6e5]
- Updated dependencies [9397a1a]
- Updated dependencies [3be1e5f]
- Updated dependencies [d43ec7e]
- Updated dependencies [967bd1b]
- Updated dependencies [b353095]
- Updated dependencies [a7e313d]
- Updated dependencies [d72cad2]
- Updated dependencies [62dba1d]
- Updated dependencies [370715c]
- Updated dependencies [8d92c9c]
- Updated dependencies [1156ff3]
- Updated dependencies [c3c6eab]
- Updated dependencies [633fb8f]
- Updated dependencies [885ca6f]
- Updated dependencies [8f30776]
- Updated dependencies [76cc702]
- Updated dependencies [d8ddd78]
- Updated dependencies [246b800]
- Updated dependencies [9fc8665]
- Updated dependencies [8e6a225]
- Updated dependencies [d4eab47]
- Updated dependencies [79fdaf4]
- Updated dependencies [d18f149]
- Updated dependencies [d44e75f]
- Updated dependencies [07740e1]
- Updated dependencies [1939b97]
- Updated dependencies [77cd00a]
- Updated dependencies [8628395]
- Updated dependencies [7c8480a]
- Updated dependencies [88c6754]
- Updated dependencies [8db452d]
- Updated dependencies [2c7a43d]
- Updated dependencies [59528ec]
- Updated dependencies [db3f8e4]
- Updated dependencies [bd95b30]
- Updated dependencies [dadf0ce]
- Updated dependencies [98d2ad2]
- Updated dependencies [36d6630]
- Updated dependencies [3893794]
- Updated dependencies [dc35b98]
- Updated dependencies [ff1e121]
- Updated dependencies [e2e5ec4]
- Updated dependencies [6fae1a6]
- Updated dependencies [8c52feb]
- Updated dependencies [aadc9b5]
- Updated dependencies [55cc2b1]
- Updated dependencies [46eb61b]
  - @brink-lang/web@0.15.0

## 0.14.0

### Patch Changes

- Updated dependencies [9481137]
- Updated dependencies [a6e8a6a]
  - @brink-lang/web@0.14.0

## 0.13.0

### Patch Changes

- Updated dependencies [17ad933]
- Updated dependencies [f53c6c7]
- Updated dependencies [7e8aa7f]
- Updated dependencies [b9a86e2]
  - @brink-lang/web@0.13.0

## 0.12.0

### Patch Changes

- Updated dependencies [6cb663a]
  - @brink-lang/web@0.12.0

## 0.11.1

### Patch Changes

- Updated dependencies [c246a4a]
- Updated dependencies [ae66340]
- Updated dependencies [7baa01f]
- Updated dependencies [aa43bb6]
- Updated dependencies [edf92bc]
- Updated dependencies [d350551]
- Updated dependencies [3c1e1e1]
- Updated dependencies [c03a73a]
- Updated dependencies [83717d3]
- Updated dependencies [302c6a2]
- Updated dependencies [4a08940]
- Updated dependencies [b86fee8]
- Updated dependencies [1e1be68]
- Updated dependencies [c36b8c4]
- Updated dependencies [71dd2fc]
- Updated dependencies [213a7f5]
- Updated dependencies [730c947]
- Updated dependencies [a0d9ee2]
- Updated dependencies [7ac0a5d]
- Updated dependencies [1198586]
- Updated dependencies [058f410]
- Updated dependencies [7500e27]
- Updated dependencies [bcb5cd3]
- Updated dependencies [c62687c]
- Updated dependencies [8870113]
- Updated dependencies [e16e8f8]
- Updated dependencies [820f6c5]
- Updated dependencies [45eb96b]
- Updated dependencies [e8cb050]
- Updated dependencies
- Updated dependencies [fe0c16d]
- Updated dependencies [6266cbf]
- Updated dependencies [9e9f07a]
- Updated dependencies [878be79]
- Updated dependencies [c66409b]
- Updated dependencies [86c4bee]
- Updated dependencies [fdf94f6]
- Updated dependencies [9d559a3]
- Updated dependencies [cc1d11e]
- Updated dependencies [62cb759]
- Updated dependencies [a350dcf]
- Updated dependencies [3ad1bc5]
- Updated dependencies [2b7dd5a]
  - @brink-lang/web@0.11.1

## 0.8.3

### Patch Changes

- Updated dependencies [8a3635d]
- Updated dependencies [34951ec]
- Updated dependencies [81ddfa7]
- Updated dependencies [9c58d6e]
- Updated dependencies [f68c094]
- Updated dependencies [b9ad39f]
- Updated dependencies [b7b7eb0]
- Updated dependencies [d29671d]
- Updated dependencies [ca45425]
- Updated dependencies [abc369a]
- Updated dependencies [30e09f9]
- Updated dependencies [2541c08]
- Updated dependencies [5b07740]
- Updated dependencies [d02c4e2]
- Updated dependencies [20d2bfa]
- Updated dependencies [d38fa08]
- Updated dependencies [9bef954]
- Updated dependencies [1e71455]
- Updated dependencies [c9475df]
  - @brink-lang/web@0.11.0

## 0.8.2

### Patch Changes

- Updated dependencies [73e2746]
- Updated dependencies [36bf266]
- Updated dependencies [973858f]
- Updated dependencies [54c37df]
- Updated dependencies [1bca37c]
- Updated dependencies [6289b0e]
  - @brink-lang/web@0.10.0

## 0.8.1

### Patch Changes

- Updated dependencies [5075db7]
- Updated dependencies [cbc27aa]
  - @brink-lang/web@0.9.0

## 0.8.0

### Minor Changes

- 6785663: Dialogue dialect editor integration (#368): the screenplay behavior (`@Name:<>` character cues, `(beat)<>` parentheticals, the dialogue chain) is now driven by a `DialogueDialect` — a versioned, pure-JSON schema — instead of hardcoded regexes.

  - **`brinkStudio({ dialect })`** (default `AT_CUE_DIALECT`, byte-identical to the old hardcoded behavior). `dialect: null` tears down the screenplay layer — classification, decorations, dialect transition rows, dialect keybinding behaviors — for true headless composition (pair with `theme: false`, #363); the structural weave keymap (Choice/Gather/Narrative Tab/Enter transitions) stays active, per the spec's structural-rows-stay-interpreter-owned rule. A custom `DialogueDialect` object drives classification/decorations/transitions/conversions with zero editor code changes.
  - **`@brink-lang/web`**: `EditorSessionHandle` gains `setDialect(dialect)` / `clearDialect()` (wrapping the wasm `set_dialect`/`clear_dialect` seam from #386), and the `DialogueDialect` schema types + the `LineContext.dialect` facet are published from the type surface.
  - **`setDialect(view, dialect)`** live-reconfigures an already-mounted editor: swaps the screenplay compartment, forces reclassification, and re-runs the wasm `set_dialect`/`clear_dialect` when a document handle is present.
  - **`extendDialect(base, overrides)`** adds a kind (or overrides chain/transitions/templates) without forking a preset.
  - Classification is authoritative in Rust (`brink_ir::dialect` + `line_contexts_with_dialect`, landed in #386) when a wasm document handle is present. Without one, the editor falls back to a thin TS interpreter over the identical JSON (`ResolvedDialect`), pinned against the same conformance corpus (`tests/dialect_fixtures/at_cue.json`) as the Rust side so both paths agree on every case.
  - Screenplay geometry (`screenplay.ts`'s hidden decorations, atomic ranges, edit guard, cursor clamps) is now derived from the resolved dialect's hidden-group match indices — computed once at classification time and cached, never re-matched in per-keystroke hot paths. The `CHAR_SUFFIX_LEN`/`GLUE_LEN` constants and the public `characterName()` export are gone; the geometry is dialect-derived and internal.
  - The Tab/Enter/Shift-Tab transition table and name-surgery keybindings now consult a dialect's declared overlay rows before the built-in structural weave table (inert for the default preset, which ships no overlay rows).

  ### BREAKING CHANGE: `ElementType` enum → open string union (0.x hard cut, ruled 2026-07-05)

  `ElementType` used to be a numeric TS `enum`. It is now a `const` object of kebab-case kind strings mirroring the existing `brink-<kind>` CSS class scheme — `ElementType.Character`-style call sites migrate mechanically (the values still compare correctly), but the type is now `string`, and two PascalCase leaks are now kebab-case:

  - `@brink-lang/studio`'s published `StudioApi`: `StudioPublicState.element.type` was `"KnotHeader"`, `"NarrativeText"`, `"Choice"`, … — now `"knot-header"`, `"narrative"`, `"choice"`, ….
  - `@brink/studio-store`'s duplicate `ElementType` enum is deleted; it now re-exports the real one from `@brink-lang/editor` (still available as `ElementTypeEnum`).

  Full PascalCase→kebab mapping table in `docs/editor-consumer-guide.md`. No compat shim — both packages are pre-1.0.

### Patch Changes

- fabd5a5: Chromium 88 (RMMZ/NW.js) compatibility: remove every `color-mix()` from the editor and studio themes — Chromium 88 has no `color-mix()` (Chrome 111+), so those declarations were dropped wholesale, most visibly leaving text selection with no fill.

  - Behind-text highlight layers (`.cm-selectionBackground`) now use a solid `var(--bs-accent)` fill plus layer `opacity`, which composites identically and works on any host that defines the base tokens.
  - The active line uses a new optional theme token `--bs-active-line-bg`, falling back to the opaque `var(--bs-surface-bg)` for hosts that define only base tokens.
  - All other alpha-tinted highlights (search/selection matches, bracket matching, binder/search/graph chrome) are written as `rgb(var(--bs-X-rgb) / N%)` over new per-theme sRGB triplet tokens (`--bs-accent-rgb`, `--bs-error-rgb`, …) defined by the built-in Mocha/Latte themes.
  - Opaque two-color mixes (story-graph node borders/fills, conflict banner) are precomputed per theme as `--bs-graph-*` / `--bs-conflict-banner-bg` tokens.

  Visual output on modern Chromium is unchanged; hosts embedding `@brink-lang/editor` with a custom token set get correct selection/active-line out of the box and can define the new tokens for the tinted variants.

- ed2446b: Headless-ready editor (#363): the `brinkTheme` skin is now opt-out — pass `theme: false` to `brinkStudio(...)` (or `DocumentSessions`'s new options bag) for a fully headless editor, or pass your own CM `Extension` to substitute it; the default is unchanged and brink-studio now opts into `brinkTheme` explicitly. All presentational inline styles on editor-owned popups and widgets (code-actions menu, inline element picker, widget popover, argument-form anchor, inlay hints, color swatch + picker) moved onto classes; dynamic values (popup coordinates, swatch colors) ride on CSS custom properties consumed by a new always-on, zero-specificity (`:where(...)`) structural stylesheet (`ensureStructuralStyles`, exported), so hosts can restyle the full class taxonomy directly. The taxonomy — element line classes (an open `brink-<kind>` scheme), structural decoration classes, floating-surface classes, and custom properties — is now documented as a semi-stable contract in docs/editor-consumer-guide.md.
- Updated dependencies [3cf1062]
- Updated dependencies [58d93ee]
- Updated dependencies [6785663]
- Updated dependencies [f72f181]
- Updated dependencies [9d1dd69]
- Updated dependencies [1f91422]
- Updated dependencies [a11b115]
  - @brink-lang/web@0.8.0

## 0.7.0

### Minor Changes

- 8be15da: Wire the #311 editor features into the studio: inline rename, external-conflict merge view, editable search buffer, code-actions menu (with extract-to-knot/function), auto-import, and the INCLUDE-block fold.

### Patch Changes

- Updated dependencies [8be15da]
  - @brink-lang/web@0.7.0

## 0.6.0

### Minor Changes

- b0746e7: Knot/stitch **Rename** — a full, cross-file, safe-by-default rename on the shared symbol context menu (editor / Binder / Story Graph) and the editor's **F2**. A clean rename applies immediately; one that would introduce diagnostics flips to an in-place breakage report whose only override is an explicit **Force rename** (mirroring the `brink ide rename` CLI's `--unsafe` gate). An open symbol-view tab survives its own rename (re-keyed in place).

  F2 is now a full cross-file rename — the previous single-file F2 was a bug. `@brink-lang/web` gains `rename_symbol` / `rename_symbol_at` and drops the superseded `rename_doc` / `rename` exports (and the corresponding `doRename` handle methods).

### Patch Changes

- Updated dependencies [b0746e7]
  - @brink-lang/web@0.6.0

## 0.5.1

### Patch Changes

- 080a715: Fix: screenplay indents (character / parenthetical / dialogue) no longer collapse to flush-left on browser engines without CSS container-query support (older Chromium-based embeds such as NW.js / CEF). The layout now degrades to viewport-relative scaling there, and keeps pane-relative scaling on engines that support container queries. (#188)
- Updated dependencies [080a715]
  - @brink-lang/web@0.5.1

## 0.5.0

### Minor Changes

- a6bceef: Binder file lifecycle — manage whole files and folders directly in the binder.

  - **Delete** files and folders from the context menu, with undo.
  - **Rename** files and folders inline (F2 or the context menu). Every `INCLUDE` that points at a renamed or moved file is rewritten automatically, and `..`-relative include paths now resolve correctly across the toolchain.
  - **Move** files by dragging onto a folder, drag a file back out to the project root, and multi-select to move several files at once — all undoable, with one "Moved N files" step.
  - Renaming a file keeps its open editor tab in place (pin, split, and selection are preserved) instead of reopening it.

  `@brink-lang/web` gains the `rename_file` session op, which computes the edit set for a file move: the re-keyed file content plus the referrer `INCLUDE` rewrites.

### Patch Changes

- Updated dependencies [a6bceef]
  - @brink-lang/web@0.5.0

## 0.4.4

### Patch Changes

- 5431d8e: Clickable value-list picker. A value-list argument (a semantic type with a
  declared `values` list) now renders an interactive chip instead of a passive
  label: click it to open a filterable dropdown of the items and rewrite the
  literal in place. Hosts get a click-to-pick combobox for free from a declared
  value-list — no custom `ArgumentWidget` required (#224).

## 0.4.3

### Patch Changes

- 9ce2764: Fix host-widget Edit on non-string arguments. A host `ArgumentWidget` on a
  non-string semantic type (e.g. an `int`) opened and called `host.resolve(...)`
  but never wrote back when replacing an existing literal — the in-place edit
  resolved the literal range with a quote-only finder, so a bare literal like `1`
  was a silent no-op. Bare int/float/bool literals are now handled, so host
  widgets can edit already-filled arguments of any type (#242).

## 0.4.2

### Patch Changes

- 05325c0: Argument-widget + editor polish.

  - **Bundle the editor font** — the studio now self-hosts JetBrains Mono
    (Latin, regular/bold/italic), so embedders without it installed (e.g. RPG
    Maker MZ / NW.js) no longer fall back to the system monospace.
  - **Typed widgets in the Host Functions panel** — composing a fresh call from
    the panel now uses the same value-list dropdowns, host widgets, and
    arg-group controls as the in-editor call Form, not plain text fields.
  - **Host-sourced value-lists in the Form** — a slot whose semantic type
    declares `values: host` now surfaces its dropdown items from the pushed host
    cache, not just static manifest items.

- Updated dependencies [05325c0]
  - @brink-lang/web@0.4.2

## 0.4.1

### Patch Changes

- facc579: Argument-widget fixes.

  - **Embedded host content theming/positioning** — widget popovers (the color
    picker, host pickers, the call Form) now mount inside the `.brink-studio` root
    and use `position: fixed`, so embedded host content inherits the theme tokens
    and positions correctly when the studio is embedded in a host page (rather than
    rendering unstyled or mis-placed against `document.body`).
  - **Auto-open on completion-accept** — the completion kind map was keyed by the
    wrong casing, so every completion was typed `"text"`. This both mis-iconed
    completions and disabled "open the Form when accepting a function completion".
  - **The call Form is driven by the signature metadata**, not the live call-site,
    so a partial or over-full call still renders its declared widgets (e.g. an
    arg-group picker) instead of degrading to plain text fields; Apply writes a
    well-formed call.

- Updated dependencies [facc579]
  - @brink-lang/web@0.4.1

## 0.4.0

### Minor Changes

- 755868c: Argument widgets — rich, type-driven call-site authoring.

  - A whole-call **Form** that renders one control per argument, chosen by the
    argument's type: a text input, the built-in color picker, a host-declared
    **value-list dropdown**, or a host **custom widget** — including **arg-groups**
    (one widget over several parameters, e.g. a 2D point picker) whose editor
    embeds inline. The Form holds live draft state, so an arg-group's inter-arg
    context resolves from the current form (pick a map, then a spot on that map)
    before anything is written.
  - **Inline editing** of typed arguments in the editor: color swatches,
    value-list name labels, host-rendered chips, and arg-group chips — Edit a
    filled literal, Fill an empty slot, or open the Form (an opt-in inline glyph,
    the always-on hover-card action, the `Mod-Shift-A` keybind, or the Host
    Functions panel).
  - A host **argument-widget API** (`StudioExtensions.argumentWidgets`): built-in
    and host-provided widgets, popover/modal editor surfaces, and arg-group
    widgets that receive resolved inter-arg context.
  - The `argument_widgets` IDE query now reports per-slot value-list items and
    per-group inter-arg context indices across the wasm boundary, so the studio
    can render dropdowns and resolve context from live form state.

### Patch Changes

- Updated dependencies [755868c]
  - @brink-lang/web@0.4.0

## 0.3.0

### Minor Changes

- bcd23b7: Live inspector and host-aware authoring.

  - The story session is driven by a `SessionProvider`, so the transcript, State
    View, and Story Graph render against a provider rather than the wasm runner
    directly — the groundwork for inspecting a VM running in a host.
  - Capability-gated session commands, program-identity degraded mode, and
    multi-session support (independent runners + shared-context flows) with a
    session/flow picker.
  - A host-aware argument picker: a value dropdown and inline value labels for
    `EXTERNAL` arguments whose semantic type declares a value source (static, or
    pushed live by a host), plus a `StudioExtensions.argumentProviders` surface
    for embedders to supply those values.

### Patch Changes

- Updated dependencies [bcd23b7]
  - @brink-lang/web@0.3.0

## 0.2.1

### Patch Changes

- Updated dependencies [20764ef]
  - @brink-lang/web@0.2.0

## 0.2.0

### Minor Changes

- 6276f29: File-content egress for embedding hosts (#154, closing #137): a debounced,
  batched `onFilesChanged(changes: FileChange[])` mount option fed by every
  mutation path (editor edits, binder structural ops, search replace,
  `file.new`), an `api.getFiles()` / `api.getDirtyFiles()` pull surface,
  `file.save` (Mod-S) / `file.saveAll` commands that flush and deliver
  immediately, and a `dirtyFiles` count on `StudioPublicState` (additive —
  `version` stays 1). Also: a `wasmLocation` mount option forwarded to
  `initWasm` for IIFE-plugin hosts, and a Chromium-88 `adoptedStyleSheets`
  feature-detect shim in the mount bootstrap (NW.js / RPG Maker MZ).
