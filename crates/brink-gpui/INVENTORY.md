# brink-gpui — surface inventory

**Written:** 2026-09-05 · **Last trued up:** 2026-09-06 (the Player and the
Program Explorer) · **Branch:** `bronch/gpui-native-desktop-spike-f7a90c`

What has been built in the native studio so far, and — surface by surface —
what each one **leaves out**. `HANDOFF.md` says how to work here and
`docs/gpui-studio-spec.md` says how it is designed; this file is only the
ledger of gaps, so a slice can be picked from it without re-reading every
module doc. The comparison baseline is the web studio
(`docs/studio-shell-spec.md` §4 is its surface list) — by the 2026-09-04
ruling the native app is the destination and must eventually cover all of
it.

**Keep it current.** A slice that closes an item strikes it here in the same
commit; a slice that consciously leaves something out adds it. The module
docs (`//! ## Not ported yet`, `//! ## Held back`, "Deliberately skipped")
remain the primary record — this file should agree with them, and where it
does not, the module doc wins and this file is wrong.

Legend: **ruled** = a maintainer decision exists (`docs/decision-log.md`);
**open ruling** = blocked on a decision an agent must not make;
**parity gap** = the web studio has it and nothing stops a port;
**worker query** = `brink-ide` has the concept and the worker does not yet
expose it (a `QueryKind` plus the UI); **engine gap** = the shared layer
lacks the concept, so the port needs engine work first (and by the layering
ruling, that work goes below `IdeSession`, never into `app/`).

## 0. Priority — `.ink` first

The maintainer's standing priority (2026-09-05): **the `.ink`-specific gaps
matter most; the `.brink`-specific ones less.** The native studio's first
job is to be the place the maintainer writes strict ink. So when picking a
slice from this list: an item tagged **`.brink` only** is lower priority
than anything untagged; every untagged item applies to an `.ink` author
(most of the editor and the tool windows are surface-neutral — conventions
included, since a `[dialogue]` dialect runs over `.ink` sources too).

In `.ink`-author order, the gaps that bite first:

1. ~~**The editor's navigation** — go-to-definition, references, rename,
   folding.~~ **Built 2026-09-05** (`app/src/navigation.rs`,
   `app/src/rename.rs`): F12 / Cmd-click, Shift-F12 into Search cards with
   kind badges, F2 with the ruled breakage report and Force, in both Code
   and Continuous views; Cmd-click / F12 on an `INCLUDE` opens the file.
   Residue: the toolkit exposes no Fold All / Unfold All.
2. ~~**The Player** and everything session-bound behind it.~~ **Built
   2026-09-05** (`model/src/play.rs`, `app/src/player.rs`): a compile +
   runtime session on the worker (`Request::Play`), a Code-view tab with
   the transcript, tags, live choice buttons, Restart / From start, the
   stale-sources status, compile-failure and runtime-error rows, and lines
   that open their source; `Play` (cmd-r), `Restart` (cmd-shift-r), and
   the Binder's "Play from here" on knots and stitches. Residue: only the
   Code-view placement (the other two views' placement is still the open
   ruling), no execution highlight, no wake for `await` parks, no number
   keys for choices.
3. ~~**Fixes** — code actions in the editor, Fix buttons in Problems.~~
   **Built 2026-09-05** (`model/src/fixes.rs`, `app/src/fixes.rs`): the
   `cmd-.` menu (fixes every tier + whole-source refactors), Problems' per-row
   **Fix** and header **Fix all safe (N)**, palette "Fix All Safe in
   File/Project". Residue: no fix-on-save (§6.2's app ceiling setting is
   not in the native studio), the structural moves (promote/demote/move
   stitch) stay off the menu until they get the breakage gate, and the
   context-menu fix entries the web has.
4. **Find/replace inside a document**, and Search's Replace.
5. ~~**Quick-open**~~ **built 2026-09-06** (`app/src/quick_open.rs`,
   `cmd-p`) — files and every knot/stitch, fuzzy-ranked, Enter revealing
   the declaration. `Escape` back to the editor is still open, and is
   **not** the small binding it looks like: the command registry models a
   keystroke but not a key CONTEXT, so a global `Escape` would compete
   with every overlay's own dismissal (the palette, the find panel, the
   `cmd-.` menu). It needs a context on the binding first.
6. **Layout persistence** and an open-project dialog.

**Suggested next order (2026-09-06)**, cheapest-first against what the
worker now holds:

1. ~~**Compiled Output** and **Output / compile log**.~~ **Built
   2026-09-06** (`model/src/compiled.rs`, `app/src/compiled_output.rs`,
   `app/src/output_log.rs`). Residue: the log has no severity filter or
   copy-all, no row opens anything, and nothing but the project and the
   Player writes to it; Compiled Output has no find-in-dump of its own and
   no jump from a dump row to its source.
2. ~~**An `.inkt` highlighter.**~~ **Built 2026-09-06**
   (`app/src/inkt_highlight.rs`): a hand-written lexer over the token
   shapes `inkt.pest` itself defines — head words, `$def_id`s, strings
   with escapes, integers/floats/`0x`, `:type`, `key=` attributes, `->`
   and `+`, and `;` comments for Compiled Output's own error text.
   Residue: the Program Explorer's Disasm view still draws its own rows
   and does not share it.
3. **State View** (the debugger) — what the Program Explorer's executing-
   instruction overlay and `stepi` are both waiting on, with the Player's
   session as the base. The engine work (exposing state off a running
   `Story`) goes below `IdeSession`, per the layering ruling.
4. **Story Graph** — the largest remaining piece: a story-graph query in
   the worker plus a pan/zoom canvas.

## 1. Surfaces built, and what each leaves out

### The frame — rails, docks, status bar (`shell/src/workspace.rs`, `rail.rs`, `region.rs`, `tool_window.rs`)

Built: the ruled rail→dock mapping, two rails with start/end groups, tab
groups per dock (`TabSlot`), badges with tones, the status bar's left cells
(root, file count, problems → opens the dock, analyze/worst timings).

| Left out | Kind | Note |
|---|---|---|
| Layout persistence | parity gap | `DockAreaState::dump/load` and `RailSlot::persistence_key` exist; nothing calls them (HANDOFF "Known broken" 5). |
| Strip drag to re-dock | parity gap | studio-shell-spec §5.1; Phase 3 in the web too. |
| Tool-window / editor-group maximize | parity gap | studio-shell-spec §5.4. |
| Responsive tiers (wide/medium/narrow) | parity gap | studio-shell-spec §5.3; the window is one tier. |
| Status bar right segment | parity gap | cursor position, element type + the conversion dropdown, key hints (§7.3). Only the left group exists. |
| Notification service / toasts | partly built | `Root::render_notification_layer`/`render_dialog_layer` are composed by the app root (`app/src/main.rs`), so `window.push_notification` and `open_dialog` work — rename, fix-all and failed navigation all use them. Missing is §7.5's *service*: no registry, no severities, no dismissal policy, and no one place errors are routed to. |
| Open-project dialog, recents | parity gap | the project is a CLI argument (HANDOFF 4). |
| Binder draws two headers | cosmetic | the dock's title strip and its own "BINDER" header both render (HANDOFF, "Two things noticed"). |

### Code view (`app/src/code_view.rs`, `document.rs`)

Built: an inner `DockArea` of documents — tabs, drag between groups, splits
— owning the active document; `brink.toml` opens here as a TOML tab.

| Left out | Kind | Note |
|---|---|---|
| Quick-open (`cmd-p`) | parity gap | spec §4.5 defers it. |
| Session documents (Player, Compiled Output, Story Graph, Settings-as-tab) | mostly built | the Player and Compiled Output dock as centre tabs (`CodeView::show_player`/`show_compiled`); Story Graph not started; the Settings tab is replaced by the modal by ruling. |

### Single File view (`app/src/single_view.rs`)

Built: the active document alone, no strip.

| Left out | Kind | Note |
|---|---|---|
| The companion split (Player beside the file) | **open ruling** | part of the view's definition; absent until Player placement is ruled (HANDOFF "Open, parked"). |

### Continuous view (`app/src/continuous.rs`)

Built: every file in binder order as a file-level-virtualised stack with
headings, editable, on the shared buffer.

| Left out | Kind | Note |
|---|---|---|
| Player swap-in | **open ruling** | the direction noted is swap, not split. |
| First section shows a partial row above the next heading | cosmetic | the measured-line-height issue the module doc describes. |

### The editor itself (`app/src/document.rs`, `model/src/tokens.rs`, `model/src/query.rs`)

Built: per-segment incremental paint for `.ink`, hover, completions, inlay
hints, diagnostics squiggles, the TODO band, gutters/inlays toggles from
App settings, the seed-edit guard.

This is the widest gap. `packages/ink-editor/src/` carries the web editor's
feature set; the native editor has the four providers above and nothing
else. Everything below is listed against that directory.

| Left out | Kind | Note |
|---|---|---|
| ~~Go-to-definition, references, rename~~ | built 2026-09-05 | `QueryKind::{Definition, References, PrepareRename, Rename}`; rename is a dialog prompt (the web studio's is inline in the editor — a parity gap in shape, not in behaviour). |
| ~~Folding: gutter chevrons~~ | built 2026-09-05 | `QueryKind::FoldingRanges` → the highlighter's `fold_ranges` → gutter chevrons on hover / the caret's line. (They were invisible only because no asset source was registered — HANDOFF #6.) |
| Fold All / Unfold All | engine gap (toolkit) | gpui-base keeps `display_map` private and offers no fold-all; only the gutter toggle exists. |
| ~~Code actions, fixes~~ | built 2026-09-05 | `QueryKind::{FixesAt, FixOffers, FixAll, Refactors, ResolveRefactor}`; `cmd-.` in every brink editor. Extract actions and the gated structural moves are still out. |
| Highlighting for `.inkt` | see below | the dump paints as plain text and wants a hand-written lexer (§0 item 2). `brink.toml` is highlighted as of 2026-09-06. Own subsection after this table. |
| ~~Find/replace panel inside a document~~ | built 2026-09-06 | `cmd-f` / `cmd-alt-f`. The panel is the TOOLKIT's — `EditorState::new` already sets `searchable`, so every brink editor carried it and only the key was missing; the kit's own `Search`/`Replace` actions are registered as commands rather than wrapped, so there is one implementation and both are in the palette. Case, regex, match count, prev/next, Replace and Replace All all come with it. |
| Signature help | parity gap | `signature-help.ts`. |
| Argument widgets, colour chips + picker, doc strings | parity gap | in-text chips are proven good enough (ruled, the chip ruling) but none is built. |
| Inline markup / screenplay / structural styles / hanging indent | parity gap | `inline-markup.ts`, `screenplay.ts`, `structural-styles.ts`, `hanging-indent.ts`. |
| Per-LINE styles: cue lines, dimmed comment/include lines | worker query + port | `IdeSession::line_contexts` exists; the worker does not carry it per file (HANDOFF "Themes and paint"). Only the TODO band is laid. |
| Execution highlight, play-from-here | play-from-here built | Binder row menu → `PlayFromHere` action → `BinderEvent::Play`; execution highlight not started (lines carry `source`, so the data is there). |
| Conflict view, breakage/boundary editing, element-type transitions | parity gap | `conflict-view.ts`, `breakage.ts`, `boundary.ts`, `element-type.ts`, `keybindings.ts`'s modal editing keys. |
| Prose checker diagnostics in the editor | not wired | `crates/brink-prose` is Rust, but the worker does not link or run it; see Problems and Settings ▸ Prose. |
| `.brink` incremental paint | **open ruling** (#3562), **`.brink` only** | a native file re-parses whole per keystroke; the segmentation boundary is a language ruling. |
| Hover verified by hand | verification | typing, completions and save were driven headless; hover was not. |

### Player (`model/src/play.rs`, `app/src/player.rs`)

Built: the runtime on the worker beside the analysis session
(`Request::Play`), a Code-view centre tab with the transcript (lines with
tags, echoed choices, turn boundaries, runtime warnings, compile and
runtime errors), live choice buttons, Restart / From start, the
"sources changed" status, and transcript lines that open their source.
`Play` (cmd-r), `Restart` (cmd-shift-r), and the Binder's "Play from here"
on knots and stitches.

| Left out | Kind | Note |
|---|---|---|
| Placement in Continuous and Single File | **open ruling** | the Code-view tab is the one placement the parked direction settles (HANDOFF "Open, parked"). |
| Hot-swapping a running story after an edit | deliberate | the module doc says why: the story keeps running on what it compiled from, the status says so, a restart picks the edit up. |
| Waking an `await` park | not started | `Step::Suspended` is shown as a turn boundary; there is no `wake_check` affordance. |
| Number keys for choices | parity gap | choices are buttons only. |
| Execution highlight in the editor | not started | lines carry `source`, so the data is there; see also the Program Explorer's overlay. |
| External-function binding | not started | `FallbackHandler` only — an external with no fallback body faults. |

### Program Explorer (`model/src/program.rs`, `app/src/program.rs`)

Built: a right-dock tool window over one worker compile
(`QueryKind::Program`), read four ways — **Structure** (globals, lists,
externals, the knot → stitch tree with bytecode-track/lines-fill size
bars), **Lines** (the compiled tables, scoped as the compiler scopes them,
templates spelled inline), **Disasm** (every scope and anonymous `c-N`
container's name-resolved bytecode), **Size** (sections, per-scope line
tables, bytecode by knot). Rows with provenance open their source. It
re-queries after an analysis only while it is the shown tab; hidden, it
marks itself stale and asks when shown.

| Left out | Kind | Note |
|---|---|---|
| Executing-instruction overlay, `stepi` | not started | needs the State View's session state (D9/W9 in the web). |
| ~~"open .inkt" button~~ | built 2026-09-06 | right-aligned in the view row; raises `ProgramEvent::OpenCompiledOutput` rather than opening the tab itself, since a tab is the host's to open — the same rule the panel's navigation rows follow. |
| Size treemap | parity gap | `ProgramSizeView.tsx` draws a treemap; this draws bars. |
| Jumps between views (disasm row → its line, size row → its container) | parity gap | the web's cross-view targeting. |
| Checksum staleness against a running session | not started | `sessionDegraded` in the web; needs the Player to report its program's checksum. |

### Syntax highlighting outside `.ink`/`.brink`

Brink's own files paint from brink's own CST (`BrinkHighlighter` over
`TokenCache`). Every other language the studio shows was unhighlighted
until 2026-09-06 — easy to miss, because the text is legible either way
and carries no structure. Both were checked on screen rather than
inferred, and they turned out to need different fixes.

| Surface | Today | Note |
|---|---|---|
| `brink.toml` | ~~no highlighting at all~~ **highlighted 2026-09-06** | `Document::new` had always set `.language("toml")`; the grammar simply was not in the build, because `crates/brink-gpui/Cargo.toml` took `gpui-component` with `features = ["tree-sitter"]` — tree-sitter plus JSON only. Adding `"tree-sitter-toml"` to that list was the entire change. **An unresolved language name is silent, not an error**, which is why this read as weak highlighting rather than none; if another language is ever named here, check its grammar is actually enabled. |
| Compiled Output (`.inkt`) | ~~no highlighting at all~~ **highlighted 2026-09-06** | No grammar would have helped — the kit's list has no lisp/S-expression entry, and `.inkt` is brink's own format whose in-tree reader is a `pest` grammar. `app/src/inkt_highlight.rs` is a hand-written lexer instead, taking its token shapes from `inkt.pest`'s own primitives so the painting cannot drift from what the reader accepts. Roles are Zed's names directly (brink's own roles ride `theme::syntax_key` to get there; these do not need to). |
| `dialect.json` | **not openable at all** | The Conventions section writes it when a dialect does not fit the `[dialogue]` table, and nothing can then show it: `collect_sources` takes only `.ink`/`.brink`, so it never becomes a `Document`. `language_of` routes `.json` correctly and the grammar is compiled in, so the highlighting is ready the moment the file is reachable — what is missing is the worker returning it beside `brink.toml` and the Binder listing it. |

**A language name is only half the wiring**, and `document.rs`'s
`language_of` now says so where it is decided: the kit resolves a name
against the grammars compiled into the binary, and an unresolved one is
silent. A name added there needs its grammar enabled in
`crates/brink-gpui/Cargo.toml` too.

### Compiled Output (`app/src/compiled_output.rs`, `model/src/compiled.rs`)

Built 2026-09-06: the `.inkt` dump as a read-only Code-view tab, a
singleton on the Player's terms (docked on first ask, selected after),
written on the worker off the same memoized compile the Program Explorer
and the play session use. Refreshed on the Program Explorer's rule —
while shown it re-asks after each analysis, hidden it marks itself stale.
Errors are reported in the buffer rather than leaving a stale dump up.

| Left out | Kind | Note |
|---|---|---|
| Syntax highlighting | parity gap | the dump paints as plain text; the web's tab has a minimal `.inkt` CM6 mode. See "Syntax highlighting outside `.ink`/`.brink`" above — it wants a hand-written lexer, not a grammar. |
| A find-in-dump of its own | parity gap | the web's CM6 mode carries search; this has the editor's selection and scrolling only. |
| A dump row jumping to its source | parity gap | the Program Explorer's disassembly rows do this; the dump does not. |
| ~~Reached only from the palette~~ | built 2026-09-06 | the Program Explorer's `.inkt` button opens it too. |

### Output log (`app/src/output_log.rs`)

Built 2026-09-06: the bottom-dock compile log, third tab beside Problems
and TODOs. Project open/save, analysis timings and the Player's compile
and runtime failures; nothing that has a file and a span, which is
Problems' business. An analysis earns a row when it is the first, when
the problem count moved, or when it was slow, and the quiet ones fold
into a `+N more` tail; `Every analysis` turns the filter off. 500 rows,
oldest dropped, with the dropped count in the header. Follows its tail.

| Left out | Kind | Note |
|---|---|---|
| Severity filter, copy-all | parity gap | the header carries `Every analysis` and Clear only. |
| A row opening anything | parity gap | a compile error names a code but does not navigate; Problems is where a span-carrying diagnostic goes. |
| Only two writers | parity gap | the project and the Player. A save failure, a config write, a format run and a fix-all say nothing here; each is a call to `Log::push` away, and the notification service (§1, the frame) is the other half of that question. |
| No timestamps | parity gap | rows are in order but undated, which is enough for one session and not for a long one. |

### Binder (`app/src/binder.rs`)

Built: Files and Structure modes, the fill rule, entry mark, closure
dimming, diagnostic marks, drag to reorder (in memory), filter, expand and
collapse all, keyboard navigation, hover row actions, right-click menu,
`brink.toml` listed beside sources.

| Left out | Kind | Note |
|---|---|---|
| Undo stack | parity gap | "Deliberately skipped" in the module doc. |
| Library section (mounted `std/`) | parity gap | ruled 2026-08-06 in the web; not built here. |
| Multi-select | parity gap | |
| Inline create (new file / knot) | parity gap | |
| `.binder.json` drag-order persistence | parity gap | reorder lives in memory only. |

### Problems (`app/src/problems.rs`)

Built: canonical order, grouped by file with a flat toggle, severity
buckets with counts, TODO bucket off by default, text filter,
click-to-reveal, rail badge, status-bar cell, `CONFIG` rows for a broken
`brink.toml`.

| Left out | Kind | Note |
|---|---|---|
| Prose bucket | not wired | the worker runs no prose checker. |
| ~~Fix buttons~~ | built 2026-09-05 | per-row **Fix** (the row's first offer) and **Fix all safe (N)**, `N` from `collect()`. The row's context menu listing every offer is not built. |
| Suppress context menu (#3148) | parity gap | |
| "Configure Exxx…" door into Settings ▸ Diagnostics | parity gap | the section exists; the row menu does not open it (HANDOFF "Not here yet"). |

### TODOs (`app/src/todos.rs`)

Built to the ruling: grouping, filter with tag chips, navigate, badge,
the leaving row, the editor band. Nothing consciously left out.

### Search (`app/src/search.rs`)

Built: the engine with the 1000-match cap, editable per-match cards on the
shared buffer with edit-mapping and `edited` badges, the frozen snapshot,
the summary strip, `cmd-shift-f`.

| Left out | Kind | Note |
|---|---|---|
| Replace previews / Replace All | parity gap | ruled surface (`docs/search-results-cards-spec.md`); held back. |
| References mode | worker query + port | see the editor's navigation row. |
| Context knob (lines above/below) | parity gap | the window is the ruled default and not tunable. |

### Commands — palette, menu, keymap (`shell/src/commands.rs`, `palette.rs`)

Built: one registry, `cmd-shift-p`, the hamburger menu, `cmd-1…9`
toggles, view switching, per-theme commands, overrides from settings.

| Left out | Kind | Note |
|---|---|---|
| `Escape` from a tool window back to the editor | parity gap | spec §4.5 defers it. |
| ~~Quick-open~~ | built 2026-09-06 | `cmd-p`; `app/src/quick_open.rs`. |
| `cmd-shift-<digit>` chords | platform | cannot match on Linux — do not bind them (HANDOFF). |

### Settings (`shell/src/settings*.rs`, `app/src/settings_*.rs`)

Built: the modal (rail, scope switch, cross-scope search), App ▸
Appearance and Keymap, Project ▸ General, Formatting, Diagnostics, Prose,
Conventions — all over the `brink.toml` seam, with the file itself opening
in Code view (ruled 2026-09-05).

| Left out | Kind | Note |
|---|---|---|
| App ▸ Editor (default view, fix-on-save) | parity gap | the web's `EditorViewSection` + `EditorSection`; font sizes and **Format on save** (built 2026-09-05, `brink-fmt` over every dirty `.ink`) live in Appearance here; fix-on-save is not built. |
| App ▸ Player (playback, debug info, external-function check) | parity gap | the Player exists now (§1), so this section has something to configure and nothing is drawn. |
| Creating `brink.toml` for a project without one | parity gap | every Project section says so and stops; the worker would need to adopt a new config path. |
| Formatting: tabs vs spaces | **open ruling** | the row is drawn disabled. |
| Diagnostics: prose codes | not wired | the registry lists compiler codes only. |

### Themes and paint (`shell/src/theme.rs`)

Built: the studio's five themes from the same token values, syntax roles on
Zed's names, per-theme commands, persistence.

| Left out | Kind | Note |
|---|---|---|
| Per-line styles from `editor.css` | worker query + port | see the editor row (`line_contexts`). |
| A theme switch does not repaint bands already laid in manuscript sections and Search cards | cosmetic | they follow at their next edit. |

### The shared buffer and the mirror (`app/src/project.rs`)

Built: one canonical text per file, `SourceDelta` broadcast, per-file
dirty/save, `cmd-s`, `brink.toml` and artifacts (`dialect.json`) through
the same road. Nothing consciously left out.

### The worker (`model/src/worker.rs`, `query.rs`)

Built: the `IdeSession` on its own thread, edit coalescing, `Hover`,
`Completions`, `DocumentSymbols`, `InlayHints`, `PassageIndex`, `Passage`,
config re-application, drafts report, resolved dialect.

| Left out | Kind | Note |
|---|---|---|
| ~~CodeActions query~~ | built 2026-09-05 | (Definition / References / Rename / Folding / Fixes / Format: all built 2026-09-05.) |
| `line_contexts` per file | worker query | for per-line styles. |
| ~~Compile to `StoryData` and a runtime session~~ | built 2026-09-05/06 | `Request::Play` (the runtime, `model/src/play.rs`) and `QueryKind::Program` (the compile read three ways, `model/src/program.rs`). Both go through the memoized `IdeSession::compile` under one entry rule (`play::entry_file`). |
| Story-graph query | worker query | for the Story Graph document. |
| Prose checking | not started | `brink-prose` is a separate wasm module in the web; nothing native runs it (Problems' prose bucket depends on it). |

## 2. Surfaces not started at all

Against studio-shell-spec §4's inventory:

| Surface | Blocked on |
|---|---|
| **Player** | ~~not started~~ **built** — see §1. Continuous swap-in and the Single File split remain the open ruling. |
| **Program Explorer** | ~~not started~~ **built** — see §1. |
| **State View** (debugger) | nothing in the shared layer exposes a running `Story`'s state. The Player owns the session, so this is engine work below `IdeSession` plus a panel. |
| **Output / compile log** | ~~unblocked~~ **built 2026-09-06** — see §1. |
| **Compiled Output** (`.inkt` tab) | ~~unblocked~~ **built 2026-09-06** — see §1. |
| **Story Graph** | the story-graph query in the worker; a canvas. |
| **Story transcript** | listed as future in the web too. The Player's transcript is per-session and is not this. |
| **Notification service** | the layers render (§1); the service does not exist. |
| **Library** (Binder section) | nothing; a Binder slice. |

## 3. Cross-cutting

- **No CI lane** runs this workspace's fmt/tests/clippy. Every change is
  gated by hand (HANDOFF "Known broken" 2). Whether the GUI tier is gated
  at all, or only `model` + `shell`, wants a ruling.
- **The editor acceptance gate has not moved down** onto `IdeSession`
  (required by the layering ruling of 2026-09-04, not by any slice).
- **The dialect golden corpus is duplicated** in `brink-ide` and
  `packages/dialect`; a case added to one must be added to the other.
- **The spec's benches are not in the repo** (HANDOFF "Things that will
  bite you"); the numbers are reproducible only by reconstruction.
- **Hover is the one editor feature verified only in the compiler.**

## 4. Open rulings this inventory waits on

1. Player placement in Continuous and Single File (HANDOFF "Open, parked"; the Code-view tab is built).
2. `#3562` — the `.brink` segmentation boundary (`.brink` only; lower priority).
3. Tabs vs spaces in Formatting.
4. Which tiers of `crates/brink-gpui` CI gates.
