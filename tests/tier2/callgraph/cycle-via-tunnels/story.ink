VAR depth = 0

-> begin

=== function bump(d)
~ return d + 1

=== begin ===
The bouncing begins.
-> ping ->
All done bouncing.
-> END

=== ping ===
~ depth = bump(depth)
Ping at depth {depth}.
{ depth < 4:
    -> pong ->
}
->->

=== pong ===
Pong at depth {depth}.
{ depth < 4:
    -> ring ->
}
->->

=== ring ===
Ring at depth {depth}.
{ depth < 4:
    -> ping ->
}
->->
