// ALGORITHMS CORPUS — graphs lane (issue #822)
// A*: cheapest path on the same weighted grid as dijkstra-grid, guided by
// a Manhattan-distance heuristic.
//
// TYPES POLICY: gradual (default). Deliberately the mirror image of
// dijkstra-grid/story.ink's choice: same two structs (`Point`, `PQEntry`),
// same shape of code, but left under gradual inference here so the pair
// is a direct A/B on `types = strict`'s cost/benefit for identical
// struct-heavy code, not just a claim about it. Nothing in this file
// needed strict's escape-checking to compile correctly.
//
// SAME GRID, SAME ANSWER, FEWER NODES: this file reuses dijkstra-grid's
// exact grid, start (0,0), and goal (5,5) on purpose. Both algorithms are
// guaranteed to find the same optimal cost (both find 12 here) because
// the Manhattan-distance heuristic is admissible on a 4-directional
// unit-minimum-cost grid (it never overestimates the true remaining
// cost — the cheapest possible move is 1, so `manhattan(a, b)` is always
// <= the true cost from `a` to `b`). What differs is HOW MANY nodes get
// settled before the goal is reached: Dijkstra explores strictly by
// distance-from-start and has no notion of "closer to the goal", so it
// fans out in all directions; A*'s priority adds the heuristic
// (`priority = cost_so_far + manhattan(node, goal)`), which biases the
// search toward the goal and settles noticeably fewer nodes on this grid
// (see both files' golden transcripts for the exact counts). This A/B is
// the single clearest "why would a game reach for A* over Dijkstra"
// demonstration this corpus can make without a much bigger map.
//
// Also note (see bfs-grid-path/story.ink's header): array/map sigil
// literals can't span multiple lines — `VAR grid` below is one line.
//
// PRIORITY QUEUE — NO NATIVE HEAP: identical situation and identical
// `pq_insert` shape to dijkstra-grid/story.ink; see that file's header
// for the full finding (the O(n) insertion scan, the O(n) `remove(pq,
// 0)` pop, the non-short-circuit `and`/`or` trap in the scan guard, and
// why icebox #829's slice/range sketch would help the copy cost but not
// the search cost). Not re-derived here to avoid duplicating the same
// finding twice — this file's own addition to it is narrower:
// - The heuristic call sits on the SAME hot path as the PQ insertion
//   (every relaxation computes `manhattan(nr, nc, goal.r, goal.c)`
//   immediately before calling `pq_insert`), which makes A* the first
//   file in this lane where "no native heap" and "no vector/distance
//   math helpers" (no `abs`, no `min`/`max` — see `manhattan` below,
//   hand-rolling absolute value with an `if`) compound on the exact same
//   line of the algorithm. Neither gap is new on its own (the epic's own
//   catalog flags float-vector friction repeatedly for the procgen/AI
//   lanes), but this is the first place in the corpus so far where two
//   separate stdlib gaps stack inside one hot loop instead of appearing
//   in different files.

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

    temp dist = make_grid(rows, cols, unreachable)
    temp visited = make_grid(rows, cols, false)
    temp parent = make_grid(rows, cols, Point#{r: -1, c: -1})

    dist[start.r][start.c] = 0
    temp start_h = manhattan(start.r, start.c, goal.r, goal.c)
    temp pq = #[PQEntry#{priority: start_h, r: start.r, c: start.c}]

    while len(pq) > 0 {
        temp top = pq[0]
        remove(pq, 0)

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
                            temp h = manhattan(nr, nc, goal.r, goal.c)
                            pq_insert(pq, PQEntry#{priority: new_dist + h, r: nr, c: nc})
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

// Manhattan distance — hand-rolled: no `abs` builtin (see header finding).
=== function manhattan(ar, ac, br, bc) ===
~ {
    temp dr_abs = ar - br
    if dr_abs < 0 {
        dr_abs = 0 - dr_abs
    }
    temp dc_abs = ac - bc
    if dc_abs < 0 {
        dc_abs = 0 - dc_abs
    }
    return dr_abs + dc_abs
}

// Sorted-insertion priority queue push — see the header comment above and
// dijkstra-grid/story.ink's fuller writeup of the same finding.
=== function pq_insert(ref pq, entry) ===
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
