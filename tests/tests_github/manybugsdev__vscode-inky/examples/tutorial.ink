# The Ink Language Tutorial
# An Interactive RPG Adventure

// ============================================
// GLOBAL VARIABLES - Declared at the top
// ============================================

// Player stats and attributes
VAR player_class = "Adventurer"
VAR strength = 5
VAR intelligence = 5
VAR agility = 5
VAR gold = 100
VAR health = 100
VAR max_health = 100
VAR experience = 0
VAR level = 1
VAR player_name = "Hero"
VAR walking_count = 0

// List variables (for tracking state)
LIST inventory = sword, health_potion, magic_scroll, lockpick, shield, fireball_spell
VAR items = ()

LIST quest_status = not_started, in_progress, completed
VAR dragon_quest = not_started
VAR village_quest = not_started

LIST reputation = unknown, known, respected, legendary
VAR fame = unknown

LIST enemy_state = alive, wounded, defeated
VAR goblin_chieftain = alive
VAR goblin_count = 0
VAR battle_rounds = 0

// ============================================
// STORY START
// ============================================
// The story begins by diverting to the first knot

-> character_creation

// ============================================
// SECTION 1: BASIC TEXT AND CHOICES
// ============================================

=== character_creation ===
Welcome, brave adventurer! This interactive story will teach you everything about the Ink language while you explore a fantasy world.

This tutorial covers all Ink language features with RPG-themed examples. Each section demonstrates a different aspect of Ink scripting.

You stand before the Guild Hall, ready to register as an adventurer.

"Welcome!" says the Guild Master. "What brings you here today?"

// This demonstrates basic choices
// Choices start with * (once-only) or + (sticky/reusable)
* [Seek glory and fortune]
    "Ah, an ambitious one! I like that."
    -> choose_class
* [Help people in need]
    "A noble heart! We need more like you."
    -> choose_class
* [Just looking around...]
    "Well, take your time. When you're ready, come find me."
    -> choose_class

// ============================================
// SECTION 2: KNOTS AND STITCHES
// ============================================

=== choose_class ===
// Knots are major story sections (marked with ===)
// Stitches are subsections within knots (marked with =)

= class_selection
The Guild Master pulls out a large tome.

"Now then, what is your calling?"

* [Warrior - Master of combat]
    -> warrior_path
* [Mage - Wielder of arcane power]
    -> mage_path
* [Rogue - Shadow and subtlety]
    -> rogue_path

= warrior_path
~ player_class = "Warrior"
~ strength = 8
~ intelligence = 3
~ agility = 5

"A warrior! Strong and brave."
-> stats_confirmed

= mage_path
~ player_class = "Mage"
~ strength = 3
~ intelligence = 8
~ agility = 4

"A mage! Wise and powerful."
-> stats_confirmed

= rogue_path
~ player_class = "Rogue"
~ strength = 4
~ intelligence = 5
~ agility = 8

"A rogue! Quick and clever."
-> stats_confirmed

// ============================================
// SECTION 3: VARIABLES
// ============================================
// All global variables are declared at the top of the file
// See the variable declarations above for examples of:
// - VAR for global variables
// - LIST for enumerated states
// Variables can store numbers, strings, or list values

=== stats_confirmed ===
"Your path is set!"

The Guild Master inscribes your name in the register.

<> # registration_complete

// This demonstrates conditional text based on variables
You are now a registered {player_class}.

Your stats:
- Strength: {strength}
- Intelligence: {intelligence}
- Agility: {agility}
- Gold: {gold} coins

* [Continue]
    -> training_ground

// ============================================
// SECTION 4: CONDITIONALS AND LOGIC
// ============================================

=== training_ground ===
You enter the training ground behind the Guild Hall.

// Conditional text using { condition: text }
{player_class == "Warrior": The weapon racks gleam with promise.}
{player_class == "Mage": You sense magical energy in the air.}
{player_class == "Rogue": You notice several shadows where you could practice stealth.}

An instructor approaches.

// This demonstrates conditional choices
{player_class == "Warrior":
    * [Practice with weapons]
        -> warrior_training
}
{player_class == "Mage":
    * [Study spell tome]
        -> mage_training
}
{player_class == "Rogue":
    * [Practice lockpicking]
        -> rogue_training
}
* [Skip training and start adventure]
    "Confident, are we? Very well."
    -> first_quest

=== warrior_training ===
~ strength = strength + 2
~ items += sword
~ items += shield

You train with the blade, learning proper form and technique.

Your strength increased by 2! (Now: {strength})

You acquired: Sword and Shield!

* [Finish training]
    -> first_quest

=== mage_training ===
~ intelligence = intelligence + 2
~ items += magic_scroll
~ items += fireball_spell

You study ancient tomes, learning to channel magical energy.

Your intelligence increased by 2! (Now: {intelligence})

You learned: Basic Magic and Fireball Spell!

* [Finish training]
    -> first_quest

=== rogue_training ===
~ agility = agility + 2
~ items += lockpick

You practice moving silently and picking locks.

Your agility increased by 2! (Now: {agility})

You acquired: Lockpick Set!

* [Finish training]
    -> first_quest

// ============================================
// SECTION 5: GATHERS AND WEAVE
// ============================================

=== first_quest ===
The Guild Master calls you over.

"Your first quest! The village of Millhaven needs help."

// Gather points (marked with -) collect multiple choice paths
* "What's the problem?"
    "Goblins have been raiding their supplies."
    - - "I see."
* "How much does it pay?"
    "50 gold pieces, plus whatever you find."
    - - "Reasonable."
* "I'm ready!"
    - - "Excellent!"

// All paths gather here
- The Guild Master marks your quest log.

~ village_quest = in_progress

* [Set out for Millhaven]
    -> journey_to_millhaven

// ============================================
// SECTION 6: ALTERNATIVES AND SEQUENCES
// ============================================

=== journey_to_millhaven ===
You walk along the forest road toward Millhaven.

// Sequences use {}
// & creates cycling alternatives
// ! creates once-only alternatives
// {} creates stopping alternatives

// Cycling sequence (repeats from beginning)
{&The sun shines through the trees.|Birds sing in the canopy.|A gentle breeze rustles the leaves.|The forest path winds ahead.}

{walking_count < 3:
    * [Keep walking]
        ~ walking_count++
        -> journey_to_millhaven
}

// After walking enough, continue
{walking_count >= 3:
    You arrive at a fork in the road.
    
    A weathered signpost points in three directions.
    
    * [Take the main road]
        -> main_road
    * [Follow the forest trail]
        -> forest_trail
    * [Investigate the old ruins path]
        -> ruins_path
}

// ============================================
// SECTION 7: TEMPORARY VARIABLES AND LOGIC
// ============================================

=== main_road ===
You follow the well-traveled main road.

// Temporary variables (only exist within this knot)
~ temp time_taken = 2
~ temp encounters = 0

After {time_taken} hours, you reach Millhaven safely.

{player_class == "Rogue": Your keen eyes helped you avoid any trouble.}

-> millhaven_arrival

=== forest_trail ===
You venture into the deeper forest.

~ temp encounter_roll = RANDOM(1, 10)
~ temp combat_success = false

// Demonstrating comparison operators
{encounter_roll >= 7:
    A wild boar charges at you!
    
    {player_class == "Warrior" and strength >= 8:
        ~ combat_success = true
        You expertly dodge and counter with your blade!
    }
    {player_class == "Mage" and intelligence >= 8:
        ~ combat_success = true
        You cast a spell that sends the boar running!
    }
    {player_class == "Rogue" and agility >= 8:
        ~ combat_success = true
        You nimbly leap aside and disappear into the shadows!
    }
    {not combat_success:
        You manage to escape, but take some damage.
        ~ health = health - 15
        Health: {health}/{max_health}
    }
}

You eventually reach Millhaven.

-> millhaven_arrival

=== ruins_path ===
Curiosity leads you to explore the ancient ruins.

~ temp treasure_found = RANDOM(1, 6)

{treasure_found >= 4:
    Among the rubble, you find an old chest!
    
    {items has lockpick:
        You use your lockpick to open it.
        ~ gold = gold + 50
        ~ items += health_potion
        Inside: 50 gold and a health potion!
    - else:
        Unfortunately, you cannot open the locked chest.
    }
}

You continue to Millhaven.

-> millhaven_arrival

// ============================================
// SECTION 8: ADVANCED CONDITIONALS
// ============================================

=== millhaven_arrival ===
You arrive at the village of Millhaven as {time_of_day()}.

// Multiple conditions
{village_quest == in_progress and health < max_health:
    An elderly healer notices your wounds.
    "Let me help you, adventurer."
    ~ health = max_health
    Your health is restored!
}

The village is modest but well-kept. Villagers eye you with hope.

* [Find the village elder]
    -> meet_elder
* {items has health_potion} [Use health potion first]
    -> use_health_potion
* [Explore the village]
    -> explore_village

// ============================================
// SECTION 9: FUNCTIONS
// ============================================

// Functions can return values and take parameters
=== function time_of_day()
~ temp hour = RANDOM(6, 20)
{
    - hour < 12:
        ~ return "morning"
    - hour < 17:
        ~ return "afternoon"
    - else:
        ~ return "evening"
}

=== function calculate_damage(attacker_stat)
~ temp base_damage = attacker_stat * 2
~ temp variance = RANDOM(0, 5)
~ return base_damage + variance

=== function skill_check(stat, difficulty)
~ temp roll = RANDOM(1, 20)
~ return roll + stat >= difficulty

// ============================================
// SECTION 10: TUNNELS
// ============================================

=== use_health_potion ===
// Tunnels are like temporary diversions that return
// They use -> and ->-> syntax

You drink the health potion.

~ items -= health_potion
~ health = max_health

Health restored to {health}!

* [Continue]
    ->->

=== explore_village ===
You wander through the village streets.

* [Visit the blacksmith]
    -> blacksmith_shop ->
* [Check the tavern]
    -> village_tavern ->
* [Go to the elder]
    -> meet_elder

=== blacksmith_shop ===
The blacksmith's forge glows with heat.

"Looking for equipment?" asks the smith.

* {gold >= 50} [Buy armor upgrade (50 gold)]
    ~ gold -= 50
    ~ max_health += 20
    ~ health += 20
    "Good choice! This will protect you well."
    Max Health increased!
    ->->
* {gold < 50} [Can't afford anything]
    "Come back when you have more coin."
    ->->
* [Leave]
    ->->

=== village_tavern ===
The tavern is warm and inviting.

// Alternative sequences - once-only
{!The barkeep nods at you.|A few patrons glance your way.|The tavern is lively with conversation.}

* [Buy a drink (5 gold)]
    {gold >= 5:
        ~ gold -= 5
        You enjoy a refreshing drink.
        You overhear rumors about a dragon in the mountains...
        ~ dragon_quest = not_started
    - else:
        "Sorry friend, you need 5 gold."
    }
    ->->
* [Leave]
    ->->

// ============================================
// SECTION 11: MAIN QUEST AND THREADS
// ============================================

=== meet_elder ===
You find the village elder in the town hall.

"Thank the gods you've come!" he says urgently.

"Goblins from the Darkwood have been stealing our supplies. We're running out of food!"

* "I'll take care of it."
    "Bless you, adventurer! The goblin camp is north of here."
* "Tell me more about these goblins."
    "They're led by a crafty chieftain. Be careful!"
    ** "I'm ready."

- "May fortune favor you!"

~ fame = known

* [Head to the goblin camp]
    -> goblin_camp

// ============================================
// SECTION 12: COMBAT SYSTEM WITH LISTS
// ============================================
// Lists and variables are declared at the top of the file
// See the goblin_chieftain variable for enemy state tracking

=== goblin_camp ===
You approach the goblin encampment carefully.

Crude wooden palisades surround several tents. Goblins patrol the perimeter.

* {agility >= 8} [Sneak in quietly]
    -> stealth_approach
* [Charge in boldly]
    -> direct_approach
* [Use magic to create a distraction]
    {items has fireball_spell}
    -> magic_approach

=== stealth_approach ===
You slip past the guards like a shadow.

~ temp stealth_success = skill_check(agility, 15)

{stealth_success:
    You reach the chieftain's tent undetected!
    -> chieftain_tent
- else:
    A guard spots you!
    "INTRUDER!"
    -> goblin_battle
}

=== direct_approach ===
You charge through the gates, weapon ready!

{player_class == "Warrior":
    Your martial prowess is intimidating.
    Several goblins flee at the sight of you.
}

-> goblin_battle

=== magic_approach ===
You conjure a fireball and hurl it at the far side of camp!

~ items -= fireball_spell

The explosion draws all the guards away.

You slip in during the chaos!

-> chieftain_tent

// ============================================
// SECTION 13: COMBAT MECHANICS
// ============================================

=== goblin_battle ===
~ goblin_count = 3
~ battle_rounds = 0

Three goblins attack you!

= battle_loop

{goblin_count <= 0:
    -> battle_won
}

{health <= 0:
    -> battle_lost
}

~ battle_rounds++

The goblins snarl and attack! (Round {battle_rounds})

* {player_class == "Warrior" and items has sword} [Swing your sword]
    -> warrior_attack
* {player_class == "Mage" and items has magic_scroll} [Cast spell]
    -> mage_attack
* {player_class == "Rogue" and agility >= 8} [Quick strikes]
    -> rogue_attack
* [Defend yourself]
    -> defend

= warrior_attack
~ temp damage = calculate_damage(strength)

You strike with your blade! Damage: {damage}

{damage >= 15:
    You defeat a goblin! {goblin_count - 1} remain.
    ~ goblin_count--
}

~ temp goblin_damage = RANDOM(3, 8)
~ health = health - goblin_damage

The goblins counterattack! You take {goblin_damage} damage. Health: {health}/{max_health}

* [Continue fighting]
    -> battle_loop

= mage_attack
~ temp damage = calculate_damage(intelligence)

Your spell erupts with power! Damage: {damage}

{damage >= 15:
    You defeat a goblin! {goblin_count - 1} remain.
    ~ goblin_count--
}

~ temp goblin_damage = RANDOM(3, 8)
~ health = health - goblin_damage

The goblins counterattack! You take {goblin_damage} damage. Health: {health}/{max_health}

* [Continue fighting]
    -> battle_loop

= rogue_attack
~ temp damage = calculate_damage(agility)

You strike from the shadows! Damage: {damage}

{damage >= 15:
    You defeat a goblin! {goblin_count - 1} remain.
    ~ goblin_count--
}

~ temp goblin_damage = RANDOM(2, 6)
~ health = health - goblin_damage

The goblins attack but you dodge most hits! You take {goblin_damage} damage. Health: {health}/{max_health}

* [Continue fighting]
    -> battle_loop

= defend
You take a defensive stance!

~ temp goblin_damage = RANDOM(1, 4)
~ health = health - goblin_damage

You minimize the damage! You take {goblin_damage} damage. Health: {health}/{max_health}

* [Counter-attack]
    -> battle_loop

= battle_won
You defeat the goblins!

~ experience += 30
~ gold += 20

Gained 30 XP and 20 gold!

-> chieftain_tent

= battle_lost
You fall to the goblin blades...

"Perhaps you need more training," says a voice.

The Guild Master has rescued you!

~ health = max_health / 2

"Don't give up, young {player_class}."

-> guild_hall_return

// ============================================
// SECTION 14: STORY CLIMAX
// ============================================

=== chieftain_tent ===
You enter the chieftain's tent.

The goblin chieftain sits on a throne made of stolen village furniture.

"So, the village sent a {player_class}," he sneers. "You're braver than I thought."

* "Release the stolen supplies!"
    "Or what?"
    ** [Attack]
        -> chieftain_battle
    ** [Negotiate]
        -> negotiate_with_chieftain
* "We can resolve this peacefully."
    -> negotiate_with_chieftain

=== negotiate_with_chieftain ===
{intelligence >= 8:
    You present a compelling argument about mutual benefit.
    
    The chieftain considers your words carefully.
    
    "Hmm... perhaps you speak sense. Take your supplies. We will find food elsewhere."
    
    ~ village_quest = completed
    ~ experience += 50
    ~ gold += 100
    ~ fame = respected
    
    The chieftain orders his goblins to return the supplies.
    
    Diplomatic victory! +50 XP, +100 gold!
    
    -> quest_complete
- else:
    Your negotiation falters.
    
    "Bah! Enough talk!"
    
    -> chieftain_battle
}

=== chieftain_battle ===
"You want the supplies? Come take them!"

~ goblin_chieftain = alive

= battle_start
The chieftain is a skilled fighter!

* {strength >= 10} [Overpower him with strength]
    -> strength_victory
* {agility >= 10} [Outmaneuver him with speed]
    -> agility_victory
* {intelligence >= 10} [Outsmart him with tactics]
    -> intelligence_victory
* [Give it your all!]
    -> standard_victory

= strength_victory
Your powerful strikes overwhelm the chieftain!

~ goblin_chieftain = defeated
~ experience += 75
~ gold += 150

You land a devastating blow!

The chieftain yields!

-> chieftain_defeated

= agility_victory
You're too fast for the chieftain to hit!

~ goblin_chieftain = defeated
~ experience += 75
~ gold += 150

You strike from every angle!

The chieftain yields!

-> chieftain_defeated

= intelligence_victory
You predict every move!

~ goblin_chieftain = defeated
~ experience += 75
~ gold += 150

You lure the chieftain into a trap!

The chieftain yields!

-> chieftain_defeated

= standard_victory
You fight with determination!

~ temp victory_roll = skill_check(strength + agility, 18)

{victory_roll:
    ~ goblin_chieftain = defeated
    ~ experience += 60
    ~ gold += 120
    
    After a hard fight, you emerge victorious!
    
    -> chieftain_defeated
- else:
    The chieftain lands a powerful blow!
    ~ health = health - 20
    
    {health <= 0:
        -> goblin_battle.battle_lost
    - else:
        Health: {health}/{max_health}
        
        * [Keep fighting!]
            -> battle_start
    }
}

=== chieftain_defeated ===
The goblin chieftain bows his head in defeat.

"You... are strong. The supplies are yours."

~ village_quest = completed
~ fame = respected

The goblins return the stolen goods.

-> quest_complete

// ============================================
// SECTION 15: MULTIPLE ENDINGS AND TAGS
// ============================================

=== quest_complete ===
You return to Millhaven, the stolen supplies in tow. # triumphant_return

The villagers cheer as you arrive!

The elder rushes to greet you. "You've saved us! Thank you!"

~ gold += 50

You receive your reward: 50 gold coins!

Total wealth: {gold} gold

{fame >= respected:
    "Your fame spreads across the land!"
    Word of your deeds reaches far and wide.
}

= check_level_up
{experience >= 100:
    ~ level++
    ~ experience = experience - 100
    ~ max_health += 20
    ~ health = max_health
    
    *** LEVEL UP! ***
    You are now level {level}!
    Max Health increased to {max_health}!
    
    -> check_level_up
}

* [Ask about the dragon rumors]
    {dragon_quest == not_started:
        "Ah yes, there have been sightings in the mountains."
        "But that's a quest for another day..."
        ~ dragon_quest = in_progress
    }
* [Rest and celebrate]
    -> celebration

=== celebration ===
That evening, the village holds a feast in your honor.

{player_class == "Warrior": You share tales of battle with the village guards.}
{player_class == "Mage": You demonstrate simple magic tricks for the children.}
{player_class == "Rogue": You teach the village youth some... harmless tricks.}

= feast
The feast is {&delicious|magnificent|wonderful}!

{items has health_potion:
    A grateful villager offers to teach you healing magic in exchange for your potion.
    
    * [Accept the trade]
        ~ items -= health_potion
        ~ intelligence += 1
        You learn the basics of healing magic!
        Intelligence +1!
        -> ending_choice
}

* [Enjoy the evening]
    -> ending_choice

// ============================================
// SECTION 16: ENDINGS WITH VARIATIONS
// ============================================

=== ending_choice ===
As the moon rises, you contemplate your next move.

* [Continue adventuring]
    -> continue_adventure
* [Return to the Guild Hall]
    -> guild_hall_return
* [Pursue the dragon quest]
    {dragon_quest == in_progress}
    -> dragon_tease

=== continue_adventure ===
You decide to continue your journey.

The world is vast, and many adventures await!

// Conditional ending text
{level >= 2:
    As a level {level} {player_class}, you're ready for greater challenges!
}

{gold >= 200:
    Your purse is heavy with {gold} gold coins.
}

{fame == legendary:
    Your legendary status opens doors everywhere.
- else:
    {fame == respected:
        You're respected in these lands.
    - else:
        {fame == known:
            Your name is starting to be known.
        - else:
            You're still building your reputation.
        }
    }
}

The road ahead beckons...

# the_journey_continues

-> END

=== guild_hall_return ===
You return to the Guild Hall to report your success.

The Guild Master greets you warmly. "Well done! I knew you had potential."

{village_quest == completed:
    Your quest is marked complete in the guild records.
}

"There are always more quests available, {player_class}."

{fame >= respected:
    "In fact, we have some special assignments for someone of your reputation."
}

* [View available quests]
    "Let's see..."
    
    {dragon_quest == in_progress:
        - Dragon in the mountains (Dangerous!)
    }
    - Escort merchant caravan
    - Clear out cave of bandits
    - Investigate haunted manor
    
    "Plenty of opportunities!"
    
* [Thank them and leave]
    -> end_for_now

=== dragon_tease ===
You set your sights on the mountains.

A dragon quest! This could be legendary!

But that's a story for another time...

{level >= 3:
    With your skills and experience, you might just have a chance.
}

# dragon_quest_begun

-> END

=== end_for_now ===
Your adventure continues...

# to_be_continued

-> END

// ============================================
// TUTORIAL COMPLETE!
// ============================================

// This tutorial has demonstrated:
// 1. Basic text and choices (* for once-only, + for sticky)
// 2. Knots (===) and stitches (=)
// 3. Diverts (->)
// 4. Gathers (-)
// 5. Variables (VAR for global, ~ temp for temporary)
// 6. Conditionals { condition: text }
// 7. Functions (=== function name)
// 8. Tunnels (->->)
// 9. Lists (LIST and membership)
// 10. Tags (#)
// 11. Logic (and, or, not, ==, >=, etc.)
// 12. Alternatives (&, !, {}, cycling/stopping)
// 13. Random numbers RANDOM()
// 14. Combat systems
// 15. Multiple endings

// Thank you for playing through this tutorial!
// Now you know all the core features of Ink!
// Start writing your own interactive stories!
