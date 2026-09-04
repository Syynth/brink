# Editor sweep — CM6 vs `gpui-base`

**Date:** 2026-09-04 · **Status:** audit, no ruling requested ·
**Scope:** `packages/ink-editor/src` (21,128 non-test lines, 69 modules)
against `gpui-base`'s editor (18,457 lines, the widget the three spike
rounds used).

This is the inventory the spikes could not settle: **what the studio's
editor actually does, and which of it the native editor can carry.**

## The headline

One capability decides most of this. **CodeMirror's `WidgetType` — an
arbitrary DOM element placed inline or as a block inside the text flow —
has no `gpui-base` equivalent.** The only elements `gpui-base` puts inside
an editor are fold icons in the gutter and the ghost text of an inline
completion (`element.rs`: the sole `AnyElement`s in the layout). There is
also no custom gutter API: line numbers and the fold column are the gutter.

`gpui-base` offers *styling* over ranges (`TextDecoration` =
`Range<usize>` + `HighlightStyle`) and squiggles via `DiagnosticSet`. That
covers colouring and underlining. It does not cover **putting a thing in the
line**.

Bucketing the package by what it needs:

| Class | Lines | Share |
|---|---:|---:|
| Needs in-text widgets or custom gutters — **no seam today** | 6,988 | 33% |
| Maps onto an existing `gpui-base` seam | 3,976 | 19% |
| Plumbing, not editor capability | 8,864 | 42% |
| (remainder: small helpers) | ~1,300 | 6% |

## What ports directly — proven, not predicted

These the spike already ran end to end:

| Studio module | `gpui-base` seam | State |
|---|---|---|
| `highlight.ts` | `InputHighlighter` | **Working** — brink's own CST drives it, no tree-sitter grammar |
| `diagnostics.ts` | `DiagnosticSet` | **Working** — squiggles + Problems list |
| `hover.ts` | `HoverProvider` | **Working** |
| `completions.ts` | `CompletionProvider` | **Working** |
| `folding.ts` | `FoldRange` + fold map | Seam exists (fold gutter is built in) |
| `goto-definition.ts` | `DefinitionProvider` | Seam exists |
| `code-actions.ts` | `CodeActionProvider` | Seam exists |
| `find-panel.ts` | `SearchSession` | Seam exists, incl. replace-all |
| `theme.ts` | theme tokens + `HighlightTheme` | Working |
| `keybindings.ts` | GPUI actions + key contexts | Working |
| `color-widget` *(picker half)* | `DocumentColorProvider` | Seam exists for the swatch; the **picker popover** is a widget |

Plus what `gpui-base` gives for free that the studio had to build or buy:
soft wrap with wrapping indent, indent guides, a fold map, undo history, IME,
multi-line selection, a context menu, and a search session.

## What has no seam

**In-text widgets.** Everything here is a CM6 `WidgetType`:

| Module | Lines | What it puts in the line |
|---|---:|---|
| `argument-widgets.ts` + `argument-form.ts` | 1,610 | Per-call argument chips and forms — the argument-widget spec's whole surface |
| `inline-name-input.ts` | 590 | The in-editor rename chip with a "⚠ breaks N" badge |
| `search-results-buffer.ts` + `search-card.ts` | 843 | Editable result cards — *nested editors as widgets* |
| `hir-overlay.ts` | 605 | Structural marks with `data-*` identity + rails |
| `inline-markup.ts` | 453 | Host-defined inline markup rules |
| `screenplay.ts` | 376 | Hidden sigil suffixes (`:<>`), i.e. atomic/replaced ranges |
| `inlay-hints.ts` | 142 | Inlay hints — also absent from `gpui-base`'s LSP set |
| `color-picker-ui.ts` + `color-widget.ts` | 281 | The HSV picker popover |
| `widget-registry` / `-modal` / `-popover` | ~200 | The registry hosts attach widgets through |

**Custom gutters.** `play-from-here.ts` (816), `host-gutter.ts` (209),
`gutter-layout.ts` (331) — the hover-revealed ▶ run icon, breakpoints, and
the published host gutter-marker API. `gpui-base` has one gutter and it is
not extensible.

**Whole components with no counterpart:** `conflict-view.ts` (234) wraps
`@codemirror/merge`'s 2-way MergeView; there is no GPUI merge view.

**Absent LSP providers:** signature help and inlay hints (`lsp/` has
completions, hover, definitions, code actions, document colors, semantic
tokens — and nothing else), so `signature-help.ts` and `inlay-hints.ts` have
no home even before the widget question.

## What disappears

The 42% marked plumbing is not work to redo — much of it is work that
**stops existing**:

- `worker/` (4 modules), `deferred-refresh.ts`, `idle-schedule.ts`,
  `perf/wasm-proxy.ts` — the async session facade, the worker transport, and
  the scheduling that exists because analysis must not block the main
  thread across a wasm boundary. Natively, `IdeSession` is a direct call at
  **1.14 ms p50** (round 1). Most of this machinery has no reason to exist.
- `document-sessions.ts` (1,893) + `document-handle.ts` (646) — per-view
  wasm document handles and their lifecycle. Native code holds a `FileId`.
- `convert.ts`, `classifier-mirror.ts` — TS mirrors of Rust wire shapes.
  Gone: the types *are* the Rust types.
- `tooltip-portal.ts`, `hanging-indent.ts`, `gutter-layout.ts` — three
  modules that exist to work around browser/WebKit layout behaviour.

`project-session.ts` (1,641) is the real exception: genuine session logic
that ports as ordinary Rust rather than disappearing.

## Tractability

**The question is not "can GPUI do this" — it is "are we willing to own an
editor widget."** Round 4 already established that using `gpui-component` at
all means vendoring it. Once vendored, adding what is missing is *our*
work in *our* fork:

- **In-text widgets** are the load-bearing item. `gpui-base` already lays
  out per-line shaped text with fold-collapsed ranges and paints elements in
  the gutter; inserting a measured element into a line means teaching the
  line layout about a third span kind. That is real editor work — the fold
  map and wrap map both have to see it — and it is the single largest
  unknown in this evaluation. **It has not been prototyped, and it should be
  the next spike if this goes further.**
- **Custom gutters** are much easier: the gutter already renders per-line
  elements (fold icons) via a host-supplied renderer. Generalising that to
  a per-line marker API is a contained change.
- **Signature help / inlay hints** are new providers alongside six that
  already exist, following the same shape.

## Honest reading

- **Two thirds of the editor package is either already proven on the native
  side or evaporates.** The direct-seam work is wiring, and the spike ran
  four of those seams for real.
- **One third rests on a capability that does not exist**, and the most
  distinctive parts of the authoring surface — argument widgets, the HIR
  overlay, screenplay sigils, editable search cards, play-from-here — are
  exactly that third. These are not incidental features; several are ruled
  spec surfaces.
- **The decision is therefore about the widget layer, not about GPUI.**
  If in-text widgets are tractable in the fork, the rest follows from work
  already demonstrated. If they are not, the native editor cannot host the
  studio's authoring surface as specified, and no amount of framework choice
  changes that.

**Recommended next probe, if any:** implement one in-text widget end to end
in the vendored `gpui-base` — an inlay hint is the smallest honest test,
an argument chip the real one. Everything else in this sweep is estimable;
that is not.
