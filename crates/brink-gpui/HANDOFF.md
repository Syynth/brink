# brink-gpui — handoff

**Written:** 2026-09-04, kept current · **Branch:** `claude/gpui-desktop-app-e8jb90`
(draft PR #3568 against `main`) ·
**Architecture:** `docs/gpui-studio-spec.md` — read that first; this file is
only what a fresh session needs on top of it.

## Read these first, in order

1. `docs/gpui-studio-spec.md` — the design, with every performance number
   measured rather than estimated.
2. `docs/decision-log.md`, the four entries dated 2026-09-04 beginning "The
   native studio's analysis runs off the main thread". Those are rulings, not
   proposals; do not re-litigate them.
3. This file.
4. `INVENTORY.md` — every surface built so far and what each one leaves
   out, with the open rulings they wait on. Pick a slice from there.

## ⚠ Before planning anything: can you even build it?

**`crates/brink-gpui` has never been built anywhere but this maintainer's
macOS machine.** Two independent risks, neither verified:

- **gpui on Linux.** ~~A cloud container almost certainly lacks the
  headers.~~ Settled: see "Linux builds" below — two apt packages.
- **The fork dependency.** `gpui-component` comes from
  `https://github.com/Syynth/gpui-kit` at rev `c3f5bcac` (branch `brink`,
  three commits on top of upstream `v0.6.0`). The repo is **public**, so no
  auth is needed, but the fetch does need network at build time.

**Run `cargo check --manifest-path crates/brink-gpui/Cargo.toml --workspace`
as the very first thing.** If it fails on platform libraries, say so and stop
rather than working blind — and note that the two lower crates are still
useful on their own:

```sh
# These two are where the logic and the tests live (23 tests).
cargo test --manifest-path crates/brink-gpui/Cargo.toml -p brink-gpui-model
cargo test --manifest-path crates/brink-gpui/Cargo.toml -p brink-gpui-shell
```

`brink-gpui-model` depends on gpui only for `Task`/entity types, and
`brink-gpui-shell` on `gpui-component`. If even those will not build, the
productive work in a cloud session is in `crates/internal/**` instead — see
"If the GUI will not build" below.

**`crates/brink-gpui` is its own cargo workspace and is in the root's
`exclude`.** A root `cargo test --workspace` does **not** touch it, and
neither does a root `cargo fmt --all`. Same trap as
`packages/brink-desktop/src-tauri` — see CLAUDE.md's "Which gate covers which
files". Its own gate is:

```sh
cargo fmt --manifest-path crates/brink-gpui/Cargo.toml --all -- --check
cargo test --manifest-path crates/brink-gpui/Cargo.toml --workspace
```

There is **no CI lane for it yet.** That is a real gap (see below).

## Where things stand

The three tiers of spec §2 all exist. 23 tests, all green.

| crate | what it holds |
|---|---|
| `model/` | `worker.rs` (the `IdeSession` on its own thread), `tokens.rs` (per-segment paint cache), `query.rs` (hover/completions/symbols/inlays) |
| `shell/` | `region.rs` (the ruled rail→dock mapping), `rail.rs`, `workspace.rs`, `tool_window.rs`, `editor_view.rs` (the three views' root), `skin.rs` |
| `app/` | `project.rs` (the mirror entity), `document.rs` (editor + highlighter + providers), `code_view.rs` (documents, tabs, the active file), `single_view.rs`, `continuous.rs`, `binder.rs`, `problems.rs` (the studio's Problems view, ported — see its module doc for what is and is not), `main.rs` |

**Verified running** (screenshots taken against the real app, on macOS,
before the views landed): rails with both groups, the Binder, syntax
highlighting from brink's own CST with no tree-sitter grammar, dock
toggling, the status bar, and project load in ~10 ms. **The tab bar, the
three views and the switcher were verified running headless on Linux**
(2026-09-05, screenshots on PR #3568): two tabs after a Binder click,
Single File showing the active document with no strip, Continuous showing
both files with headings, and Code view's tabs intact on the way back.

## Running it headless (a cloud session CAN see the app)

No display is needed. On the ubuntu container, after the two xkbcommon
packages below:

```sh
apt-get install -y xvfb mesa-vulkan-drivers libgl1-mesa-dri x11-apps imagemagick xdotool
cargo build --manifest-path crates/brink-gpui/Cargo.toml -p brink-gpui
Xvfb :99 -screen 0 1280x840x24 &
DISPLAY=:99 target/debug/brink-gpui tests/tier1-native/conventions-cross-file &
sleep 10
DISPLAY=:99 xdotool mousemove 107 133 click 1        # click the Binder's second row
DISPLAY=:99 import -window root shot.png             # screenshot the whole display
```

gpui's Linux backend goes through wgpu (`Backends::VULKAN | GL`), and
mesa's lavapipe (`lvp_icd.json`) satisfies it in software. First frame is
up within ~5 s of launch in a debug build; `xdotool` drives clicks and
keys by screen coordinate. This is how the views were verified, and it is
the way to verify any UI change from a session with no screen — do not
merge UI work seen only in the compiler again.

**Problems** (2026-09-05) is the studio's panel ported: canonical order,
grouped by file by default with collapsible headings, severity toggles with
counts, text filter, click-to-reveal (opens the file, selects the span,
focuses the editor), the rail badge with the error count, and the status
bar's problems cell opening the dock. Verified headless against a scratch
project with errors, info and a TODO note. Not ported: the prose bucket (no
native prose checker), Fix buttons (the worker offers no fixes), the
suppress context menu (#3148). The shell grew what those needed:
`ToolWindow::badge` (`tool_window.rs`), `StatusCell` with `opens`, and
`Workspace::open_tool_window`.

**TODOs** (2026-09-05, `app/src/todos.rs`; ruled 2026-08-23): the
studio's TODOs window over ink's `TODO:` author notes, read from the
`E189` diagnostics the mirror already holds — grouped by file → containing
knot/stitch (flat a toggle away), a text filter with `TODO(tag):` chips,
click-to-navigate, an amber rail badge (`BadgeTone::Advisory` — the shell's
`Badge` now carries a tone), and a removed note lingering struck through
for 1.4 s, keyed by location. **The editor band** is the highlighter's:
`TokenCache` returns the `AUTHOR_WARNING` ranges from the same parse that
paints (`todo_ranges`), and `BrinkHighlighter` lays the theme's `todo_band`
/`todo_ink` over every run on those lines with the keyword bold
(`overlay_todo`) — same frame, no analysis in the loop, and it wins on every
word, which decorations could not guarantee (the editor composes decoration
and syntax colours through an unordered set). The `E189` squiggle is
suppressed in the editor, as the studio does: the band is the note's
presentation there. The band colours are snapshotted when the highlighter
updates, so a theme switch reinstalls the highlighter (`Document`'s
`observe_global::<Theme>`); a manuscript section or a Search card keeps its
old band colours until its next edit. Verified headless: bands in Mocha and
Inky Dark, the panel grouped, a row click opening the file at the note, a
deleted note leaving.

**Commands** (2026-09-05, spec §4.5): every shortcut, the palette
(`cmd-shift-p`), the hamburger menu and the `cmd-1…9` tool-window toggles
go through one registry (`shell/src/commands.rs`); the app registers its
commands with `Workspace::register_command`. Verified headless: palette
filter + Enter switches views; the menu lists View/File groups with keys;
`cmd-1`/`cmd-2` toggle Binder/Problems from a fresh launch and after view
switches. Deferred, in the spec: `Escape` back to the editor, quick-open.
Do not bind `cmd-shift-<digit>` to anything — it cannot match on Linux.
Keymap overrides landed with Settings (below).

**Settings** (2026-09-05, `shell/src/settings*.rs`; ruled 2026-08-27): the
studio's modal — a searchable section rail with the App / Project scope
switch on the left, one section on the right — on `cmd-,` and as "App:
Settings…" in the palette. Sections are registered entries
(`Workspace::add_settings_section`); the shell registers the two App
sections it owns. **Appearance**: the theme picker as live tiles (each
painted from its own `theme::Tokens`), gutters, inlay hints, editor and app
font sizes. **Keymap**: every command with its keystroke and source; Record
takes the next chord through `App::intercept_keystrokes` — it runs BEFORE
the keymap, which an element key listener does not (the first version
switched views instead of recording); a chord taken from another command
displaces it and the row says so (ruled 2026-08-30). Overrides live in the
settings, keyed by the command's full title, and are installed by binding
LATER (gpui's keymap only grows; later bindings win) with a taken-away
default shadowed by the `Unbound` action the workspace swallows. Chords are
canonicalised (`commands::canonical_chord`) so "cmd-alt-3" and gpui's own
"alt-cmd-3" spelling compare equal. **App settings are one JSON file**
(`settings.json` in `$BRINK_STUDIO_CONFIG_DIR` or the platform config dir;
the old `theme` file is read once) behind the `AppSettings` global that
editors observe: gutters and inlay hints apply live; the editor size
re-applies the theme; the app size scales the window's rem. Verified
headless: the modal, tiles, a font step, the scope switch, the keymap
table, a recording that displaced a default and fired after closing, the
file.

**Project scope: General** (2026-09-05, `app/src/settings_general.rs`,
ruled 2026-08-27 and 2026-08-29 for the form; the maintainer's call
2026-09-05 for where the text lives): **`brink.toml` opens in Code view
like any file** — unlike the web studio, which routes it to a Settings
takeover — and Settings holds only the form: `entry` / `conventions` /
`dialect` / `types` as selects over the project's real files (a configured
file the project lacks kept and marked "(missing)"), the drafts list with
each glob's report (drafts / "matches nothing" / "also matches N files the
story reaches"), and an "Open brink.toml" button for everything else.
**The seam is the shared buffer**: `brink.toml` is a file the mirror holds
(`Project::config_path`, text through `loaded_source`, edits through
`Project::edit`, saved by `cmd-s` with everything else) but not one of
`files()`; `Document::new` gives it the `toml` language and none of
brink's providers; the worker routes an edit to it into
`apply_config_text` (`model/src/worker.rs`, see `ConfigState`'s doc for
the two rules: whole-file re-application, and a malformed text keeping the
last good config while its parse error sits in Problems as a `CONFIG` row
that opens the tab at the span). Every analysis carries the resolved
`entry`, the config warnings and the per-glob drafts report, so the
Binder's entry mark and the closure follow a select. Structured edits go
through `brink_project_config::edit::ConfigDocument` — `remove_key` and
`string` were added to it for this — so comments survive. Verified
headless: the tab (TOML paint, a parse error squiggled and in Problems),
the section, a select repointing the entry with the tab and the Binder
following, a draft added and its row's report, save. 

**Project scope: Formatting, Diagnostics, Prose** (2026-09-05,
`app/src/settings_{formatting,diagnostics,prose}.rs`, sharing
`settings_config.rs`): the studio's three, over the same seam.
Diagnostics lists the compiler's own registry — moved to
`brink_ide::diagnostic_registry` with its category table and drift
guards, so `brink-web` and this crate read one table — as the studio's
two lists (configured / not, Configure and remove moving a code between
them, level and Fix pickers, deny-warnings, inline markdown explanations
through the kit's `TextView`, unknown codes kept). Formatting is the
indent stepper with a key-removing Reset. Prose is enable, the dialect
select and the dictionary list. Verified headless on a project whose
`brink.toml` exercises every table: level change, deny toggle, Configure
(the open `brink.toml` tab showed the appended line, `cmd-s` wrote it
with the comments intact), an explanation unfolded, the indent stepped
with Reset appearing, a word added and removed, the dialect picked.
**Two things to know when driving it headless:** the window repaints an
edit a beat late (a screenshot 1 s after a click can show the previous
state; wait ~3 s or nudge the pointer), and a scratch `settings.json`
from a keymap test can carry `"File: Save": null` — which is why `cmd-s`
"did nothing" for twenty minutes here. **Not here yet**: the Problems row
menu's "Configure Exxx…" door into Diagnostics, and creating a
`brink.toml` for a project that has none (the sections say so and stop;
the worker would need to adopt a new config path).

**Conventions** (2026-09-05, `app/src/settings_conventions.rs`): the
teach-by-example editor over `brink_ide::dialect_infer` +
`dialogue_section` — the Rust port of `@brink-lang/dialect`'s inference
and studio-store's section writer, made for this (the maintainer's call:
the logic moves to Rust so the native studio can carry the UI). The golden
corpus is duplicated in both suites; **a case added to one is added to the
other.** The worker gained `PassageIndex`/`Passage` queries, carries the
resolved dialect on every analysis, and routes an edit to a non-source
file (`dialect.json`) into the config's artifact reader. Verified
headless: the picker (typed filter, pick, choices hidden), six marks on
the canvas sample, the four learned sentences with 2/2 · 2/2 · 1/1 · 3/3,
the Player preview folding MARA's run, "Use these rules" writing the
stamped `[dialogue]` table, the worker re-applying it ("Current
conventions: at-cue"), and `cmd-s` writing the file. The kit's `Focus`
input event did not open the list on the first click here, so the picker
reads the field's focus handle at render instead.

**Search** (2026-09-05, `app/src/search.rs`): the studio's engine (plain
or regex, case, whole word, one composed pattern, 1000-match cap) over the
mirror's current sources; per-match cards with `file:line`, containing
knot/stitch, the match line with 1↑2↓ context and the hit highlighted; a
frozen snapshot replaced only by a new query, an option, or `↻`; the
summary strip with the Binder's expand/collapse-all; `search.focus` on
`cmd-shift-f`. **Cards are editors** (same day, on the shared buffer
below): each card is an `EditorState` over a line-aligned window of its
file, built the first time it scrolls into view, with the file's line
numbers in a gutter of its own (gpui-base has no line-number offset) and
the hit as a `TextDecoration` the editor carries through its own edits.
An edit in a card is spliced into the file through `Project::edit`; every
change to the file — from a tab, the manuscript, or another card — is
**edit-mapped** through every card of that file (`Match::map_edit`: slide,
apply in place, or re-snap to whole lines and reset), and a card whose
window no longer reads as the search saw it wears an `edited` badge and
stays. Verified headless: card→tab, tab→cards (two cards sharing lines),
an inserted line above shifting every gutter, reveal at the mapped hit,
one `cmd-s` writing all of it. Still held back: replace previews, the
context knob, references mode (a worker query).

**The shared buffer** (2026-09-05, spec §6): the mirror is the canonical
text per file and every `EditorState` follows it through `SourceDelta`
broadcasts (`app/src/project.rs`, `Document::apply_delta`); dirty and save
are per file in the project. Verified: an edit in Code view appears in the
manuscript, an edit in the manuscript appears in Code view, one `cmd-s`
writes both. Search cards are editors over this buffer (above).

**Themes and paint** (2026-09-05, `shell/src/theme.rs`): the studio's five
themes — Catppuccin Mocha (default), Catppuccin Latte, Manuscript, Inky,
Inky Dark — carried as the SAME token values the CSS sheets hold
(`packages/studio-shell/src/styles/themes/*.css`), built into
gpui-component's own theme-file shape and installed with `Theme::change`,
so chrome, editor and cards all repaint from one place. Brink's 19 token
types ride Zed's fixed syntax names in the kit's highlight table
(`theme::syntax_key` is the one mapping; `BrinkHighlighter::styles` asks
through it), with the studio's CSS fallbacks resolved at build time
(marker/divert → operator, halt → keyword) and its dressing (keywords
semibold, comments italic, the escape mark at 40%). One palette command
per theme ("Theme: Manuscript"); the choice persists in the platform
config dir (`$BRINK_STUDIO_CONFIG_DIR` overrides — the headless runs use
a scratch dir). Verified headless: all five on screen, a switch through
the palette, the choice surviving a relaunch. **Not yet ported** from
`editor.css`: the per-LINE styles other than the TODO band (which the
highlighter lays, see TODOs above) — cue lines in `--bs-cue` at
`--bs-cue-weight`, dimmed comment/include lines — need the worker's
`LineContext` per file.

Of the two things noticed in those screenshots, one is fixed: the Binder
no longer draws both the dock's title strip and its own header — the
side docks draw no tab bar at all since the barless skin (`shell/src/
skin.rs`, maintainer 2026-09-05 "why is it there?"), so its own
"BINDER" + toolbar row is the only header, confirmed on screen
2026-09-07. Still open: the manuscript's first section shows a partial
row above the next heading (the measured-line-height issue
`continuous.rs` already describes).

**Linux builds** (verified 2026-09-05, contrary to the earlier worry): the
whole workspace, app included, builds and links on an ubuntu container with
`libxkbcommon-dev` and `libxkbcommon-x11-dev` installed; `xcb`,
`fontconfig` and `freetype` were already present.

**Typing, completions and `cmd-s` were verified headless on Linux**
(2026-09-05, with `xdotool type`): keystrokes reach the editor, the
completion popup opens, the tab goes dirty and saves. Hover is still
unverified by hand.

## Known broken / unfinished, most blocking first

1. ~~**The centre dock draws no tab bar.**~~ **Fixed 2026-09-05**: the
   area had no `DockSkin`, so it wore gpui-base's bare renderer. The three
   views (spec §4.4) then landed on top: the centre holds one `EditorRoot`
   panel that renders Code (an inner `DockArea` of documents, with tabs),
   Single File, or Continuous; switcher in the title bar, `cmd-alt-1/2/3`.
2. **No CI lane.** Nothing runs this workspace's tests or fmt. Adding one
   means a macOS runner (or solving the Linux question above) — worth a
   ruling on whether the GUI tier is gated at all, or only `model` + `shell`.
3. ~~**Rail toggling is dock-level, not tab-level.**~~ **Fixed 2026-09-05**
   with Search as the second tab in the left dock: a tool window records
   its tab group through `TabSlot` (`shell/src/tool_window.rs`), and the
   rail opens-and-selects, switches, or closes accordingly; a button is
   pressed only when its window is the one on screen.
4. **Open-project is a CLI argument only.** No file dialog, no recents.
5. ~~**No layout persistence.**~~ **Partly fixed 2026-09-06**: the three
   docks' open state and width and the current editor view ride
   `AppSettings` (`Workspace::layout`/`apply_layout`), saved on every
   discrete change and again on quit — `on_app_quit` alone loses them to
   a crash, and SIGTERM does not run it either. The panel TREE is still
   not persisted: rebuilding open documents needs the toolkit's
   `PanelRegistry`, and a `Document` panel is per-file.
6. ~~**Fold chevrons do not paint.**~~ **Fixed 2026-09-05.** The app
   never registered the kit's asset source (`gpui_kit_assets::Assets`), so
   every `IconName` — the fold chevrons included — drew nothing while its
   hitbox was there all along. `Application::with_assets` in `main.rs`.
   Chevrons show on gutter hover and on the caret's line, by the toolkit's
   design (`paint_fold_icons`).
7. **`#3562` — `.brink` files have no incremental paint path.** Native
   segmentation does not exist, so a native file pays a whole-file parse per
   keystroke (2.1 ms at 700 lines, 12.4 ms at 8,400) where `.ink` pays
   17–51 µs per knot. **The boundary question is a language ruling and must
   not be decided by an agent.**

## Open small things (2026-09-05, end of day)

- ~~**The Program Explorer is unverified by hand.**~~ **Verified
  2026-09-06** (headless, on a two-file scratch project): the right rail
  opens it, all four views render — Structure with globals and the knot
  tree with size bars, Lines with the scoped tables, Disasm with every
  container's instruction and byte counts, Size with sections, line
  tables and the shipping/debug split — and an edit while it is shown
  refreshes it, reporting `No program: the story has 1 error(s)` with the
  `E024` when the project stops compiling. **Still unchecked**: a
  disassembly or line row opening its source, and the hidden→stale→shown
  path.

- ~~**The Player is unverified by hand.**~~ **Verified 2026-09-06**
  (headless): cmd-r docks the Player tab in Code view with the first
  lines and the numbered choice buttons; a choice echoes as `* text` and
  the story runs on to the next lines and choices; tags render beside
  their line; a compile failure lists the errors and says to fix them in
  Problems. Driving it found one defect, now fixed: a long line pushed
  its own tags off the right edge instead of wrapping (`min_w_0` — a flex
  item's minimum is its content by default). **Still unchecked**:
  Restart / From start, the Binder menu's "Play from here", a transcript
  line opening its source, and the "sources changed" status. Hot-reload
  of a running story is deliberately not done (see `model/src/play.rs`).

- **Compiled Output and the Output log** (2026-09-06,
  `app/src/compiled_output.rs`, `app/src/output_log.rs`,
  `model/src/compiled.rs`) — the two surfaces INVENTORY's order put next,
  both off the compile that already exists. Verified headless: the dump
  paints in full with line numbers and refuses typing, it reports the
  project's errors rather than leaving a stale dump up, the log coalesces
  quiet analyses into `+N more` and follows its tail, and a Player
  compile failure reaches it in red. **Two traps worth keeping**, each
  found on screen rather than in the compiler: `flex_1` belongs on the
  `Editor` element, not a wrapper div (an editor sizes its visible range
  from its own height, so a wrapped one lays out ONE line), and
  `readonly` belongs on the element too — `Editor` pushes its own flag
  into the state every render, so `set_readonly` at construction is
  overwritten on the first frame and the "read-only" dump was editable.

- **Escape did not close the `cmd-.` code-action menu** when driven by
  automation; `CodeActionMenu::handle_action` handles Escape when its own
  `open` is true, which `sync_lsp` sets on render — unverified whether a
  real keypress behaves the same. Check by hand first.
- **Popover placement fix unverified by hand.** Fork commit 7504917 adds
  the scroll offset to `range_to_bounds` (the caret already applies it),
  which is what made hover/diagnostic popovers drift by the scroll
  distance on both axes. Reasoned from the code; the screen was locked
  before it could be driven.
- The web studio's "format document" only sorts knots; the native
  Format Document / Format on save run `brink-fmt` (the `brink fmt`
  formatter, `[project] indent` honoured). The two surfaces now differ
  here on purpose.

## Rendering contract worth knowing (2026-09-05)

gpui-component's `Root::render` draws the view, tooltips and native menus
— and **not** its dialog, sheet and notification layers. Those are free
functions (`Root::render_dialog_layer`, `render_notification_layer`,
`render_sheet_layer`) the application root composes in, AFTER its own
content so they paint on top. `Studio::render` does this now; before it
did, every `open_dialog` and `push_notification` landed in a list nothing
rendered, silently.

## Open, parked by the maintainer (2026-09-05)

- **Where the Player sits in each view.** Today it is three different
  answers: a session *document* in a Code-view split (ruled 2026-06-10), a
  companion split *native to* Single File view, and absent from Continuous.
  The maintainer's direction, noted for later rather than ruled: **Code** —
  a tab is fine, as today; **Continuous** — it has to *swap in and out*,
  because the manuscript is one scroller and a permanent split fights the
  scrolling; **Single File** — "a side-by-side split, maybe". So it is
  per-view, not one root-level companion. **2026-09-05: the Player is
  built as the Code-view tab** (`app/src/player.rs`, `model/src/play.rs`;
  the maintainer asked for "a working version of the player next") — the
  one placement the direction already settles. `Play` (cmd-r) and the
  Binder's "Play from here" switch the manuscript to Code first; the
  Continuous swap-in and the Single File split are still open, and the
  companion slot stays a placeholder.

## Deliberately not done

- The **editor acceptance gate has not moved down** onto the shared session.
  The layering ruling (2026-09-04, "Both studio consumers sit on the same
  layer") requires it; this slice did not do it.
- Story graph, debugger, Output/Compiled Output/Program Explorer — all
  out of the ruled first slice. (The Player is in, 2026-09-05; the compile
  it needed now lives in the worker's play session, so the three
  compile-bound tool windows are unblocked.)

## Things that will bite you

- **The compile closure is not a proxy for anything else.** Two bugs today
  came from treating it as one: drafts silently reported nothing because
  `compilation_closure` is "empty when no entry is set" and
  `refresh_analysis` never sets one, and Problems said "Not analyzed yet."
  forever because it read `closure_known` when it meant `has_analyzed`. If
  you find yourself reading the closure, check you do not mean something
  else.
- **No debounce, ever** (ruled). If a keystroke path is too slow, make its
  work O(edit); do not add a timer.
- **`Window::available_actions()` cannot list a `no_json` action.** It
  builds each listener's action type from nothing, which a data-carrying
  action (`ToggleToolWindow`, `SelectTheme`) cannot do, so the list omits
  it and a palette row keyed on that list reads as disabled — every
  "View: Toggle …" row was, silently, until 2026-09-05. Ask per action:
  `window.is_action_available(action, cx)`.
- **The editor asks its providers synchronously, inside the keystroke.**
  `Document::on_edited` runs from the `Change` event, delivered after the
  update — so a completion query sent from the provider overtook the edit
  and reached the worker with an offset past the text it held (it
  panicked the analysis thread). `seed_edit` in `document.rs` pushes the
  editor's text ahead of every query; `query::clamp_offset` is the guard
  behind it. Keep both if you add a provider.
- **Nothing in `app/` may touch an `IdeSession`.** The session is on the
  worker thread. Paint comes from `TokenCache`; everything else is a query.
- **The shell must not depend on the feature crate.** That one-way edge is
  the reason the three-crate split exists.
- The benches that produced every number in the spec (`scale`, `paint`,
  `incr`, `native`) were scratch crates and are **not in the repo**. If you
  need to re-measure, they are reconstructible from the spec's tables — or
  better, promote them next to `crates/internal/brink-test-harness/src/bin/
  ide_bench.rs`, which already has a 50×20 synthetic fixture.

## If the GUI will not build in your environment

Useful work that needs none of gpui, in rough priority order:

- Move the editor acceptance gate down onto `IdeSession` (the layering
  ruling's outstanding half).
- Promote the scale/paint/incr benches into `brink-test-harness` so the
  spec's numbers are reproducible in CI rather than from a scratch directory.
- `#3562` groundwork — but only the parts that do not require the boundary
  ruling.
