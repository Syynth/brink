// ALGORITHMS CORPUS — spatial lane (issue #822)
// Quadtree: recursively subdivide a 2D region into four quadrants once a
// leaf holds more than `max_points`, down to `max_depth` — broad-phase
// spatial queries (which entities are near this point/region) for
// collision or render culling.
//
// LICENSE NOTE (per issue #822's catalog comment): the catalog cites Red
// Blob Games' quadtree notes and Wikipedia's Quadtree page (CC BY-SA).
// This port is written from the general "subdivide into four
// quadrants once a leaf overflows a point-count cap, recurse on
// insert/query" shape of the technique, common to essentially every
// from-scratch implementation, not transcribed from either source's
// specific prose.
//
// SAVE-MID-RUN INTEREST: LOW (per the catalog). Every point in this file
// is inserted in one non-yielding pass; there is no natural pause point.
//
// TYPES POLICY: gradual (default). Every value is an `int`, a `bool`, or
// a `QuadNode`/`Point` struct built entirely from ints; nothing here
// needs strict's escape-error discipline.
//
// ERGONOMICS-FINDINGS:
//
// 1. THE HEADLINE FINDING — WHY THIS FILE USES AN ARENA (`Array<QuadNode>`
//    + INT CHILD INDICES), NOT A SELF-REFERENTIAL STRUCT, AND WHY THAT
//    ANSWER DIFFERS FROM `behavior-tree/story.ink`'S FINDING NEXT DOOR.
//    That file discovered `STRUCT BTNode = #{ …, children: Array<BTNode> }`
//    compiles and runs correctly — "self-referential structs work,
//    contradicting this epic's own prediction" — and warned future ports
//    in this same epic not to "reflexively reach for an arena when a
//    plain recursive struct already works." This file is exactly the
//    counter-case that warning anticipated, and the reconciliation is
//    precise, not a contradiction: `BTNode`'s tree is built ONCE, bottom-
//    up, in a single non-yielding pass, and never mutated again — a
//    value-semantics recursive struct is fine for that, because nothing
//    ever needs to reach back into an already-constructed subtree. A
//    quadtree's defining operation is the opposite: `quad_insert` is
//    called once PER POINT, and a later insert routinely needs to mutate
//    a node that an earlier insert already built (converting a leaf into
//    an internal node, or appending a point to an existing leaf's
//    bucket). With `children: Array<QuadNode>` as a value, the child
//    subtree handed to a recursive call is a COPY — mutating it inside
//    the call cannot be observed by the caller's own copy without
//    threading a freshly-rebuilt struct back up through every stack
//    frame on every single insert (a full root-to-leaf copy per point,
//    same cost shape `bsp-dungeon`'s header already flags for its
//    one-time, build-only tree). The arena sidesteps this completely:
//    `nodes` is one flat top-level `VAR Array<QuadNode>`, exactly the
//    kind of global this corpus's DP lane already mutates directly by
//    name (`memoized-fibonacci`'s `memo`, `knapsack-01`'s `memo`) with no
//    `ref` parameters — `quad_insert`/`quad_subdivide` read and write
//    `nodes[idx].<field>` in place, and a child "pointer" is just an
//    `int` index into that same array, never a nested copy. The general
//    rule this pair of files now documents together: a recursive struct
//    that is built once and read thereafter can be a plain value type; a
//    recursive structure that is MUTATED after construction (inserted
//    into over time, as any real quadtree/HFSM is) needs the arena+index
//    idiom the epic's own catalog originally predicted — the value-
//    semantics friction was never about self-reference, it was always
//    about mutation-after-construction. Feeds directly into #521/#829's
//    tree-representation questions.
//
// 2. THE ACTUAL ARENA-MUTATION FRICTION, CONFIRMED EMPIRICALLY AGAINST
//    A WRONG FIRST GUESS — read this alongside finding 1's "why an
//    arena" case, because getting the arena to WORK turned out to have
//    its own sharp edge finding 1 doesn't cover. The first draft of this
//    file wrote `nodes[idx].nw = nw_idx` (an index, THEN a field write)
//    directly, on the theory that `t1e-spec`'s `ref party[leader].hp`
//    example meant index-then-field chains were valid lvalues generally.
//    That theory was wrong for assignment: `nodes[idx].nw = nw_idx`
//    fails to compile with `E074` ("chained field-write projection
//    (p.a.b = v) is not supported") — `t1e-spec`'s example is a `ref`
//    ARGUMENT (a read-reference passed into a call), not an assignment
//    target, and the two turn out to follow different rules. Worse,
//    `push(nodes[idx].pt_xs, px)` (index-then-field as a MUTATOR's first
//    argument) rejects with a DIFFERENT diagnostic, `E055` ("collection
//    mutator's first argument is not an lvalue") — two distinct error
//    codes for what looks like the same underlying restriction from the
//    author's side. A third variant used to be worse than either compile
//    error: pushing into a plain (non-indexed) struct temp's array field
//    directly — `temp b = Box#{xs: #[]}; push(b.xs, 5)`, no array
//    indexing anywhere in the chain — compiled with ZERO diagnostics and
//    produced a `.inkb`, but then FAILED AT LINK TIME with
//    `unresolved global: $07_...` when actually run: a genuine
//    silent-miscompile-shaped bug (compiles clean, breaks downstream with
//    no diagnostic pointing back at the cause), squarely the kind of thing
//    the project's own "flag silent data drops" rule exists for. That was
//    issue #1495 (`try_lower_mutator_stmt`'s bare-lvalue fast path resolving
//    a dotted `a.items` path to its root symbol instead of the field, hence
//    the link-time "unresolved global" for a Temp-rooted root) and is fixed
//    — a single-level struct-field mutator lvalue now lowers through
//    `lower_field_mutator`, exercised end-to-end (including this exact
//    Temp-rooted shape) by `tests/tier1-brink/struct-field-mutator-lvalue/`.
//    It is not applied retroactively in this port (out of this epic's
//    scope; feeds #521/#829 same as finding 1) because THE WORKING IDIOM,
//    used throughout this file's `quad_insert`/`quad_subdivide`, remains the
//    better fit for an *indexed* root regardless: never write through a
//    chained lvalue at all — read the whole element out
//    (`temp node = nodes[idx]`), mutate plain local temps (a bare
//    struct's own field write, `node.nw = value`, and a bare array's own
//    `push`/`insert` both work fine, because neither is a chained
//    projection), then write the WHOLE modified value back in one shot
//    (`nodes[idx] = node`). Slower to write than the chained form would
//    have been, but every step is a single-level projection, which is
//    exactly the shape every restriction above allows — and #1495's fix
//    only reaches a single-level *bare* root (`a.items`), not an indexed
//    one (`nodes[idx].pt_xs`), so `push(nodes[idx].pt_xs, px)`'s `E055`
//    two sentences up is unaffected.
//
// 3. NO ARRAY CONCATENATION, AGAIN: merging the four children's query
//    results back into one list is the same manual `for`/`push` loop
//    quicksort/mergesort/bsp-dungeon have all already flagged — noted
//    here only because a 4-way recursive merge (vs. those files' 2-way)
//    makes the missing primitive slightly more visible, not because
//    anything new was learned about it.
//
// 4. THE "SIGIL LITERALS CAN'T SPAN MULTIPLE LINES" GAP `bfs-grid-path`
//    DOCUMENTS FOR `#[...]`/`#{...}` EXTENDS TO STRUCT-CONSTRUCTION
//    LITERALS TOO: a first draft of `make_leaf` below wrote
//    `QuadNode#{ x: x, y: y, ... }` spread across several lines (one
//    field per line, for readability against `QuadNode`'s dozen fields)
//    and got a wall of `E037`/`E015`/`E025` starting at the first
//    line break inside the braces — the failure mode is worse than the
//    array case, because the parser doesn't just reject the literal, it
//    resyncs on the next line and reports every bare field name
//    (`is_leaf`, `pt_xs`, `nw`, …) as an unresolved variable reference,
//    burying the real "you can't break this literal across lines" cause
//    under a dozen misleading diagnostics. `bfs-grid-path`'s finding was
//    scoped to `#[...]`/`#{...}` collection literals specifically; this
//    confirms the same `NEWLINE`-is-not-trivia-inside-a-sigil-literal
//    restriction applies uniformly to `TypeName#{...}` construction too,
//    and a struct with this many fields makes the single-line
//    requirement's readability cost sharply more visible than any prior
//    corpus file's shorter structs did.
//
// 5. BOUNDED BY CONSTRUCTION: `max_depth` caps subdivision (a leaf at
//    `max_depth` accepts unlimited points rather than recursing further),
//    satisfying the house "guard against unbounded growth" rule for the
//    one loop in this file that isn't already bounded by a fixed input
//    size — a pathological input with many coincident points can't drive
//    recursion depth past `max_depth` regardless of point count.

STRUCT QuadNode = #{
    x: int,
    y: int,
    w: int,
    h: int,
    depth: int,
    is_leaf: bool,
    pt_xs: Array<int>,
    pt_ys: Array<int>,
    pt_ids: Array<int>,
    nw: int,
    ne: int,
    sw: int,
    se: int,
}

VAR max_points = 3
VAR max_depth = 3

#@local
VAR nodes = #[]

VAR point_xs = 0
VAR point_ys = 0

VAR node_count = 0

VAR region_ids = ""
VAR region_count = 0
VAR brute_region_count = 0
VAR region_matches = false

VAR corner_ids = ""
VAR corner_count = 0
VAR brute_corner_count = 0
VAR corner_matches = false

~ {
    point_xs = #[3, 5, 60, 58, 30, 31, 32, 33, 2, 61, 45, 20, 10, 50]
    point_ys = #[4, 6, 61, 59, 30, 31, 5, 33, 60, 2, 10, 50, 55, 45]

    push(nodes, make_leaf(0, 0, 64, 64, 0))

    temp i = 0
    while i < len(point_xs) {
        quad_insert(0, point_xs[i], point_ys[i], i)
        i = i + 1
    }
    node_count = len(nodes)

    temp region = quad_query(0, 24, 24, 16, 16)
    region_ids = ids_to_string(sort_ids(region))
    region_count = len(region)
    temp brute_region = brute_query(24, 24, 16, 16)
    brute_region_count = len(brute_region)
    region_matches = same_id_set(region, brute_region)

    temp corner = quad_query(0, 56, 0, 8, 8)
    corner_ids = ids_to_string(sort_ids(corner))
    corner_count = len(corner)
    temp brute_corner = brute_query(56, 0, 8, 8)
    brute_corner_count = len(brute_corner)
    corner_matches = same_id_set(corner, brute_corner)
}

Points inserted: {len(point_xs)}. Arena nodes after insert (root + subdivisions): {node_count}.
Query region (24,24) 16x16 quadtree hits: {region_ids} ({region_count}). Brute-force count: {brute_region_count}. Match: {region_matches}.
Query corner (56,0) 8x8 quadtree hits: {corner_ids} ({corner_count}). Brute-force count: {brute_corner_count}. Match: {corner_matches}.
-> END

=== function make_leaf(x, y, w, h, depth) ===
~ {
    return QuadNode#{x: x, y: y, w: w, h: h, depth: depth, is_leaf: true, pt_xs: #[], pt_ys: #[], pt_ids: #[], nw: -1, ne: -1, sw: -1, se: -1}
}

=== function quad_insert(idx, px, py, pid) ===
~ {
    temp node = nodes[idx]
    if node.is_leaf {
        if len(node.pt_xs) < max_points or node.depth >= max_depth {
            temp new_xs = node.pt_xs
            temp new_ys = node.pt_ys
            temp new_ids = node.pt_ids
            push(new_xs, px)
            push(new_ys, py)
            push(new_ids, pid)
            node.pt_xs = new_xs
            node.pt_ys = new_ys
            node.pt_ids = new_ids
            nodes[idx] = node
        } else {
            quad_subdivide(idx)
            insert_into_child(idx, px, py, pid)
        }
    } else {
        insert_into_child(idx, px, py, pid)
    }
}

=== function insert_into_child(idx, px, py, pid) ===
~ {
    temp node = nodes[idx]
    temp mid_x = node.x + node.w / 2
    temp mid_y = node.y + node.h / 2
    if py < mid_y {
        if px < mid_x {
            quad_insert(node.nw, px, py, pid)
        } else {
            quad_insert(node.ne, px, py, pid)
        }
    } else {
        if px < mid_x {
            quad_insert(node.sw, px, py, pid)
        } else {
            quad_insert(node.se, px, py, pid)
        }
    }
}

=== function quad_subdivide(idx) ===
~ {
    temp node = nodes[idx]
    temp hw = node.w / 2
    temp hh = node.h / 2
    temp next_depth = node.depth + 1

    temp nw_idx = len(nodes)
    push(nodes, make_leaf(node.x, node.y, hw, hh, next_depth))
    temp ne_idx = len(nodes)
    push(nodes, make_leaf(node.x + hw, node.y, hw, hh, next_depth))
    temp sw_idx = len(nodes)
    push(nodes, make_leaf(node.x, node.y + hh, hw, hh, next_depth))
    temp se_idx = len(nodes)
    push(nodes, make_leaf(node.x + hw, node.y + hh, hw, hh, next_depth))

    // `node` was copied by value before the four `push(nodes, ...)`
    // calls above grew the arena — value semantics mean that growth
    // can't invalidate or alias this local copy, so it's still safe to
    // mutate here and write back as one whole element (see the
    // ERGONOMICS-FINDINGS entry on chained-lvalue restrictions above for
    // why it has to be "mutate the local copy, then write back whole"
    // rather than writing through `nodes[idx].<field>` directly).
    node.nw = nw_idx
    node.ne = ne_idx
    node.sw = sw_idx
    node.se = se_idx
    node.is_leaf = false

    temp old_xs = node.pt_xs
    temp old_ys = node.pt_ys
    temp old_ids = node.pt_ids
    node.pt_xs = #[]
    node.pt_ys = #[]
    node.pt_ids = #[]
    nodes[idx] = node

    temp i = 0
    while i < len(old_xs) {
        insert_into_child(idx, old_xs[i], old_ys[i], old_ids[i])
        i = i + 1
    }
}

=== function rect_intersects(node, qx, qy, qw, qh) ===
~ {
    if qx + qw <= node.x {
        return false
    }
    if qx >= node.x + node.w {
        return false
    }
    if qy + qh <= node.y {
        return false
    }
    if qy >= node.y + node.h {
        return false
    }
    return true
}

=== function point_in_rect(px, py, qx, qy, qw, qh) ===
~ {
    if px < qx or px >= qx + qw {
        return false
    }
    if py < qy or py >= qy + qh {
        return false
    }
    return true
}

=== function quad_query(idx, qx, qy, qw, qh) ===
~ {
    temp node = nodes[idx]
    temp hits = #[]
    if rect_intersects(node, qx, qy, qw, qh) == false {
        return hits
    }

    if node.is_leaf {
        temp i = 0
        while i < len(node.pt_xs) {
            if point_in_rect(node.pt_xs[i], node.pt_ys[i], qx, qy, qw, qh) {
                push(hits, node.pt_ids[i])
            }
            i = i + 1
        }
        return hits
    }

    temp from_nw = quad_query(node.nw, qx, qy, qw, qh)
    temp from_ne = quad_query(node.ne, qx, qy, qw, qh)
    temp from_sw = quad_query(node.sw, qx, qy, qw, qh)
    temp from_se = quad_query(node.se, qx, qy, qw, qh)

    temp j = 0
    while j < len(from_nw) {
        push(hits, from_nw[j])
        j = j + 1
    }
    j = 0
    while j < len(from_ne) {
        push(hits, from_ne[j])
        j = j + 1
    }
    j = 0
    while j < len(from_sw) {
        push(hits, from_sw[j])
        j = j + 1
    }
    j = 0
    while j < len(from_se) {
        push(hits, from_se[j])
        j = j + 1
    }
    return hits
}

=== function brute_query(qx, qy, qw, qh) ===
~ {
    temp hits = #[]
    temp i = 0
    while i < len(point_xs) {
        if point_in_rect(point_xs[i], point_ys[i], qx, qy, qw, qh) {
            push(hits, i)
        }
        i = i + 1
    }
    return hits
}

=== function contains_id(ids, target) ===
~ {
    temp i = 0
    while i < len(ids) {
        if ids[i] == target {
            return true
        }
        i = i + 1
    }
    return false
}

=== function same_id_set(a, b) ===
~ {
    if len(a) != len(b) {
        return false
    }
    temp i = 0
    while i < len(a) {
        if contains_id(b, a[i]) == false {
            return false
        }
        i = i + 1
    }
    return true
}

=== function sort_ids(ids) ===
~ {
    temp out = #[]
    temp i = 0
    while i < len(ids) {
        push(out, ids[i])
        i = i + 1
    }
    temp n = len(out)
    temp a = 0
    while a < n {
        temp b = 0
        while b < n - 1 {
            if out[b] > out[b + 1] {
                temp tmp = out[b]
                out[b] = out[b + 1]
                out[b + 1] = tmp
            }
            b = b + 1
        }
        a = a + 1
    }
    return out
}

=== function ids_to_string(ids) ===
~ {
    temp out = ""
    temp i = 0
    while i < len(ids) {
        out = out + string(ids[i])
        if i < len(ids) - 1 {
            out = out + " "
        }
        i = i + 1
    }
    return out
}
