VAR partial = 0

~ temp d = #fn(double)
~ temp direct = d(21)
Direct call: {direct}.
~ temp e = call(d, 5)
Explicit call: {e}.
~ partial = #fn(add, 10)
~ temp summed = partial(7)
Partial then call: {summed}.
-> END

=== function double(x: int): int ===
~ return x + x

=== function add(a: int, b: int): int ===
~ return a + b
