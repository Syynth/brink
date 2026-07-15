// ALGORITHMS CORPUS — procgen lane (issue #822)
// Value noise (lattice-based, Perlin-adjacent): hash a sparse lattice of
// integer coordinates to pseudo-random unit values, then bilinearly
// interpolate between the four surrounding lattice corners at every
// sample point, with a smoothstep fade curve so the field reads as a
// smooth 2-D pseudo-random surface instead of blocky steps — heightmaps,
// cloud/foliage density, anything wanting cheap organic variation without
// Wave Function Collapse's cost.
//
// LICENSE NOTE (per issue #822's catalog comment, 🚩 FLAGGED row): the
// catalog explicitly prefers FastNoiseLite (MIT,
// github.com/Auburn/FastNoiseLite) over Ken Perlin's own reference Java
// code, which carries only a bare "Copyright 2002 Ken Perlin" notice with
// no formal license grant — the GPL-adjacent-risk case this epic's
// methodology exists to route around. This port does not transcribe
// FastNoiseLite's implementation (or Perlin's) at all; it implements the
// generic "hash lattice corners, bilinear-interpolate with a smoothstep
// fade" value-noise shape from first principles, which is public
// algorithmic knowledge independent of any one implementation. FastNoiseLite
// is cited here as the permissively-licensed alternative per the catalog's
// own methodology note, not as a source this file's code derives from.
//
// SAVE-MID-RUN INTEREST: LOW (per the catalog's own rating). The field is
// pure a function of lattice hash + sample coordinate — every value is
// independently recomputable from the fixed seed at any time, so there is
// no meaningful "mid-generation" state to save at all; the whole field
// already IS the save state, trivially.
//
// SEEDED RNG: deliberately NOT vanilla ink's `RANDOM`/`SEED_RANDOM` — see
// fisher-yates-shuffle/story.ink's header for why this lane's other files
// use it. Value noise needs a PURE FUNCTION of (x, y, seed) -> unit value,
// re-evaluable in any order and at any point, not a sequential draw from
// RNG state (`RANDOM` has no "value at coordinate (x, y)" query, only
// "next value in the draw sequence"). `hash_to_unit` below is exactly the
// catalog's own "fixed-seed hashing" finding predicted this lane would
// need: a small multiplicative integer hash (`x * P1 + y * P2 + seed *
// P3`, then a normalize-to-non-negative modulo), converted to `[0, 1)` via
// a final `float(h) / float(range)` division. This is NOT the catalog's
// separate "PCG" randomness-lane row (a sequential generator as an
// algorithm in its own right) — it's a coordinate-keyed hash, a different
// tool for a different job, confirmed by trying to reuse `RANDOM` here
// first and finding it structurally doesn't fit.
//
// TYPES POLICY: gradual (default). Floats, ints, and one nested
// `array<array<float>>` lattice/field pair; gradual inference resolves
// everything without annotation ceremony.
//
// ERGONOMICS-FINDINGS:
// - `FLOOR(x)` returns a FLOAT, not an int — confirmed empirically before
//   committing to this design: indexing an array with `FLOOR(x)` directly
//   faults at runtime (`InvalidArrayIndex("float")`), even though the
//   value is mathematically integral. Every lattice-cell lookup below
//   needs the explicit `int(FLOOR(x))` double-wrap. This is the sharpest
//   float-vs-int friction point in the whole procgen lane so far (the
//   catalog predicted "floats-heavy math... no native lerp/smoothstep
//   helpers" in the abstract; this is the concrete shape that prediction
//   takes) — a heightmap/terrain algorithm's single most common operation
//   ("which cell is this continuous coordinate in?") requires knowing
//   this gotcha up front, not discovering it via a runtime fault.
// - No `lerp`/`smoothstep`/`clamp` built-ins: `lerp(a, b, t) = a + (b - a)
//   * t` and the smoothstep fade `t * t * (3 - 2 * t)` are both one-line
//   expressions once you know the formula, so this isn't a coverage gap —
//   but it means every port of a noise/easing algorithm re-derives the
//   same two formulas from scratch with no shared vocabulary to name them
//   by, which is exactly the kind of "small, load-bearing math helper"
//   the epic's animation/easing catalog lane will hit again.
// - No bitwise XOR/shift operators (only `^` exists, and it's ink's LIST
//   intersection operator, not integer XOR) — a proper avalanche-quality
//   integer hash (xorshift, FNV, etc.) is unavailable; `hash_to_unit`
//   below falls back to a multiplicative-only hash (`+`/`*`/`%` only),
//   which is weaker mixing but sufficient for a 3x3 demo lattice. A real
//   terrain generator at production scale would want this gap fixed
//   first — a repeatedly-flagged item now that both this file and
//   astar-grid/story.ink hit float/int-vector-math gaps independently.
// - `int` is 32-bit and arithmetic WRAPS silently on overflow (verified
//   empirically: `x * 374761393` for this file's inputs overflows i32
//   immediately and wraps rather than panicking or promoting to a wider
//   type) — this is actually exactly what a multiplicative hash wants
//   (deterministic, reproducible wraparound is a feature, not a bug, for
//   hash mixing), but it's a silent behavior with no diagnostic, worth
//   flagging for any port that assumes overflow either panics or widens.
// - `%` is C-style truncating remainder, NOT Euclidean/flooring mod: `(-7)
//   % 1000` is `-7`, not `993`. `hash_to_unit` normalizes with the
//   standard `(h % range + range) % range` idiom rather than assuming a
//   single `%` already returns a non-negative result — a second
//   documented pitfall for the same reason binary-search/insertion-sort
//   already flag the non-short-circuit `and`/`or` trap: a textbook
//   formula ported verbatim without checking the target language's
//   operator semantics silently produces wrong-but-plausible-looking
//   output instead of a compile error.
// - Printed floats carry full `f32` rounding noise (e.g. `0.7` prints as
//   `0.70000005`) and an exactly-integral float prints with NO decimal
//   point at all (`5.0` prints as `5`, indistinguishable from the int
//   `5`) — no format-precision control exists in string interpolation.
//   Both are cosmetic, not correctness, issues for this file's own golden
//   transcript (still byte-identical every run), but either one would be
//   a real readability problem for a human-facing heightmap/debug display
//   built on top of this technique.
// - Same bare-`#`-in-prose-opens-a-tag gotcha drunkards-walk/story.ink's
//   header documents in full — this file's original band legend
//   (`bands ' .:#' low to high`) hit it too, dropping everything from the
//   `#` onward; spelled the bands out in words instead.
// - A SECOND, sharper printed-text gotcha this file discovered first: a
//   line's TRAILING whitespace is silently stripped by ink's own
//   line-formatting rules, even when that whitespace came from
//   interpolated `{…}` runtime content, not source-text glue. The lowest
//   noise band was originally mapped to a literal `" "` (space) glyph —
//   correct in isolation, but any sample row ending in one or more
//   low-band cells came out SHORTER than `sample_n` characters in the
//   actual printed transcript (confirmed empirically: a 9-wide row
//   collapsed to 7 or even 5 printed characters when its tail was blank
//   cells), silently breaking the "every row is a fixed-width ASCII grid"
//   invariant a heightmap display depends on. INTERNAL runs of a single
//   space (mid-row, not at the line's end) print untouched — only a
//   trailing run is affected — which makes the bug invisible for any grid
//   whose low band never happens to land in the last column, and exactly
//   the kind of thing a golden-transcript diff catches that eyeballing
//   the source code never would. Fixed by mapping the lowest band to `-`
//   instead of a space; any procgen ASCII-art renderer in this corpus
//   needs to remember NEVER to use a trailing/interior run of plain
//   spaces as a meaningful glyph.

VAR lattice_n = 3
VAR steps_per_cell = 4
VAR sample_n = 9
VAR seed = 1013

VAR lattice_vals = 0
VAR field = 0

VAR row0 = ""
VAR row1 = ""
VAR row2 = ""
VAR row3 = ""
VAR row4 = ""
VAR row5 = ""
VAR row6 = ""
VAR row7 = ""
VAR row8 = ""

VAR corner_sample = 0.0
VAR center_sample = 0.0

~ {
    lattice_vals = build_lattice(lattice_n, seed)
    field = build_field(lattice_vals, lattice_n, steps_per_cell, sample_n)

    row0 = row_to_bands(field, sample_n, 0)
    row1 = row_to_bands(field, sample_n, 1)
    row2 = row_to_bands(field, sample_n, 2)
    row3 = row_to_bands(field, sample_n, 3)
    row4 = row_to_bands(field, sample_n, 4)
    row5 = row_to_bands(field, sample_n, 5)
    row6 = row_to_bands(field, sample_n, 6)
    row7 = row_to_bands(field, sample_n, 7)
    row8 = row_to_bands(field, sample_n, 8)

    corner_sample = field[0][0]
    center_sample = field[4][4]
}

Lattice ({lattice_n}x{lattice_n} hashed corners): {lattice_vals}.
Field ({sample_n}x{sample_n} interpolated samples, bands dash/dot/colon/solid low to high):
{row0}
{row1}
{row2}
{row3}
{row4}
{row5}
{row6}
{row7}
{row8}
Corner sample field[0][0]: {corner_sample}.
Center sample field[4][4]: {center_sample}.
-> END

=== function hash_to_unit(x, y, seed) ===
~ {
    temp h = x * 374761393 + y * 668265263 + seed * 1274126177
    temp range = 100003
    h = (h % range + range) % range
    return float(h) / float(range)
}

=== function build_lattice(n, seed) ===
~ {
    temp g = #[]
    temp ly = 0
    while ly < n {
        temp row = #[]
        temp lx = 0
        while lx < n {
            push(row, hash_to_unit(lx, ly, seed))
            lx = lx + 1
        }
        push(g, row)
        ly = ly + 1
    }
    return g
}

=== function sample_noise(lattice, n, steps, sx, sy) ===
~ {
    temp cx = float(sx) / float(steps)
    temp cy = float(sy) / float(steps)

    temp cellx = int(FLOOR(cx))
    if cellx > n - 2 {
        cellx = n - 2
    }
    temp celly = int(FLOOR(cy))
    if celly > n - 2 {
        celly = n - 2
    }

    temp tx = cx - float(cellx)
    temp ty = cy - float(celly)

    temp fx = tx * tx * (3.0 - 2.0 * tx)
    temp fy = ty * ty * (3.0 - 2.0 * ty)

    temp v00 = lattice[celly][cellx]
    temp v10 = lattice[celly][cellx + 1]
    temp v01 = lattice[celly + 1][cellx]
    temp v11 = lattice[celly + 1][cellx + 1]

    temp top = v00 + (v10 - v00) * fx
    temp bottom = v01 + (v11 - v01) * fx

    return top + (bottom - top) * fy
}

=== function build_field(lattice, n, steps, sample_n) ===
~ {
    temp out = #[]
    temp sy = 0
    while sy < sample_n {
        temp row = #[]
        temp sx = 0
        while sx < sample_n {
            push(row, sample_noise(lattice, n, steps, sx, sy))
            sx = sx + 1
        }
        push(out, row)
        sy = sy + 1
    }
    return out
}

=== function band_char(v) ===
~ {
    if v < 0.25 {
        return "-"
    }
    if v < 0.5 {
        return "."
    }
    if v < 0.75 {
        return ":"
    }
    return "#"
}

=== function row_to_bands(field, n, r) ===
~ {
    temp out = ""
    temp c = 0
    while c < n {
        out = out + band_char(field[r][c])
        c = c + 1
    }
    return out
}
