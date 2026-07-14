STRUCT Point = #{
    x: float,
    y: float,
}

STRUCT Line = #{
    start: Point,
    end: Point,
}

VAR p = 0
VAR pts = 0
VAR seg = 0
VAR grid = 0

~ {
    p = Point#{y: 2.0, x: 1.0}
    p.x = 9.0
    pts = #[Point#{x: 1.0, y: 1.0}, Point#{x: 2.0, y: 2.0}]
    seg = Line#{start: Point#{x: 0.0, y: 0.0}, end: Point#{x: 5.0, y: 5.0}}
    grid = #[#{"a": #[1, 2, 3], "b": #[4, 5, 6]}, #{"a": #[7, 8, 9]}]
    grid[0]["a"][2] = 30
}

{p.x} {p.y}
{pts[0].x} {pts[1].x}
{seg.start.x} {seg.end.y}
{grid[0]["a"][2]}
-> DONE
