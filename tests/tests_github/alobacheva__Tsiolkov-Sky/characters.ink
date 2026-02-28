// Все доступные специализации персонажей:
// - Технический анализ
// - Навигация и пилотирование
// - Интуиция
// - Переговоры
// - Анализ неструктурированной информации
// - Техника и ремонт
LIST char_specs = tech_analyse, navigation, intuition, negotiation, unstruct, repair

// Доступные игроку персонажи.
LIST player_chars = Alex, Igor, Vadim, Ekaterina, Maria, Olga

// Персонаж из player_chars выбранный в функции select_current_char_*
VAR current_char = ()

// Буфер персонажей для построения селекторов в функциях *_dynamic
VAR _char_buffer = ()

/*
* Функции-справочники свойств персонажей.
*/

// Имя и фамилия персонажа.
=== function char_name(id) ===
{ id:
- Alex: Алексей Криптов
- Igor: Игорь Селин
- Vadim: Вадим Рейснер
- Ekaterina: Екатерина Домова
- Maria: Мария Лазарева
- Olga: Ольга Вереск
}

// Предыстория персонажа.
=== function char_desc(id) ===
{ id:
- Alex: программист
- Igor: психотерапевт
- Vadim: гонщик
- Ekaterina: гадалка
- Maria: DIY блогер
- Olga: многодетная мать
}

// Характеристика лазера-чувства (int 0-5) персонажа.
=== function char_laser(id) ===
{ id:
- Alex: ~ return 5
- Igor: ~ return 2
- Vadim: ~ return 3
- Ekaterina: ~ return 3
- Maria: ~ return 5
- Olga: ~ return 2
}

// Специализация персонажа.
=== function char_spec(id) ===
{ id:
- Alex: ~ return char_specs.tech_analyse
- Igor: ~ return char_specs.negotiation
- Vadim: ~ return char_specs.navigation
- Ekaterina: ~ return char_specs.intuition
- Maria: ~ return char_specs.repair
- Olga: ~ return char_specs.unstruct
}

/*
* Короткие функции-обертки для текущего пользователя (cc – current char):
*/

=== function ccname ===
~ return char_name(current_char)

=== function cclaser ===
~ return char_laser(current_char)

=== function ccspec ===
~ return char_spec(current_char)


/*
* Сцены, варианты которых вычисляются динамически.
*/

// Сцена выбора персонажей.
=== select_characters(->redirect) ===
->select_variant(redirect)
= select_variant(->redirect)
~ _char_buffer = LIST_ALL(player_chars) - player_chars
{ LIST_COUNT(player_chars):
- 0: Кто был первым?
- 1: Кто был следующим?
- 2: Кто был последний?
- 3: ->redirect
}
->next_variant(redirect)
= next_variant(->redirect)
~ temp char_variant = LIST_MAX(_char_buffer)
~ _char_buffer -= char_variant
{ _char_buffer:
    <-next_variant(redirect)
}
+ [{char_name(char_variant)}, {char_desc(char_variant)}]
    ~ player_chars += char_variant
- ->select_variant(redirect)

// Сцена выбора персонажа из имеющихся у игрока.
=== select_current_char(->redirect) ===
~ _char_buffer = player_chars
->next_variant(redirect)
= next_variant(->redirect)
~ temp char_variant = LIST_MAX(_char_buffer)
~ _char_buffer -= char_variant
{ _char_buffer:
    <-next_variant(redirect)
}
+ [{char_name(char_variant)}]
    ~ current_char = char_variant
- ->redirect

