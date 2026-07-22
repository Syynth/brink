// ALGORITHMS CORPUS — graphs lane (issue #822)
// Depth-first search: A path (not necessarily shortest) on a grid with
// obstacles, via an explicit stack.
//
// TYPES POLICY: gradual (default). Same shape as bfs-grid-path: ints,
// bools, arrays, and a two-field int `Point` struct throughout — nothing
// here is ever `Unknown`/`Conflicted` under inference, so `types = strict`
// buys nothing.
//
// THE GRID: byte-for-byte the same 4x6 grid, start (0,0), and goal (3,0)
// as bfs-grid-path/story.ink — deliberately, so this pair is a direct A/B
// demonstration of "same input, different traversal order, different
// answer." The grid has exactly two disjoint, dead-end-free routes from
// start to goal: a 4-cell drop straight down column 0, and a 14-cell loop
// across row 0, down column 5, and back across row 3. This port's fixed
// neighbor-expansion priority (up, right, down, left) always prefers
// "right" over "down" at the start (the start's "up" neighbor is off-grid,
// so "right" is the first *available* direction) — and because the long
// loop never dead-ends, DFS walks it to completion and reports it as *a*
// path without ever backtracking to try the short one. BFS, on the exact
// same grid, always reports the true 4-cell shortest path. Neither answer
// is a bug: DFS was never promised optimality, and this pairing is the
// clearest way this corpus has to make that concrete instead of asserted.
//
// ERGONOMICS-FINDINGS:
// - Same "array/map sigil literals can't span multiple lines" finding as
//   bfs-grid-path (see its header for the full writeup) — this file's
//   `VAR grid` literal below is collapsed onto one line for the same
//   reason.
// - The stack side of this port has NONE of bfs-grid-path's queue-removal
//   pain: `push`/pop-by-popping-the-back is `push(stack, v)` +
//   `temp top = stack[len(stack) - 1]` + `remove(stack, len(stack) - 1)`,
//   and removing the LAST element is O(1) (no shift), unlike removing
//   index 0. Same stdlib (`push`/`remove`), opposite cost — worth stating
//   plainly since v1's "data-structure gymnastics" finding already noted
//   stack-over-arrays is cheap; this is the direct side-by-side proof
//   that queue-over-arrays (bfs-grid-path) is the one that actually
//   hurts, not arrays-as-stacks in general.
// - Getting the LIFO push order right to match a *declared* neighbor
//   priority (up, right, down, left) needs the deltas pushed in REVERSE:
//   the last-pushed neighbor is popped first, so to explore "up" before
//   "right" before "down" before "left", the loop below pushes them
//   left, down, right, up. This inversion is easy to get backwards during
//   authoring (it was, once, while drafting this file) and there's no
//   language help — no `push_front`/deque primitive that would let the
//   push order match the intended visit order directly.
// - `break` exits the nearest loop but the search still needed a `found`
//   flag threaded out of the loop to distinguish "stack ran empty, no
//   path" from "hit the goal" — same shape as binary-search's
//   already-documented finding about `while` having no break-with-value.

STRUCT Point = #{
    r: int,
    c: int,
}

VAR grid = #[#[0, 0, 0, 0, 0, 0], #[0, 1, 1, 1, 1, 0], #[0, 1, 1, 1, 1, 0], #[0, 0, 0, 0, 0, 0]]
VAR rows = 4
VAR cols = 6

// Push order is the REVERSE of visit priority (up, right, down, left) —
// see the ERGONOMICS-FINDINGS note above: the stack is LIFO, so whatever
// is pushed last is explored first.
VAR push_dr = #[0, 1, 0, -1]
VAR push_dc = #[-1, 0, 1, 0]

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
    temp stack = #[start]

    while len(stack) > 0 {
        temp top_index = len(stack) - 1
        temp cur = stack[top_index]
        remove(stack, top_index)
        nodes_visited = nodes_visited + 1

        if cur.r == goal.r and cur.c == goal.c {
            found = true
            break
        }

        temp i = 0
        while i < 4 {
            temp nr = cur.r + push_dr[i]
            temp nc = cur.c + push_dc[i]
            if nr >= 0 and nr < rows and nc >= 0 and nc < cols {
                if grid[nr][nc] == 0 and visited[nr][nc] == false {
                    visited[nr][nc] = true
                    parent[nr][nc] = cur
                    push(stack, Point#{r: nr, c: nc})
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
Nodes popped: {nodes_visited}.
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
