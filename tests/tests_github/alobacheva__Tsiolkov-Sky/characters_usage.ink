INCLUDE characters.ink

Тестирование выбора персонажей.
//-> select_characters_static((), ->selected_chars)
-> select_characters_dynamic(->selected_chars)

= selected_chars
Ура, персонажи выбраны: {player_chars}
Тест выбора персонажа для сцены:
//->select_current_char_static->selected_current
->select_current_char_dynamic(->selected_current)

= selected_current
Выбран персонаж: {char_name(current_char)}. Лазеры(0)-чувства(5) = {char_laser(current_char)}. Специализация: {char_spec(current_char)}.
Пример использования короткой функции: {ccname()}, laser/feel={cclaser()}, {ccspec()}.
-> END