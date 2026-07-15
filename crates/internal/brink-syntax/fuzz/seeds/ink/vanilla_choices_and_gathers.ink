VAR visited = false

=== knot_a ===
You stand at a crossroads. <>
* [Go north] -> north
* [Go south] -> south
- (gather) The wind picks up.
-> DONE

= north
~ visited = true
You walk north.
-> knot_a.gather

= south
You walk south.
-> knot_a.gather

=== function english_number(x) ===
{ x == 1: one|two}
