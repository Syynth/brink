VAR gold: int = 100
CONST RATE: float = 1.5
VAR sum = 0

~ temp name: string = "hero"
~ noop()
~ gold = heal(gold, 10)

~ {
    temp i = 0
    while true {
        i = i + 1
        if i > 10 {
            break
        }
        if i mod 2 == 0 {
            continue
        }
        sum = sum + i
    }
}

{name} has {gold} gold at rate {RATE}. Sum is {sum}.
-> END

=== function heal(ref hp: int, amount: int): int ===
~ temp total: int = hp + amount
~ return total

=== function noop(): void ===
~ return
