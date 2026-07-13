VAR gold: int = 100
CONST RATE: float = 1.5

~ temp name: string = "hero"
~ noop()
~ gold = heal(gold, 10)

{name} has {gold} gold at rate {RATE}.
-> END

=== function heal(ref hp: int, amount: int): int ===
~ temp total: int = hp + amount
~ return total

=== function noop(): void ===
~ return
