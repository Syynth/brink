// ALGORITHMS CORPUS — procgen lane (issue #822)
// Cellular-automata cave generation: random-fill a grid with wall/floor,
// then iteratively smooth it via a Conway-style neighbor-count rule
// (out-of-bounds counts as wall) until it reads as an organic cave.
//
// LICENSE NOTE (per issue #822's catalog comment): the catalog cites
// RogueBasin's "Cellular Automata Method for Generating Random Cave-Like
// Levels" page (GFDL 1.2). Same treatment as bsp-dungeon/story.ink next
// door: GFDL is read-for-the-idea only under the epic's methodology, the
// prose/pseudocode is never transcribed. This port implements the
// well-known "count wall neighbors in the 8-cell Moore neighborhood,
// >=5 becomes wall, else floor" rule from the general technique
// description, not from that page's specific wording. Nothing GPL-only
// was in scope.
//
// SAVE-MID-RUN INTEREST: HIGH (per the catalog's own rating — "'smooth N
// more generations' is a natural chunked/paused operation for a loading-
// screen budget; each generation is a clean serialization boundary").
// This port demonstrates the double-buffered single-generation step and
// prints the floor-tile count after each of `generations` passes — proof
// the state after any generation is a valid, inspectable snapshot — but
// stops short of an actual `*_resumable` save/reload port; the catalog
// draws that as a DIFFERENT, dedicated entry from a plain pass/fail port
// like this one, and this file stays in the latter category on purpose.
//
// SEEDED RNG: vanilla ink's `SEED_RANDOM`/`RANDOM` (inclusive both ends),
// same as every other file in this corpus — see
// fisher-yates-shuffle/story.ink's header for why a hand-rolled in-ink PCG
// isn't needed.
//
// TYPES POLICY: gradual (default). Every value is an int, a bool
// (`wide`/`wall` conditionals), or a string; nothing here escapes gradual
// inference.
//
// ERGONOMICS-FINDINGS:
// - THE DOUBLE-BUFFERING TRAP the catalog specifically calls out
//   ("double-buffering the grid without aliasing bugs is the trap") is
//   actually a non-issue here, for a reason worth recording: brink arrays
//   are copy-on-write VALUES, not references (same fact quicksort's
//   header leans on) — `step_automaton` below reads `grid` (the OLD
//   generation) while building a brand-new `new_grid` array via `push`,
//   and returning `new_grid` to overwrite the caller's `grid` can never
//   alias the array `wall_neighbors` is still reading mid-loop, because
//   there is no in-place mutation of the array being iterated. A language
//   with reference-semantic arrays would need an explicit second buffer
//   allocated up front specifically to dodge this; here it falls out for
//   free from the value model. The "trap" the catalog warns about is real
//   in most languages this technique gets ported to, but brink's array
//   semantics happen to make it unreachable by construction.
// - Treating out-of-bounds neighbors as "always wall" needed the same
//   nested-if bounds-check shape bfs-grid-path's header documents (a
//   separate `if in_bounds { … } else { count = count + 1 }`, never
//   folded into the bounds check itself) — but here the `else` branch is
//   doing real work (counting the edge as a wall), not just a no-op skip,
//   which is a new variant of that finding: the non-short-circuit `and`
//   trap shapes not just what you CAN'T write, but what the `else` branch
//   ends up meaning.
// - No 2-D array literal for the random-fill seed grid (same "sigil
//   literals can't span multiple lines" gap bfs-grid-path/story.ink
//   documents) — moot here anyway, since the initial grid has to be
//   randomly filled cell-by-cell at runtime regardless of literal syntax.
// - Same bare-`#`-in-prose-opens-a-tag gotcha drunkards-walk/story.ink's
//   header documents in full — this file's own legend line hit it first
//   independently before that write-up existed, confirming it's not a
//   one-off: any ASCII-art legend describing a `#` glyph needs to spell
//   it out in words instead of typing the character into prose.

VAR rows = 8
VAR cols = 10
VAR generations = 4
VAR fill_percent = 45

VAR grid = 0
VAR gen_floor_counts = 0

VAR row0 = ""
VAR row1 = ""
VAR row2 = ""
VAR row3 = ""
VAR row4 = ""
VAR row5 = ""
VAR row6 = ""
VAR row7 = ""

~ SEED_RANDOM(777)
~ {
    grid = make_grid(rows, cols, 0)

    temp r = 0
    while r < rows {
        temp c = 0
        while c < cols {
            temp roll = RANDOM(1, 100)
            if roll <= fill_percent {
                grid[r][c] = 1
            }
            c = c + 1
        }
        r = r + 1
    }

    gen_floor_counts = #[]
    push(gen_floor_counts, floor_count(grid, rows, cols))

    temp g = 0
    while g < generations {
        grid = step_automaton(grid, rows, cols)
        push(gen_floor_counts, floor_count(grid, rows, cols))
        g = g + 1
    }

    row0 = row_to_string(grid, 0, cols)
    row1 = row_to_string(grid, 1, cols)
    row2 = row_to_string(grid, 2, cols)
    row3 = row_to_string(grid, 3, cols)
    row4 = row_to_string(grid, 4, cols)
    row5 = row_to_string(grid, 5, cols)
    row6 = row_to_string(grid, 6, cols)
    row7 = row_to_string(grid, 7, cols)
}

Cave after {generations} generations (wall cells solid, floor cells a dot):
{row0}
{row1}
{row2}
{row3}
{row4}
{row5}
{row6}
{row7}
Floor count per generation (gen 0 = raw fill): {gen_floor_counts}.
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

=== function wall_neighbors(grid, rows, cols, r, c) ===
~ {
    temp count = 0
    temp dr = -1
    while dr <= 1 {
        temp dc = -1
        while dc <= 1 {
            if dr != 0 or dc != 0 {
                temp nr = r + dr
                temp nc = c + dc
                if nr >= 0 and nr < rows and nc >= 0 and nc < cols {
                    if grid[nr][nc] == 1 {
                        count = count + 1
                    }
                } else {
                    count = count + 1
                }
            }
            dc = dc + 1
        }
        dr = dr + 1
    }
    return count
}

=== function step_automaton(grid, rows, cols) ===
~ {
    temp new_grid = #[]
    temp r = 0
    while r < rows {
        temp new_row = #[]
        temp c = 0
        while c < cols {
            temp n = wall_neighbors(grid, rows, cols, r, c)
            if n >= 5 {
                push(new_row, 1)
            } else {
                push(new_row, 0)
            }
            c = c + 1
        }
        push(new_grid, new_row)
        r = r + 1
    }
    return new_grid
}

=== function floor_count(grid, rows, cols) ===
~ {
    temp count = 0
    temp r = 0
    while r < rows {
        temp c = 0
        while c < cols {
            if grid[r][c] == 0 {
                count = count + 1
            }
            c = c + 1
        }
        r = r + 1
    }
    return count
}

=== function row_to_string(grid, r, cols) ===
~ {
    temp out = ""
    temp c = 0
    while c < cols {
        if grid[r][c] == 1 {
            out = out + "#"
        } else {
            out = out + "."
        }
        c = c + 1
    }
    return out
}
