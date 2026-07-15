// ALGORITHMS CORPUS — graphs lane (issue #822)
// Breadth-first search: shortest path on a grid with obstacles.
//
// TYPES POLICY: gradual (default). Every value is an int, a bool, an
// array, or a `Point` struct of two ints — gradual inference resolves the
// whole file. `types = strict` would add annotation ceremony (function
// params, `make_grid`'s generic fill value) for zero payoff: nothing here
// is ever `Unknown`/`Conflicted`.
//
// THE GRID: shared with dfs-grid-path/story.ink on purpose — same 4x6
// grid, same start (0,0), same goal (3,0), so the two files are a direct
// A/B comparison of traversal order on identical input. There are exactly
// two disjoint, dead-end-free routes from start to goal: a 4-cell drop
// straight down column 0, and a 14-cell loop across row 0, down column 5,
// and back across row 3. BFS's level-order search always finds the
// 4-cell route (see dfs-grid-path's header for why DFS does not).
//
// ERGONOMICS-FINDINGS:
// - No native queue type: a FIFO is "array + push at the back, `remove`
//   at index 0 for the front". `remove(arr, 0)` is an O(n) shift-left on
//   every single dequeue (per `docs/book/.../stdlib.md`'s own description
//   of `remove`'s array semantics) — for this corpus's tiny grids that's
//   invisible, but it means BFS's `O(V+E)` textbook complexity quietly
//   becomes `O(V*(V+E))` in wall-clock terms the moment a grid is large
//   enough to matter. A true ring buffer (fixed-size array + head/tail
//   modular indices, `docs/book`'s own "Data-structure gymnastics" lane
//   item from the epic catalog) is the fix, and it is a bigger lift than
//   this corpus wants to take on for a shortest-path demo. Noting it here
//   because every BFS port of a real pathfinder will hit this immediately.
// - No `Option`/nullable type for "no parent yet": `parent[r][c]` uses a
//   sentinel `Point#{r: -1, c: -1}` the same way `binary-search`'s port
//   uses `-1` for "not found" — consistent with the rest of the corpus,
//   but worth flagging that every port so far has independently reached
//   for "a sentinel that can't be a real value" rather than any language
//   feature, because there isn't one.
// - No `reverse()` on arrays: path reconstruction walks parent pointers
//   goal-to-start, which comes out backwards, and there's no built-in to
//   flip it — a second `while` loop copying elements back-to-front is the
//   idiom (see `reverse_path` below). A read-only reversed *view* over an
//   array is exactly the kind of thing a slice/range type (icebox #829)
//   would give for free instead of a manual copy loop; this port doesn't
//   need a sub-range, just the reversal, so it's adjacent to #829's scope
//   rather than squarely inside it.
// - Array/map SIGIL LITERALS cannot span multiple lines: `NEWLINE` is not
//   trivia in this grammar (it terminates lines and delimits blocks
//   elsewhere), and `#[...]`'s element loop only skips `WHITESPACE`/
//   comments between elements, not `NEWLINE` — so a literal like
//   `#[\n    #[0, 0, ...],\n    ...\n]` fails to parse with a bare
//   "expected R_BRACKET" (`E037`) at the first line break inside the
//   brackets, with no hint that the fix is "put it all on one line."
//   This grid's 4x6 literal (`VAR grid = #[#[...], #[...], ...]` below)
//   had to be collapsed onto a single line to compile at all — for a
//   hand-authored grid much bigger than this corpus's toy examples, that
//   single-line requirement is a real authoring/readability tax with no
//   workaround inside the literal syntax itself (the only escape is
//   building the grid programmatically via `push`, as `make_grid` below
//   does for `visited`/`parent`, which is exactly why this file doesn't
//   also hand-write those two as literals).
// - Bounds-checking an indexed grid access needs to stay a SEPARATE `if`
//   from the index itself, never folded into one `and` chain: brink's
//   `and`/`or` never short-circuit (same rule `insertion-sort` documents
//   for its loop guard), so
//   `if nr >= 0 and nr < ROWS and grid[nr][nc] == 0 { ... }` would
//   evaluate `grid[nr][nc]` even when `nr` is out of bounds and fault.
//   Nesting `if in_bounds { if grid[nr][nc] == 0 { ... } }` is the only
//   safe shape. This is the same underlying gap as the sorting lane's
//   finding, but it bites harder here because grid algorithms guard
//   *indexed access* on every single neighbor check, not just a loop
//   condition.

STRUCT Point = #{
    r: int,
    c: int,
}

VAR grid = #[#[0, 0, 0, 0, 0, 0], #[0, 1, 1, 1, 1, 0], #[0, 1, 1, 1, 1, 0], #[0, 0, 0, 0, 0, 0]]
VAR rows = 4
VAR cols = 6
VAR dr = #[-1, 0, 1, 0]
VAR dc = #[0, 1, 0, -1]

VAR path_text = ""
VAR path_len = 0
VAR nodes_visited = 0
VAR found = false

~ {
    temp start = Point#{r: 0, c: 0}
    temp goal = Point#{r: 3, c: 0}

    temp visited = make_grid(rows, cols, false)
    temp parent = make_grid(rows, cols, Point#{r: -1, c: -1})

    visited[start.r][start.c] = true
    temp queue = #[start]

    while len(queue) > 0 {
        temp cur = queue[0]
        remove(queue, 0)
        nodes_visited = nodes_visited + 1

        if cur.r == goal.r and cur.c == goal.c {
            found = true
            break
        }

        temp i = 0
        while i < 4 {
            temp nr = cur.r + dr[i]
            temp nc = cur.c + dc[i]
            if nr >= 0 and nr < rows and nc >= 0 and nc < cols {
                if grid[nr][nc] == 0 and visited[nr][nc] == false {
                    visited[nr][nc] = true
                    parent[nr][nc] = cur
                    push(queue, Point#{r: nr, c: nc})
                }
            }
            i = i + 1
        }
    }

    if found {
        temp backward = #[]
        temp node = goal
        while node.r != start.r or node.c != start.c {
            push(backward, node)
            node = parent[node.r][node.c]
        }
        push(backward, start)

        temp forward = reverse_path(backward)
        path_len = len(forward)
        path_text = path_to_string(forward)
    }
}

Path found: {found}.
Path: {path_text}.
Path length: {path_len}.
Nodes dequeued: {nodes_visited}.
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

=== function reverse_path(xs) ===
~ {
    temp out = #[]
    temp i = len(xs) - 1
    while i >= 0 {
        push(out, xs[i])
        i = i - 1
    }
    return out
}

=== function path_to_string(xs) ===
~ {
    temp out = ""
    temp i = 0
    while i < len(xs) {
        temp p = xs[i]
        out = out + "(" + string(p.r) + "," + string(p.c) + ")"
        if i < len(xs) - 1 {
            out = out + " -> "
        }
        i = i + 1
    }
    return out
}
