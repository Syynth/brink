INCLUDE dice_rolling.ink

Начало игры.
-> test_player_roll

=== test_player_roll ===
Тестирование броска игрока.
~ temp result = player_roll(3, 1, 1, 1, 0, 1, true)
Итоговый результат броска кубов: {result}
-> test_opposite_roll

=== test_opposite_roll ===
Тестирование оппозитного броска.
~ temp result = opposite_roll(2, 3, 1, 1, 2, 3, 4, 0, true)
Победил: {result: Игрок!|Оппонент =(}
->END
