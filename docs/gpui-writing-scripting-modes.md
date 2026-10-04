# Writing and Scripting modes — the native studio's two personas

**Status:** designed 2026-10-03 in one design round; not yet built. This
document records what was ruled and how to build it. It amends
`docs/gpui-studio-spec.md` §4.4 ("The editor root and its three views"), and
supersedes two earlier rulings (§2.3).

**Design canvas:** "Brink Studio — Writing & Scripting modes"
(https://claude.ai/artifact/CKzcs7yufM25xUbTHeLisG, private to the
maintainer). It holds the baseline — headless captures of today's Continuous
and Code views — and the mockups described here. Where the two disagree, this
document wins: the canvas is a sketch, this is the record. The reference
boards are **"Write B — Tools on demand"** (Writing mode with its panels
open), **"Write A — Blank page"** (the same screen with them closed),
**"Write — Read view on"**, and **"Script"**. Board C (focus dimming) was
explored and dropped.

## 1. The idea

The studio stops being one IDE with three editor views and becomes **one app
with two modes**, each built for one kind of work:

- **Writing** — the manuscript (today's Continuous view) with almost no
  chrome. For drafting and revising prose.
- **Scripting** — the tabbed editor (today's Code view) with the full tool
  set. For structure, logic and debugging.

The **Single File view is removed**: neither persona needs it. Writing reads
the whole story at once, and Scripting already shows one file per tab.

Both modes edit the same buffers. Switching mode never changes text, only how
it is presented and which tools are at hand.

## 2. Rulings

### 2.1 The modes

| # | Ruling |
|---|---|
| R1 | Two modes, **Writing** (Continuous) and **Scripting** (Code). A two-way **Write / Script** switch in the title bar replaces today's three-way view switcher. |
| R2 | **Single File view is removed** — code, actions, settings, tests, docs. |
| R3 | The switch is **icon-only**: a pen for Write, `</>` for Script; names and shortcuts on hover. |
| R4 | **Keybindings come later.** Every mode, view and panel toggle in this document is an **action that can be bound**; no default chord is ruled here. (The canvas's `⌘1`/`⌘2`/`⌘⇧R` are placeholders — `⌘⇧R` is already Restart.) |

### 2.2 Writing mode

| # | Ruling |
|---|---|
| W1 | Writing mode **removes most of the chrome**: no rails, no docks, no status bar, no tab strip. |
| W2 | It must be what **GPUI's (Zed's) editor can actually draw**: a line-number gutter, soft wrap, a current-line highlight, indent guides, the syntax colours, squiggles, per-file section headers. No rendered headings or per-line font sizes. |
| W3 | The manuscript is **left-aligned and stretches** to the available width — not a fixed-width centred column. |
| W4 | A **macOS-style sidebar**, toggled by a button **immediately right of the traffic lights** (Claude Desktop's placement), that **slides out** and pushes the manuscript. The toggle is visible on the bare page too. |
| W5 | The sidebar has **two columns, Inky-style**: **Files**, then the **current file's structure**. The second column's toggle lives **in the first column's own header**. |
| W6 | The structure column lists the **current file's** knots and stitches, **functions** and **globals** (`VAR`/`CONST`/`LIST`), with **hover actions** — new stitch on a knot, a `⋯` menu on every row — and a `+` per section. |
| W7 | A **prominent Play button** in the title bar slides the **Player in from the right, beside the manuscript**. |
| W8 | A **Read view** — a proportional font, ink markup extremely de-emphasised, prose prominent — is a **toggle inside Writing mode**, not a third mode. |
| W9 | With the sidebar closed, a **small chip, bottom-right**, shows the word count and the problem count; clicking it opens the sidebar on the problems. |
| W10 | **No focus dimming** (board C's idea is dropped). |
| W11 | For the first build the Writing sidebar is **its own component**, separate from Scripting's Binder. Migrating it into Scripting mode comes **afterwards**. |

### 2.3 Scripting mode, and what this supersedes

| # | Ruling |
|---|---|
| S1 | Scripting is today's Code view with the **Write / Script** switch, **no Single File**, and a **lean status bar**: no absolute path and no analysis timings (those belong in the Output log). **Both rails stay.** |
| S2 | **Supersedes** "Side docks draw no tab strip; both rails are always drawn" (`decision-log.md`, 2026-09) **for Writing mode only**: rails are still always drawn in Scripting. |
| S3 | **Supersedes** the parked direction that the Player "swaps, not splits" in the Continuous view (`crates/brink-gpui/HANDOFF.md`, "Open, parked"): in Writing mode it slides in beside the manuscript (W7). It also closes the INVENTORY open ruling "Player placement in Continuous and Single File". |

## 3. Writing mode, screen by screen

### 3.1 Title bar

Left to right:

1. **Traffic lights**, then the **sidebar toggle** (W4), then search and
   back/forward. When the sidebar is open these sit in the sidebar's own
   header row, at the same position, so the toggle never moves under the
   pointer.
2. A **breadcrumb**: story title · knot › stitch at the caret.
3. **Read** (W8) — an open-book toggle, lit while on.
4. **Play** (W7) — filled with the accent colour; a ring while the Player is
   open.
5. The **Write / Script** switch, icon-only (R3).

### 3.2 The sidebar (W4–W6, W11)

- Full height from the window's left edge; it pushes the manuscript.
  Animated open and closed.
- **Column 1 — Files.** Header: `FILES`, `+` (new file), and the structure
  column's toggle. Rows: the entry file emphasised with its problem count;
  the other story files; the `std` library folder. Foot: `N problems`.
- **Column 2 — Structure of the current file.** Sections, each with a `+`:
  **Knots** (stitches indented beneath, the caret's stitch highlighted),
  **Functions**, **Globals** (each with its initial value faint on the
  right). Hover reveals `+` (new stitch) on knots and `⋯` on every row.

### 3.3 The manuscript (W2, W3)

Today's Continuous view: per-file section headers, line numbers restarting
per file, soft wrap, a current-line band, indent guides, squiggles.
Left-aligned and full-width.

### 3.4 The Player (W7)

Slides in from the right and pushes the manuscript; header "Playing from
<stitch>", Restart, close. The same Player as today, re-homed for Writing
mode — not a dock tab there.

### 3.5 The Read view (W8)

The same editors, presented differently:

- the buffer font switches to a **proportional** face;
- every markup token (`===`, `=`, `*`, `[ ]`, `->`, `INCLUDE`, and the
  knot/stitch/divert names) drops to a **very faint** colour, and so do the
  line numbers;
- **prose stays at full strength** — and so does **choice text**, which the
  player reads;
- no current-line band; squiggles still show.

## 4. How to build it

Paths are under `crates/brink-gpui/` unless they start with `docs/`. The code
references come from a survey of `main` at `84d54baab`.

### 4.1 Slices, in order

Each slice is one PR, verified headlessly (§4.2).

1. **Two modes; Single File removed** (R1–R4).
   - `shell/src/editor_view.rs`: `EditorView` loses `Single`; the actions
     become the two mode actions (bindable, no ruled default). The
     `EditorRoot` occupants array goes from 3 to 2.
   - `shell/src/workspace.rs`: `view_switcher` becomes the icon-only
     Write/Script switch; registration and handlers follow.
   - Settings: `default_view` and `Layout.editor_view` must read the old
     `"single"` value as Script (`shell/src/settings.rs`), and the settings
     UI drops the option (`shell/src/settings_editor.rs`).
   - `app/src/main.rs` stops building `SingleFileView`; delete
     `app/src/single_view.rs`.
   - Fix the tests that name Single File (`editor_view.rs`, `commands.rs`,
     `menus.rs`) and update INVENTORY, HANDOFF and `gpui-studio-spec` §4.4.
2. **Writing mode's chrome** (W1, W3, S2).
   - `Workspace::render` hides the rails, the docks and the status bar in
     Writing mode. Today nothing looks at the view, and `MaximizeEditor`
     only closes docks — so this is new code, though `toggle_maximize`'s
     remembered dock state is the model for restoring Scripting's docks.
   - The title bar gains the Writing layout (§3.1). `TitleBar` reserves
     80px for the traffic lights, so the toggle sits just after that inset.
   - The manuscript goes left-aligned and full-width (`app/src/continuous.rs`).
   - *Built:* Write mode draws the editor root bare (no dock area, rails or
     status bar). The docks are hidden, not closed, so Script and the saved
     layout are untouched. The title bar shows the story's name and Play. The
     manuscript was already left-aligned and full-width. A tool window
     asked for from Write (a shortcut, Search) opens in Script, the way the
     Player already does. The sidebar toggle, the knot › stitch crumb and
     Read come with slices 5 and 3.
3. **Read view** (W8).
   - Font: the kit's `Editor` is `Styled`, and its own test shows a
     per-editor `.text_size()` override resizes the rows, so the Read font
     is set on the manuscript's `Editor` elements at render. The
     manuscript's height maths assumes `mono_font_size × LINE_HEIGHT_FACTOR`
     (`continuous.rs`); `adopt_measured_line_height` re-measures from the
     real rows, but the fallbacks must follow the Read font.
   - Colour: a Read flag into `BrinkHighlighter` (`app/src/document.rs`),
     captured by `highlighter_factory`, maps markup roles to a faint colour.
     The theme's shared resolver cannot do it alone.
   - One bindable toggle action.
   - *Built:* `ToggleReadView` (no default key) and an open-book button in
     the Write title bar, lit while on. From Script it goes to Write with
     Read on. "Prose" is positive, not "whatever has no token": the worker
     ships each file's prose ranges with every analysis, cut the same way
     the prose checker cuts them (content minus nested machinery). Untokened
     punctuation like `{` would otherwise have stayed bright. The face is
     the UI font at the editor's size, so rows keep their height and only
     the wrapping moves. Faint line numbers and no current-line band come
     from two per-editor options added to the gpui-kit fork
     (`active_line_highlight`, `line_number_color`; decision log
     2026-10-03). A convention-claimed cue line stays bright
     (`prose::read_ranges`).
4. **The Player beside the manuscript** (W7, S3).
   - Today `play_at` forces Code view (`require_editor_view`) and docks the
     Player as a centre tab (`CodeView::show_player`). In Writing mode it
     instead opens a right-hand panel that pushes the manuscript. Scripting
     keeps today's tab.
   - The title bar's Play button and the existing `Play`/`PlayRestart`
     actions both open it.
   - *Built:* `app/src/write_view.rs` is Write mode's occupant, holding the
     manuscript and, when out, a 400px Player panel that slides in (a
     160ms width animation) with a "Player" header and a close button.
     `play_at`, Restart and the debug verbs show the Player where the
     current mode keeps it. `TogglePlayer` ("Show/Hide Player", bindable,
     no default key) slides it away and back without touching the
     session. Title-bar Play is ringed while the Player is out. Closing
     has no slide-out yet.
5. **The Writing sidebar** (W4–W6, W11) — a new component, not the Binder.
   - The structure column uses the per-file `DocumentSymbols` query. Its IDE
     source (`brink_ide::document::document_symbols`) already returns
     `VAR`/`CONST`/`LIST` with a `SymbolKind`, but the model's `Symbol` and
     `convert` (`model/src/query.rs`) **drop the kind**, keeping only
     `is_function`. Carry the kind through; the Binder benefits too (today
     those declarations appear to render as knot rows).
   - New knot, new stitch and the `⋯` menu reuse the Binder's existing
     events (new knot/stitch, promote/demote, Play from here).
   - *Built:* `Symbol` carries `kind` and a global's value as written, and
     the Binder now shows knots, functions and stitches only. Sidebar
     columns (`app/src/write_view.rs`):
     - **Files (200px):** the entry first and bold, with problem counts;
       a collapsible `std` folder whose files open in Script, since the
       manuscript doesn't hold them; `N problems` at the foot.
     - **Structure (240px):** Knots with a `+`, Functions, Globals with
       their values faint. Hover shows `+` (new stitch) on knots and `⋯`
       on every row (Go to, Play from here, New Stitch, Promote/Demote).
     The current file is the caret's, or the file at the top of the
     scroller. The manuscript follows the caret through its sections' focus
     and through reveals. The title bar paints its left end as the sidebar
     while open, with the toggle (`ToggleWritingSidebar`) right of the
     traffic lights, and shows `story · knot › stitch`.
     `ToggleStructureColumn` is the second column's toggle, mirrored by a
     button in the Files header. **Not yet:** a `+` for Functions and
     Globals (no creation flow exists to reuse), back/forward and search in
     the bar, a slide animation for the sidebar, and remembering it open
     across launches.
6. **The bare-page chip** (W9): word count and problem count, bottom-right;
   clicking opens the sidebar on the problems.
   - *Built:* the chip sits over the manuscript's bottom-right corner (so
     it stays beside the text when the Player is out) while the sidebar is
     closed. Words are counted in the Read view's prose ranges, so markup,
     comments and code don't count; the count is cached per analysis.
     Clicking opens the sidebar, whose Files column carries each file's
     problems and the total. The sidebar has no problems *list* yet, so
     "on the problems" means the counts.
7. **Scripting's lean status bar** (S1): drop the absolute path and the
   timing cells; make sure the Output log carries the timings.
   - *Built:* the status bar is `N files`, `N problems` (opens Problems),
     the session state, then the file and `Ln, Col`; counts of one are
     singular now. The Output log already logged each notable analysis's
     time (the first, a moved problem count, a slow one, or every pass
     under Verbose). Its rows now also carry the session's worst timing
     when it isn't this one.

### 4.2 Verification

Every slice is verified with the headless harness
(`app/src/harness.rs`, decision log 2026-10-03), not by driving the screen:
behaviour as tests, the look as screenshots compared against the canvas.
`crates/brink-gpui` is not built by CI, so each PR runs its fmt, clippy
(`-D warnings`) and tests by hand and lists them.

### 4.3 Later

- Migrate the Writing sidebar into Scripting mode, replacing the Binder
  (W11).
- Default keybindings for the mode switch, Read, Play and the sidebar (R4).
