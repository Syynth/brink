VAR world_hp = 10

~ temp f = #fn(add3)
~ temp g = bind(f, 1)
~ temp h = bind(g, 2)
~ temp chained = h(3)
Chained bind: {chained}.
~ temp rebind = call(g, 20, 30)
Rebind from g: {rebind}.
Display g: {g}.
~ temp healer = #fn(heal, world_hp)
~ temp curried = bind(healer, 5)
Display healer: {healer}.
~ temp healed = curried()
Curried ref call: {healed}.
HP after: {world_hp}.
-> END

=== function add3(a, b, c) ===
~ return a + b + c

=== function heal(ref hp, amount) ===
~ hp = hp + amount
~ return hp
