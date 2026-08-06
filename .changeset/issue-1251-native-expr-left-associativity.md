---
"@brink-lang/web": patch
---

Fix #1251: `brink-syntax-native`'s `expr::expression_bp` parsed every
symmetric-precedence infix operator (`-`, `/`, `%`, `<`, `>`, `<=`, `>=`,
`==`, `!=`, `&&`, `||`) right-associative instead of left-associative —
the recursive call for an infix RHS reused the just-consumed operator's
own precedence as the child's `min_bp`, so a second operator at the
*same* precedence was pulled into the RHS instead of being left for the
parent's loop. `a - b - c` parsed as `a - (b - c)` instead of
`(a - b) - c` (`10 - 3 - 2` = 9 instead of the correct 5). Fixed with the
standard Pratt-parser recursion, `min_bp = prec + 1` (added as
`Prec::next`, saturating at the highest level). Unobservable for `+`/`*`
(mathematically associative); observable for every other operator on
this list.
