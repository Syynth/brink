# brink-gpui — surface inventory

**Written:** 2026-09-05 · **Branch:** `claude/gpui-desktop-app-e8jb90` (PR #3568)

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
| Notification service / toasts | parity gap | §7.5. Nothing in the app can notify; errors go to Problems or nowhere. |
| Open-project dialog, recents | parity gap | the project is a CLI argument (HANDOFF 4). |
| Binder draws two headers | cosmetic | the dock's title strip and its own "BINDER" header both render (HANDOFF, "Two things noticed"). |

### Code view (`app/src/code_view.rs`, `document.rs`)

Built: an inner `DockArea` of documents — tabs, drag between groups, splits
— owning the active document; `brink.toml` opens here as a TOML tab.

| Left out | Kind | Note |
|---|---|---|
| Quick-open (`cmd-p`) | parity gap | spec §4.5 defers it. |
| Session documents (Player, Compiled Output, Story Graph, Settings-as-tab) | not started | see §2; the Settings tab is replaced by the modal by ruling. |

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
| Go-to-definition, references, rename (inline name input) | worker query + port | `brink_ide::navigation::goto_definition` and `rename` exist; the worker exposes no `Definition`/`References`/`Rename` `QueryKind`. Search's references mode waits on the same query. |
| Folding | worker query + port | `brink_ide::folding::folding_ranges` exists; the worker exposes no query for it and the editor wires no fold provider. |
| Code actions, fixes, extract actions | worker query + port | `brink_ide::code_actions` exists; the worker offers no fixes query and the editor has no action UI (`docs/autofix-spec.md`). Also blocks Problems' Fix buttons. |
| Find/replace panel inside a document | parity gap | `find-panel.ts`. |
| Signature help | parity gap | `signature-help.ts`. |
| Argument widgets, colour chips + picker, doc strings | parity gap | in-text chips are proven good enough (ruled, the chip ruling) but none is built. |
| Inline markup / screenplay / structural styles / hanging indent | parity gap | `inline-markup.ts`, `screenplay.ts`, `structural-styles.ts`, `hanging-indent.ts`. |
| Per-LINE styles: cue lines, dimmed comment/include lines | worker query + port | `IdeSession::line_contexts` exists; the worker does not carry it per file (HANDOFF "Themes and paint"). Only the TODO band is laid. |
| Execution highlight, play-from-here | not started | session-bound; waits on the Player. |
| Conflict view, breakage/boundary editing, element-type transitions | parity gap | `conflict-view.ts`, `breakage.ts`, `boundary.ts`, `element-type.ts`, `keybindings.ts`'s modal editing keys. |
| Prose checker diagnostics in the editor | not wired | `crates/brink-prose` is Rust, but the worker does not link or run it; see Problems and Settings ▸ Prose. |
| `.brink` incremental paint | **open ruling** (#3562) | a native file re-parses whole per keystroke; the segmentation boundary is a language ruling. |
| Hover verified by hand | verification | typing, completions and save were driven headless; hover was not. |

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
| Fix buttons | worker query + port | the worker offers no fixes (see the editor's code-actions row). |
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
| Quick-open | parity gap | as above. |
| `cmd-shift-<digit>` chords | platform | cannot match on Linux — do not bind them (HANDOFF). |

### Settings (`shell/src/settings*.rs`, `app/src/settings_*.rs`)

Built: the modal (rail, scope switch, cross-scope search), App ▸
Appearance and Keymap, Project ▸ General, Formatting, Diagnostics, Prose,
Conventions — all over the `brink.toml` seam, with the file itself opening
in Code view (ruled 2026-09-05).

| Left out | Kind | Note |
|---|---|---|
| App ▸ Editor (default view, fix-on-save) | parity gap | the web's `EditorViewSection` + `EditorSection`; font sizes live in Appearance here, and fix-on-save has no fixes to run. |
| App ▸ Player (playback, debug info, external-function check) | not started | nothing to configure until the Player exists. |
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
| Definition / References / Rename / Folding / CodeActions queries | worker query | `brink-ide` has each; the worker does not expose them. |
| `line_contexts` per file | worker query | for per-line styles. |
| Compile to `StoryData` and a runtime session | not started | everything session-bound waits on it. |

## 2. Surfaces not started at all

Against studio-shell-spec §4's inventory:

| Surface | Blocked on |
|---|---|
| **Player** | **open ruling** on where it sits per view, plus a compile + runtime session in the worker. Do not build into any view until ruled. |
| **State View** (debugger) | the Player. |
| **Output / compile log** | a compile in the worker. |
| **Program Explorer** | a compile in the worker. |
| **Compiled Output** (`.inkt` tab) | a compile in the worker. |
| **Story Graph** | the story-graph query in the worker; a canvas. |
| **Story transcript** | listed as future in the web too. |
| **Notification service** | nothing; a shell service. |
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

1. Player placement per view (HANDOFF "Open, parked").
2. `#3562` — the `.brink` segmentation boundary.
3. Tabs vs spaces in Formatting.
4. Which tiers of `crates/brink-gpui` CI gates.
