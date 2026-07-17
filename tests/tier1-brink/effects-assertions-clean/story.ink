VAR gold = 100

Starting gold: {gold}.
~ temp remaining = spend(30)
Remaining gold: {remaining}.
~ temp healed = heal(5)
Healed amount: {healed}.
-> END

=== function spend(cost) ===
#@effects(reads: gold, writes: gold)
~ gold = gold - cost
~ return gold

=== function heal(amount) ===
#@effects(pure)
~ return amount * 2
