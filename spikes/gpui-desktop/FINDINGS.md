# Spike — a GPUI-native brink studio (Zed-style)

**Date:** 2026-09-04 · **Status:** two rounds complete, no ruling requested yet ·
**Base:** rebased onto `main` @ `ecfea3c9b` ·
**Code:** `spikes/gpui-desktop/` (its own cargo workspace, untracked by the
root one — same exclusion pattern as `packages/brink-desktop/src-tauri`)

## The question

Can the existing Rust analysis engine (`brink-ide` / `brink-db`) drive a
native GPU-rendered desktop window directly — no wasm, no CodeMirror, no
React — and would that be a better authoring surface than the Tauri +
webview studio?

## Verdict

**Feasible, and further along than expected.** A working shell — file list,
code editor, syntax highlighting, hover, completion, diagnostics, live
re-analysis on every keystroke — is **~700 lines of Rust** against
`gpui-component`, with the whole brink analysis stack linked in directly as
path dependencies. Everything below was observed running, not inferred.

What is *not* settled by this spike: the whole rest of the studio (Player,
story graph, binder, debugger, settings, search cards, the command system,
the region model in `docs/studio-shell-spec.md`) is untouched. See "What a
port would actually cost".

## What the spike does

| Surface | Wiring | Result |
|---|---|---|
| Project load | walk dir → `IdeSession::update_source` per file, `brink.toml` via the compiler's own `discover_from_entry_in_tree` | 44 files / 13.9k lines loaded + fully analyzed in **31.5 ms** (release) |
| Syntax highlighting | custom `InputHighlighter` over `brink_ide::semantic_tokens` | works on `.brink` and `.ink`, **no tree-sitter grammar** |
| Diagnostics | `db.diagnostics(file)` → `effective_severity` → editor `DiagnosticSet` + a Problems strip | live, squiggles + list |
| Hover | `brink_ide::hover::hover` → markdown popover | live |
| Completion | `detect_completion_context` + symbol index + `stdlib_completions` | live, project-wide knots |
| Re-analysis | `update_and_analyze` on every edit | see perf below |

## The one real trap found

`gpui-base` exposes LSP-shaped provider traits, including
`DocumentRangeSemanticTokensProvider`. Wiring brink's semantic tokens
through it **computes correctly and paints nothing** — silently. The paint
pass (`element.rs::highlight_lines`) returns early when the editor has no
`InputHighlighter`, and `gpui-component` only ever builds one from
tree-sitter, for a language with a registered grammar. Semantic tokens
*layer over* a highlighter; they cannot be the only source.

The fix is not a `.brink` tree-sitter grammar. It is to implement
`InputHighlighter` directly over brink's own CST + analysis and install it
with `set_highlighter_factory` **before** `gpui-component`'s render calls
`ensure_highlighter_factory` (which only fills an empty slot). That is
`BrinkHighlighter` in `src/main.rs`, and it is the honest architecture: one
parse, brink's own, feeding both analysis and paint. Colour names come from
`brink_ide::semantic_tokens::token_type_names()`, which already lines up
with the theme vocabulary (`comment`, `keyword`, `function`, `string`, …);
brink's narrative-specific names (`marker`, `divert`, `halt`, `escape`)
resolve to nothing today and would want theme entries.

## Perf

Measured by typing real keystrokes into `bandn/rina.ink` (1,892 lines)
inside the 44-file project, on this Mac (M-series, macOS 26.5).

| | debug build | **release build** |
|---|---|---|
| Project load (44 files, full analysis) | 224 ms | **31.5 ms** |
| Per keystroke — `update_and_analyze` | p50 10.3 / p95 10.8 ms | **p50 1.14 / p95 1.37 ms** |
| Per keystroke — total (analyze + full-file token recompute) | ~24 ms | **p50 2.41 / p95 2.73 ms** (max 3.45) |

n=46 keystrokes, same typed sentence in both builds. Re-measured after
rebasing onto `main` at `ecfea3c9b` (56 commits, including the repo's own
move to Rust 1.98.1): load 30.3 ms, analyze p50 1.13 ms, total p50
2.40 ms — unchanged.

Two things to keep in mind before quoting these:

1. **The token recompute is deliberately naive.** `BrinkHighlighter::update`
   re-derives semantic tokens for the *whole* file on every keystroke
   (~14 ms of the debug 24; ~1.3 ms of the release 2.4). A real
   implementation would recompute the edited
   range, as `#3064`'s per-segment memoization already does on the db road.
2. **Comparison point.** `project_editor_perf_real_usage` measured the web
   studio at **input p50 48 ms / p95 80 ms** per keystroke on a 1,125-line
   file — but that was dominated by two specific defects (#3490 eager HIR
   projection pulls, #3491 main-thread prose checking), both of which have
   since been worked. The right reading is *not* "native is 2× faster than
   web"; it is that **the same analysis work costs ~1 ms here, and the
   overhead around it is small and in our own hands** rather than spread
   across a wasm boundary, a worker, CodeMirror, and React. At **2.4 ms**
   a keystroke costs about a seventh of a 60 fps frame, with no
   optimization work done at all.

Prose checking (Harper, the #3491 freeze) is not wired in this spike at
all — in a native shell it is an ordinary background thread, not a wasm
module on the main thread.

## Toolchain / dependency notes

- **crates.io `gpui` is stale** — 0.2.2, Oct 2025. The live line is
  longbridge's `gpui-pre` (0.3.3, published 2026-09-03), a snapshot of Zed's
  gpui, plus `gpui-base` / `gpui-component` 0.6.0 (the *GPUI Kit* stack,
  extracted from a shipped commercial app).
- **`gpui-pre` publishes no platform backend.** Windows only open once
  `gpui-pre-platform` is added and the app is built with
  `Application::with_platform(gpui_platform::current_platform(false))`.
  `Application::new()` does not exist.
- **No Metal toolchain needed.** This Mac has no `xcodebuild
  -downloadComponent MetalToolchain`, and the build and render work anyway.
- **Cold build: ~3m20s**, 435 crates. Incremental edit-rebuild: **~3 s**.
- `tinyvec` 1.13.0 (published 2026-09-03) fails to compile on both 1.97.1
  and 1.98.1 (`cannot find macro vec`); the spike's lockfile pins 1.11.0,
  the version the root workspace already resolves to.
- The spike links `brink-ide`, `brink-db`, `brink-ir`, `brink-analyzer`,
  `brink-driver`, `brink-project-config` as **path dependencies from a
  separate workspace** — exactly the arrangement `src-tauri` already uses,
  and it worked with no changes to those crates.

## Round 2 — one complex studio surface, rebuilt: the Binder

The first round proved the engine reaches a native editor. It said nothing
about what porting the studio's *own* UI feels like, so the second round
takes the Binder — the file/symbol tree — and rebuilds it against the same
rules the studio's version follows.

**Size:** `src/binder.rs` + `src/icons.rs` = **1,286 lines of Rust**, against
**4,208 lines** for the studio's (`Binder.tsx` 2,271, `BinderContextMenu.tsx`
417, `slices/binder.ts` 816, `binder-order.ts` 152, `binder.css` 552). Not a
like-for-like ratio — the native version skips the undo stack, the Library
section, multi-select, inline create, and sidecar persistence — but the
*shape* is the same widget.

**What works, verified by driving it:**

| Feature | Notes |
|---|---|
| Files / Structure modes (#3036) | 47 rows → 236 rows on the same project |
| The fill rule (ruled 2026-08-23) | Icon IS the expander; filled = collapsed over content, outline = expanded/leaf; folders swap to the open silhouette |
| Entry file mark (#3014/#3021) | The brand drop with the divert carved out — the SVG **mask** renders, so the collapsed/expanded pair is pixel-faithful |
| Diagnostic marks (#3041) | Per-file roll-up and per-symbol counts over each symbol's `full_range` |
| Out-of-scope dimming | With the "closure empty means nothing to contradict" rule |
| Drag to reorder | Insertion line between rows, drop-into highlight on folders |
| Filter | Prunes folders left empty |
| Keyboard nav | ↑↓ move, → expand/descend, ← collapse/ascend, Enter opens |
| Row actions | Right-click menu, plus the hover-revealed ⋯ |
| Open → editor | Clicking a knot row opens the file and reveals the symbol |

**The icon language ports verbatim.** GPUI paints an SVG as a monochrome
mask tinted by the element's text colour, so the studio's `currentColor`
icons transfer with their `d` attributes unchanged — including the entry
mark's `<mask>`, the draft drop's `stroke-dasharray`, and the chevrons'
`opacity=".45"` second stroke. `svg().data(bytes)` takes them inline, so no
asset pipeline is involved.

### The finding that matters: drag-and-drop

The studio's binder drag needed **two WebKit-specific fixes**: #3351 (WebKit
requires `preventDefault()` on both `dragenter` *and* `dragover`; the code
wired only `dragover`, so folder reorder silently did nothing in
Tauri/Safari while Chromium worked), and its follow-up (an unscoped
`-webkit-user-drag: element` rule re-armed rows React had rendered
`draggable={false}`, because in WebKit the `draggable` attribute is a
*presentational hint* that loses to author CSS in the cascade).

Neither failure mode exists here. GPUI's drag is a typed value with a real
preview view — `on_drag(payload, …)`, `on_drag_move::<T>`, `on_drop::<T>` —
so there is no `dataTransfer` string to encode into, no `dragenter`/
`dragover` contract to satisfy, and no cascade that can re-arm a row the
code declared undraggable. Reorder worked on the first attempt with no
platform-specific handling. **That is the clearest case in either round for
what the native surface buys: a whole category of browser-contract bug is
absent by construction, not fixed.**

### What was harder than expected

- **`gpui-component` 0.6.0 has no click-triggered popup menu** — only the
  right-click `ContextMenu`; `PopupMenuExt` does not exist in the published
  crate. The studio's hover ⋯ affordance therefore had to be built directly
  (`anchored` + `deferred`, ~50 lines). Fine, but it is the kind of gap that
  a UI as large as the studio would hit repeatedly.
- **Borrow discipline in `render`.** `cx.theme()` immutably borrows `cx`
  while `cx.listener(…)` and child-render calls want it mutably, so colours
  have to be copied out first and helpers must return `AnyElement` rather
  than `impl IntoElement` (which would capture `cx`'s lifetime). Mechanical,
  but it shapes every render function and would need to be a house pattern.
- **One real interaction bug the port introduced:** revealing a symbol calls
  `set_cursor_position`, which focuses the editor — killing the binder's own
  arrow-key navigation after the first click. Fixed by re-focusing the panel
  after an open, which is Zed's project-panel behaviour.

Zed's own `crates/project_panel` (21k lines) was used as the implementation
reference for the drag wiring and `uniform_list` usage.

## What a port would actually cost

The spike proves the *hard* part (engine → native editor) is easy. The bulk
of the work is the part it skipped:

- **Rewrite, not port, the studio UI.** `packages/brink-studio`,
  `studio-ui`, `studio-shell`, `studio-store`, `ink-operations`,
  `ink-editor` are TypeScript/React/CM6. None of it transfers. The
  *specs* transfer (`docs/studio-shell-spec.md`'s region model,
  `docs/search-results-cards-spec.md`, the debugger UI round), the code does
  not.
- **What `@brink-lang/web` and `@brink-lang/editor` are for.** They are
  published packages with an external embedding consumer
  (`project_real_editor_consumer`), and the browser playground is how the
  editor acceptance gate is exercised. A native shell does not replace them;
  it would be a *third* consumer alongside browser and Tauri, or it would
  mean deciding the web surface is no longer the product.
- **The editor acceptance gate** (`crates/brink-web/src/editor/acceptance_gate.rs`)
  guards the wasm `EditorSession`. A native shell driving `IdeSession`
  directly bypasses it; it would need its own equivalent, or the gate would
  need to move down a layer.
- **Mobile is foreclosed.** The 2026-08-06 desktop ruling kept Tauri 2
  partly because it keeps a future iOS/Android client on the same stack.
  GPUI has no mobile story.
- **Ecosystem risk.** `gpui` is versioned by Zed for Zed; the usable
  published line today is a third party's snapshot crate (`gpui-pre`) that
  is three days old. `gpui-component` is credible (Longbridge Pro ships on
  it) but this is a much younger dependency than Tauri + the web platform.

## Reproducing

```bash
cd spikes/gpui-desktop
cargo run --release -- ../../tests/tier1-native/conventions-cross-file
cargo run --release -- ../../tests/tests_github/Boyquotes__signal_creek/assets/ink
```

Per-keystroke timings print to stderr. The spike never writes to disk —
edits live in memory only.
