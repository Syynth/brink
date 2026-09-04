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

### Measuring it properly

The first pass of this sweep bucketed by module and got the answer badly
wrong — it counted any module that renders anything as widget-dependent. The
right test is which CodeMirror decoration kind a module actually uses:
`Decoration.mark`/`.line` is **pure styling** and maps onto `TextDecoration`
today; `Decoration.replace`/`.widget` is the capability that is missing.
Counting that way:

| Module | mark/line | replace/widget | Verdict |
|---|---:|---:|---|
| `argument-widgets.ts` | 0 | 15 | **Widget** |
| `inline-name-input.ts` | 0 | 2 | **Widget** |
| `rename.ts` | 1 | 3 | **Widget** |
| `extract-actions.ts` | 0 | 4 | **Widget** |
| `inlay-hints.ts` | 0 | 3 | **Widget** |
| `screenplay.ts` | 4 | 3 | Mixed — see below |
| `hir-overlay.ts` | 5 | 0 | Styling |
| `inline-markup.ts` | 2 | 0 | Styling |
| `search-card.ts` | 2 | 0 | Styling |
| `execution-highlight.ts` | 1 | 0 | Styling |
| `references.ts` | 1 | 0 | Styling |

And seven modules use **no decorations at all** — `search-results-buffer`
(465), `argument-form` (490), `color-picker-ui` (225), `color-widget` (56),
`widget-registry`/`-modal`/`-popover` (264). These are overlay and
composition UI, not editor capability: GPUI does popovers natively, and a
buffer-of-buffers is the same `list`-of-editors the Continuous view already
uses.

**`screenplay.ts`'s only widget is `EmptySigilWidget`**, which hides the
`:<>` suffix. Maintainer ruling (2026-09-04): **that hiding can go** — the
cue/screenplay treatment only has to colour and highlight correctly. Drop
it and the module is its four `mark` decorations, i.e. pure styling.

### Corrected buckets

| Class | Lines | Share |
|---|---:|---:|
| Truly needs in-text widgets | 2,518 | 12% |
| Needs a custom gutter (separate, easier axis) | ~1,025 | 5% |
| Styling, overlay or composition — seam exists | ~5,650 | 27% |
| Plumbing, not editor capability | 8,864 | 42% |
| (remainder: small helpers) | ~3,000 | 14% |

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

**In-text widgets — 2,518 lines, five modules.** These place an element in
the text flow and cannot be expressed as styling:

| Module | Lines | What it puts in the line |
|---|---:|---|
| `argument-widgets.ts` | 1,120 | Per-call argument chips — the argument-widget spec's inline surface |
| `inline-name-input.ts` | 590 | The in-editor rename chip with its "⚠ breaks N" badge |
| `rename.ts` | 412 | The rename affordance around it |
| `extract-actions.ts` | 254 | The extract prompt |
| `inlay-hints.ts` | 142 | Inlay hints — also missing from `gpui-base`'s LSP set |

**Custom gutters — a separate and much easier axis.** `play-from-here.ts`
(816) and `host-gutter.ts` (209) need per-line gutter markers;
`gutter-layout.ts` (331) is a WebKit workaround that simply disappears. The
gutter already renders per-line elements (fold icons) through a
host-supplied renderer, so generalising it is contained work.

**Absent LSP providers:** signature help and inlay hints. `lsp/` has
completions, hover, definitions, code actions, document colors and semantic
tokens — adding two more follows an established shape.

**One whole component:** `conflict-view.ts` (234) wraps
`@codemirror/merge`'s 2-way MergeView; there is no GPUI equivalent.

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
editor widget."** Round 4 established that using `gpui-component` at all
means vendoring it. Once vendored, what is missing is our work in our fork,
and it is not equally hard:

- **Custom gutters** — contained. The gutter already paints per-line
  elements via a host-supplied renderer; generalising that to arbitrary
  markers is a bounded change on a surface that already exists.
- **Signature help / inlay hints as providers** — two more alongside six,
  following the same shape.
- **In-text widgets** — the one real unknown. `gpui-base` lays out per-line
  shaped text with fold-collapsed ranges; inserting a measured element into
  a line means teaching the line layout a third span kind that both the fold
  map and the wrap map have to see. **Not prototyped. This is the next
  probe if this goes further.**

## Honest reading

- **Roughly 85% of the editor package is already proven, evaporates, or maps
  onto a seam that exists.** The plumbing does not port because a native
  editor has no wasm boundary to survive; the styling maps onto
  `TextDecoration`; the overlay and composition UI is ordinary GPUI, of
  which this spike has already built two examples.
- **12% genuinely needs in-text widgets**, and it is concentrated: argument
  widgets are two thirds of it. The HIR overlay, inline markup, execution
  highlight, references and (with the sigil-hiding dropped) screenplay are
  all *styling* — they were miscounted in the first pass of this sweep.
- **So the decision narrows to one question:** can the vendored editor learn
  to put an element inside a line? If yes, everything else here is wiring or
  contained work. If no, the argument-widget surface specifically cannot be
  hosted natively as specified — and that is a much smaller blast radius
  than "one third of the editor".
