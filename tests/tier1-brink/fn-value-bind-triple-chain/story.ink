VAR base_hp = 100

~ temp f = #fn(adjust, base_hp)
~ temp g = bind(f, 3)
~ temp h = bind(g, 4)
~ temp result = h()
Result: {result}.
HP cell is now {base_hp}.
-> END

=== function adjust(ref hp: int, delta1: int, delta2: int): int ===
~ hp = hp + delta1 + delta2
~ return hp
