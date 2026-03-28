VAR x = true

-> test

=== test ===
- "Pick one."
 * { x }      [A] -> target_b
 * { not x }  [B]
        - - (target_b) "Reached B."
- "Done."
-> DONE
