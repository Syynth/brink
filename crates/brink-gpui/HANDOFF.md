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
| `app/` | `project.rs` (the mirror entity), `document.rs` (editor + highlighter + providers), `code_view.rs` (documents, tabs, the active file), `single_view.rs`, `continuous.rs`, `binder.rs`, `problems.rs`, `main.rs` |

**Verified running** (screenshots taken against the real app, on macOS,
before the views landed): rails with both groups, the Binder, syntax
highlighting from brink's own CST with no tree-sitter grammar, dock
toggling, the status bar, and project load in ~10 ms. **The tab bar, the
three views and the switcher have NOT been seen running** — they were
built in a cloud session with no display; they compile, format, pass
clippy and the unit tests. First thing on a machine with a screen: open a
project, confirm tabs appear for two documents, and cycle the three views.

**Linux builds** (verified 2026-09-05, contrary to the earlier worry): the
whole workspace, app included, builds and links on an ubuntu container with
`libxkbcommon-dev` and `libxkbcommon-x11-dev` installed; `xcb`,
`fontconfig` and `freetype` were already present.

**Not verified by hand**, because GPUI's text area no-ops the macOS
accessibility text-insert path so keystrokes could not be injected: typing,
hover, completions, and `cmd-s` save. The edit → analyze → diagnostics path
*is* covered by `model/src/worker.rs`'s tests; the UI half of it is not.

## Known broken / unfinished, most blocking first

1. ~~**The centre dock draws no tab bar.**~~ **Fixed 2026-09-05**: the
   area had no `DockSkin`, so it wore gpui-base's bare renderer. The three
   views (spec §4.4) then landed on top: the centre holds one `EditorRoot`
   panel that renders Code (an inner `DockArea` of documents, with tabs),
   Single File, or Continuous; switcher in the title bar, `cmd-shift-1/2/3`.
2. **No CI lane.** Nothing runs this workspace's tests or fmt. Adding one
   means a macOS runner (or solving the Linux question above) — worth a
   ruling on whether the GUI tier is gated at all, or only `model` + `shell`.
3. **Rail toggling is dock-level, not tab-level.** `Workspace::toggle_tool_
   window` opens/closes the whole dock; the toolkit exposes no way to
   activate one panel inside a tab group from outside it. Fine while each
   dock holds one tool window, wrong as soon as one holds two.
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
