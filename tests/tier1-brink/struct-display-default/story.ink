// NS-A3 (issue #1109, docs/stdlib-spec.md §9.6): the display protocol's
// STRUCTURAL DEFAULT for structs — shape name + fields in declared order,
// mirroring the construction literal — pinned on BOTH consumers of the one
// display path (F1, ruled 2026-07-19): interpolation `{p}` and the
// `string()` conversion intrinsic must render identically. Nested structs
// recurse; structs inside collections and Options render through the same
// path. `some(…)` renders totally at both consumers (F28); a final `none`
// at the interpolation boundary now renders as *nothing* (§1.6b, Track B4)
// while `string(none)` still renders `"none"` (F28's totality, preserved
// for that one intrinsic) — the two consumers deliberately diverge for
// `None` only, proven by the `absent` / `absent via string` pair below.

STRUCT Point = #{
    x: float,
    y: float,
}

STRUCT Line = #{
    start: Point,
    end: Point,
}

VAR p = 0
VAR seg = 0
VAR pts = 0
VAR o = 0

~ {
    p = Point#{x: 1.0, y: 2.0}
    seg = Line#{start: Point#{x: 0.0, y: 0.0}, end: Point#{x: 5.0, y: 5.5}}
    pts = #[Point#{x: 1.0, y: 1.0}, Point#{x: 2.0, y: 2.0}]
    o = some(p)
}

whole: {p}
via string: {string(p)}
nested: {seg}
in array: {pts}
option: {o}
absent: {find("abc", "z")}
absent via string: {string(find("abc", "z"))}
-> DONE
