# The GPUI-native studio — architecture

**Status:** ruled 2026-09-04, unimplemented ·
**Code:** `crates/brink-gpui/` ·
**Supersedes for the native surface:** `docs/studio-shell-spec.md`'s region
model (the web studio keeps it unchanged) ·
**Rests on:** decision log 2026-09-04 — "The GPUI-native app is the
destination", "Both studio consumers sit on the same layer", "The native
studio's region model drops the bottom rail", "No debounce".

Every number in this document was measured on this hardware in a release
build, not estimated. The benches are named where they appear.

## 1. What this is

The native studio replaces the Tauri/webview studio. This document settles
its internal structure: how the code is layered, where analysis runs, what
the window is made of, and what the first slice builds.

It does **not** re-open language or authoring semantics. Those are ruled
elsewhere and the native app implements them unchanged.

## 2. Layering

Three tiers, following Zed's own split (surveyed 2026-09-04 at `c91e24a`).

The instructive part of Zed's layout is *where the seam is not*: `gpui` is
not the UI layer. It is the **application** layer — entity graph,
subscriptions, executors, `Task` — and 157 of Zed's 244 crates depend on it,
including `text` (the rope/CRDT) and `project` (82k lines, ~40 event
variants, 150 background spawns, renders nothing). The seam is `ui`/`theme`:

| Zed crate | loc | gpui | ui |
|---|---|---|---|
| `text` | 6.6k | yes | no |
| `language` | 26k | yes | no |
| `project` | 82k | yes | no |
| `workspace` | 53k | yes | yes |
| `editor` | 171k | yes | yes |

The second half is the inversion: `workspace` defines `Item` (a thing in a
tab) and `Panel` (a thing in a dock) as **traits**, and depends on `project`
but **not on `editor`**. Features implement the traits; the shell never
learns what they are; the top-level `zed` crate does the concrete wiring.

Brink mirrors this with three crates:

| crate | contains | may depend on |
|---|---|---|
| `brink-gpui-model` | `Project`, `Document`, `StorySession` entities over `IdeSession`; the analysis worker; events | gpui, brink-* |
| `brink-gpui-shell` | regions, rails, docks, tabs, commands, layout persistence; the `Item`/`Panel` traits | model, gpui-component |
| `brink-gpui` | Binder, Editor, Problems, Player, Program Explorer; `main` | shell, model |

The shell must not depend on the feature crate. That edge is the one this
split exists to prevent, and it is the expensive one to retrofit.

Within a tier, one crate until it hurts.

## 3. Threading

### 3.1 The ruling

**No debounce.** Every keystroke starts its work immediately. Debounce buys
latency headroom by making the user wait a fixed interval for their own
edits to be reflected, and it lets per-keystroke work stay O(file) or
O(project) indefinitely because the cost is merely paid less often.
Removing it forces each keystroke's main-thread work to be **O(edit)**,
which is the structurally correct shape.

### 3.2 The two costs

They scale on different axes and are solved differently.

**Project analysis — scales with project size.** Measured with a synthetic
project of cross-file-diverting `.ink` files (`scale` bench):

| project | lines | words | cold analyze | keystroke (median / max) |
|---|---|---|---|---|
| studio-scale | 14k | 101k | 61 ms | 0.90 / 1.5 ms |
| Disco Elysium scale | 113k | 807k | 473 ms | 7.8 / 11.6 ms |
| 3x | 338k | 2.4M | 1.43 s | 23 / 37 ms |
| 6x | 676k | 4.8M | 3.0 s | 53 / 102 ms |

Linear in words out to 4.8M — there is no cliff. But 7.8 ms at the scale of
a large commercial script is 8x Zed's own synchronous budget, so this does
not belong on the main thread.

**Paint tokens — scales with file size.** Whole-file parse + classify is
already over 1 ms at 282 lines, and 21.6 ms at 16.8k lines (`paint` bench).
It cannot run per keystroke either.

### 3.3 The design

Split by *does the next frame need it*, not by *is it slow*.

**Main thread — the open document's syntax.** Per keystroke:

1. `brink_syntax::segment_file(source)` — lex-only, splits into one segment
   per top-level knot/stitch (#3084, `docs/per-knot-incremental-lowering-spec.md`).
2. Reparse and classify **only the edited segment**, via
   `brink_ir::semantic_tokens::tokens_with_kinds(source, &root, &kinds)`.
3. Paint.

Measured (`incr` bench):

| file | whole-file | segment (lex) | one knot | incremental total | speedup |
|---|---|---|---|---|---|
| 282 lines | 0.97 ms | 0.09 | 0.051 | **0.15 ms** | 7x |
| 1,402 | 2.96 ms | 0.22 | 0.023 | **0.24 ms** | 12x |
| 5,602 | 7.17 ms | 0.65 | 0.018 | **0.67 ms** | 11x |
| 16,802 | 21.6 ms | 2.22 | 0.017 | **2.24 ms** | 10x |
| 56,002 | 77.4 ms | 7.46 | 0.017 | **7.48 ms** | 10x |

Reparsing one knot is **17–51 microseconds and flat** — independent of file
size. The residual O(file) term is the lex-only segmentation pass, ~10x
cheaper than parsing; it stays under 1 ms through ~6k-line files and is
itself incrementalizable later if a real project needs it.

**Worker thread — the project.** It owns the single `IdeSession` outright.
`IdeSession` is already `Send` (`brink-lsp` runs it as
`Arc<Mutex<NativeProjects>>` under a multi-threaded server), so it *moves*
rather than being shared. It receives edits and returns **plain data**:
diagnostics, the refined `kinds` map, resolved symbol information.

### 3.4 Why no database snapshots

An earlier draft proposed `ProjectDb: Clone` plus salsa snapshot handles
(rust-analyzer's model). That is **rejected as unnecessary**, not as
unworkable.

The clone itself is cheap — `salsa::Storage::clone` is two `Arc` bumps and a
counter; memo tables live in `Arc<Zalsa>` and are shared, not copied. The
real cost is cancellation: writing an input requires `zalsa_mut`, whose
`cancel_others` "sets cancellation flag and blocks until all other workers
with access to this storage have completed". Cancellation is cooperative, so
a keystroke's latency becomes *how long until the background query reaches
its next checkpoint* — an unbounded quantity we would then have to bound.

Moving the session wholesale to a worker avoids the question. **`brink-db`
requires no changes.**

### 3.5 Staleness

Only the `kinds` map can lag, and lag degrades **semantic refinement**
alone — an identifier not yet known to name a knot. Structure (keywords,
strings, comments, diverts, choices, tags) is decidable from syntax and is
always current. Nothing ever renders from a stale *structure*, which is a
stronger guarantee than Zed's interpolated-syntax-tree fallback offers.

Diagnostics carry the revision they were computed at, and are shifted past
subsequent edits rather than hidden or re-rendered at wrong offsets.

## 4. The window

### 4.1 Region model

Simplified from `docs/studio-shell-spec.md` (ruled 2026-09-04): **the bottom
rail is removed**, following what JetBrains actually does rather than what
the earlier spec assumed. Four rail slots address three docks:

| rail slot | dock | position |
|---|---|---|
| left, upper | left dock | — |
| right, upper | right dock | — |
| left, **lower** | **bottom dock** | left |
| right, **lower** | **bottom dock** | right |

Plus the editor center and a status bar. **Five surfaces, one placement
rule**: a tool window's rail slot is the only place its home is declared,
and re-homing is one operation.

The bottom dock is a horizontal `Split` of two `Tabs`. Degenerate cases need
no special handling — `gpui-kit`'s `normalize.rs` rule 2 replaces a
one-child `Split` with that child, which keeps its `NodeId`, so a
single-sided bottom dock takes the full width without tearing down the
panel entity, and re-splits when the other side opens.

Free-form docking (`Tiles`) is available in the toolkit and deliberately
unused, per `studio-shell-spec`.

### 4.2 Toolkit

`gpui-component`'s `DockArea` is adopted. It is not a compromise: its dock
layout is a full pane tree —

```rust
enum NodeKind {
    Split { axis: Axis, children: Vec<PaneNode>, sizes: Vec<Option<Pixels>> },
    Tabs  { panels: Vec<PanelId>, active_ix: usize },
    Tiles { panels: Vec<TilePanel> },
}
```

— so every dock splits and tabs natively, and `DockAreaState` serializes it.
Zed's own `Dock` is weaker here: it holds `active_panel_index:
Option<usize>`, one visible panel per edge, and splitting is a `PaneGroup` a
panel must opt into individually (its terminal panel does exactly that).

### 4.3 State

Entities and observation, never `Rc<RefCell<_>>`. The spike's shared cell
has no change notification, which is why it calls `rebuild()` by hand;
panels observe `Project` and re-render themselves.

`Document` owns its own identity — path, `FileId`, editor entity, dirty
flag — and providers are constructed against their document. This replaces
the spike's `ActiveKey` (`Rc<RefCell<String>>`) indirection, after which
tabs work by construction rather than by coordination.

### 4.4 Commands

GPUI `actions!` plus key contexts. Keybindings, palette entries, menu items
and buttons all dispatch the same action, which satisfies
`studio-shell-spec`'s command contract with no bespoke layer.

### 4.5 Persistence

`DockAreaState` keyed on `(edge, group)`, plus recents and settings. Stable
under everything except deliberate re-homing.

## 5. First slice

**Shell + Binder + Editor + save/open.** No Player.

It makes the app usable for writing, and it exercises every decision above —
the tier split, the worker boundary, incremental paint, the region model,
documents, commands, persistence — before anything else depends on them.

Two defects carried from the spike are fixed here, not later:

- it never mounts the stdlib, so no stdlib reference resolves;
- it hand-rolls `brink.toml` resolution instead of calling
  `IdeSession::apply_project_config`, which now exists at the shared layer.

## 6. Deliberately deferred

- Incrementalizing the segmentation pass itself (only needed past ~6k-line
  files).
- Moving the editor acceptance gate down onto the shared session — required
  by the layering ruling, not by this slice.
- Player, story graph, debugger, search cards, settings.
- The `#3064` per-segment token path on the db road is the worker's
  business; the main thread uses `segment_file` directly and needs no db.
