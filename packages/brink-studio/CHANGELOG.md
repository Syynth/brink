# @brink-lang/studio

## 0.18.0

### Minor Changes

- 07359b2: Settings → Conventions is now the teach-by-example editor (#3411, ruled
  2026-09-02): pull a passage through the Player launcher's knot/stitch
  typeahead (or paste lines), mark each line — Cue, Dialogue, Action,
  Narration, Aside — and the studio shows the rules it learned in plain
  words with the lines that support each, what it could not settle, and
  how the passage reads in the Player under those rules. "Use these rules"
  writes the `[dialogue]` section of `brink.toml` (the `at-cue` recipe plus
  your rows when that fits, otherwise a `dialect.json` the section points
  at) and asks before replacing a section it did not write.

  `@brink-lang/editor` re-exports the inference and `[dialogue]`-table
  helpers from `@brink-lang/dialect` (`inferDialect`, `dialectFromConfig`,
  `toDialogueConfig`, …).

- ec4d573: The editor follows the Player (#3437, ruled 2026-09-02): a **Follow**
  toggle in the Player toolbar (on by default, persisted with the Player
  settings, also in Settings → Player). While the story plays, each
  revealed line scrolls the editor to its source — opening the file as a
  preview if needed, never taking focus — and bands it (accent, full
  width). Editing the document pauses follow until Run/Restart or the
  toggle. Hovering a transcript row bands its source line in the editor
  with a neutral hover band. `@brink-lang/editor` gains
  `DocumentSessions.scrollTo` (scroll without focus or selection) and the
  `follow` / `hover` execution-highlight kinds.
- 369ee87: The Player's reading surface, restyled (#3436, ruled 2026-09-02): a
  spine — one faint rail down the transcript — that a speaker's block
  paints in the speaker's colour and an action row paints dotted; the
  speaker's name as a small label over its lines; narration at full
  strength and action dimmed; even row padding with a full-width hover
  tint and no stripes; the echo of a taken choice as a ring on the rail
  carrying its `*`/`+`; the choices on offer as cards hanging off the rail
  with their marker; and provenance as a small go-to-source button that
  hangs off the hovered row's edge with `file:line` as its tooltip.
- bcbe3b5: Settings › Player gains **Reading** and **Reading aids** (#3438): a font
  picker with a live specimen list (a curated set of reading faces on the
  web, plus a family you type; the desktop app can supply the machine's
  fonts through the new `systemFonts` mount option), line spacing and
  measure steppers, and toggles for the go-to-source button and the
  choice markers. All app scope, persisted with the Player settings, and
  applied through CSS variables (`--bs-player-font-family`,
  `--bs-player-line-height`, `--bs-player-measure`) the same way the
  Player font size already is.

### Patch Changes

- ba08f3c: Settings' Diagnostics section gains a Fix column beside severity
  (`docs/autofix-spec.md` §6.1): `off | ask | auto`, per diagnostic code,
  written into `brink.toml`'s `[fix]` table through the same write path the
  severity picker already uses for `[lints]`. `[fix]` and `[lints]` are
  independent tables keyed by the same code, so a code's Fix policy shows and
  edits regardless of whether it is also `[lints]`-configured.
- b9820be: The playground gains `?fixture=fixable`: a deterministic five-file project
  whose diagnostic set is closed and deliberate — one cross-module import
  (Suggested), five Safe-fixable warnings, one warning with no fixer at all,
  and one Safe-fixable code turned `allow` in the project's own `[lints]`
  table. It is the fixture the auto-fix end-to-end suite drives, and it makes
  every auto-fix surface — the Problems row Fix button, "Fix all safe (N)",
  both context menus, the palette commands, fix-on-save, and the Settings
  Diagnostics Fix column — something an author can look at rather than only
  something a test asserts.
- d0acebb: Auto-fix reaches the studio (`docs/autofix-spec.md` §7). The Problems panel
  gains a per-row **Fix** button labelled with the fix's tier, a header **Fix
  all safe (N)** whose `N` is the batch's own count, and fix entries in each
  row's context menu beside the existing suppress items. The editor context
  menu offers the fixes for the diagnostic under the pointer plus "Fix all safe
  in this file", and the command palette gains "Fix: Fix all safe in project"
  and "Fix: Fix all safe in this file".

  Settings ▸ Editor gains **Fix on save** (`off | safe | project`, default
  off) — an app-scope ceiling over the project's own `[fix]` policy, so it can
  only ever be more conservative than `brink.toml`, never more aggressive.

- 0e64731: Binder: fix folder drag-reorder silently failing in WebKit (Tauri desktop
  app / Safari). The row's drag handlers wired `onDragOver`/`onDrop` but had
  no `onDragEnter` at all — WebKit's HTML5 drag-and-drop requires
  `preventDefault()` on both `dragenter` and `dragover` for an element to
  remain a valid drop target, while Chromium tolerates `dragover` alone, so
  the reorder worked in the browser preview but never in the desktop app.
  `onDragEnter` now runs the same accept/reject logic as `onDragOver` on
  every Binder row (files, folders, knots, stitches) and the root drop zone;
  rows also opt into `-webkit-user-drag: element` as a defensive measure
  against WebKit's stricter interactive-children drag-start gating, scoped to
  `.brink-binder-row[draggable="true"]` only — React renders `draggable={false}`
  as the literal attribute, and an unscoped rule would have let WebKit's
  presentational-hint cascade re-arm non-draggable rows (read-only
  `FileProvider`, pre-seed window) as drag sources.
- b82cb34: Break-on-write data breakpoints (W18, spec §F6 RULED): right-click a global in the Debugger panel's Variables section → "Break on write" — a write to the watched global pauses the run AND Continue tiers at the writing instruction, with the watchpoint named in the stop reason (the Player chip reads "Paused on write — gold"). Armed watchpoints are listed in the Breakpoints section with the diamond glyph (`◆ gold — on write`), checkbox enable/disable and remove like position breakpoints, stored by author name so they survive hot reloads. `WebSession` gains `debugWatchpointAdd`/`debugWatchpointRemove`/`debugWatchpoints`; the watchpoint stop reason now carries the global's `name`.
- 11dc916: Source-anchored breakpoints with a shared-column editor gutter
  (W4/#3297). The editor's play gutter now renders breakpoint dots (bound
  solid / unbound hollow / disabled dimmed, plus a hover preview) in the
  same column as the play ▶ — click a plain line to toggle, header lines
  keep play-from-here with "Set breakpoint here" in the symbol context
  menu. The store keeps `(file, line)` anchors as the identity (range-keyed
  per the debugger spec's v1 ruling), derives the runtime breakpoint set by
  re-binding through `resolveSourceLine` on every compile/session change,
  snaps no-code clicks to the nearest following bindable line, maps anchors
  through document edits, and persists them per project.
- d90a460: A choice carries its kind and its source (#3435): `Choice.sticky` (`+`
  vs `*`, as written) and `Choice.source` (the choice text's location, the
  same shape a line's provenance uses) on both the journaled `choices`
  line and the debug snapshot's `pending_choices`. The studio's transcript
  echo of a taken choice (`> text`) now records `choiceKind` and `source`,
  so the Player can draw the marker and link the echo back to the script.
- daaf25f: Choice-point visualization (W11/#3304, RULED). At a choice stop the
  editor lights the whole choice point: every PRESENTED choice's line gets
  the success band (the plural highlight seam's headline case), and
  authored siblings not added to the block dim with the reason beside
  them — "once-only · used" (derived from the new id-keyed visit counts)
  or the line's own failing condition ("gold > 20 = false", enriched from
  source; a by-elimination catch-all). No new runtime seam beyond two
  additive snapshot fields: `DebugChoice.def_id` and
  `DebugState.visit_ids` — both `DefinitionId`-keyed, string-equal to the
  HIR overlay projection's `def_id` (#3234's identity join, now verified
  end to end on the studio compile road, including that the path-keyed
  visit list genuinely drops anonymous choice bodies). The editor's
  highlight seam gains the `rejected` kind with a `note` chip; degraded
  still suppresses everything.
- deb671b: Continue runs to the next content line (2026-08-30 ruling, extending
  #3321). The wasm sessions gain `debugRunToLine` — advance until a content
  line COMMITS (running through the glue/commit boundary, so the crossed
  line is in the outcome at the stop — no one-advance delivery lag), or a
  breakpoint/choices/terminal stop comes first; needs no debug line info.
  The Player's Continue and the reveal-while-paused click both route
  through it and RESUME play on an ordinary stop (band back to live, chip
  clears) — an author no longer grinds through `~` statements one click at
  a time to reach content. Step Over/Into/Out stay statement-granular for
  the programmer tier, and choosing while paused still stays paused (F7's
  choice presentation), now delivering the consequence's content line in
  the same gesture.
- 26c699e: `currentPath()` (#3389 follow-up): the knot or `knot.stitch` the story is
  executing in — ink's `currentPathString` without weave indices — on
  `StoryRunnerHandle`, `StorySessionHandle` and `FlowHandle`. As in ink it
  is where the story IS, so read it before a continue to know where the
  coming line is from. The studio's session provider now steps one line
  per call on every road and stamps each transcript row with that path,
  and the Player ends a speaker's run when consecutive rows come from
  different knots or stitches — narration after a divert no longer reads
  as the last speaker's lines.
- 1bb9565: Debug info is on by default for studio compiles (W1/#3294, ruled
  2026-08-29). A fresh `EditorSession` (and the studio store mirroring it)
  now emits the `DebugInfo` section on every compile, so breakpoints bind
  and positions resolve from the studio's own bytes with no toggle touched;
  `setDebugInfoEnabled(false)` remains the opt-out, now surfaced as an
  App-settings "Debugging" section ("Emit debug info in studio compiles")
  persisted per machine. Release export and the CLI default are unchanged.
- ab8c4a1: Debug transport keybindings + status bar (W10/#3303). The spec's F-row
  lands on the command descriptors (user-remappable via keymap overrides):
  F5 continue · F6 pause · F9 toggle breakpoint at the focused editor's
  cursor line (a new `debug.toggleBreakpoint` command, gated on a focused
  ink file rather than debug capability — anchors exist without a session)
  · F10 step over · F11 step into · Shift-F11 step out · Shift-F5 restart.
  Function keys fire globally, including from the editor. The status bar's
  story segment shows the paused state (warning dot + "paused"), and the
  retired multi-session picker is fully removed (its behaviors — switch
  active flow, primary not closable — live in the Debugger panel's Flows
  section).
- ebd0cf7: The Debugger panel (W8/#3301) — the StateView replacement (RULED:
  redesign, not extension), in StateView's strip slot with a transport
  mirror in its header so stepping works with the Player hidden. Sections:
  Flows (the open-flows list lives here now — the status bar's
  SessionPicker retires; selection scopes everything below) · Frames (an
  interactive call stack: click selects, scopes the Variables section's
  locals, reveals the frame's exact line, and draws the editor's accent
  frame band) · Variables (selected frame's locals, then globals with the
  step-diff highlight) · Breakpoints (checkbox enable/disable,
  click-to-reveal, remove, disable-all/clear-all) · Story (the old
  StateView's inspection content, collapsed). Placeholders keep the old
  honesty: no session → start; no debug info → names the App setting.
- 3729f92: `@brink-lang/dialect` (RULED 2026-08-30, "Engines consume the RESOLVED dialect as a compile output"): the dialogue-dialect artifact, validator, `ResolvedDialect`, `DialectParser`, `extendDialect`, `detectCast` and the `runsOf` run rule move into a pure-TypeScript package with no runtime dependencies, so a game engine can read its project's conventions without depending on the editor. `@brink-lang/editor` re-exports the whole surface unchanged (its one editor-coupled helper is now `convertibleShapesOf`). `brink compile` writes the project's resolved dialect as `<story>.dialect.json` beside the compiled story when `brink.toml` declares `[dialogue]`, and the desktop app's Export Story does the same. Book: _Conventions for Your Engine_.
- 7768032: Dialogue-convention diagnostics + preview (RULED 2026-08-30): a `brink.toml [dialogue]` declaration that fails to resolve (unknown preset, bad element shape, missing artifact) is now an **error row in the Problems panel** keyed to `brink.toml` — the session keeps the resolver's message as state (`getConfiguredDialogueError()`), so the row reflects the current truth rather than a one-shot warning; a malformed `brink.toml` is an error row too. The dialect's own `malformed` near-miss rules (a cue missing its terminator) surface as **warnings on story lines**, re-evaluated on every compile and config apply. A new **Settings → Conventions** section shows the project's resolved dialect and a paste-to-preview pane: how the editor classifies sample lines as source, and the speaker runs the Player would fold the same lines into as emitted text.
- f20d4c2: Project-declared dialogue dialect (RULED 2026-08-30): `brink.toml` gains a `[dialogue]` table — `preset = "at-cue"` plus `[[dialogue.elements]]` overlays in the spec's affix sugar (`kind`, `prefix`/`suffix`/`glued`/`content-role`, or `pattern`/`template`), or `file = "path.json"` for a full artifact — resolved in the wasm session (`EditorSessionHandle.getConfiguredDialogueDialect()`) and pushed to every editor view by `DocumentSessions` (live on `brink.toml` edits via `ProjectSession`'s new `onProjectConfigApplied` hook and `DocumentSessions.refreshDialectFromProject`). **No dialect by default**: an absent `dialect` option now means plain lines with the screenplay layer's structural decorations kept (`setDialect(view, undefined)`); the `at-cue` preset is opt-in — the demo project opts in through its own `brink.toml`. An explicit `dialect: null` still tears the layer down for headless embedding. Also fixes a latent affix-sugar bug (a suffix-less prefix compiled to the invalid regex `[^]*`).
- 832f23f: The `[dialogue]` section of `brink.toml` as one owned block (#3410):
  `renderDialogueSection` (table or file form, stamped with a marker that
  hashes the body), `findDialogueSection` (with `owner`: editor / hand /
  edited — the UI asks before replacing anything not its own), and
  `setDialogueSection` (replace, append, or remove the section; every byte
  outside it preserved). Key-level edits cannot write `[[dialogue.elements]]`.
- 4b96bf1: Drafts are editable in Settings, and each pattern shows what it matched

  `[project] drafts` has been readable by the compiler since drafts landed and
  editable nowhere — reaching it meant hand-editing `brink.toml`, and nothing
  said whether the pattern worked. Settings ▸ General now lists the patterns
  with add and remove, alongside the prose dictionary's shape.

  Each row also reports what its pattern currently matches, because a bare list
  of globs hides both of the ordinary mistakes. A pattern matching nothing — a
  typo, or a renamed folder — looks exactly like one that is working, and now
  says so. And a pattern matching a file the story still reaches produces no
  draft at all (reachability wins), so those files are listed separately as
  still in the story rather than silently counting for nothing.

  `EditorSessionHandle.getDraftGlobReport()` exposes that per-pattern
  attribution; draft status itself is still computed only in Rust.

- c179371: The editor's named actions are rebindable, and listed in Settings ▸ Keymap

  Rename Symbol (F2), Find References (Shift-Alt-F), Code Actions (Mod-.),
  Edit Arguments (Mod-Shift-A) and Insert Element (Alt-Enter) existed only
  as chords hardcoded inside their CodeMirror extensions — invisible to the
  keymap surface and unrebindable.

  Each extension now provides its behaviour through a runner registry while
  the chords live in one rebindable keymap
  (`@brink-lang/editor`'s `EDITOR_ACTIONS` / `setEditorActionKeys` /
  `runEditorAction`). The studio registers the five as ordinary commands, so
  they appear in Settings ▸ Keymap and the palette, and a rebind flows back
  into every open editor live — one source of truth, so the table can never
  show a chord the editor disagrees with. Embedders that never touch keys
  get exactly the bindings that shipped.

- 6a405dd: Break on write from the editor (W18 follow-up): right-clicking a global's identifier in the source offers "Break on Write 'name'" in the context menu's identity group — the same verb as the Debugger panel's variable rows, resolved against the live session's globals or, before any session runs, the compiled program model. Already-watched names show the Remove form.
- 2b9eb59: Applying a Safe fix from the Problems row no longer scrolls the editor away
  from the edit (#3496): `applyMoveResult` threads a single `Fix`'s own
  precise `edits` through to the document layer's new
  `applyEditsToViews`, so the mounted view gets a minimal change instead of a
  whole-document reload. A structural op (rename/move/promote/demote/reorder)
  has no such precise edit list and still benefits from the document layer's
  own minimal-diff fallback.
- d1265be: The live execution highlight (W6/#3299 — "play is stepping"). The wasm
  sessions gain `resolveDebugLine(containerIdx, offset)` — the
  position→source road: file, 0-based line, and the covering debug entry's
  exact byte range (kept on the seam for future instruction-level
  stepping). The editor gains `executionHighlightExtension`, a plural
  highlight seam (a choice point or selected stack frame can light several
  lines at once): a subtle full-line band per position — green while
  playing, amber when paused (with a filled gutter arrow in the shared
  play/breakpoint column), accent for a selected frame (hollow arrow). The
  studio wires it end-to-end: the band follows every reveal, pausing
  scrolls the editor to the stop (reveal-on-stop) and shows a
  "Paused — file:line" chip in the Player, and degraded sessions suppress
  the highlight rather than showing a stale one.
- e9c8b84: The execution-highlight policy no longer pulls the file's HIR projection
  unless it can use it. `executionHighlightsFor` now accepts the projection as
  a thunk, and only the choice-point branch resolves it — "no session",
  "ended", "error", "degraded" and plain "running" all answer without touching
  it. The studio passes `() => documents.getHirProjection(path)`, so the
  synchronous whole-document `getHirSpansDoc` query that pull entails stops
  running on every keystroke of an idle editor.

  Passing a plain projection (or `null`) still works exactly as before.

  The studio's own wiring of that seam is now the named export
  `executionHighlightsHook(getState, getProjection)` instead of an arrow inlined
  in `mountStudio` — the eager evaluation was a property of the call site, so
  the call site is what had to become testable.

- 93b6c4b: "Suppress a code in this file" now suppresses that code, not the whole file.

  The Problems panel offered **Suppress E157 in this file** and wrote a bare
  `// brink-disable-file`, which silences every diagnostic in the file. The
  label and the effect disagreed, and `// brink-disable-file E157` — written
  by hand, in the obvious analogy to the line-scoped form — matched no
  directive at all and was dropped in silence.

  - `// brink-disable-file E027 E035` suppresses those codes for the whole
    file. Whitespace-separated, matching `// brink-disable E027 E035`.
  - `// brink-disable-file-all` is the blanket gesture's new spelling.
  - The Problems menu offers both as separate items, each labelled for what it
    does.
  - A `brink-disable`/`brink-expect` comment the parser cannot read is now
    reported as **E192** instead of vanishing.

  `// brink-disable-all` (project-wide) is unchanged.

- f21f6a6: Fix on save (`docs/autofix-spec.md` §7) now persists every file a batch
  touched, not only the focused one. `file.save` (⌘S) narrows its host-save
  write to the focused path — correct for an ordinary edit — but the
  fix-on-save step running inside that same save can rewrite other files too
  (a cross-file fix); those were staying staged and silently unpersisted
  while the save reported success. `file.save` now checks
  `runFixOnSave`'s own return (every path it actually wrote) and, when that
  names more than the focused file, routes the write through the same
  per-path confirm→retire algorithm `file.saveAll` already uses, narrowed to
  exactly the touched set. A toast names the other file(s) written; the
  focused file's own "Saved" notice, and fix-on-save's deliberate no-toast
  rule for the file being saved, are unchanged.
- 7bfa924: App setting to hide inlay hints (#3350). Settings ▸ Editor gets a "Show
  inlay hints" toggle, persisted app-scope alongside the other editor
  preferences and broadcast live to every open editor via
  `DocumentSessions.setInlayHints` (the same `_documents?.setXxx(...)`
  broadcast shape `setFormGlyph`/`setAutoOpenForm` already use) — matching
  editors opened later too. Default stays ON (current behavior).
- f98a655: Hot reload (W15/#3308, spec §F8 REVISED). Edits during play reach the
  running Player: on every successful compile the live session migrates —
  journal replay when it lands cleanly (exact position and transcript
  survive), and the W14 checkpoint road (snapshot → fresh session →
  loadState → divert to the recorded knot) when replay diverges, fails,
  throws, or reports "clean" while regressing the turn count (the
  journal-bypass reality of debug-driven sessions, #3335). Globals, visit
  counts, and the turn index survive the edit; a lossy migration surfaces
  the LoadReport as a "Reloaded — …" transcript notice; the status chip
  flashes a brief "Reloaded". Degraded mode is demoted to the fallback
  (failing compile keeps the old program; the supersession is recorded in
  live-inspector-spec §5).
- c179371: Rebind keys by pressing them, in Settings ▸ Keymap

  The keymap surface was a raw JSON textarea: it asked an author to know
  both a command id and the `"Mod-Shift-P"` spelling, and offered no way to
  discover either. It is now a searchable table of every registered command,
  grouped by category, with its current bindings.

  Recording a binding uses the same function the global key handler
  dispatches through, so what you press is exactly what will fire — a typed
  binding can be spelled correctly and still not be the chord your keyboard
  produces. Commands keep all their bindings as chips, because several ship
  two or three defaults to dodge browser-reserved chords and an override
  replaces the whole set.

  Taking a key that another command holds displaces that command and says
  so before saving, naming what will lose the key. The resolution table is a
  map from chord to command, so two commands holding one chord means one of
  them silently does nothing — the editor will not let you build that state.

  The JSON stays, below the table, for anything the table cannot express.

- e9a7fcf: The Debugger and State View locals tables hide compiler-minted temps
  (`DebugLocal.synthetic`, the #3395 lift-order hoist's `$liftN`), so an
  author only sees the variables they wrote.
- f50c84a: A delivered line's `source` now spans every source line that contributed text to it (glue, a prose-dialect cue + aside + dialogue), and the editor's follow/hover bands cover all of those lines (`ExecutionHighlight.endLine`).
- 372c5cd: The Player renders dialogue RUNS from the project's dialect (RULED 2026-08-30): delivered lines are classified with the resolved `brink.toml [dialogue]` artifact — the same one the editor uses — and folded into runs by the shared `runsOf` rule: the cue header once (speaker coloured by a deterministic palette index — the hardcoded demo cast table is gone), its spoken lines beneath, parentheticals styled inline, action/narrative outside. No dialect ⇒ plain lines, as Inky. The `@NAME:` regex is gone with it. Also fixes the choice-echo bug: an echo is styled because the row's `kind` is `marker`, never because its text starts with `> ` — a story line beginning with `> ` (an action convention) is story text.
- f38e85b: Player feedback round (RULED 2026-08-30): saves carry the STRUCTURAL transcript (the runtime's part stream as human-readable JSON — `WebSession.exportTranscript`/`renderTranscript`) and loads, forks, and hot-reload migrations re-render it against the CURRENT compile, so an edited line's restored row shows the edited prose; fast-forward is a one-shot ContinueMaximally (run to the next choice/stop, paced per settings, no sticky auto mode); Player toolbar sub-sections collapse one group at a time into a ⋯ overflow menu when the pane is too narrow, with hysteresis on re-expansion.
- a48b4d5: Player appearance settings (W13/#3306, RULED). Settings → Player gains a
  font-size knob for the Player's prose — its own `--bs-player-font-size`
  variable on the `--bs-editor-font-size` precedent (the reading surface's
  size is not the UI's size), falling back to the app type scale at the
  default 0. Stepping below the readable floor resets to follow-scale
  rather than sticking at a clamp. Persisted with the paced-reveal
  setting; room to grow (line spacing, face) without re-ruling.
- a8c4e13: Peek: hovering Continue or a choice card in the Player forks the live story (`StorySessionHandle.speculate()`, new, at the exact position), runs one continue call on the fork and highlights what it would hit in the editor with a dashed `peek` bar; `SpeculationHandle.currentPath()` reports the fork's knot. Execution highlights split into tint (state) and bar (attention) channels: `follow`/`hover`/`peek` are bar-only and stack on a tinted line, and the cursor's active line gets its own colour on a tinted line.
- b92f124: The rebuilt Player (W7/#3300). Every delivered line now carries its
  source (`Line.source` / `DebugOutputLine.source` — file + byte range,
  from the line table's own locations), and the Player makes each
  transcript row a provenance handle: full-width line rows with a subtle
  alternating tint, hover shows `file:line`, click (or ⌘-click the row)
  reveals the source in the editor. A tags toggle renders per-line tags as
  muted mono chips (off by default, persisted). The status chip is the
  single home of stop reasons — ready / playing / paused at file:line /
  waiting on choice / ended / error / out-of-sync — and clicking it
  reveals the current line. The story no longer auto-starts (RULED): the
  Player opens idle with the toolbar live; Run compiles and starts.
  Auto-reveal is paced by default (RULED, ~150 ms per line, Settings →
  Player to switch to all-at-once); pausing or a breakpoint stops the run
  instantly. Auto-scroll suspends while reading back. Narrow-tier layouts
  regain the hamburger route to a closed player, and the reopen split
  honors "when there is room" (#2795).
- c179371: One Player settings section, and the entry file wears the brink mark

  Debugging, Player and External functions were three App-scope rail rows;
  they are now subheadings of one "Player" section — they are all "how the
  story behaves when I press play", and three rows made the rail longer
  than the settings under them warranted.

  The Binder's entry file no longer carries an "entry" text badge. Collapsed
  over its knots and stitches it shows the brink mark itself — the brand's
  ink drop with the divert carved out as negative space, geometry lifted
  verbatim from the brand asset. Expanded (or empty) it follows the
  Binder's fill rule like every other row: the ordinary outline drop, with
  the divert inlaid as a stroke at the same spot, so the arrow does not
  move when a row opens or closes.

- 17e6912: Program Explorer additions (W9/#3302). Instruction stepping (`stepi`
  into/over/out) lives in the explorer's header — the granularity ladder's
  programmer-assist tier, never in the Player toolbar. The
  current-instruction highlight follows the Debugger panel's selected
  stack frame, not just the top (degraded still suppresses). The editor's
  line context menu gains "Reveal in Program Explorer" (the inverse of the
  `.inkt` open): the line's instructions open, auto-expanded, scrolled to,
  and flashed — with honest notices when no session is running or the line
  compiles to nothing. The editor package exposes the
  `onRevealInstructions` callback on its play-from-here options.
- 1f1a500: The Program Explorer's new shell and Structure view

  The Program Explorer becomes one instrument with a view switch (Structure
  · Line tables · Disassembly · Size — the last three disabled slots until
  their phases land, each naming where its view is). The program reads as a
  named thing: entry-file stem, status dot, checksum chip, and counts
  replace the bare hex toolbar.

  Structure: knot rows carry size at a glance — a bar of bytecode with a
  lines fill inside it, on a shared scale, with per-row byte/line/container
  counts rolled up from the knot's whole subtree. The definitions column
  groups globals, lists, and externals, and each external states its
  contract: a `fallback` body the story can run on, or `host` — a binding
  the host must register. A footer totals the program; while paused, it
  names the executing container the way a save file would.

  The existing behavior contract is untouched: expansion, the
  current-instruction and reveal-target highlights, and the stepi actions
  all work exactly as before, pinned by the same tests.

- 1f1a500: The Program Explorer becomes one instrument with four views

  Structure, Line tables, Disassembly and Size behind one segmented switch
  (#3339), with a shared identity header and one execution thread through
  all of them.

  Structure: knot rows with size bars (bytecode + lines on a shared
  scale), externals stating their contract (fallback vs host), totals in
  the footer. Line tables: the compiled lines scoped as the compiler
  scopes them, template slots and selects as chips reading like prose,
  source cells as line-numbered links that convert byte offsets to the
  editor's UTF-16 before revealing. Disassembly: every operand resolves —
  emit_line to its line text (linking into the Line tables view), globals
  to live values while paused, jumps to their landing offset, externals to
  their binding contract — with per-instruction source provenance from the
  DebugInfo section and stepi beside the code it steps. Size: a squarified
  treemap of real on-disk section bytes, with an exact "shipping only"
  re-flow showing what a release export strips.

  New runner-free surfaces on `@brink-lang/web`: `linesTableOf`,
  `sizeReportOf`, per-scope `byte_size`/`container_count` and anonymous
  child containers (labeled by their real weave-label names) on the
  program model, and per-instruction `src` provenance.

- 790e1cb: Prose checking no longer freezes the editor. The studio's `ProseChecker`
  now runs the `brink-prose` wasm module inside a Web Worker, which lazily
  imports it there on the first check — so an embedder that never checks
  prose still downloads nothing, and a check no longer blocks input for
  the length of the document. A check superseded by a newer edit is
  dropped before it is posted rather than queued behind the one in flight.

  Measured on the 8k-line perf fixture, before and after on one machine:
  the long task co-located with a check falls from 6,465 ms to 184 ms
  (fast-scroll), and the worst long task of a typing burst from 6,689 ms
  to 760 ms. The check itself is not the point — its latency is roughly
  unchanged, and a cold first check can be slower (worker spawn plus a
  second instantiation of the 6.5 MB module); it simply no longer runs
  where keystrokes are handled, and the cached dictionary and rule set pay
  the cold cost back from the second check on.

  Environments with no `Worker` (jsdom, a bundler that leaves the
  `new URL(..., import.meta.url)` shape alone) and a crashed worker fall
  back to the previous in-process road, so checking degrades in speed
  rather than stopping — from the check after the failure, since a boot
  failure surfaces asynchronously and rejects the check already in flight
  (#3491).

- ce076fe: The program and session location resolvers are now registered (W3/#3296):
  a program-address Location resolves to source through the live session's
  DebugInfo road, gated on `sessionDegraded` at the caller (suppressed
  before the provider is even consulted — never stale), and a
  position-shaped session ref chains session → program → source. The
  symbol resolver moves into the same `registerLocationResolvers` module.
- e5ae131: Runtime-value hover (W12/#3305, RULED). While a session is live and
  in-sync, hovering a variable in the editor appends its current runtime
  value to the existing hover card — globals always, frame locals while
  paused in the Debugger panel's selected frame's scope. Pairs with the
  choice-point visualization: hover the failing condition's variable to
  see exactly why it failed. No new wasm surface — the editor extracts
  the identifier under the cursor and asks the host
  (`getRuntimeValueNote`); the studio's policy suppresses under degraded
  and outside a live session, never stale.
- 5e67883: Runtime save/load — the idle-Player launcher (W14/#3307, RULED;
  re-scopes #57's save half). The wasm session's `loadState` now RETURNS
  the runtime's `LoadReport` (the session layer used to discard it) — a
  stale load's drops surface inline, never silently. The idle Player body
  becomes the launcher: "Run from the start" beside a typeahead over
  knots/stitches (KNOT/STITCH chips + file context; plays from there via
  the play-from-here start path), then the checkpoint stores as PROJECT
  and THIS COMPUTER sections in the landing Recent-list style — TURN-count
  chips, amber OLD for saves against an older compile, and hover
  Load/Fork/delete. Load ATTACHES the session to the slot ("Save state" —
  the new toolbar button — writes back); Fork starts from a copy and the
  next save picks a new slot. The payload is the runtime's existing
  `SaveState` boundary (no execution position — loading diverts to the
  slot's recorded knot). Both stores are localStorage on the web;
  `mountStudio`'s new `saveStores` option is the seam for desktop's
  file-backed stores. Settings → Player picks the default target for new
  saves.
- 7b44661: Settings chrome follows the theme, and the Project/App switch is legible in all of them

  The scope switch drew its selected half in `--bs-panel-bg` — the same
  surface the modal already sits on in most themes — so which scope you were
  on came down to `--bs-fg` vs `--bs-fg-muted`. That step is only large
  enough to see where a theme happens to make it large, which is why
  inky-dark read fine and the rest did not. It is now accent-filled:
  `--bs-on-accent` is defined as the colour legible on `--bs-accent`, so the
  contrast comes from the token contract rather than from luck.

  Underneath it, a chunk of the Settings UI could not respond to the theme at
  all. `mocha` is the bare-class default, so it defines the raw Catppuccin
  palette for every theme while the others override only the semantic
  `--bs-*` layer — meaning `var(--ctp-base, …)` resolved to mocha's dark
  blue-grey under all five, including the light ones. That pinned the
  Settings rail and the toggle track. The same shape appeared as
  `var(--bs-bg, #1e1e2e)`, where no theme defines `--bs-bg` at all and the
  "fallback" was simply the value: that one put dark dropdowns in the middle
  of latte's Settings.

  Both are gone, along with `--bs-draft` and `--bs-accent-secondary`, which
  were read but defined nowhere and so drew fixed Catppuccin peach and mauve
  in every theme; each theme now names its own. Two guards keep the class
  from returning: theme-agnostic chrome may not read a raw `--ctp-*` token,
  and may not fall back to a literal colour for a token no theme defines.

- f186ac8: Suppressing a diagnostic project-wide now clears its squiggle immediately,
  instead of leaving it until the file is edited or reopened.

  Compile squiggles are published by the diagnostics extension's ViewPlugin,
  which wakes on a document change. A compile that lands for some other reason
  — a `brink.toml` edit changing `[lints]`, a suppression written into a
  sibling file — has no document change in the view showing the diagnostic, so
  nothing republished. The prose checker already had this seam
  (`refreshProseEffect`); `refreshDiagnosticsEffect` is its compile-side twin,
  dispatched wherever a compile is delivered.

- 75cfdd3: Structural rails CSS (#3501, ruled 2026-09-03): the rails bar layer packs
  with `gap: 0` and tighter side padding (2px -> 1px), so the reserved
  one-lane column shrinks from 7px to 5px alongside `@brink-lang/editor`'s
  `RAIL_LANE_WIDTH_PX`. The rail tooltip now renders a list of entries (one
  `.brink-rail-tooltip-entry` per container in the line's stack, outermost
  first) instead of a single label/meta pair.
- 60ba4be: Fixed two "navigation loses my place" bugs (#3355, #3356): switching an editor tab away and back now keeps your scroll position in long files instead of resetting to the top (`InkFileDocument`'s CM6 mount effect now snapshots scroll on unmount via `useLayoutEffect`, before React detaches the deactivated tab's container — a plain `useEffect` cleanup ran too late to read it); and single-clicking a knot/stitch whose file is already open as a tab now jumps to it in place, or focuses that tab in another group, instead of always opening a new fragment tab. Double-clicking a knot/stitch still opens its own dedicated (pinned) tab, unchanged.
- 33185cc: TODO notes can carry a tag — `TODO(audio): mix the vault door` — and the
  TODOs panel turns each distinct tag into a chip you can toggle to filter.

  The tag needs no language support: the ink parser already takes everything
  after `TODO` to end of line, so the tag arrives as part of the note's text
  and is split off for display. A note's tag renders as a chip on its row
  rather than staying in the text, since `(audio)` repeated down a column is
  noise.

  The panel's title bar gains the two controls the Problems panel has: a
  funnel that folds out the filter row — now holding the text filter _and_
  the tag chips — and a group-by-file toggle for switching between the
  per-file sections and a flat list.

  Grouping persists; the tag selection deliberately does not. A tag is a
  property of one project's notes, so restoring `(audio)` into a project
  without it would filter the panel empty with no visible cause. Closing the
  filter row clears the selection for the same reason.

- 221af23: TODO notes get their own Problems-panel filter, **off by default**, so they
  report in the TODOs panel without also filling Problems.

  An author who wanted them out of Problems previously had only
  `[lints] E189 = "allow"` to reach for. That suppresses the code at the
  COMPILER, and the TODOs panel reads the same diagnostics — so turning them
  off in one place emptied the other. Panel visibility is not a compiler
  concern.

  `E189` now buckets as `todo` rather than `info`, alongside the `prose`
  bucket added earlier and for the same reason: it is a SOURCE, not a
  severity, which is what lets it default off while `info` stays on. Turn the
  bucket on to see TODO notes in both panels.

  A stored preferences record written before this bucket existed reads as off,
  so upgrading never puts TODO notes into Problems unasked.

- 099b471: Tooltips no longer collapse to one-word width in the studio (#3497). The tooltip portal layer introduced in #3349 was zero-width, and when CodeMirror falls back from fixed to absolute placement a tooltip sizes against that layer; the layer is now full-width, zero-height and click-through, so tooltips size against the editor root again.
- f67b91a: Fix editor tooltips (hover cards, lint popups, autocomplete) rendering
  clipped under the Player pane. CodeMirror mounted every tooltip inside the
  editor's own `.cm-editor` element, so a sibling pane with its own stacking
  context or `overflow` (the Player split, `z-index: 30`) could clip or paint
  over it — a `position: fixed` tooltip escapes scroll clipping, not an
  ancestor's stacking order or `overflow` box.

  Tooltips now reparent (`tooltips({ parent })`) into a dedicated
  `.brink-tooltip-layer` mount point the shell renders inside the real
  `.brink-studio` theme root (found via the same `closest(".brink-studio")`
  lookup `widget-popover.ts` already uses), so `--bs-*` design tokens keep
  applying — a headless embed with no `.brink-studio` root, or a host that
  doesn't render the layer, falls back to `document.body`, which still escapes
  the clip. The layer is a dedicated mount rather than `.brink-studio` itself
  because CM6's own tooltip container is `position: relative`, not
  `fixed`/`absolute`; mounted directly on `.brink-studio` (a flex column) it
  became an in-flow flex item that broke the shell's layout. `@brink-lang/studio`'s
  tooltip CSS no longer requires a `.cm-editor` ancestor, since the reparented
  node is no longer inside one.

- 9d3f5fa: Play and debug are one loop (W5/#3298). `debugRun`/`debugStep` — and the
  new `debugStepLine` (the author-tier source-line step, bounded by armed
  breakpoints) — now return the emitted-lines delta, drained from the SAME
  delivery cursor the journaled `continue` road hands lines out of, so a
  line the production lookahead already completed surfaces exactly once
  whichever loop advances past it. The studio Player routes reveals through
  the debug verbs whenever breakpoints are armed or the session is paused;
  `pause` is a first-class verb (Player transport: pause/continue + step
  over/into/out with a "Paused — location" chip); choices stay journaled,
  so restore/replay is unchanged.
- 061ff61: The last `brink.toml` key without a Settings surface: `unprune-dirs`

  An audit of the config schema against the Settings UI found twelve of the
  thirteen settable keys had a surface and one did not. `[project]
unprune-dirs` names which of discovery's always-skipped directories
  (`target`, `.git`, `node_modules`) a project wants walked anyway — for a
  project that genuinely keeps story files in one of them.

  It is three checkboxes rather than another free-text list, because the
  value set is closed: naming anything outside those three un-prunes
  nothing, and the config parser already answers such an entry with "it was
  never pruned, so this has no effect". A text field could only produce one
  of three right answers or a silent typo.

  The three names restate a Rust constant, so a test reads
  `brink_source_tree::IGNORED_DIR_NAMES` out of the source and compares,
  rather than repeating the names and agreeing with itself forever.

- 88d8352: Live value editing (W16, spec §F6 RULED): scalar globals and frame locals are click-to-edit in the Debugger panel while paused — inline mono input, Enter commits, Esc cancels, a parse/type-refused edit red-shakes with nothing written; edits can never change a value's type. Globals commit through the observed write path (`WebSession.debugEditGlobal`); locals through the new set-temp-in-frame debug seam (`debugEditTemp`), disabled at choice stops where choosing would restore the choice's captured thread over the edit. "Reveal in Program Explorer" now only appears in the editor's line menu while a session can actually resolve it (`canRevealInstructions` gate).
- 57aa2dc: Watch — the full mini-REPL (W17, spec §F18 RULED): a Watch section in the Debugger panel evaluates arbitrary typed expressions (`gold >= pour(2)` → `false`) and divert/content fragments (`-> market.haggle` → an expandable transcript preview of what it _would_ produce, reached choices included) against the live session's current state — side-effect-proof over the shipped speculation engine (discard-on-drop sandbox, budgeted), re-evaluated once per stop/turn boundary, fragment compiles cached per program version, degraded suppressing re-evaluation, failures inline on the row.
- Updated dependencies [fc827ec]
- Updated dependencies [d0acebb]
- Updated dependencies [16072b0]
- Updated dependencies [c0695b8]
- Updated dependencies [ba08f3c]
- Updated dependencies [94b8c37]
- Updated dependencies [b9820be]
- Updated dependencies [c0ffbce]
- Updated dependencies [b0d3fce]
- Updated dependencies [b82cb34]
- Updated dependencies [d90a460]
- Updated dependencies [daaf25f]
- Updated dependencies [f236910]
- Updated dependencies [e1111ab]
- Updated dependencies [deb671b]
- Updated dependencies [127dee4]
- Updated dependencies [26c699e]
- Updated dependencies [1bb9565]
- Updated dependencies [ef1ac8a]
- Updated dependencies [3729f92]
- Updated dependencies [7768032]
- Updated dependencies [f20d4c2]
- Updated dependencies [2c10dd3]
- Updated dependencies [4b96bf1]
- Updated dependencies [52881be]
- Updated dependencies [272df89]
- Updated dependencies [4dc8b89]
- Updated dependencies [706fe0b]
- Updated dependencies [dce2827]
- Updated dependencies [3448c50]
- Updated dependencies [d1265be]
- Updated dependencies [93b6c4b]
- Updated dependencies [92e114b]
- Updated dependencies [0938a9d]
- Updated dependencies [4a1d7df]
- Updated dependencies [301925e]
- Updated dependencies [88ef785]
- Updated dependencies [f90881f]
- Updated dependencies [8b9b045]
- Updated dependencies [f77033b]
- Updated dependencies [d05de1f]
- Updated dependencies [a960fc4]
- Updated dependencies [0f7af5d]
- Updated dependencies [f50c84a]
- Updated dependencies [b0e2d3a]
- Updated dependencies [29dfd78]
- Updated dependencies [fd34329]
- Updated dependencies [11af92c]
- Updated dependencies [12ef8e9]
- Updated dependencies [221307b]
- Updated dependencies [7be34d0]
- Updated dependencies [f38e85b]
- Updated dependencies [a8c4e13]
- Updated dependencies [b92f124]
- Updated dependencies [e8d75f2]
- Updated dependencies [1f1a500]
- Updated dependencies [1f1a500]
- Updated dependencies [4f70d28]
- Updated dependencies [368d7fa]
- Updated dependencies [5e67883]
- Updated dependencies [a6b6f7f]
- Updated dependencies [a343249]
- Updated dependencies [4abd6c8]
- Updated dependencies [e680185]
- Updated dependencies [c0ffbce]
- Updated dependencies [4c142de]
- Updated dependencies [d1cf91b]
- Updated dependencies [a103f15]
- Updated dependencies [e48d343]
- Updated dependencies [9d3f5fa]
- Updated dependencies [88d8352]
- Updated dependencies [2314f79]
  - @brink-lang/web@0.18.0

## 0.17.0

### Minor Changes

- d7092db: Clicking `brink.toml` in the Binder now opens the **Settings** takeover, in
  every editor view. Settings gains a "Project" section carrying the whole
  config document — the structured form and the raw text beneath it.

  Continuous view renders the project's manuscript and deliberately excludes
  `brink.toml` from it, so the config file was simply unreachable there
  (#3166). Routing to Settings answers that once for every view rather than
  per-view.

- e9cabaa: Documents now carry a file icon before their name in every view that names
  one — the Code view's tab, the Single File header, the Continuous section
  heading, the takeover header.

  Draft status (#3145) moves into that icon: a draft is the same ink-file
  drop drawn provisionally, dashed and orange, replacing the "DRAFT" text
  badge. A badge was a second element competing with the filename for the
  same row; the icon is already beside the name, so it carries the status
  for free and cannot drift away from what it describes.

  The shell prop `documentMark` is now `documentIcon`, and renders before the
  name rather than after.

- cf2d5a4: `[project] drafts` in `brink.toml`: path globs naming work the author has
  deliberately not wired into the story. A file matching one that is also
  unreachable from the entry is a **draft** — it shows no "not included"
  banner, and is marked as a draft wherever the studio names it (the Binder
  row, the Continuous section heading, the Single File header, the Code
  view's tab).

  Reachability wins: a marked file the entry still `INCLUDE`s is not a draft
  at all, so draft status can never exclude a file the story reaches.

  New: `EditorSessionHandle.getDraftPaths()`, and a `documentMark` slot on
  `ShellProvider` for any host that wants a status beside a document's name.

- 7f29f7e: Settings gains a **Diagnostics** section for `[lints]`: two lists, where
  which list a code is in _is_ whether it is in `brink.toml`. "Configure"
  moves a code up — writing the key at its current default, so the first
  click changes nothing about the build — and the down arrow moves it back
  out, removing the key.

  Both lists group by category, rows carry the Problems panel's own severity
  glyphs showing each code's _effective_ level, and a written explanation
  expands in place.

  What is listed comes from the compiler, not from the studio: only
  overridable codes appear (30 of 189), and a project is only offered codes
  its own source surfaces can produce — so a `.ink`-only project sees no
  settings for `.brink` markup spans.

  The previous "Diagnostics" section is now titled **External functions**,
  which is what it configures. Unlike the lints, it is a studio preference
  rather than a `brink.toml` setting.

- 37b9a5d: The Problems panel gains header controls: per-severity toggles (errors,
  warnings, info/hints — each showing its count and muting that severity
  when off), a funnel button that reveals a text filter over messages and
  locations, and a group-by-file toggle with collapsible per-file sections
  and per-file counts. The controls live in the panel's chrome header via
  the new tool-window `actions` slot. Defaults reproduce the previous
  panel exactly — every severity shown, ungrouped, no filter — so nothing
  changes until a control is used.
- 545cd2b: Right-click a row in the Problems panel to silence that diagnostic, at
  three scopes — this line, this file, or this project. Each writes a
  directive the compiler already understands: `// brink-disable Exxx` above
  the line, `// brink-disable-file` at the top, or `[lints] Exxx = "allow"`
  in `brink.toml`.

  A code the compiler will not let you suppress — anything error-tier — gets
  no suppression items, since every channel would refuse it.

- ca23b71: Settings is now a modal with a searchable section rail — Project,
  Diagnostics, Editor, Appearance, Keymap — showing one section at a time,
  rather than a takeover of the editor area with everything in one scrolling
  column.

  Whatever you were reading stays on screen behind it. Search matches what a
  section is about as well as its name, so "todo" finds Diagnostics.

  `registerSettingsCommand` now takes an open-callback rather than the shell
  layout store, and the `settings` document type is gone.

- 4c9914a: Tool windows can contribute controls to their chrome header:
  `ToolWindowDescriptor.actions` takes a component, rendered between the
  panel title and the close button. It follows the existing `badge`
  contract — the registering app supplies the component, so it subscribes to
  that app's own store and stays reactive without the shell depending on any
  app store. The header's uppercase, letter-spaced title styling is reset
  inside the slot so action components render with ordinary control
  typography.
- 5b33c88: Adjustable text size, on both knobs. The **editor** has its own size
  (Mod-= / Mod-- / Mod-0, the palette, or Settings), and the **app** has one
  that scales the whole UI. Behind the app knob, 179 hardcoded font sizes
  across 24 stylesheets were replaced by a nine-step type scale derived from
  a single `--bs-font-base`, so components now reach for a named step
  instead of inventing a number. The sweep is pixel-identical at the default
  size apart from twelve declarations that snapped 0.5px to the nearest
  step. Also defines `--bs-font-mono`, which every use site referenced but
  nothing declared.

### Patch Changes

- b5397c6: Interpolated bindings read differently from prose: `--bs-syn-variable` (and the previously unstyled `tok-property` for dotted field segments) map to Catppuccin maroon in both themes instead of the plain text color — an interpolated `{binding}` no longer renders in the same color as the dialogue around it (author feedback; interim tweak ahead of the writing-first color design pass).
- 7603a3e: The editor's prose no longer slides sideways when a file opens. The
  structure-rails gutter was sized by its content, and that content only
  exists once the HIR projection arrives a few hundred milliseconds later, so
  the column grew from nothing and the compensating content padding — which is
  the text's offset — was rewritten by the same delta. The column is now a
  fixed one-lane width that does not depend on the open file's nesting depth
  or on when the projection lands, so there is no growth to compensate for.
  Deeper stacks paint their extra lanes over the neighbouring play gutter,
  which is empty except on the hovered line; the bars live in an
  absolutely-positioned layer and still render every lane at full size. Also
  reclaims 10px of permanently blank gutter on every file.
- af7e80c: Settings and the Story Graph now take over the editor area instead of opening
  as tabs, so they are reachable from every view. Previously they were tabs,
  which only works in a view that has tabs — in Continuous view, opening
  Settings put it behind the manuscript where it never appeared. A document
  type opts into this with `takeover` on its descriptor. The takeover has a
  header with a close button, choosing any view dismisses it, and it is not
  restored on reload: consulting the graph is an interruption, not a place.
  The view commands are also renamed to "View mode: Code" / "View mode: Single
  File" / "View mode: Continuous", and they update the same setting the
  Settings picker shows.
- 97f7ca0: A hover card and a diagnostic on the same symbol now read as one panel
  rather than a block bolted underneath.

  They were already one tooltip with two sections, and both sections were
  transparent — what made the diagnostic read as a different window was a 3px
  severity rail no other row had, and a different padding to accommodate it.
  The rail is gone and the padding matches the card's rows exactly, so
  `warning` sits in the same column as `knot` and `effects`.

  Severity is carried by the label word instead, which is what it was added
  for: it survives a colourblind reader and a screenshot pasted into an issue,
  which a rail never did. The lint panel keeps its rails — there is no label
  there, and rows are scanned as a list.

- 0b07df5: Debugger D8's control half (issue #3186) bridged through wasm to the studio
  (#3232) — D9 (#3187) bridged only the read half (program → source position
  resolution). `StoryRunnerHandle` and `StorySessionHandle` (`@brink-lang/web`)
  gain `debugRun`/`debugStep`/`debugBreakpointAdd`/`debugBreakpointRemove`/
  `debugBreakpointSetEnabled`/`debugBreakpoints`, wrapping the runtime's
  `Story::debug_run`/`debug_step`/`BreakpointSet` (feature `debug-hooks`, now
  built unconditionally into the `brink-web` wasm package rather than a
  build-time toggle nothing in the studio's pipeline passes). `@brink/wasm-types`
  gains the `Breakpoint`/`DebugRunOutcome`/`DebugStopReason`/`StepMode` wire
  shapes.

  `@brink/studio-store` gains a `DebugSessionProvider` capability extension on
  `SessionProvider` (one extension covering both pause/step/breakpoints and
  D9's previously-uncaptured position-resolution capability, per the issue),
  implemented by `LocalSessionProvider`, plus a new debug slice
  (`debugCapable`/`debugBreakpoints`/`debugLastOutcome`/`debugStatus`) and the
  `debug.run`/`debug.stepInto`/`debug.stepOver`/`debug.stepOut`/`debug.
breakpointAdd`/`debug.breakpointRemove`/`debug.breakpointToggle` commands,
  registered alongside `story.*` at the app boundary.

  **Scope honesty**: this is real, working plumbing (proven over a real
  `WebSession` in `crates/brink-web/src/session.rs`'s `debug_control_tests`,
  plus a vitest suite over the store slice) — but the studio still cannot
  compile a project WITH debug info at all (#3229, a separate, un-made
  maintainer ruling on the toggle mechanism), so an end user will not see any
  of this working yet. No UI consumes the new slice either — the editor
  gutter / current-line highlight is a separate, later ticket. This PR lands
  the bridge ahead of #3229 because the plumbing is independent of which
  toggle mechanism wins.

- 76bbdeb: Add a per-session debug-info compile toggle (#3229).

  `EditorSessionHandle.setDebugInfoEnabled(enabled)` / `debugInfoEnabled()`
  control whether this session's compiles emit the D6 `DebugInfo` section.
  Off by default, matching the ship policy; a host turns it on for the
  session it is about to debug and off when that session ends.

  This is what makes the debugger reachable at all: without the section, the
  runtime position, locals table and program→source resolver landed by
  D4/D6/D7/D9 resolve to nothing, because the studio's live session runs on
  exactly the bytes the editor session compiles.

  The caller must recompile for the flag to take effect — it governs what the
  next compile emits. Toggling bumps the session generation, so the next
  compile is a real one. The studio store exposes it as `setDebugInfoEnabled`,
  which recompiles for you.

- 5079c84: Debugger D9 (issue #3187): the wasm bridge for D4's runtime position (#3182)
  and D6's `DebugInfo` section (#3184) — the program→source resolver the
  studio Location protocol's `program` space names as landing "with its
  consumer" (`docs/studio-shell-spec.md` §6.1).

  `@brink-lang/web`:

  - `StoryRunnerHandle.resolveDebugPosition(containerIdx, offset)` and
    `StorySessionHandle.resolveDebugPosition(containerIdx, offset)` resolve a
    runtime `(containerIdx, offset)` position — exactly what `debugSnapshot()`'s
    `position`/call-stack frame `position` fields report — to the source range
    it was compiled from, via the loaded program's `DebugInfo` section. Returns
    `null`, not a throw, when the program carries no `DebugInfo` section (a
    compile without `--debug-info`) or the position doesn't resolve; callers
    must gate on program-identity checksum before trusting a non-null result
    (`docs/live-inspector-spec.md` §5's `sessionDegraded`).
  - `ProgramModel`'s `KnotNode` gains `container_idx` (the container's index in
    the compiled program, matching a runtime `DebugPosition`) and its `disasm`
    changes shape from `string[]` to `{ offset, text }[]` — each decoded
    instruction now keeps the byte offset it decoded from, so a "current
    instruction" highlight in the Program Explorer has something to key on.
    This is a breaking shape change to `disasm`, gated behind the same
    `--debug-info`-independent Program Explorer feature that already ships —
    every consumer in this repo is updated in this same PR.

  `@brink-lang/studio` (bundles `studio-shell`/`studio-ui`):

  - `@brink/studio-shell` implements the `program` Location resolver
    (`makeProgramResolver`) and the `session → program` half of the chain
    (`resolveSessionPositionRef`), plus the `programIdx:offset` address
    encoding (`encodeProgramAddress`/`parseProgramAddress`).
  - The Program Explorer (`ProgramView`) highlights the currently executing
    knot and instruction, gated on `sessionDegraded` — suppressed, not stale,
    the moment the running program's checksum diverges from the studio's
    latest compile.

- df3e5b4: The playground's default demo project now ships a `brink.toml`, so it looks
  like a real project rather than relying on the host's constructor-time entry
  argument — and the Settings view has something to show.

  It declares `drafts = ["scratch/**"]`, and the demo gains
  `scratch/cut-scene.ink`: deliberately not `INCLUDE`d, so the draft treatment
  (#3145) is visible in the demo — Binder badge, draft mark beside the name,
  and no "not included in the project" banner.

- 19d913a: Diagnostic tooltips get a fixed anatomy and a width cap.

  Both producers — the compiler and the prose checker — now render through one
  shape: a severity/kind label, the message, the fix buttons on their own row,
  and the diagnostic's code as a source tag.

  - **Width is capped**, at the same 460px the hover card has always used, now
    shared through one token so the two floating explainers cannot drift apart.
    The lint tooltip previously had no cap at all, so a long message ran to a
    200-character measure and pushed the fixes out of reach.
  - **Fixes sit on their own row** with 26px targets, hover, active and
    focus-visible states. Inline, a long message pushed them toward the far
    edge, so reaching one meant crossing the whole message without leaving the
    tooltip.
  - **The label carries severity as a word as well as a colour** — the rail
    alone fails a colourblind reader and fails a screenshot pasted into an
    issue, which is how most of these get reported. Prose lints label with the
    checker's rule name (`spelling`), which says more than `info` would.
  - **The diagnostic code is shown.** It was computed and then dropped, so
    there was no way to look a diagnostic up from the tooltip.
  - `info` severity was never themed, so every prose lint inherited the error
    rail and announced a spelling suggestion in the colour reserved for "this
    will not compile".
  - Hover-card rows wrap rather than widening the card, so an `effects` row
    listing several variables no longer fights the cap.

- 0fed188: The `\` of an escape now carries its own `escape` semantic token, so the
  editor dims it while the character it protects reads as ordinary prose. An
  escape exists to say "this character is text"; the mark that says so should
  be legible when looked for and invisible when reading.

  Also fixes the same mis-highlight #3154 fixed for `.ink` on the NATIVE
  surface: an escaped `{` in a `.brink` prose line was painted as
  interpolation syntax, because the native prose carve-out
  (`is_prose_run_container`) listed `TEXT`/`CUE_NAME`/`TAG`/`SCENE_TITLE` but
  not `ESCAPE`.

  `escape` is appended to the token legend as index 18; existing indices are
  unchanged.

- b0f5ccf: Editor gutter visibility toggle: a Settings checkbox and an editor context-menu item ("Hide Gutters" / "Show Gutters") hide all editor gutters (line numbers, structure rails, fold/play markers), persisted with the other editor settings. Besides the visual preference, hiding gutters removes a WebKit per-gutter-element layout cost (#3119), roughly halving felt keystroke latency again on large projects in the desktop app — the interim escape hatch until the structural fix lands.
- fae5eb5: References in the hover card are now navigable. The cells an `effects` row
  names, and the file in _Defined in_, are links to their declarations —
  clicking one reveals it, the same route goto-definition already used.

  The card named things without letting you reach them, which made it a
  readout rather than a way to move.

  - `HoverInfo` gains `links`, and content refers to them as `[text](#N)`. An
    index rather than a path inside the link target, deliberately: a path in
    markdown has to survive `)` and `:` inside it, and that escaping is a
    silent-corruption bug waiting on the first bracket in a filename.
  - Atoms with nowhere to go stay plain text — `calls` atoms are raw external
    names with no symbol to point at, and the compiler-owned `rng` cell has no
    declaration. A link that navigates nowhere is worse than plain text.
  - An embedder that passes no navigate hook gets plain text too, the same
    rule "Add to dictionary" follows.
  - Effect atoms are now individually code-styled rather than the whole row
    being one code span, and clause labels and status words (`pure, silent,
total`) read as prose.

- 67dd310: Indent guides line up with the column they mark, and break between rows.

  The guides were painted half a character right of their column — literal in
  the upstream package, which appends `.5` to every gradient stop — so a caret
  at that indent sat left of its own guide and read as needing one more space.
  The shift is `0.5ch`, font-relative, because the editor font size is
  user-settable.

  Each row's guide is now slightly shorter than its row, leaving the small
  vertical break between rows that Inky shows.

  Two smaller fixes: Single File view remembers whether you hid the player
  (it reopened on every reload and every switch back from Code view), and the
  "not included in the project" banner can be dismissed — per file, for the
  session, since what it states can stop being true.

- 0d32184: `[project] indent` is now the single source for indentation width, and the
  default when it is unset is **4** (ruled 2026-08-27).

  - `brink-fmt` no longer keeps a default of its own — it defaulted to two
    spaces while the editor indented by four, which is exactly the
    disagreement this setting exists to prevent.
  - `brink fmt` discovers the `brink.toml` for each file it formats.
  - The language server reads the project's width and ignores the client's
    `tabSize`, which would otherwise be a silent second source.
  - The editor's `indentUnit` reads the configured width instead of
    hardcoding four spaces; the indent guides follow it automatically.

  New: `EditorSessionHandle.getConfiguredIndent()`, and `DEFAULT_INDENT` from
  `@brink-lang/editor`.

  Also: the status bar no longer says "— file not analyzed" for a draft
  (#3145), matching the out-of-scope banner it accompanies.

- 641e278: Three new selectable themes: **Manuscript** — the writing-first colorway (brightest-on-screen prose, hot-red structure markers and halt words, one tight cool machinery band ordered by conceptual distance, yellow tags, cues rendered as plain prose) — plus faithful **Inky** and **Inky Dark** ports of Inky's editor colors. Supporting hooks: `.tok-marker`/`.tok-divert`/`.tok-halt` rules with fallbacks that keep existing themes byte-identical, and theme-tunable cue styling (`--bs-cue`, `--bs-cue-weight`).
- 2c2903a: Remember the editor across a reload: open tabs and their order, pin/preview
  state, the active tab per group, the split structure and its sizes, and each
  open document's cursor and scroll. State is scoped per project — the host
  names the scope (`mountStudio`'s `sessionScope`; the desktop passes the
  project root) — so two projects keep their own layouts instead of
  overwriting one another, with a least-recently-used cap on how many are
  remembered. A project with nothing remembered still opens as the default
  two-up, and tabs naming files that no longer exist are dropped on restore.
- c5a4d5c: Navigation works in Continuous view: it scrolls. Clicking a file in the
  Binder, a search result, a Problem, or a go-to-definition now moves the
  manuscript to the target line, clear of the sticky heading, instead of doing
  nothing visible. Clicking a knot or stitch in the Binder's structure mode
  works too — those name a symbol, which this view resolves to a position
  inside the file's section rather than to a separate document it does not
  render. Re-navigating to somewhere in the file you are already in scrolls as
  well.
- 7a6560a: Performance instrumentation ships in all builds (prod-perf ruling 2026-08-25): the probe, browser observers, `__brinkPerf` harvesting global, and the Performance tool window are no longer dev-only — `mountStudio` enables them by default and `perf: false` (or the playground's `?perf=0`) strips the whole surface. The session worker now runs its own probe and wasm counters, reported through new host-level queries (`hostPerfReport` / `hostPerfReset` / `hostPerfSetEnabled` — answered by the hosting realm, never the session facade), and the HUD grows worker-plane and wasm-counter sections plus a combined JSON export; since W5 the analysis cost lives in the worker, so a main-thread-only panel could not see it. The probe's User Timing mirror now periodically clears its own entries (only its own — an embedding page's timeline is untouched), bounding an always-on session's growth. Perf payloads remain structurally content-free: static span/counter names and numbers only, nothing from the author's project.
- 029dae2: Prose checking: spelling and light grammar over a manuscript's prose.

  The engine is Harper, in its own lazily-loaded wasm module — 6.5 MB gzipped,
  larger than the entire compiler, so it is never in the main bundle and an
  embedder that registers no checker pays nothing.

  What makes it usable on fiction rather than hostile to it: the checker only
  ever sees `content` spans with interpolations subtracted (never diverts,
  tags, or logic), and its dictionary is seeded from the project's own names —
  including the character cues, so writing the manuscript teaches it. Without
  that, every invented name reports as a misspelling.

  `@brink-lang/web` gains `getProseDictionary`, `getConfiguredProseDialect`
  and `getConfiguredProseEnable`. `@brink-lang/editor` gains the `ProseChecker`
  seam and a shared diagnostic-source registry, so the compile and the prose
  check no longer overwrite each other's squiggles. `@brink-lang/studio` gains
  the Prose settings section and registers the checker.

- c3ebae8: The author's prose dictionary now lives in `brink.toml`, under `[prose]
dictionary`, and is visible and editable in Project → Prose.

  It previously went to a `.brink-dictionary` sidecar with no UI anywhere, so
  "Add to dictionary" wrote a file nothing displayed — the word stayed
  underlined until the next compile and there was no way to see the list or
  undo an entry. The settings panel now shows the words, adds and removes
  them, and the editor action writes to the same place.

  Matching is literal: `Griswold` and `GRISWOLD` are two separate entries.

  Package-level notes:

  - `@brink-lang/web` gains `EditorSession.getConfiguredProseDictionary()`,
    reading `[prose] dictionary` from the applied config. Like the other
    `configured*` readers it is wholesale-replaced on every apply, so a word
    removed from the file stops being a known word.
  - `@brink-lang/editor` gains a `onAddToDictionary` document-session option
    and no longer owns dictionary storage: the list is the embedder's
    `brink.toml`, so the editor package no longer writes it. The
    `PROSE_DICTIONARY_FILE` export is removed. An embedder that does not pass
    `onAddToDictionary` no longer sees the "Add to dictionary" action at all,
    rather than seeing one that silently does nothing.

- ab5efa5: Spelling and grammar findings now appear in the Problems panel, behind a
  filter toggle that is **off by default**.

  This completes behaviour that was specified when prose checking was first
  scoped — results "render as squiggles and are listable, but the Problems
  panel filters them out by default; the author opts in to seeing them in the
  list". Only the squiggles half had shipped, so a typo was visible in the
  buffer and findable nowhere else.

  - A fourth filter bucket, `prose`, sits beside error/warning/info. It is a
    SOURCE rather than a severity, which is what lets it default off while
    every severity defaults on — folding spelling into `info` would bury the
    E189 TODO notes an author actually reads.
  - Prose findings are stored separately from compile diagnostics and joined
    for display. The two producers have different lifetimes — a compile
    replaces its whole set at once, prose lints arrive per view on their own
    debounce — so one list would mean each erasing the other's rows.
  - A prose row's context menu offers **Prose settings…** rather than
    "Configure <code>…", which would have opened the Diagnostics section and
    offered nothing about it.

  An existing author's stored preferences have no `prose` key, and it reads as
  off: the severity rule ("only an explicit false hides it") is deliberately
  inverted for this bucket, so upgrading never switches spelling rows on.

  `@brink-lang/editor` gains an `onProseLints` document-session callback
  reporting findings per file, fired from the same guarded point as the
  squiggles so a host list can never hold rows the editor has cleared.

- 3cb34b7: Quick open (⌘P) no longer lists symbols from mounted `std/` library files or
  from `brink.toml` — the same set the Binder tree and Continuous view show,
  since those aren't places you navigate to while writing. Symbol entries are
  also keyed by span, so two knots declaring the same stitch name can't collide
  on one React key (which silently dropped or duplicated rows).
- fee52b2: Revealing a location now opens the file as the editor group's **preview**
  tab instead of a pinned one. `editor.reveal` is the shared destination of
  every navigation surface — search results, Problems, TODOs, Find
  References, cross-file go-to-definition — so each jump used to mint a
  permanent tab and a few minutes of browsing buried the tab strip. The next
  reveal now replaces the preview in place; editing it (or double-clicking
  the tab) pins it, and revealing into a file that is already pinned leaves
  it pinned.
- fe9ab69: Scrollbars are styled to blend with the theme instead of using the loud
  platform default: no track, a thin rounded thumb tinted from the theme's
  muted foreground (so all five themes, light and dark, get a correct thumb),
  darkening on hover and drag. Applies to every scrollable surface under the
  studio root — the editor, tool windows, the binder, the search results —
  and is overridable per theme via `--bs-scrollbar-thumb`,
  `--bs-scrollbar-thumb-hover`, and `--bs-scrollbar-thumb-active`.
- f96e4a8: Add Single File view, the first of the three editor views. The editor root
  area now holds one occupant: Code view (today's tabs and splits) or Single
  File view, which shows one file with the player beside it and no tab strip at
  all. Navigating — from the Binder, search, Problems, go-to-definition —
  replaces what is on screen instead of accumulating tabs, and the player split
  belongs to the view rather than being a document that happens to be open, so
  it collapses and returns but never closes into an empty pane. The two views
  share the active file, so switching keeps the document you were working on,
  and the chosen view persists with the rest of the layout. Switch with the
  "View: Single File" and "View: Code" commands.
- e82a275: The State View shows each call frame's local variables.

  Function parameters and `~ temp`s now appear under the frame that owns
  them, with their live values — so a function that computes with locals is
  no longer opaque exactly while it is the thing running.

  Values render structurally rather than as display strings: a list shows its
  members, a struct shows its fields, and an empty list is distinguishable
  from a null. A frame from a story built without debug info says so, rather
  than showing an empty panel that would read as "this function has no
  locals".

- 6edcf72: Add Continuous view, the third editor view: every file in the project stacked
  in binder order as one manuscript, with a heading between each and a single
  scroller carrying you across file boundaries. Files are stacked as separate
  editors rather than concatenated into one document, so each keeps its own
  wasm document handle and diagnostics, tokens and completion stay per-file and
  correct. Order comes from the same `.binder.json` sidecar the Binder tree
  uses, so the manuscript reads in exactly the order the Binder shows.
  Selectable from Settings or the "View: Continuous" command.
- 31303b5: Desktop update offers arrive as an actionable toast instead of a blocking
  modal dialog: a sticky notification with "Install and Restart" and "Later",
  which amends itself in place while downloading and reports a failure with a
  "Try Again" action. The landing screen — which has no studio surface yet —
  keeps the native dialog.
- b0f5ccf: Rails-gutter WebKit layout fix: the percent-height inline-flex rail marker made every forced layout cost ~1 ms per visible marker in WebKit (~110 ms per keystroke-burst refresh on a real project — the dominant slice of desktop typing latency; Chromium was unaffected). Markers now use an in-flow fixed-width spacer plus an absolutely-positioned bar layer — same visuals, measured 120 ms → 36 ms full-layout and ~3x lower felt keystroke latency under WebKit. Also: `cm.dispatch`/`cm.dispatch.state`/`cm.dispatch.view` perf spans on the main editor view, `__brinkPerf.report(worstCount)`, and the playground's `?fixtureUrl=` loader for measuring real-project shapes without baking content into the repo.
- Updated dependencies [40e941a]
- Updated dependencies [0b07df5]
- Updated dependencies [b43ebbc]
- Updated dependencies [e4a20b3]
- Updated dependencies [132a3a4]
- Updated dependencies [76bbdeb]
- Updated dependencies [5079c84]
- Updated dependencies [b0f5ccf]
- Updated dependencies [953daff]
- Updated dependencies [0fed188]
- Updated dependencies [cf2d5a4]
- Updated dependencies [237fd39]
- Updated dependencies [42efdf1]
- Updated dependencies [87521b2]
- Updated dependencies [fae5eb5]
- Updated dependencies [0d32184]
- Updated dependencies [cfa5738]
- Updated dependencies [a260c8c]
- Updated dependencies [736061f]
- Updated dependencies [029dae2]
- Updated dependencies [c3ebae8]
- Updated dependencies [b6d2af7]
- Updated dependencies [ef99ec9]
- Updated dependencies [641e278]
  - @brink-lang/web@0.17.0

## 0.16.0

### Minor Changes

- 7bf7844: Binder v2, part 1 (#3036, #3037): Files/Structure mode toggle — symbol
  rows (knots/stitches/functions) now render only in Structure mode;
  files-only is the default, cutting the always-on tree-of-trees noise. A
  header toolbar carries the segmented icon toggle plus expand-all /
  collapse-all. Every glyph-character icon (📄 📁 ◆ ◇ ƒ 📚 ▶) is replaced
  by a monochrome currentColor SVG set (the brink droplet for .ink files),
  and draggable rows reveal a grab handle on hover. All structural ops are
  unchanged in Structure mode.
- 41574db: Binder v2, part 3 (#3039): inline creation. Every container (and the
  binder foot for the root) carries a 50/50 pair of dashed icon buttons —
  new file / new folder — expanding in place to a full-width name input
  (bare name, .ink implied for files) with inline validation (no paths,
  duplicate check normalizing the extension). Folder creation goes through
  the order sidecar's empty-folder registry, so a new folder renders
  immediately and survives reloads — in-app folder creation exists at
  last. The folder context menu gains "New folder here"; "New file here"
  now opens that folder's own input (container implied, no seeded path
  prefix). The caret discipline of the old New File input (#2571) carries
  over unchanged.
- 7b148c7: Binder v2, part 4 (#3041, #3042): diagnostics marks and the pinned
  config row. File rows carry error/warning counts (a file sums its
  diagnostics — the roll-up rule; a knot/stitch shows its own, computed
  from diagnostic spans against the symbol's body; Info/Hint never mark),
  and brink.toml leaves the file tree for a dedicated pinned row above the
  binder foot — gear icon, monospace name, click opens it (where the form
  view renders).
- 3db2eaf: Binder v2, part 2 (#3038): the `.binder.json` order sidecar — placement
  is authorship. Per-container display order (files and folders
  interleave; the fallback is entry first, folders before files, then
  alphabetical), drag-to-reorder for files and folders within their
  container (folders reorder-only; a file's drop-into-folder move is
  unchanged), an empty-folder registry so in-app folder creation can
  render before any file exists, re-keying on rename/move and cleanup on
  delete, and subtle indent guides in the tree. The sidecar is loaded and
  written through the host FileProvider; it never enters the wasm session,
  a corrupt file self-heals to the fallback, and hosts without persistence
  keep working in-memory.
- 779059e: Binder scope marks (#3014, #3021): the entry file carries an `entry`
  badge; a source file outside the compile closure renders dimmed with a
  `not included` badge (on disk, not in the story). The Library section (mounted stdlib) is hidden
  entirely for ink projects, where the compiler provably excludes the
  mounted `.brink` stdlib from every compile closure — it stays for
  native entries and before the first compile.
- a4eb3fb: Binder v2, part 5 (#3040): search. A header toggle opens a filter row —
  one case-insensitive query over file names and structural names
  (knots/stitches/functions). Matches keep their file context, a stitch
  match survives as its knot's context, matching symbols reveal in BOTH
  modes (Files mode included), the collapsed state is ignored while
  searching, and Escape/× clears. The #tag namespace from the design is
  deliberately deferred: the tag data does not exist at any layer yet —
  #474 owns wiring per-flow tags through HIR → format → the wasm boundary,
  and the binder search grows the third namespace when it lands.
- c2c05f0: Structured form view for brink.toml (#3015): opening a brink.toml now
  renders a form panel above the raw text editor — entry and conventions
  offer the project's actual files (a typo'd entry reproduces the
  silent-dead-Player failure, which is why free text was the problem),
  dialect and types offer the schema's values, and a configured value
  naming a missing file is flagged "(missing)" rather than rewritten.
  Edits are comment-preserving targeted line operations, never a
  parse-and-reserialize; the text editor below remains the escape hatch
  for anything the form doesn't model (e.g. [lints]).
- 8bd2fcb: Out-of-scope editor banner (#3017): the compile closure is now surfaced
  through the wasm boundary — `EditorSession.compilation_closure()` /
  `EditorSessionHandle.getCompilationClosure()` return the project-relative
  paths of the exact file set the latest compile built from (empty before
  any compile; read-only). The studio renders a banner above the editor of
  any source file outside that closure ("Not included in the project —
  nothing INCLUDEs this file, so it is not analyzed"), with a one-click
  "Add INCLUDE to <entry>" action for the ink flow, plus a "— file not
  analyzed" status-bar note. Absent diagnostics look identical to clean
  diagnostics; this makes the difference visible.
- bc3b08a: File-anchored project open model (epic #3021, ruled 2026-08-23): new
  `entryIsExplicit` option on `ProjectSessionOptions` and
  `MountStudioOptions`. When set, a discovered `brink.toml`'s
  `[project] entry` never supersedes the host-given `entryFile` — the
  #2331 precedence ("`[project] entry` beats `mountStudio`'s `entryFile`")
  stands for host-supplied defaults, but a human's explicit file open is
  not a default. Config discovery itself still runs (lints, conventions,
  warnings all apply). Default `false`, the pre-existing behavior.
- f1b7c76: Literal-whitespace editor presentation (ruled 2026-08-23): the editor no
  longer imposes layout of its own. Removed: standalone-divert
  right-align, the weave-depth artificial indent and its superscript
  depth-sigil collapse (nested `* *` sigil runs now render as typed), the
  screenplay character/parenthetical/dialogue indents and dialogue column
  width, CHARACTER uppercase, and the 8.5in page cap/margins. Colors and
  highlighting are unchanged, and the classification taxonomy (element
  classes, `data-depth`, `brink-divert-standalone`) remains the host
  contract — an embedder that wants a styled layout adds its own CSS over
  those hooks. New: whitespace/tab indent guides
  (@replit/codemirror-indentation-markers), themed from the `--bs-*`
  tokens, spaced at the editor's 4-column indent unit; default on,
  `indentGuides: false` to opt out. New: hanging indent for soft-wrapped
  lines — continuation rows align even with the first row's text start
  (not flush-left, not Inky's extra padding), carried by a `--line-indent`
  custom property per line.
- c5193ad: Performance probe + dev-only HUD (measure-first ruling, 2026-08-24).
  `@brink-lang/editor` gains a perf module — `setPerfEnabled`/`perfSpan`/
  `perfTime`/`perfReport` over a preallocated ring buffer, every span also
  emitted as a `performance.measure` so DevTools recordings show named bars —
  plus browser observers (long tasks, event-timing input latency, long
  frames), a CM6 viewport/scroll probe (`cm.viewportLag`), a wasm-boundary
  Proxy timing every session call (`wasm.<method>`), and spans at the hot
  extension sites (element-type, highlight, HIR overlay + rails gutter,
  inlay hints, folding, screenplay passes, argument widgets, hanging indent,
  inline markup, the debounced compile cycle, project initialize). The studio
  wires the dev edge (`import.meta.env.DEV`): store-write sweep timing
  (`store.set.<field>`), compile fan-out spans, startup marks, a React
  commit profiler, and a "Performance" tool window (aggregates, worst
  events, marks, Copy JSON). Everything is inert single branches when
  disabled — production builds neither collect nor register the HUD.
- e680f17: Player advances one line per reveal, with an "auto" toggle for run-to-pause

  `LocalSessionProvider.reveal()` called `continueToPause()` unconditionally, so
  a single Continue press dumped every line up to the next choice. Its own doc
  comment said it revealed "the next line" — the comment described the intent and
  the code did something else.

  All three reveal paths (initial load, after a choice, Continue) now advance a
  single line. A new `auto` capability + `setAuto()` switches them to
  run-to-next-pause, surfaced as an unchecked-by-default checkbox in the Player
  toolbar. `SessionSnapshot` gains `auto` so the control reflects provider state
  rather than a separate copy that can drift.

  Flow sessions honour the toggle too, via `continueFlowMaximally`.

- 9985bcf: References dressing (card-stack PR E). New wasm entry point
  `find_references_with_kinds_at` (wrapper:
  `EditorSessionHandle.findReferencesWithKindsAt`): every reference site
  classified by how it uses the symbol — `decl`, `call` (UFCS-desugared
  calls included), `divert`, `read`, or `write` (assignment targets and
  `++`/`--`). In the Search panel's references mode, the declaration card
  pins first with an accent border and `decl` badge, and every site
  carries its kind badge; the store re-resolves through the kinds variant
  at the declaration anchor (plain locations remain the graceful
  fallback).
- 3a44fe6: Search replace previews (card-stack PR D). With the replace row open,
  every still-matching card renders a display-only old→new preview — the
  previews ARE the confirmation; the arm/confirm step is gone. Per-card
  Accept applies one replacement (the card keeps its row with a
  "✓ replaced" receipt — frozen snapshot); per-card skip excludes it from
  Accept all (undo available); the summary strip counts pending/stale/
  skipped/replaced and carries Accept all (N). Excluded matches are
  per-match (skipped, edited-stale, or failing the live-text guard),
  badged with why — never a global abort. The old results-buffer view is
  removed from the studio; `@brink-lang/editor`'s `SearchResultsBuffer`
  class is deprecated but stays exported for external embedders. Card
  chevrons now reuse the fold gutter's glyph in a proper hit target, and
  the reveal arrow matches its slot.
- ba67f95: Search result cards (card-stack PR C). The Search panel's results render
  as one card per match, in both text-search and references mode: a header
  row (file:line, containing knot/stitch, `edited` badge, reveal ↗) over
  the match's own small editable buffer — the match line plus a tunable
  context window (default 1 above / 2 below), fully syntax-highlighted via
  a per-file semantic-token cache. Cards collapse to a header preview
  (per-card chevron, plus the binder-style expand/collapse-all buttons in
  the summary row alongside the context knob and the snapshot ↻). The
  list is virtualized: off-screen cards render as static HTML instead of
  live editors. Card edits write through to the source and never remove
  rows — the frozen snapshot flags them instead.

  Also fixes cmd/ctrl-click goto/references from a real pointer: the
  handler now binds mousedown (CodeMirror's own cmd-mousedown multi-cursor
  preventDefault suppressed the browser click event a click-bound handler
  was waiting for). And cmd/ctrl-clicking the file path in an INCLUDE
  statement now opens that file.

- e4de0cc: Frozen search snapshot model (search-result cards, PR B). The Search
  panel's result set is now a snapshot: edits never remove or re-filter
  rows. Match spans are edit-mapped through document changes (driven by
  the compile seam), flagging rows `edited`/`stale` instead of dropping
  them; only a new search or the explicit refresh replaces the set. The
  store gains the context-lines setting (default 1 above / 2 below), the
  per-card collapse map, and `refreshSearchSnapshot()` — query snapshots
  re-run their frozen query, references snapshots re-resolve from the
  edit-mapped declaration anchor. The editor's Find References surfaces
  (`onShowReferences`) now pass the symbol's declaration location as an
  anchor when goto-definition resolves one.

### Patch Changes

- 4302f46: Adaptive deferral for advisory paint (#3064 C2): in documents of 1,000+ lines, the HIR overlay and inlay hints map their decorations through each edit (positions stay exact) and rebuild content once the document has been quiet for ~120 ms — a typing burst pays one rebuild at its end instead of one per keystroke. Documents under the threshold rebuild synchronously exactly as before, so small-file behavior is byte-identical.
- 9bf177e: Context-menu matrix, identity rows: right-clicking any identity-bearing token — divert targets, VAR/CONST/temp/param references, list items, labels, EXTERNAL calls, including refs inside `{interpolations}` — adds a Navigate/Rename group above the text group: Go to Definition (the ⌘-click path), Find References (the ⇧⌥F highlight), and Rename '<name>'… opening the inline-rename UI with its breakage report. The identity test is "goto-definition resolves here", so exactly the tokens with definitions get the group; the actions reuse the same callbacks as their keyboard/mouse counterparts (`navigateToLocation` and `showReferencesAt` extracted as shared entry points). The inline-rename input now hugs its content — the `size` attribute tracks the value, replacing the browser's ~20-char default that rendered as a line-breaking slab — and grows live as you type. The rename UI itself is redesigned as a floating badge below the token (the Zed/JetBrains shape): the symbol stays in the document with a highlight mark while the input floats in a `showTooltip` — which also fixes Escape appearing to delete the token (the old design REPLACED the token with the widget, so its text was hostage to widget lifecycle; now the rename is an inserted editor row and the document is untouched by construction). The row is a block widget beneath the target's line — no gutter number, lines pushed down — with the input rendered as bare inline text: the token's own `tok-*` highlight classes copied onto it, exact column alignment via a hidden spacer carrying the line's real prefix text, no chrome and no focus ring. Escape cancels, and moving the editor cursor off the target's line also dismisses without committing. Structural rows land too: INCLUDE lines offer Open <file>, foldable lines offer Fold/Unfold (through the registered fold service), TODO lines offer Show in TODOs Panel, and Rename is gated per-token by the same prepareRename query F2 uses — externals (whose names are the host-binding contract) get Navigate items with no dead Rename item.
- 8bdb676: The editor context menu is now always ours (docs/editor-context-menu-spec.md, phase 1): right-click anywhere in the editor suppresses the native menu — knot/stitch headers open the shared symbol menu (including **function** headers, whose clicks previously vanished: `headerName` treated the `function` keyword as part of the path), and everything else opens a text menu (Cut / Copy / Paste / Select All with shortcuts, Cut/Copy disabled without a selection) whose actions are bound to the raising view. New `onTextContextMenu` option threads through `brinkStudio`/`DocumentSessions`; the studio renders it via a new `EditorTextMenuHost` sharing the symbol menu's chrome and dismiss contract (`useContextMenuDismiss`, extracted).
- fe696cf: Editor gutter polish: the fold gutter renders one fixed-slot SVG chevron for both states (collapsed = the same glyph rotated by CSS, so the marker never shifts; open-fold chevrons appear only while the pointer is over the gutter, collapsed markers stay visible and accented) via `brinkBasicSetup`, a drop-in copy of `basicSetup` with the brink fold gutter. The play-from-here ▶ is a centered SVG triangle with a hover pill instead of a font glyph. Structure rails show a real floating tooltip on hover — the container's name (bare knot/stitch name, choice/gather text) plus kind and line range (e.g. "Knot · lines 25–31"). Fold and play markers top-align with the first visual row of wrapped lines, matching the line numbers. The shell's corner menu button is an SVG aligned to the strip's icon axis.
- 1609068: Keystroke micro-work toward the 8 ms frame budget (#3064): a config epoch invalidates delta-slice caches on dialect/host-manifest swaps (fixing a stale-classification bug under unchanged segment keys); one manifest fetch per document version; element-type derives per-line infos per segment under the delta protocol's version keys; the keystroke path serves the edited knot's semantic tokens from a classifier-only slice (no analysis pull — the symbol index and resolution passes leave the synchronous path entirely) with resolution-refined colors landing on the deferred refresh; occurrence highlights defer during large-document typing bursts (selection moves stay instant). Per-keystroke instrumented work on a 6k-line document drops to ~6–7 ms, with most keystrokes completing below the Event Timing API's 16 ms reporting floor.
- ee6f0e4: The one-shot analysis family rides the worker road (#3110, closing the last main-thread analysis paths outside documented fallbacks): goto-definition and find-references become async sources (the cmd-click gesture is claimed immediately and lands on resolution — with CM's multi-cursor emulated when nothing resolves), the inline-rename family resolves through the client (`startInlineRename` pre-resolves the target via a resolver facet and dispatches on landing; the live breakage badge lands through `InlineNameInput`'s existing pending machinery; the context menu's identity/rename gating resolves before the menu opens), symbol-tab ranges resolve hint-first with an async worker verify that restores a degraded fragment at its fresh offsets, and search-card highlighting fetches asynchronously (cards render unhighlighted and colorize on landing). The main-thread analysis boundary guard's allowlist shrinks to the choke-point fallbacks only.
- bd2e490: Two-range model for container spans (#3054): `HirSpan` gains `content_end_line` — the TIGHT end (last line of actual content, trailing whitespace and the next declaration's doc block excluded) alongside the structural `end_line` that runs to the next sibling. Rails and their tooltips use the tight range, so a two-line function no longer paints (or reports) itself through the next function's docs; choice rails get eight golden-step color buckets so siblings are distinct; conditional-branch tooltips show the condition.
- 79277a6: Find References works — and presents through the Search panel (the spec's open question, now ruled): the menu item and ⇧⌥F route results into the search results surface, grouped by file with line previews, cross-file included, click-to-reveal and inline-editable like text-search results. A references-mode chip names the symbol and count; typing a query returns the panel to text search. (The old in-view 3s highlight painted raw cross-file offsets into the current document — broken by design; it remains only as a fallback for hosts that wire no references surface.)
- 4ee9d9b: The editor/story-graph symbol context menu (and the rename modal) now actually appear: both hosts rendered OUTSIDE the `.brink-studio` root, where the scoped `position: fixed` styles and theme tokens never applied — the menu landed unstyled at the end of the document (the "right-click eats it / a scrollbar flashes" bug). `App` now accepts children inside the root and the popup hosts mount there.
- 37f54ea: Tab indents and Shift-Tab dedents (by the 4-space indent unit), like every other editor. The built-in Tab/Shift-Tab line-conversion cycle (choice→body→gather→choice, character→parenthetical→dialogue, the double-blank `@:<>` template) is stripped for now (ruled 2026-08-24) — previously the keys were swallowed even where no conversion applied. Dialect-DECLARED transition rows keep first claim on Tab (#395 consumer contract; the default at-cue preset declares none). Enter/Shift-Enter transitions are untouched.
- 6ab99d2: The TODO band survives the caret: with the cursor on a TODO line, `.cm-activeLine`'s background beat the band and left the dark ink invisible (0.2.0 regression). A highest-specificity restate keeps the goldenrod band under the active line.
- edd0db5: TODO author notes are now visibly highlighted in the editor (#3050). Lines opening with the `TODO` keyword (colon optional, matching the parser's `AUTHOR_WARNING` rule) classify as the new `todo` element kind and carry the `brink-todo` line class; the opening keyword gets a `brink-todo-keyword` mark. The studio styles the class as a full-width amber band with a left bar, forcing syntax-token colors to the amber inside the note so the line reads as one called-out unit (`--bs-todo`/`--bs-todo-rgb` override per theme, falling back to the warning family). The `E189` squiggle is suppressed in the editor — the band is that diagnostic's in-editor presentation — and Info/Hint diagnostics now map to CodeMirror's `info` severity instead of rendering as warning squiggles.
- 6559596: New TODOs tool window (#3050): ink `TODO:` author notes, grouped by file → containing knot/stitch, with filter, click-to-navigate (rows and group headers), an amber strip badge, and a strikethrough exit animation when a note is removed from source. TODOs also appear in Problems as Info rows (new ℹ glyph); Info/Hint diagnostics no longer inflate the warning count.
- b0e3a91: Panel pulls ride the async session facade (editor worker architecture W2d, `docs/editor-worker-spec.md`): the compile fan-out's three panel queries — project outline, compilation closure, story graph — run through the `SessionClient` at background priority with per-panel coalesce keys, and the store fan-out lands when they resolve, in the same relative order as before. A newer compile's fan-out supersedes an in-flight one wholesale (whole-project staleness class), and a dropped or failed pull keeps the last good panels.
- b0e3a91: Structural computes ride the async session facade (editor worker architecture W2e, `docs/editor-worker-spec.md`): `ProjectSession` gains `structuralQuery` — an interactive-priority client query for the compute-only structural ops (`renameFile`, `renameDir`, `moveStitch`, `promoteStitch`, `demoteKnot`, `renameSymbol`, `renameSymbolAt`; they return new sources + a breakage report and mutate nothing, so query semantics fit exactly) — and the studio's gated-op runner awaits it, mapping a destroy-during-queue cancellation to the same swallowed result as a destroy-during-defer. The `ProjectSession` file-lifecycle mutations deliberately stay synchronous until the transport flips (sync reads couple to them; recorded in the spec). The paint-path enrolment guard now matches both the facade call shape and the raw wasm call shape, so a raw gated call reappearing anywhere still fails it.
- 46a74b3: The session worker (editor worker architecture W4, `docs/editor-worker-spec.md` §8): `WorkerTransport` + a session worker entry running `SessionHostCore` — the exact host semantics `LocalTransport` runs, extracted and shared so the two transports cannot drift — with a boot handshake and crash fallback. `ProjectSession` gains `projectQuery`: the project-level pulls (compile, outline, story graph, closure) run in the worker's own wasm session, kept current by an ordered file/config mutation stream flushed before every worker query; `triggerCompile` (the last synchronous compile caller) rides the async facade. Opt-in via `MountStudioOptions.workerSession` (the playground's `?worker=1`); fully feature-detected — environments without workers, boot failures, and crashes all keep the in-process road. In worker mode the main thread records zero compile time: the whole-project compile (up to ~1.8 s cold on studio-scale projects) leaves the UI thread entirely.
- a844808: The worker road is on by default (editor worker architecture W5 flip, decision log 2026-08-25): `mountStudio` defaults `workerSession` to true, so the project-level pulls — compile, outline, story graph, closure — run off the main thread in every studio and desktop embedding without opt-in. Fully feature-detected: environments without Web Workers (or where the worker fails to boot) silently keep the in-process road. Pass `workerSession: false` (or `?worker=0` in the playground) to force in-process.
- 69586b3: Worker architecture W5c close-out (`docs/editor-worker-spec.md` §14): the deferred-refresh rebuilds read **worker-fed stashes** instead of pulling analysis on the main thread — `DocHandle` gains per-surface stashes (projection, hints, widgets, folds; dirty-bit guarded so a stash is never served across an edit it predates) and a refined-token worker plane (`refreshRefined`: replica manifest + changed slices only, assembled synchronously at rebuild time); the compile-delivery overlay refresh fetches its projection first and dispatches on landing. Desktop export awaits the async compile landing (fixing the W4-era regression where it read story bytes synchronously after `compile.run`). A new lexical boundary guard pins every surviving main-thread analysis call to a documented allowlist — the one-shot family (goto/rename/symbols/search-cards) stays main-side at incremental cost, tracked by #3110. The synchronous session survives as content store + the in-process fallback road (decision log 2026-08-25).
- 1202806: Web dependency sweep (rides the desktop-perf measure-first work): vite
  6.4 → 8.2 and @vitejs/plugin-react 4.7 → 6.1 across the workspace's dev
  servers/builds, Playwright 1.58 → 1.62, vitest/@types/node current, and
  current minors for the runtime dependencies the published bundles carry —
  the CodeMirror 6 packages (state 6.7, view 6.43, language/lint/search/
  commands/autocomplete), zustand 5.0.15, @floating-ui/react-dom,
  @xyflow/react, @dagrejs/dagre, @fontsource/jetbrains-mono,
  react-resizable-panels, and the react 19.2.x patch line. No API changes;
  the perf scenario suite was re-recorded on the new toolchain and compared
  against the pre-sweep baseline (docs/desktop-perf-baseline.md).
  Deliberately NOT taken: TypeScript 7 and @changesets/cli 3 (majors held
  for their own decisions).
- Updated dependencies [eba0faa]
- Updated dependencies [8bd2fcb]
- Updated dependencies [c0f357b]
- Updated dependencies [d8cfbcd]
- Updated dependencies [1609068]
- Updated dependencies [29541b3]
- Updated dependencies [78e4dd0]
- Updated dependencies [80dd24f]
- Updated dependencies [85a1700]
- Updated dependencies [bd2e490]
- Updated dependencies [9985bcf]
- Updated dependencies [3c8d180]
- Updated dependencies [d043c59]
- Updated dependencies [62fdee9]
- Updated dependencies [c0d9a61]
- Updated dependencies [bfdde5e]
  - @brink-lang/web@0.16.0

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
