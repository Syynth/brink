# brink-gpui — handoff

**Written:** 2026-09-04 · **Branch:** `bronch/gpui-native-desktop-spike-f7a90c`
(27 commits ahead of `main`, all committed, nothing unpushed-and-uncommitted) ·
**Architecture:** `docs/gpui-studio-spec.md` — read that first; this file is
only what a fresh session needs on top of it.

## Read these first, in order

1. `docs/gpui-studio-spec.md` — the design, with every performance number
   measured rather than estimated.
2. `docs/decision-log.md`, the four entries dated 2026-09-04 beginning "The
   native studio's analysis runs off the main thread". Those are rulings, not
   proposals; do not re-litigate them.
3. This file.

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

**Commands** (2026-09-05, spec §4.5): every shortcut, the palette
(`cmd-shift-p`), the hamburger menu and the `cmd-1…9` tool-window toggles
go through one registry (`shell/src/commands.rs`); the app registers its
commands with `Workspace::register_command`. Verified headless: palette
filter + Enter switches views; the menu lists View/File groups with keys;
`cmd-1`/`cmd-2` toggle Binder/Problems from a fresh launch and after view
switches. Deferred, in the spec: user keymap overrides, `Escape` back to
the editor, quick-open. Do not bind `cmd-shift-<digit>` to anything — it
cannot match on Linux.

**Search** (2026-09-05, `app/src/search.rs`): the studio's engine (plain
or regex, case, whole word, one composed pattern, 1000-match cap) over the
mirror's current sources; per-match cards with `file:line`, containing
knot/stitch, the match line with 1↑2↓ context and the hit highlighted; a
frozen snapshot replaced only by a new query, an option, or `↻`; the
summary strip with the Binder's expand/collapse-all; `search.focus` on
`cmd-shift-f`. Verified headless. **Read-only cards** — the ruling makes
inline editing the point; the shared buffer that needs is now in (below),
so editable cards, replace previews and the `edited` badges are the next
Search slice. References mode waits on a worker query.

**The shared buffer** (2026-09-05, spec §6): the mirror is the canonical
text per file and every `EditorState` follows it through `SourceDelta`
broadcasts (`app/src/project.rs`, `Document::apply_delta`); dirty and save
are per file in the project. Verified: an edit in Code view appears in the
manuscript, an edit in the manuscript appears in Code view, one `cmd-s`
writes both. Search cards stay read-only until they become editors over
this buffer, which is now a UI change, not a model one.

Two things noticed in those screenshots and NOT yet fixed: the Binder
draws both the dock's title strip ("Binder") and its own header ("BINDER"
+ toolbar), and the manuscript's first section shows a partial row above
the next heading (the measured-line-height issue `continuous.rs` already
describes).

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
5. **No layout persistence.** `DockAreaState` has `dump`/`load` and
   `RailSlot::persistence_key` exists for exactly this; nothing calls them.
6. **`#3562` — `.brink` files have no incremental paint path.** Native
   segmentation does not exist, so a native file pays a whole-file parse per
   keystroke (2.1 ms at 700 lines, 12.4 ms at 8,400) where `.ink` pays
   17–51 µs per knot. **The boundary question is a language ruling and must
   not be decided by an agent.**

## Open, parked by the maintainer (2026-09-05)

- **Where the Player sits in each view.** Today it is three different
  answers: a session *document* in a Code-view split (ruled 2026-06-10), a
  companion split *native to* Single File view, and absent from Continuous.
  The maintainer's direction, noted for later rather than ruled: **Code** —
  a tab is fine, as today; **Continuous** — it has to *swap in and out*,
  because the manuscript is one scroller and a permanent split fights the
  scrolling; **Single File** — "a side-by-side split, maybe". So it is
  per-view, not one root-level companion. Do not build the native Player
  into any view until this is settled; the three-view work leaves the
  companion slot as a placeholder.

## Deliberately not done

- The **editor acceptance gate has not moved down** onto the shared session.
  The layering ruling (2026-09-04, "Both studio consumers sit on the same
  layer") requires it; this slice did not do it.
- Player, story graph, debugger, search cards, settings — all out of the
  ruled first slice.

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
