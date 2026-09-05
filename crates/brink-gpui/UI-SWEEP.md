# UI sweep — the custom surfaces, and the CSS underneath

**Date:** 2026-09-04 · **Status:** audit, no ruling requested ·
**Scope:** `packages/studio-ui` (18,965 non-test lines) and its **8,445 lines
of CSS**, against GPUI's styling and drawing model.

The editor sweep covered `ink-editor`. This covers the rest: the Program
Explorer and its treemap, the Player, and the general question — **how hard
is CSS-heavy custom UI to replicate?**

## The short answer

GPUI's layout *is* the CSS box model: taffy flexbox, plus `grid()`,
`grid_cols()`, `grid_rows()`, `aspect_ratio`, `BoxShadow`, `linear_gradient`,
`opacity`, container queries (`container_query.rs`) and a real animation
element. The studio's CSS was measured feature by feature, and almost all of
it has a direct counterpart.

| CSS feature | Uses | GPUI |
|---|---:|---|
| flex layout | pervasive | `h_flex` / `v_flex` — same model |
| `display: grid`, `grid-template` | 14 | `grid()`, `grid_cols()`, `grid_rows()` |
| `box-shadow` | 26 | `BoxShadow` |
| `text-transform:` | 19 | String casing, not a transform — uppercase the text |
| real `transform:` | 16 | See breakdown below |
| `::before` / `::after` | 21 | Not needed — add a real child element |
| `z-index` | 15 | Paint order + `deferred()` / `anchored()` |
| `linear-gradient` | 3 | `linear_gradient()` |
| `@container` | 4 | `container_query()` |
| `aspect-ratio` | 1 | `aspect_ratio` |
| `transition` | 21 | `Animation` — explicit, not declarative |
| `@keyframes` / `animation:` | 17 | `Animation::new().repeat().with_easing()` |
| `position: sticky` | 2 | **Absent** — draw an overlay (the Continuous view does) |
| 3D transforms (`rotateX`) | 2 | **Absent** |
| `backdrop-filter`, `clip-path`, `mask` | 0 | n/a — unused |

The 16 real transforms, enumerated: **4 rotations** (2×`rotate(90deg)`,
`rotate(-90deg)`, `rotate(0deg)` — disclosure triangles, which GPUI does with
a rotated SVG via `Transformation`), **2 `rotateX(180deg)`** (the card flip,
below), **3 `none`** resets, **3 static translates**, and **4 keyframe steps
of one shake animation** (`translateX` between -3px and 3px).

So the genuine absences are **`position: sticky` (2 uses)** and **3D
transforms (2 uses)**. Everything else in the table has a counterpart.

### Pseudo-elements are a downgrade in CSS, not a feature

21 of the "hard" uses are `::before`/`::after` — most of them the Player's
`player-spine` (a vertical line with nodes). Those exist because CSS cannot
add a child; a retained-mode tree just adds one. This is *easier* in GPUI,
not harder.

### Transitions are the real difference

CSS gives you an animation for free when a class flips. GPUI wants an
explicit `Animation` around the element. 21 transitions + 17 keyframe
animations is a bounded amount of work, and it is nearly all polish — but it
is the one place where the port is genuinely more typing than the original.

### The card flip, and the shake

`player-flip` uses `rotateX(180deg)` for a two-sided card — GPUI has no 3D
transform, so that becomes a cross-fade or a slide, both of which the
animation element does. One visual, two CSS rules.

The one keyframe animation with real motion is a 4-step `translateX` shake.
GPUI animates a position offset directly; this is a faithful port, not a
substitute.

## Program Explorer

| Piece | Lines | Verdict |
|---|---:|---|
| `treemap.ts` — squarified layout (Bruls/Huizing/van Wijk) | 88 | **Ports verbatim.** "Values in, rects out, deterministic" — pure geometry with no DOM in it. Rust is a better host for it than TS. |
| `ProgramSizeView.tsx` | 447 | Absolutely-positioned rects with a colour grammar. GPUI draws rects natively; `canvas()` is there if a single custom-painted surface is preferable. |
| `ProgramDisasmView` / `ProgramLinesView` / `ProgramView` | 1,329 | Virtualised lists and tables — `uniform_list` / `list`, already used by the binder and manuscript. |
| `program-view.css` | 754 | 4 container queries, 1 grid, 4 transitions, 1 sticky. |

A treemap is the *easy* case for a GPU renderer: it is rectangles with
colours, which is the primitive GPUI is fastest at. The only real work is the
1 `position: sticky` and the container queries, both of which have answers.

## Player

| Piece | Lines | Notes |
|---|---:|---|
| `PlayerPane.tsx` | 1,112 | The reading surface: stage stack, spine, choices, peek/band channels |
| `PlayerStyling.tsx` | 225 | Settings → Player: reading font, spacing, measure |
| `player.css` | 994 | 17 pseudo-elements, 13 transforms (mostly `text-transform`), 6 transitions, 2 keyframes, 1 grid, 1 gradient, 4 z-index |
| `player-runs.ts` | 142 | Session bookkeeping — plain logic |

**One thing gets strictly simpler.** `PlayerStyling` says it plainly:
"browsers cannot enumerate installed fonts, so the web build offers a curated
list … the desktop app supplies the machine's fonts through `hostFonts`
(#3439)". That is a Tauri command, a `fontdb` dependency in `src-tauri`, and
a curated fallback list, all to work around a browser limitation. GPUI has
`TextSystem::all_font_names()`. The whole mechanism collapses to one call.

The Player is otherwise flex layout, text, and colour — the part of CSS that
maps one-to-one. Its distinctive surfaces (the spine, band channels) are
pseudo-element tricks that become ordinary child elements.

## Honest reading

- **"CSS-heavy" overstates the difficulty here.** The studio's CSS is 8,445
  lines, but it is overwhelmingly flexbox, colour, spacing and typography —
  the part GPUI models directly. The exotic end of CSS (backdrop filters,
  clip paths, masks) is entirely unused.
- **Two real gaps, four total uses:** `position: sticky` (2) and 3D
  transforms (2). Both have straightforward substitutes, and the sticky one
  is already solved in this spike.
- **The treemap is a non-issue** — 88 lines of pure geometry plus rectangle
  drawing, which is the thing a GPU renderer does best.
- **Transitions are the one tax.** Roughly 38 declarative animations become
  explicit ones. Bounded, and mostly polish rather than function.
- **Some of it gets simpler:** font enumeration, and every pseudo-element
  that becomes a real child.

**What this sweep does not change:** the volume. `studio-ui` is ~19k lines
and the CSS is another 8.4k, and none of it *ports* — it is rewritten against
the same specs. That is the cost this evaluation keeps returning to, and it
is not a technical risk but a schedule one.
