VAR hp = 10
~ temp f = #fn(heal)
~ temp healed = call(f, 5)
Healed to {healed}.
-> DONE

=== function heal(ref hp, amount) ===
~ hp = hp + amount
~ return hp
