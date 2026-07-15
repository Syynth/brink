// ALGORITHMS CORPUS — procgen lane (issue #822)
// BSP (binary space partitioning) dungeon layout: recursively split a
// rectangle into two halves (alternating axis by aspect ratio) down to a
// depth/size limit, leaving a list of leaf rooms — classic roguelike
// level-layout generation.
//
// LICENSE NOTE (per issue #822's catalog comment): the catalog cites
// RogueBasin's "Basic BSP Dungeon Generation" page (GFDL 1.2) as the
// reference for this technique. GFDL is treated like CC BY-SA under the
// epic's methodology — safe to read for the idea, not to transcribe. This
// port was written from the general "recursively split, alternate axis,
// stop at a size floor" shape of the technique, not from that page's
// prose or pseudocode; no GPL-only reference was in scope here.
//
// SAVE-MID-RUN INTEREST: LOW (per the catalog's own rating — "recursive
// tree-of-structs is a decent structs/recursion combo exercise, no major
// surprises expected"). The whole tree is built in one non-yielding
// recursive call; there's no natural per-node pause point worth a
// resumable variant.
//
// SEEDED RNG: vanilla ink's `SEED_RANDOM`/`RANDOM` (inclusive both ends),
// same as every other file in this corpus — see
// fisher-yates-shuffle/story.ink's header for why a hand-rolled in-ink PCG
// isn't needed.
//
// TYPES POLICY: gradual (default). `Rect` is the only struct and every
// numeric value is an int; gradual inference resolves the whole file with
// no ambiguity worth strict's escape-checking ceremony.
//
// ERGONOMICS-FINDINGS:
// - `split` is written FUNCTIONALLY (returns a fresh `array<Rect>` of
//   leaves rather than mutating a shared `rooms` accumulator via a `ref`
//   param) — same "arrays are copy-on-write values, functional
//   partition-and-recombine reads closer to the textbook description"
//   finding quicksort's header makes. Concatenating the left/right
//   sub-trees' leaf lists needs a manual `for`/`push` loop each time (no
//   array concat primitive — same gap quicksort and mergesort both flag),
//   so a tree with more leaves pays one extra full copy per internal node
//   on the way back up. For this corpus's depth-3 tree that's invisible;
//   noted because it's the same shape of cost bfs-grid-path's `remove(arr,
//   0)` finding describes, just for `push`-heavy concatenation instead of
//   `remove`-heavy dequeuing.
// - Recursion base case needed THREE independent conditions
//   (`depth <= 0 or rect.w < min_size * 2 or rect.h < min_size * 2`) —
//   all cheap int comparisons on the same struct's fields, so (unlike the
//   graphs lane's indexed-access guards) the non-short-circuit `and`/`or`
//   trap has nothing to bite: evaluating all three unconditionally is
//   exactly as correct as short-circuiting would have been. Worth noting
//   as the counter-example to the graphs lane's repeated warning — the
//   trap is about what the operands DO, not a blanket "avoid or" rule.
// - `RANDOM(min_cut, max_cut)` reads directly as the inclusive split-point
//   draw the algorithm wants, with the base case's size guard already
//   having proven `max_cut >= min_cut` before the call — no off-by-one
//   translation, no defensive re-check needed at the call site.
// - The partition invariant (leaf areas sum to exactly the root's area,
//   since BSP never overlaps or leaves gaps) is a free correctness check
//   this port gets to print and verify against the golden transcript —
//   "living documentation" made mechanically checkable, not just narrated
//   in a comment, matching astar-grid/dijkstra-grid's cost-equality check
//   in the graphs lane.

STRUCT Rect = #{
    x: int,
    y: int,
    w: int,
    h: int,
}

VAR min_size = 3
VAR max_depth = 3

VAR root = 0
VAR rooms = 0
VAR room_count = 0
VAR room_text = ""
VAR total_leaf_area = 0

~ SEED_RANDOM(9001)
~ {
    root = Rect#{x: 0, y: 0, w: 20, h: 12}
    rooms = split(root, max_depth, min_size)
    room_count = len(rooms)
    room_text = rooms_to_string(rooms)
    total_leaf_area = sum_area(rooms)
}

Root: (0,0) 20x12.
Rooms generated: {room_count}.
{room_text}
Total leaf area: {total_leaf_area}.
Root area: {root.w * root.h}.
Partition exact (no overlap, no gap): {total_leaf_area == root.w * root.h}.
-> END

=== function split(rect, depth, min_size) ===
~ {
    temp rooms = #[]
    if depth <= 0 or rect.w < min_size * 2 or rect.h < min_size * 2 {
        push(rooms, rect)
        return rooms
    }

    temp wide = rect.w > rect.h
    if wide {
        temp cut = RANDOM(min_size, rect.w - min_size)
        temp left = Rect#{x: rect.x, y: rect.y, w: cut, h: rect.h}
        temp right = Rect#{x: rect.x + cut, y: rect.y, w: rect.w - cut, h: rect.h}
        temp left_rooms = split(left, depth - 1, min_size)
        temp right_rooms = split(right, depth - 1, min_size)
        for r in left_rooms {
            push(rooms, r)
        }
        for r in right_rooms {
            push(rooms, r)
        }
    } else {
        temp cut = RANDOM(min_size, rect.h - min_size)
        temp top = Rect#{x: rect.x, y: rect.y, w: rect.w, h: cut}
        temp bottom = Rect#{x: rect.x, y: rect.y + cut, w: rect.w, h: rect.h - cut}
        temp top_rooms = split(top, depth - 1, min_size)
        temp bottom_rooms = split(bottom, depth - 1, min_size)
        for r in top_rooms {
            push(rooms, r)
        }
        for r in bottom_rooms {
            push(rooms, r)
        }
    }
    return rooms
}

=== function rooms_to_string(rooms) ===
~ {
    temp out = ""
    temp i = 0
    while i < len(rooms) {
        temp r = rooms[i]
        out = out + "Room " + string(i) + ": (" + string(r.x) + "," + string(r.y) + ") " + string(r.w) + "x" + string(r.h)
        if i < len(rooms) - 1 {
            out = out + " | "
        }
        i = i + 1
    }
    return out
}

=== function sum_area(rooms) ===
~ {
    temp total = 0
    for r in rooms {
        total = total + r.w * r.h
    }
    return total
}
