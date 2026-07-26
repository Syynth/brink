// ALGORITHMS CORPUS — graphs lane (issue #822)
// Dijkstra's algorithm: cheapest path on a weighted grid with obstacles
// (cell value = movement cost, -1 = impassable).
//
// TYPES POLICY: strict. This port's shape is exactly what
// `docs/typed-mode-spec.md` describes strict mode paying for: two structs
// (`Point`, `PQEntry`) flowing through several helper functions via a
// `ref` param and return values. The honest finding is that getting
// there cost more than the spec's own framing suggests:
// - `docs/typed-mode-spec.md` §2 states "Internal helpers never require
//   an annotation" (only host-callable/`#fn`/entry-point boundaries do).
//   In practice, EVERY helper function below (`make_bool_grid`,
//   `make_int_grid`, `make_point_grid`, `pq_insert`, `reverse_path`,
//   `path_to_string`) needed an explicit `(param: T): R` signature —
//   without one, strict rejected every single one with "escapes strict
//   inference as `Unknown` — annotate or restructure" (`E065`). The
//   likely reason, reading §2's own inference rule back against this:
//   "Call-site-driven inference is forbidden — `infer_body(A)` reads
//   only `signature(B)`" — a helper whose body is generic over its
//   parameter's element type (e.g. `reverse_path`'s `xs[i]`/`push`
//   round-trip never pins a concrete element type from *inside* the
//   body alone) has nothing to resolve `Unknown` against without an
//   annotation, REGARDLESS of what its callers pass. So "internal
//   helpers never require an annotation" is true only for helpers whose
//   own body happens to pin a concrete type already (arithmetic on a
//   known-`int` param, say) — anything shaped like generic
//   container-in/container-out plumbing needs one in practice. This is a
//   real gap between the spec's framing and this file's lived experience
//   authoring it, worth reading back into the spec's own wording. The
//   same gap shows up one level down, too: `path_to_string`'s `temp p =
//   xs[i]` needed an explicit `temp p: Point = xs[i]` even with `xs`
//   itself already annotated `array<Point>` — indexing a known-element-
//   type array into a local didn't propagate the element type on its
//   own in this build, so the annotation had to be repeated at the
//   `temp` binding, not just the parameter.
// - The original single `make_grid(h, w, fill)` — one function reused
//   for `dist` (`int` fill), `visited` (`bool` fill), and `parent`
//   (`Point` fill), exactly as gradual mode allows and as
//   bfs-grid-path/dfs-grid-path both do — cannot type-check under strict
//   at all: `docs/typed-mode-spec.md` §2 rules "User code is monomorphic
//   in v1: every unification variable must resolve to a concrete type
//   per definition." One `make_grid` definition called with three
//   different concrete `fill` types has no single concrete type to
//   resolve to — there is no generics/parametric-polymorphism escape
//   hatch to fall back on. The fix was splitting it into three
//   monomorphic copies below (`make_bool_grid`/`make_int_grid`/
//   `make_point_grid`), each identical apart from its annotated types.
//   That is real, measurable code growth directly attributable to
//   turning on strict — three near-duplicate 12-line functions instead
//   of one 12-line function — and it is the sharpest, most concrete
//   illustration this corpus has produced so far of "no generics in v1"
//   having an authoring cost, not just a theoretical one.
// - Net assessment: strict mode did its job (every value in the final
//   file really is mono-typed, and the annotations make the PQ/grid
//   shapes explicit and self-documenting at each function boundary), but
//   "earns its keep" undersells the cost here — this file is measurably
//   longer and required three fully-typed rewrites of a helper gradual
//   mode was happy to share. Contrast with astar-grid/story.ink next
//   door: same PQ/grid shapes, gradual, one shared `make_grid`, no
//   annotations, no duplication.
//
// Also note (see bfs-grid-path/story.ink's header for the full writeup):
// array/map sigil literals can't span multiple lines — `VAR grid` below
// is collapsed onto one line for that reason, same as every other file
// in this lane.
//
// PRIORITY QUEUE — NO NATIVE HEAP (this is the finding this file exists
// to produce; see also astar-grid/story.ink, which hits the identical
// wall):
// - brink's stdlib has no heap/priority-queue type. The only array mutators
//   are `push`/`insert`/`remove_at` (`docs/book/.../stdlib.md`), all
//   flat-array operations. `pq_insert` below is textbook "sorted-insertion
//   array": linear-scan to find the insertion point, then `insert(pq, idx,
//   entry)`. That scan is `O(n)`, and popping the minimum is `remove_at(pq,
//   0)` — `bfs-grid-path`'s finding about `remove_at(arr, 0)` being an O(n)
//   shift applies here too, so a single "pop-min" is `O(n)` from the
//   removal alone, on top of the `O(n)` insertion scan every relaxation
//   pays. A binary heap would make both `O(log n)`. For this corpus's
//   36-cell grid the difference is unmeasurable; for any pathfinder
//   sized for a real map, it is the whole ballgame. This is precisely
//   the friction the epic's issue predicted before a single line of this
//   file was written, and it is real, not a strawman: there is no
//   alternative array-based idiom that avoids it. A pair-of-arrays
//   ("parallel priorities array + parallel payload array") restructuring
//   would still need the same `O(n)` scan to find where to insert into
//   the sorted priorities array — it moves the cost around, it does not
//   remove it.
// - The scan itself is a second, sharper illustration of the
//   non-short-circuit `and`/`or` trap `bfs-grid-path` already documents:
//   the naive guard `while idx < len(pq) and pq[idx].priority <=
//   entry.priority` faults the moment `idx` reaches `len(pq)`, because
//   `pq[idx]` is evaluated regardless of whether the left side was
//   false. `pq_insert` below uses the same "searching flag" idiom
//   `binary-search` established instead, specifically because a
//   PQ-insertion scan runs on every single relaxation in this algorithm
//   — it is the highest-traffic loop guard in the whole file, so it is
//   the one place in this port where getting the non-short-circuit rule
//   wrong would have been most likely, and most expensive to debug.
// - A slice/range type (icebox #829) would not remove the `O(n)` scan —
//   finding an insertion point is inherently a linear search over the
//   full priority order — but it WOULD remove the copy cost `insert`
//   and `remove` pay today (shifting every element after the insertion
//   or removal point). `#829`'s own sketch (a `(root cell, range
//   segment)` view) is about avoiding whole-array copies for
//   sub-ranges, which is a different problem from this file's `O(n)`
//   search; noting the distinction so a future reader doesn't expect
//   #829 to fix the PQ's asymptotic cost — only a real heap type would.

STRUCT Point = #{
    r: int,
    c: int,
}

STRUCT PQEntry = #{
    priority: int,
    r: int,
    c: int,
}

VAR grid = #[#[1, 1, 3, -1, 1, 1], #[1, -1, 3, -1, 1, 1], #[1, -1, 1, 1, 1, -1], #[1, -1, -1, -1, 1, 1], #[1, 1, 2, 2, 1, 1], #[-1, -1, -1, -1, 2, 1]]
VAR rows = 6
VAR cols = 6
VAR dr = #[-1, 0, 1, 0]
VAR dc = #[0, 1, 0, -1]
VAR unreachable = 999999

VAR total_cost = -1
VAR path_text = ""
VAR path_len = 0
VAR nodes_visited = 0
VAR found = false

~ {
    temp start = Point#{r: 0, c: 0}
    temp goal = Point#{r: 5, c: 5}

    temp dist = make_int_grid(rows, cols, unreachable)
    temp visited = make_bool_grid(rows, cols, false)
    temp parent = make_point_grid(rows, cols, Point#{r: -1, c: -1})

    dist[start.r][start.c] = 0
    temp pq = #[PQEntry#{priority: 0, r: start.r, c: start.c}]

    while len(pq) > 0 {
        temp top = pq[0]
        remove_at(pq, 0)

        if visited[top.r][top.c] == false {
            visited[top.r][top.c] = true
            nodes_visited = nodes_visited + 1

            if top.r == goal.r and top.c == goal.c {
                found = true
                break
            }

            temp i = 0
            while i < 4 {
                temp nr = top.r + dr[i]
                temp nc = top.c + dc[i]
                if nr >= 0 and nr < rows and nc >= 0 and nc < cols {
                    if grid[nr][nc] != -1 {
                        temp new_dist = dist[top.r][top.c] + grid[nr][nc]
                        if new_dist < dist[nr][nc] {
                            dist[nr][nc] = new_dist
                            parent[nr][nc] = Point#{r: top.r, c: top.c}
                            pq_insert(pq, PQEntry#{priority: new_dist, r: nr, c: nc})
                        }
                    }
                }
                i = i + 1
            }
        }
    }

    if found {
        total_cost = dist[goal.r][goal.c]

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
Total cost: {total_cost}.
Nodes settled: {nodes_visited}.
-> END

// Three near-duplicate monomorphizations of what was one `make_grid`
// helper under gradual mode (see bfs-grid-path/dfs-grid-path) — strict's
// "no generics, monomorphic per definition" rule (typed-mode-spec §2)
// forces this split. See the TYPES POLICY note above.
=== function make_bool_grid(h: int, w: int, fill: bool): array<array<bool>> ===
~ {
    temp g: array<array<bool>> = #[]
    temp r = 0
    while r < h {
        temp row: array<bool> = #[]
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

=== function make_int_grid(h: int, w: int, fill: int): array<array<int>> ===
~ {
    temp g: array<array<int>> = #[]
    temp r = 0
    while r < h {
        temp row: array<int> = #[]
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

=== function make_point_grid(h: int, w: int, fill: Point): array<array<Point>> ===
~ {
    temp g: array<array<Point>> = #[]
    temp r = 0
    while r < h {
        temp row: array<Point> = #[]
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

// Sorted-insertion priority queue push — the finding this file exists
// to produce; see the header comment above.
=== function pq_insert(ref pq: array<PQEntry>, entry: PQEntry): int ===
~ {
    temp idx = 0
    temp searching = true
    while searching {
        if idx >= len(pq) {
            searching = false
        } else {
            if pq[idx].priority <= entry.priority {
                idx = idx + 1
            } else {
                searching = false
            }
        }
    }
    insert(pq, idx, entry)
    return 0
}

=== function reverse_path(xs: array<Point>): array<Point> ===
~ {
    temp out: array<Point> = #[]
    temp i = len(xs) - 1
    while i >= 0 {
        push(out, xs[i])
        i = i - 1
    }
    return out
}

=== function path_to_string(xs: array<Point>): string ===
~ {
    temp out = ""
    temp i = 0
    while i < len(xs) {
        temp p: Point = xs[i]
        out = out + "(" + string(p.r) + "," + string(p.c) + ")"
        if i < len(xs) - 1 {
            out = out + " -> "
        }
        i = i + 1
    }
    return out
}
