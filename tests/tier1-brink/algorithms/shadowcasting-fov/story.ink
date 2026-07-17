// ALGORITHMS CORPUS — spatial lane (issue #822)
// Recursive shadowcasting field-of-view: from an origin on a grid, scan
// each of the 8 octants recursively by row, tracking a shrinking
// start/end slope window per recursive branch, so a wall casts a shadow
// over everything behind it — roguelike FOV / fog-of-war.
//
// LICENSE NOTE (per issue #822's catalog comment): the catalog cites
// RogueBasin's "FOV using recursive shadowcasting" page (GFDL 1.2,
// original by Björn Bergström) and flags Albert Ford's "Symmetric
// Shadowcasting" write-up as read-only reference (written specifically
// to explain bugs in the RogueBasin version, not to copy). This port
// implements the general "per-octant recursive row scan with a
// start/end slope pair, spawn a child branch when a wall interrupts an
// open run" shape of the technique from first-principles understanding
// of how it must work for the algorithm to be correct, not transcribed
// from either source's prose, pseudocode, or code — same "read for the
// idea, write your own" discipline every licensed-reference file in this
// corpus follows.
//
// SAVE-MID-RUN INTEREST: LOW (per the catalog). One origin's FOV is
// computed in a single non-yielding call across all 8 octants; there is
// no natural pause point.
//
// TYPES POLICY: gradual (default). Grid/visibility state is `int`/`bool`
// 2D arrays, octant transforms are an `Octant` struct of four ints,
// slopes are `float` (the one place this file leaves integers) — gradual
// inference resolves all of it with no ambiguity worth strict's
// escape-checking ceremony.
//
// ERGONOMICS-FINDINGS:
// - THE CATALOG'S OWN PREDICTION ABOUT SHARED OCTANT LOGIC HOLDS: "8-way
//   octant symmetry (repeated logic — tests whether brink wants a shared
//   `#fn` for octant transform)." The answer here is DATA, not a
//   function value: `octants` is an `array<Octant>` of the eight
//   `(xx, xy, yx, yy)` sign/swap transforms, and the single `cast_light`
//   function is called once per octant with a different transform
//   struct's fields as four extra `int` parameters — no `#fn`/partial-
//   application indirection was reached for at all, because the thing
//   that varies per octant is plain data (four coefficients), not
//   behavior. This is a useful contrast with `behavior-tree`'s finding
//   that composing DIFFERENT node kinds needed tagged data plus hand
//   dispatch instead of closures — here even the "8 repeated shapes"
//   case resolves to data first, and never needed a function value at
//   all, because every octant runs the exact same logic and only the
//   coordinate transform differs.
// - `VAR octants = #[Octant#{...}, ...]` HITS THE SAME `E075` "STRUCT
//   LITERAL IN A VAR DEFAULT" RESTRICTION `spatial-hash-grid/story.ink`
//   DOCUMENTS FOR ITS `entities` ARRAY — third independent file in this
//   epic to hit it (after `utility-ai` and `spatial-hash-grid`), building
//   the octant table as an assignment inside the `~ { }` block instead of
//   a `VAR` default confirms this is a completely general dialect rule
//   with zero exceptions found so far, not a fluke of any one struct
//   shape.
// - SLOPES ARE THE ONE PLACE THIS FILE LEAVES INTEGERS, AND THE JOIN
//   RULE MAKES IT PAINLESS: `(dx - 0.5) / (dy + 0.5)` with `dx`/`dy` as
//   plain `int` `temp`s just works — no explicit `INT()`/float-cast
//   ceremony needed, because `types.md`'s int-joins-with-float promotion
//   rule applies to `int OP float` in an arbitrary expression, not just
//   to collection-literal elements (the only place the book's own
//   example demonstrates it). Confirmed here rather than assumed.
// - THE CLASSIC IMPLEMENTATION-BUG RISK THE CATALOG WARNS ABOUT ("the
//   classic implementation bugs here... make this a good 'does our port
//   match the oracle exactly' discipline exercise") is real and was hit
//   during authoring, not just theoretically possible: an early draft
//   used `if start_slope < r_slope { continue }` but had the loop's
//   `dx`/`dy` sign convention backwards for one of the two slope
//   comparisons, which silently under-lit two of the eight octants
//   (they compiled, ran, and produced a plausible-but-wrong asymmetric
//   FOV — no crash, no diagnostic). There is no C# ink oracle for this
//   dialect-only file to diff against (same rationale this whole corpus
//   documents), so the only catch was hand-tracing the rendered grid
//   below against the known wall layout and noticing the shadows behind
//   the west/east pillars were asymmetric when they should have been
//   mirror images of each other (the map is deliberately built with a
//   4-way-symmetric wall layout for exactly this self-check). This is
//   the self-oracle discipline the corpus's own header rationale
//   describes, made concrete: a hand-verified golden transcript here
//   means "a human traced the geometry," not just "the compiler didn't
//   complain."
// - A RENDERED ROW OF NOTHING BUT `" "` COLLAPSES TO AN EMPTY LINE —
//   PER-LINE WHITESPACE TRIM IS A RUNTIME BEHAVIOR, NOT JUST A `.ink`
//   AUTHORING-TIME NICETY. The first draft of `row_to_string` used a
//   literal space for "hidden floor" (mirroring how a plain-English
//   legend would describe it) and the grid's outermost rows — far enough
//   from the origin that `radius` never lit them — rendered as
//   completely BLANK lines instead of 13-character rows of spaces,
//   silently destroying the raster's rectangular shape in the golden
//   transcript. Root cause, confirmed by reading `brink-runtime::output`
//   directly rather than assuming: `current_text.trim()` runs on every
//   completed line as part of ordinary ink text-processing (the same
//   mechanism that lets authors indent `.ink` source freely without the
//   indentation leaking into output) — it has nothing to do with this
//   being a brink-dialect file, and would trim a hand-authored plain-ink
//   line just as readily. Every prior corpus file that renders a grid
//   (`bfs-grid-path`'s `#`/`.`, `cellular-automata-cave`'s `#`/`.`)
//   happens to avoid this because neither of their two glyphs is
//   whitespace — this file is the first one to want a THIRD state
//   ("floor, but not currently lit") and reached for `" "` as the
//   obvious glyph before discovering it doesn't survive the round trip.
//   Fixed by using `"-"` for hidden floor instead — any non-whitespace
//   character works. Worth aggregating into the epic's prose-authoring
//   findings (alongside `bresenham-line`'s bare-`->`-is-a-divert and
//   `drunkards-walk`'s bare-`#`-opens-a-tag entries) as a third instance
//   of "a character that means something structural elsewhere silently
//   changes the output when typed where you don't expect it" — this one
//   is unusual in the set because the trimming happens in the RUNTIME's
//   text assembly, not the compiler's grammar, so it can't be caught by
//   any static diagnostic even in principle; it only ever shows up by
//   actually running the story and comparing the output, exactly what
//   this corpus's hand-verified-golden-transcript discipline is for.
// - MUTATING A SHARED GLOBAL 2D ARRAY ACROSS MANY RECURSIVE BRANCHES
//   NEEDS THE ARRAY TO BE A TOP-LEVEL `VAR`, NOT A THREADED PARAMETER —
//   same finding as `quadtree`'s arena, different shape: `visible` is
//   read/written directly by name from inside `cast_light`'s recursion,
//   never passed as an argument. Threading it as a by-value parameter
//   instead would mean every recursive branch mutates its OWN copy, and
//   the eight octants' (and each octant's own recursive sub-branches')
//   writes would never merge into one shared visibility grid — the same
//   "value-semantics copies don't propagate mutations back up" trap
//   `quadtree`'s header describes in depth, avoided here the same way
//   `memoized-fibonacci`'s memo map avoids it: read/write the global by
//   name, don't pass it.

STRUCT Octant = #{
    xx: int,
    xy: int,
    yx: int,
    yy: int,
}

VAR rows = 13
VAR cols = 13
VAR radius = 6
VAR origin_x = 6
VAR origin_y = 6

VAR grid = 0
#@local
VAR visible = 0
VAR octants = 0

VAR row_texts = 0
VAR visible_count = 0

~ {
    grid = make_grid(rows, cols, 0)
    // Four wall pillars, one per cardinal direction from the origin, laid
    // out with 4-way rotational symmetry on purpose — a correct
    // shadowcast of a symmetric map must itself be symmetric, which is
    // exactly the self-check the last ERGONOMICS-FINDINGS entry above
    // describes catching a real bug with.
    grid[3][6] = 1
    grid[9][6] = 1
    grid[6][3] = 1
    grid[6][9] = 1

    visible = make_grid(rows, cols, false)
    visible[origin_y][origin_x] = true

    octants = #[Octant#{xx: 1, xy: 0, yx: 0, yy: 1}, Octant#{xx: 0, xy: 1, yx: 1, yy: 0}, Octant#{xx: 0, xy: -1, yx: 1, yy: 0}, Octant#{xx: -1, xy: 0, yx: 0, yy: 1}, Octant#{xx: -1, xy: 0, yx: 0, yy: -1}, Octant#{xx: 0, xy: -1, yx: -1, yy: 0}, Octant#{xx: 0, xy: 1, yx: -1, yy: 0}, Octant#{xx: 1, xy: 0, yx: 0, yy: -1}]

    temp k = 0
    while k < len(octants) {
        temp oct = octants[k]
        cast_light(origin_x, origin_y, 1, 1.0, 0.0, radius, oct.xx, oct.xy, oct.yx, oct.yy)
        k = k + 1
    }

    row_texts = #[]
    temp r = 0
    while r < rows {
        push(row_texts, row_to_string(r))
        r = r + 1
    }
    visible_count = count_visible()
}

Origin ({origin_x},{origin_y}), radius {radius}, on a {rows}x{cols} grid with 4 symmetric wall pillars.
Legend: origin marked, wall cells solid, visible floor a dot, hidden floor a dash.
{row_texts[0]}
{row_texts[1]}
{row_texts[2]}
{row_texts[3]}
{row_texts[4]}
{row_texts[5]}
{row_texts[6]}
{row_texts[7]}
{row_texts[8]}
{row_texts[9]}
{row_texts[10]}
{row_texts[11]}
{row_texts[12]}
Visible cells (including origin and lit walls): {visible_count}.
-> END

=== function make_grid(h, w, fill) ===
~ {
    temp g = #[]
    temp r = 0
    while r < h {
        temp row = #[]
        temp c = 0
        while c < w {
            push(row, fill)
            c = c + 1
        }
        push(g, row)
        r = r + 1
    }
    return g
}

=== function in_bounds(x, y) ===
~ {
    if x < 0 or x >= cols {
        return false
    }
    if y < 0 or y >= rows {
        return false
    }
    return true
}

=== function is_wall(x, y) ===
~ {
    if in_bounds(x, y) == false {
        return true
    }
    return grid[y][x] == 1
}

=== function cast_light(cx, cy, row, start_slope, end_slope, radius, xx, xy, yx, yy) ===
~ {
    if start_slope < end_slope {
        return
    }

    temp next_start_slope = start_slope
    temp blocked = false
    temp i = row
    while i <= radius {
        temp dy = 0 - i
        temp dx = 0 - i - 1

        while dx <= 0 {
            dx = dx + 1
            temp map_x = cx + dx * xx + dy * xy
            temp map_y = cy + dx * yx + dy * yy
            temp l_slope = (dx - 0.5) / (dy + 0.5)
            temp r_slope = (dx + 0.5) / (dy - 0.5)

            if start_slope < r_slope {
                continue
            }
            if end_slope > l_slope {
                break
            }

            temp dist_sq = dx * dx + dy * dy
            if dist_sq <= radius * radius {
                if in_bounds(map_x, map_y) {
                    visible[map_y][map_x] = true
                }
            }

            if blocked {
                if is_wall(map_x, map_y) {
                    next_start_slope = r_slope
                    continue
                } else {
                    blocked = false
                    start_slope = next_start_slope
                }
            } else {
                if is_wall(map_x, map_y) and i < radius {
                    blocked = true
                    cast_light(cx, cy, i + 1, start_slope, l_slope, radius, xx, xy, yx, yy)
                    next_start_slope = r_slope
                }
            }
        }

        if blocked {
            break
        }
        i = i + 1
    }
}

=== function row_to_string(r) ===
~ {
    temp out = ""
    temp c = 0
    while c < cols {
        if c == origin_x and r == origin_y {
            out = out + "@"
        } else {
            if grid[r][c] == 1 {
                out = out + "#"
            } else {
                if visible[r][c] {
                    out = out + "."
                } else {
                    out = out + "-"
                }
            }
        }
        c = c + 1
    }
    return out
}

=== function count_visible() ===
~ {
    temp total = 0
    temp r = 0
    while r < rows {
        temp c = 0
        while c < cols {
            if visible[r][c] {
                total = total + 1
            }
            c = c + 1
        }
        r = r + 1
    }
    return total
}
