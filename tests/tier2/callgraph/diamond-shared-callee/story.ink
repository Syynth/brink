VAR value = 0
~ value = diamond_top(2)
-> intro

=== function diamond_top(x)
~ temp left = diamond_left(x)
~ temp right = diamond_right(x)
~ return left + right

=== function diamond_left(x)
~ return diamond_bottom(x) + 1

=== function diamond_right(x)
~ return diamond_bottom(x) + 2

=== function diamond_bottom(x)
~ return x * 2

=== intro ===
The value came out to {value}.
* [Go left] -> branch_left
* [Go right] -> branch_right

=== branch_left ===
Taking the left branch.
-> merge

=== branch_right ===
Taking the right branch.
-> merge

=== merge ===
Both branches rejoin here.
-> END
