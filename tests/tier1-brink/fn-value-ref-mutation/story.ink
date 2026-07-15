VAR player_hp = 10
VAR healer = 0

~ healer = #fn(heal, player_hp)
~ temp after_first = healer(5)
Healed to {after_first}.
~ temp after_second = healer(3)
Healed again to {after_second}.
HP cell is now {player_hp}.
-> END

=== function heal(ref hp, amount) ===
~ hp = hp + amount
~ return hp
