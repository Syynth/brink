// ALGORITHMS CORPUS — spatial lane (issue #822)
// Bresenham's line algorithm: integer-only grid-line rasterization
// between two points, in all eight octants — line-of-sight rasters,
// laser/projectile paths, and tile-based line drawing on a grid all
// reduce to this.
//
// LICENSE NOTE (per issue #822's catalog comment): the catalog cites
// Wikipedia's "Bresenham's line algorithm" page (CC BY-SA) and Red Blob
// Games' line-drawing article (MIT/Apache-2.0 code) as references. This
// port uses the well-known "unify all eight octants behind one loop by
// keeping `dy` negated and comparing `2*err` against `dx`/`dy` each step"
// reformulation — a shape common to essentially every correct from-first-
// principles implementation of this algorithm (the same "many independent
// implementations converge on one canonical form" situation
// bsp-dungeon/story.ink's header describes for its own technique), not
// transcribed from either source's specific prose or code.
//
// SAVE-MID-RUN INTEREST: LOW (per the catalog — "almost none, good
// low-friction spatial-lane starter"). One line is rasterized in a single
// non-yielding call; there is no natural pause point.
//
// TYPES POLICY: gradual (default). Every value is an `int`, a `bool`, or
// a `Point` struct of two ints; gradual inference resolves the whole file
// with no ambiguity worth strict's escape-checking ceremony.
//
// ERGONOMICS-FINDINGS:
// - NO `abs()` BUILTIN, BUT `MAX`/`MIN` (vanilla ink natives, confirmed in
//   `brink-ir::lir::lower::expr`'s builtin-name table alongside `INT`/
//   `FLOOR`/`POW`) MAKE ONE UNNECESSARY: `MAX(n, -n)` is exactly `abs(n)`
//   for any int `n`, and reads as clearly as a dedicated builtin would —
//   `iabs` below is a one-line wrapper purely for a readable call site,
//   not because `MAX(n, -n)` needed hiding. This is the first file in the
//   corpus to reach for `MIN`/`MAX` at all; worth flagging that they exist
//   and work exactly like their ink-native `int`/`int` signature promises,
//   since nothing prior in this corpus had needed them.
// - Integer-only arithmetic end to end — no float division, no `INT()`
//   truncation-conversion, none of `value-noise-field`'s float-lattice
//   friction. This is the "friction floor" for the spatial lane the
//   catalog predicted: a grid-line algorithm that never leaves integers
//   has essentially nothing left to go wrong at the language-ergonomics
//   level, in contrast to the shadowcasting/quadtree files next door in
//   this same lane.
// - The eight-octant unification trick (negate `dy`, single loop, no
//   octant-detection branch at all) means this port needed exactly ONE
//   function body for all eight octants with zero special-casing — a
//   sharp contrast with `shadowcasting-fov`'s port two files over, which
//   genuinely could not avoid an explicit 8-way octant-transform table
//   (see that file's header for why the two algorithms differ here even
//   though both are nominally "grid, eight octants, symmetry").
// - `while` with a single `break`-free exit condition (`x0 != x1 or y0 !=
//   y1`) reads cleanly as a `for`-shaped loop would in a language with
//   C-style `for` — brink has no such form (`for x in expr` is
//   collection-only, per `docs/book/.../blocks.md`), so a plain `while`
//   with a hand-maintained loop variable is the only option, same as
//   every other counted loop in this corpus.
// - NAIVE BRESENHAM IS NOT DIRECTION-SYMMETRIC, CONFIRMED EMPIRICALLY, NOT
//   JUST ASSERTED: the `Shallow (1,1) to (7,4)` and `Reversed (7,4) to
//   (1,1)` lines below traverse the identical two endpoints in opposite
//   order, and their printed cell lists are NOT the same set — e.g. the
//   forward pass visits `(6,4)` where the reversed pass visits `(6,3)`.
//   This is a well-documented property of the classic integer Bresenham
//   formulation (the tie-breaking `e2 >= dy` / `e2 <= dx` comparisons are
//   order-dependent), not a bug in this port — the catalog's own
//   shadowcasting-fov entry warns that "the classic implementation bugs
//   here... make this a good 'does our port match the oracle exactly'
//   discipline exercise" for FOV specifically, and this file turned up
//   the line-drawing lane's own version of that same caution: a first
//   draft of this file asserted cell-SET equality under reversal as a
//   free correctness check (mirroring astar-grid/dijkstra-grid's cost
//   cross-check), and that assertion does NOT hold in general — verified
//   by running exactly this pair of endpoints, not by reasoning about
//   the algorithm in the abstract. What DOES hold unconditionally is cell
//   COUNT: `max(|dx|, |dy|) + 1` is direction-invariant, and the
//   `Endpoint-order cell count agrees` line below checks that instead. A
//   truly symmetric rasterizer needs an explicit tie-breaking rule (e.g.
//   always rounding the exact-half-slope case toward one fixed endpoint)
//   that this port does not implement, since the corpus's job is "port
//   the standard algorithm," not "invent a symmetric variant."
// - A LITERAL `->` IN PROSE TEXT IS A DIVERT, no whitespace or word-break
//   needed to trigger it: the first draft of this file's closing-content
//   lines wrote `Shallow (1,1)->(7,4): ...` directly and got `E012`
//   ("divert is missing a target") plus a cascading `E037` on every such
//   line — the parser sees `->` in prose position exactly as it would in
//   `-> END`, with no exception for "obviously part of a coordinate
//   pair." Every existing corpus file that needs a literal arrow in
//   output text (`bfs-grid-path`'s ` -> ` path separator, `npc-fsm`'s
//   event-log arrow, etc.) sidesteps this by building the arrow into a
//   *string value* at runtime (`out = out + " -> "`) and only ever
//   interpolating it via `{...}`, never typing `->` directly into a
//   knot's prose line — this file's fix was simpler still, spelling the
//   word "to" in the prose lines below instead. Same family of gotcha as
//   `drunkards-walk`/`cellular-automata-cave`'s bare-`#`-in-prose finding
//   (a sigil/token that means something structural elsewhere in the
//   grammar is not automatically safe to type literally into narrative
//   text) — worth aggregating these into one "characters that need
//   escaping or runtime-string laundering in prose" finding once this
//   epic reaches its aggregation boundary.

STRUCT Point = #{
    x: int,
    y: int,
}

VAR line_shallow_text = ""
VAR line_steep_text = ""
VAR line_reversed_text = ""
VAR line_vertical_text = ""
VAR line_horizontal_text = ""
VAR line_diagonal_text = ""

VAR shallow_len = 0
VAR steep_len = 0
VAR reversed_len = 0
VAR vertical_len = 0
VAR horizontal_len = 0
VAR diagonal_len = 0

// Same two endpoints traversed in both directions: cell COUNT is
// direction-invariant even though the cell SET is not (see the
// ERGONOMICS-FINDINGS entry on Bresenham's direction asymmetry above) —
// this is the honest version of the "living documentation made
// checkable" idiom astar-grid/dijkstra-grid's cost cross-check and
// bsp-dungeon's area invariant use, scoped to the property that actually
// holds unconditionally.
VAR forward_len = 0
VAR backward_len = 0
VAR endpoint_counts_agree = false

~ {
    temp shallow = bresenham_line(1, 1, 7, 4)
    line_shallow_text = points_to_string(shallow)
    shallow_len = len(shallow)

    temp steep = bresenham_line(1, 1, 4, 7)
    line_steep_text = points_to_string(steep)
    steep_len = len(steep)

    temp reversed_dir = bresenham_line(7, 4, 1, 1)
    line_reversed_text = points_to_string(reversed_dir)
    reversed_len = len(reversed_dir)

    temp vertical = bresenham_line(3, 0, 3, 5)
    line_vertical_text = points_to_string(vertical)
    vertical_len = len(vertical)

    temp horizontal = bresenham_line(0, 2, 5, 2)
    line_horizontal_text = points_to_string(horizontal)
    horizontal_len = len(horizontal)

    temp diagonal = bresenham_line(0, 0, 5, 5)
    line_diagonal_text = points_to_string(diagonal)
    diagonal_len = len(diagonal)

    temp forward = bresenham_line(2, 2, 9, 6)
    temp backward = bresenham_line(9, 6, 2, 2)
    forward_len = len(forward)
    backward_len = len(backward)
    endpoint_counts_agree = forward_len == backward_len
}

Shallow (1,1) to (7,4): {line_shallow_text}. Cells: {shallow_len}.
Steep (1,1) to (4,7): {line_steep_text}. Cells: {steep_len}.
Reversed (7,4) to (1,1): {line_reversed_text}. Cells: {reversed_len}.
Vertical (3,0) to (3,5): {line_vertical_text}. Cells: {vertical_len}.
Horizontal (0,2) to (5,2): {line_horizontal_text}. Cells: {horizontal_len}.
Diagonal (0,0) to (5,5): {line_diagonal_text}. Cells: {diagonal_len}.
Endpoint-order cell count agrees (2,2) to (9,6) vs reversed: {endpoint_counts_agree} ({forward_len} vs {backward_len}).
-> END

=== function iabs(n) ===
~ {
    return MAX(n, -n)
}

=== function bresenham_line(x0, y0, x1, y1) ===
~ {
    temp dx = iabs(x1 - x0)
    temp dy = 0 - iabs(y1 - y0)

    temp sx = 1
    if x0 > x1 {
        sx = -1
    }
    temp sy = 1
    if y0 > y1 {
        sy = -1
    }

    temp err = dx + dy
    temp cx = x0
    temp cy = y0
    temp points = #[]

    while true {
        push(points, Point#{x: cx, y: cy})
        if cx == x1 and cy == y1 {
            break
        }
        temp e2 = 2 * err
        if e2 >= dy {
            err = err + dy
            cx = cx + sx
        }
        if e2 <= dx {
            err = err + dx
            cy = cy + sy
        }
    }

    return points
}

=== function points_to_string(pts) ===
~ {
    temp out = ""
    temp i = 0
    while i < len(pts) {
        temp p = pts[i]
        out = out + "(" + string(p.x) + "," + string(p.y) + ")"
        if i < len(pts) - 1 {
            out = out + " "
        }
        i = i + 1
    }
    return out
}
