Bullet Train

// Riding the Sleeper Train in Japan on a Heavy Snow Day. // https://www.youtube.com/watch?v=KG53Pr3pFg4

// https://github.com/inkle/inky/releases/tag/0.13.0

// Dice rolls
// 6 - 100% Positive
// 5 - 50% Positive / 50% Neutral
// 3,4 - 25% Positive / 50% Neutral / 25% Negative
// 1,2 - 50% Neutral / 50% Negative
// Go to bed to reset dice.
// Only have 5 dice.
// Health worsens and the number of dice will decrease.
// Start with most modifiers 0
// one is +1
// one is -1
// safe tasks have no status reduction
// risky tasks can reduce money and or energy
// dangerous tasks can reduce status 

// Generate a character with 5 attributes (integer) values. 
// Use an action to get (5 * (Max health / Health)) dice by sleeping.
// Remove 1 of the dice which range from 1 to 6.
// Roll one dice to a task to test 1 or more attributes by use the dice roll and the bonus from the attribute to check if larger than difficulty.
// There's a failure or success chart (4 entries).
// If the test succeeds subtract health from the task. When task has 0 health, remove.
// Lose if your character health is 0.

VAR attribute_insight = 1 
VAR attribute_intuition = 0 
VAR attribute_endurance = -1 
VAR attribute_allure = 0 
VAR attribute_willpower = 0
VAR my_task_health = 5

VAR roll1 = 0
VAR roll2 = 0
VAR roll3 = 0
VAR roll4 = 0
VAR roll5 = 0

VAR used_roll1 = false
VAR used_roll2 = false
VAR used_roll3 = false
VAR used_roll4 = false
VAR used_roll5 = false

-> main

=== function roll_dices() ===
{ used_roll1 == false:
    ~ roll1 = RANDOM(1, 6)
}
{ used_roll2 == false:
    ~ roll2 = RANDOM(1, 6)
}
{ used_roll3 == false:
    ~ roll3 = RANDOM(1, 6)
}
{ used_roll4 == false:
    ~ roll4 = RANDOM(1, 6)
}
{ used_roll5 == false:
    ~ roll5 = RANDOM(1, 6)
}

=== list_dices ===
+ roll: {roll1} -> decrement_roll1(roll1, attribute_insight) ->->
+ roll: {roll2} -> decrement_roll2(roll2, attribute_intuition) ->->
+ roll: {roll3} -> decrement_roll3(roll3, attribute_endurance) ->->
+ roll: {roll4} -> decrement_roll4(roll4, attribute_allure) ->->
+ roll: {roll5} -> decrement_roll5(roll5, attribute_willpower) ->->

=== main ===
~ roll_dices()
+ Try task (Steps remaining {my_task_health}) -> my_task
+ Give up
- -> walk_forward

=== my_task ===
-> list_dices
{ my_task_health <= 0:
    The task is done. -> DONE
}
-> main

== decrement_roll1(roll, attribute) ==
{ roll > attribute:
    ~ my_task_health = my_task_health - 1
}
~ used_roll1 = true
->->

== decrement_roll2(roll, attribute) ==
{ roll > attribute:
    ~ my_task_health = my_task_health - 1
}
~ used_roll2 = true
->->

== decrement_roll3(roll, attribute) ==
{ roll > attribute:
    ~ my_task_health = my_task_health - 1
}
~ used_roll3 = true
->->

== decrement_roll4(roll, attribute) ==
{ roll > attribute:
    ~ my_task_health = my_task_health - 1
}
~ used_roll4 = true
->->

== decrement_roll5(roll, attribute) ==
{ roll > attribute:
    ~ my_task_health = my_task_health - 1
}
~ used_roll5 = true
->->

== walk_forward ==
* Walk forward -> mess_with_the_phone

== mess_with_the_phone ==
* Mess with the phone -> get_a_ticket

== get_a_ticket ==
* Get a ticket -> enter_the_gate

== enter_the_gate ==
* Enter the gate -> open_the_door

== open_the_door ==
* Open the door -> chat_with_ai

== chat_with_ai == 
* Chat with people -> enter_train

== enter_train ==
* Enter train -> go_to_room

== go_to_room ==
* Go to room -> wardrobe_exchange

== wardrobe_exchange == 
* Sleep
    ~ used_roll1 = false
    ~ used_roll2 = false
    ~ used_roll3 = false
    ~ used_roll4 = false
    ~ used_roll5 = false
    -> intro_to_game

== intro_to_game ==
* Begin intro music. Snow bullet train fades out.
    -> END
