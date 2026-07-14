STRUCT Point = #{
    x: float,
    y: float,
}

VAR p = 0
VAR moved = 0

~ {
    p = Point#{x: 1.0, y: 2.0}
    moved = translate(p, 5.0, 5.0)
}

{p.x} {p.y}
{moved.x} {moved.y}
-> DONE

=== function translate(pt, dx, dy) ===
~ temp out = Point#{x: pt.x + dx, y: pt.y + dy}
~ return out
