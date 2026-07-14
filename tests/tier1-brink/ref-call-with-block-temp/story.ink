VAR gold = 100

~ {
    temp bonus = 10
    heal(gold)
    gold = gold + bonus
}

Gold is now {gold}.
-> END

=== function heal(ref hp) ===
~ hp = hp + 5
~ return hp
