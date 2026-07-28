// ALGORITHMS CORPUS — spatial lane (issue #822)
// Spatial hash grid: bucket a fixed set of `Entity` structs into uniform
// cells keyed by `(cell_x, cell_y)`, so a radius query only has to scan
// entities in nearby buckets instead of every entity in the world —
// broad-phase collision/proximity culling for dynamic objects.
//
// LICENSE NOTE (per issue #822's catalog comment): the catalog cites
// general "spatial hashing" technique write-ups (Red Blob Games grid
// notes, MIT/Apache-2.0) with no single canonical reference implementation
// — this port is written from the well-known "bucket by
// floor(coord / cell_size), key the bucket map on the cell coordinate
// pair" shape of the technique, common to essentially every independent
// description of it.
//
// SAVE-MID-RUN INTEREST: LOW (per the catalog). The bucket map is rebuilt
// once from a fixed entity list; there's no natural pause point.
//
// TYPES POLICY: gradual (default). Entities are `int` id/x/y; the bucket
// map is `Map<string, Array<int>>` (composite cell key, see the finding
// below); nothing here needs strict's escape-error discipline.
//
// ERGONOMICS-FINDINGS:
// - THE CATALOG'S OWN PREDICTION HOLDS EXACTLY: "needs a real hashmap
//   keyed on a composite (cell x,y) — a good simple test of brink's map/
//   struct-as-key ergonomics." Confirmed, and the answer is identical to
//   `knapsack-01`'s DP-lane finding, not a new discovery: maps only take
//   scalar keys (`int`/`string`/`bool`/`float`), so a `(cell_x, cell_y)`
//   pair has to flatten into one `string` key
//   (`string(cx) + "," + string(cy)`) exactly the way `knapsack-01`
//   flattens `(item_index, capacity)`. Same tradeoff, same caveat: a
//   compiler-checked composite key doesn't exist, so a separator typo or
//   an accidental cell-coordinate collision (e.g. cell `(1,23)` vs
//   `(12,3)` if a separator were ever dropped) is a silent wrong-bucket
//   bug, not a compile error — this file uses `","` as the separator and
//   both coordinates are always non-negative single/double-digit numbers
//   for this fixture, so no collision is actually reachable here, but the
//   general risk is the same one `knapsack-01` already flagged.
// - THE SAME `insert`-VS-ASSIGNMENT SHARP EDGE `longest-common-
//   subsequence` DOCUMENTS, HIT INDEPENDENTLY HERE: a bucket's first
//   entity can't be added with `buckets[key] = #[idx]`, because
//   `container[key] = value` only ever updates an EXISTING key (same rule
//   for maps as for arrays). The fix is the same two-step shape that
//   file's header describes — `insert(buckets, key, #[])` to create the
//   empty bucket, then `push(buckets[key], idx)` to populate it — guarded
//   by `if contains(buckets, key) == false`. Two independent files in two
//   different lanes hitting the exact same gotcha from unrelated
//   directions (DP memo table vs. spatial hash bucket) is itself a
//   finding: this is not an edge case, it is THE map-ergonomics trap of
//   this dialect, and any future map-heavy port should expect to hit it.
// - INTEGER DIVISION TRUNCATES TOWARD ZERO, NOT FLOOR — a real trap for
//   cell-coordinate math with any negative input: `-5 / 10` is `0` in
//   this dialect (confirmed against `brink-runtime::value_ops`'s
//   `wrapping_div` on `i32`), not `-1` as a floor-division convention
//   would give, so naively bucketing a negative coordinate silently
//   merges it into cell `0` instead of the cell one step further out.
//   This port sidesteps the whole question by keeping every entity and
//   every query inside a non-negative world (`0..world_size`) and
//   clamping a query's search rectangle to `0` before dividing by
//   `cell_size` (see `clamp_low` below) rather than dividing a
//   possibly-negative bound directly — correct for this fixture, but a
//   general-purpose port covering negative coordinates would need an
//   explicit floor-division helper (`(a - (a % b) + b) % b`-style
//   correction, or shift the whole coordinate space positive first), and
//   this file does neither because it doesn't need to.
// - `VAR entities = #[Entity#{...}, ...]` (A STRUCT-LITERAL ARRAY AS A
//   TOP-LEVEL `VAR` DEFAULT) IS REJECTED AT COMPILE TIME: `E075`,
//   "struct construction literal is not supported as a VAR/CONST
//   declaration default," fires on every single element of the literal.
//   `VAR` defaults apparently need to be const-foldable in a way a named
//   struct construction isn't (a plain `#[1, 2, 3]` `VAR` default is fine
//   throughout this corpus — see every grid file's `VAR grid = #[...]`).
//   `utility-ai/story.ink` had already independently worked around this
//   by building its `Array<ActionOption>` as a `temp` inside a `~ { }`
//   block rather than a `VAR` default; this file hits the same wall and
//   uses the same fix, just assigning straight to the top-level `VAR`
//   (`entities = #[Entity#{...}, ...]`) instead of a `temp` — worth
//   flagging as a second independent confirmation that "no struct-literal
//   arrays in `VAR` defaults" is a real, general dialect restriction, not
//   a one-off quirk of `utility-ai`'s specific construction.
// - THE QUERY CROSS-CHECK AGAINST BRUTE FORCE IS THE LOAD-BEARING PROOF
//   HERE, same "living documentation made checkable" idiom astar-grid/
//   dijkstra-grid's cost check and bsp-dungeon's area invariant use: a
//   spatial hash that scans the wrong buckets, or a bucket key built with
//   an off-by-one cell boundary, produces a WRONG but plausible-looking
//   answer with no crash and no diagnostic — only a brute-force O(n)
//   distance scan over every entity, compared set-for-set against the
//   hashed query's result, actually proves the acceleration structure
//   didn't drop or add anything. `same_id_set` below is doing real work,
//   not ceremony.

STRUCT Entity = #{
    id: int,
    x: int,
    y: int,
}

VAR cell_size = 10
VAR world_size = 40

VAR entities = 0

#@local
VAR buckets = #{}

VAR bucket_count = 0
VAR total_bucketed = 0

VAR near_center_ids = ""
VAR near_center_count = 0
VAR brute_center_count = 0
VAR center_query_matches = false

VAR near_corner_ids = ""
VAR near_corner_count = 0
VAR brute_corner_count = 0
VAR corner_query_matches = false

~ {
    entities = #[Entity#{id: 0, x: 2, y: 3}, Entity#{id: 1, x: 8, y: 9}, Entity#{id: 2, x: 12, y: 4}, Entity#{id: 3, x: 18, y: 8}, Entity#{id: 4, x: 22, y: 21}, Entity#{id: 5, x: 27, y: 19}, Entity#{id: 6, x: 14, y: 16}, Entity#{id: 7, x: 33, y: 33}, Entity#{id: 8, x: 5, y: 35}, Entity#{id: 9, x: 16, y: 14}, Entity#{id: 10, x: 20, y: 20}, Entity#{id: 11, x: 38, y: 2}]

    temp i = 0
    while i < len(entities) {
        bucket_entity(i)
        i = i + 1
    }
    bucket_count = len(buckets)
    total_bucketed = count_bucketed()

    temp near_center = query_radius(15, 15, 8)
    near_center_ids = ids_to_string(sort_ids(near_center))
    near_center_count = len(near_center)
    temp brute_center = brute_force_radius(15, 15, 8)
    brute_center_count = len(brute_center)
    center_query_matches = same_id_set(near_center, brute_center)

    temp near_corner = query_radius(36, 4, 6)
    near_corner_ids = ids_to_string(sort_ids(near_corner))
    near_corner_count = len(near_corner)
    temp brute_corner = brute_force_radius(36, 4, 6)
    brute_corner_count = len(brute_corner)
    corner_query_matches = same_id_set(near_corner, brute_corner)
}

Entities: {len(entities)}. Occupied buckets: {bucket_count}. Total bucketed refs: {total_bucketed}.
Query center (15,15) r=8 hashed hits: {near_center_ids} ({near_center_count}). Brute-force count: {brute_center_count}. Match: {center_query_matches}.
Query corner (36,4) r=6 hashed hits: {near_corner_ids} ({near_corner_count}). Brute-force count: {brute_corner_count}. Match: {corner_query_matches}.
-> END

=== function cell_key(cx, cy) ===
~ {
    return string(cx) + "," + string(cy)
}

=== function bucket_entity(idx) ===
~ {
    temp e = entities[idx]
    temp cx = e.x / cell_size
    temp cy = e.y / cell_size
    temp key = cell_key(cx, cy)
    if contains(buckets, key) == false {
        insert(buckets, key, #[])
    }
    push(buckets[key], idx)
}

=== function count_bucketed() ===
~ {
    temp total = 0
    for key in buckets {
        total = total + len(buckets[key])
    }
    return total
}

=== function clamp_low(v) ===
~ {
    if v < 0 {
        return 0
    }
    return v
}

=== function query_radius(qx, qy, radius) ===
~ {
    temp lo_x = clamp_low(qx - radius)
    temp hi_x = clamp_low(qx + radius)
    temp lo_y = clamp_low(qy - radius)
    temp hi_y = clamp_low(qy + radius)

    temp cx_min = lo_x / cell_size
    temp cx_max = hi_x / cell_size
    temp cy_min = lo_y / cell_size
    temp cy_max = hi_y / cell_size

    temp hits = #[]
    temp radius_sq = radius * radius

    temp cx = cx_min
    while cx <= cx_max {
        temp cy = cy_min
        while cy <= cy_max {
            temp key = cell_key(cx, cy)
            if contains(buckets, key) {
                temp bucket = buckets[key]
                temp i = 0
                while i < len(bucket) {
                    temp e = entities[bucket[i]]
                    temp ddx = e.x - qx
                    temp ddy = e.y - qy
                    if ddx * ddx + ddy * ddy <= radius_sq {
                        push(hits, e.id)
                    }
                    i = i + 1
                }
            }
            cy = cy + 1
        }
        cx = cx + 1
    }
    return hits
}

=== function brute_force_radius(qx, qy, radius) ===
~ {
    temp hits = #[]
    temp radius_sq = radius * radius
    temp i = 0
    while i < len(entities) {
        temp e = entities[i]
        temp ddx = e.x - qx
        temp ddy = e.y - qy
        if ddx * ddx + ddy * ddy <= radius_sq {
            push(hits, e.id)
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
