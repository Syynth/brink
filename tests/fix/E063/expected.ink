-> main

=== main ===
~ temp f = #fn(heal2)
~ temp healed = call(f, 1, 2)
Healed to {healed}.
-> DONE

=== function heal2(hp: int, amount: int): int ===
~ return hp + amount
