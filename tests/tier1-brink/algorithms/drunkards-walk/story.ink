// ALGORITHMS CORPUS — procgen lane (issue #822)
// Drunkard's walk (a.k.a. random-walk cave carving): a walker takes random
// steps across a grid, carving a floor tile wherever it lands, until
// enough floor has been carved or a hard step cap is hit — the cheapest
// possible organic-looking cave.
//
// LICENSE NOTE (per issue #822's catalog comment): the technique is
// "widely described" with no single canonical reference implementation —
// this port was written from the general description, not transcribed
// from any specific source. The catalog cites RogueBasin's "Cellular
// Automata Method for Generating Random Cave-Like Levels" page (GFDL 1.2)
// only for the *adjacent* CA approach (see cellular-automata-cave/
// story.ink next door, which also avoids quoting it — GFDL, like CC
// BY-SA, is read-for-the-idea-only under the epic's methodology, never
// transcribed). Nothing GPL-only was in scope for this file; no flagged
// reference needed replacing.
//
// SAVE-MID-RUN INTEREST: LOW (per the catalog's own rating for this row —
// "almost none... a good low-friction warm-up port"). The walk is a
// single flat loop with no natural chunk boundary worth demonstrating a
// resumable variant for; noted for completeness, not built here.
//
// SEEDED RNG: vanilla ink's `SEED_RANDOM`/`RANDOM` (inclusive both ends) —
// see fisher-yates-shuffle/story.ink's header for the full reasoning on
// why this lane doesn't need a hand-rolled in-ink PCG. Deterministic
// across runs, which this file's own `expected.txt` is the proof of.
//
// TYPES POLICY: gradual (default). Every value is an int, a bool, or a
// string built from grid cells — nothing here escapes gradual inference.
//
// ERGONOMICS-FINDINGS:
// - Bounded by TWO independent conditions in one `while` guard
//   (`floor_count < target_floor and steps < max_steps`) — the walk's own
//   termination (hitting the floor target) isn't provably fast on its
//   own (a pathological RNG sequence could wander forever without
//   revisiting new cells), so the epic's "guard against unbounded
//   growth" house rule needs a second, unconditional step cap even though
//   this looks at first glance like a self-terminating loop. Both
//   conditions are cheap int comparisons with no indexing, so the
//   non-short-circuit `and` trap (documented repeatedly elsewhere in this
//   corpus) doesn't bite here — worth calling out that the trap is a
//   FUNCTION of what the operands do, not of `and`/`or` themselves.
// - Same "no native queue/2-D-literal" findings as the graphs lane's
//   `make_grid` helper (bfs-grid-path/story.ink) apply verbatim; not
//   re-derived here.
// - NEW finding this file discovered first: a literal `#` typed directly
//   into PROSE text (not inside a `~` block/expression) opens an ink TAG
//   and silently swallows the rest of that source line from the printed
//   `Line::Text` output — `docs/book/.../literals.md` documents this for
//   collection sigils in expression position, but the same rule applies
//   to any bare `#`, including one meant as an ASCII-art legend character
//   ("wall = `#`"). The original draft of this file's header line read
//   `Cave (# wall, . floor):` and silently printed as just `Cave (` — no
//   compile error, no warning, because a tag is perfectly legal ink
//   syntax, just not what was intended. A `#` produced at RUNTIME inside
//   an interpolated `{…}` string (this file's grid rows themselves) is
//   unaffected — tag parsing is a static, source-text-only concept, so
//   only a `#` an author types directly into prose is at risk. Confirmed
//   empirically before settling on wording that avoids the character
//   entirely in every prose line across this whole procgen lane.
// - No character/byte type: "carve this cell" and "print this cell" both
//   reduce to a single-character STRING concatenation (`out = out + "#"`),
//   never an actual char value — fine for this corpus's toy grids, but
//   means every cell-to-glyph mapping in the whole procgen lane pays a
//   string-alloc-per-cell tax that a real char type wouldn't.

VAR rows = 6
VAR cols = 10
VAR target_floor = 26
VAR max_steps = 500

VAR grid = 0
VAR steps = 0
VAR floor_count = 0

VAR row0 = ""
VAR row1 = ""
VAR row2 = ""
VAR row3 = ""
VAR row4 = ""
VAR row5 = ""

~ SEED_RANDOM(4242)
~ {
    grid = make_grid(rows, cols, 1)

    temp dr = #[-1, 1, 0, 0]
    temp dc = #[0, 0, -1, 1]

    temp cur_r = 3
    temp cur_c = 5
    grid[cur_r][cur_c] = 0
    floor_count = 1

    while floor_count < target_floor and steps < max_steps {
        temp dir = RANDOM(0, 3)
        temp nr = cur_r + dr[dir]
        temp nc = cur_c + dc[dir]
        if nr >= 0 and nr < rows and nc >= 0 and nc < cols {
            cur_r = nr
            cur_c = nc
            if grid[cur_r][cur_c] == 1 {
                grid[cur_r][cur_c] = 0
                floor_count = floor_count + 1
            }
        }
        steps = steps + 1
    }

    row0 = row_to_string(grid, 0, cols)
    row1 = row_to_string(grid, 1, cols)
    row2 = row_to_string(grid, 2, cols)
    row3 = row_to_string(grid, 3, cols)
    row4 = row_to_string(grid, 4, cols)
    row5 = row_to_string(grid, 5, cols)
}

Cave grid (wall cells solid, floor cells a dot):
{row0}
{row1}
{row2}
{row3}
{row4}
{row5}
Floor tiles carved: {floor_count}.
Steps taken: {steps}.
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
