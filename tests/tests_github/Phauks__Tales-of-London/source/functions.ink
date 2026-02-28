=== function player_checker_inventory ===
Inventory: # CLASS: title_large

Contraband:
{player_item_box_empty: {player_item_box_empty} <> Empty Mirrorcatch Box ;}
{player_item_box_apocyan: <> Apocyan-Filled Mirrorcatch Box;}
{player_item_box_cosmogone: <> Cosmogone-Filled Mirrorcatch Box;}
{player_item_box_gant: <> Gant-Filled Mirrorcatch Box;}
{player_item_box_irrigo: <> Irrigo-Filled Mirrorcatch Box;}
{player_item_box_peligin: <> Peligin-Filled Mirrorcatch Box;}
{player_item_box_violant: <> Violant-Filled Mirrorcatch Box;}
{player_item_box_viric: <> Viric-Filled Mirrorcatch Box;}

Treasures:
{player_item_breath_void: <> Breath of the Void;}
{player_item_masters_blood: <> Vial of Masters Blood;}
{player_item_reported_location: <> Reported Location of a One-Time Prince of Hell;}
{player_item_impossible_theorem: <> Impossible Theorem;}
{player_item_veils_velvet: <> Veils-Velvet Scrap:}
{player_item_rumourmongers_network: <> Rumourmongers Network;}
{player_item_fluke_core: <> Fluke-Core;}
{player_item_tasting_flight: <> A Tasting Flight of Targeted Toxins}

Candles:
{player_item_candle_arthur: <> St. Arthur's Candle;}
{player_item_candle_beau: <> St. Beau's Candle;}
{player_item_candle_cerise: <> St. Cerise's Candle;}
{player_item_candle_destin: <> St. Destin's Candle;}
{player_item_candle_erzulie: <> St. Erzulie's Candle;}
{player_item_candle_fortigan: <> St. Fortigan's Candle;}
{player_item_candle_gawain: <> St. Gawain's Candle}

=== function player_checker_quality ===
Player Qualities: # CLASS: title_large

Reputation in London:
{event_posi == 0: <> Nobody}
{event_posi == 1: <> A Person of Some Importance}

Lodging Size:
<> {player_lodging_size}
// Probably should have a qualitative description for lodging size

Tales of London:
{pursuing_an_exceptional_tale: <> Pursuing An Exceptional Tale| <> Not Pursuing An Exceptional Tale}
// Probably should have a separate section for completed tales

Discovered:
{discovered_hinterlands: <> Discovered: The Hinterlands}
{discovered_adulterine_castle: <> Discovered: The Adulterine Castle}

Horrors:
{player_smen: <>Seeking Mr. Eatens Name: {player_smen};}
{player_item_weeping_scars: <>{player_item_weeping_scars} Weeping Scars;}
{player_item_stained_soul: <>{player_item_stained_soul} Stains on Your Soul;}
{player_item_memory_of_chains: <>{player_item_memory_of_chains} Memory of Chains;}

{debug_mode:
Quirks:
Austere: {player_quirk_austere}
Daring: {player_quirk_daring}
Foreceful: {player_quirk_foreceful} 
Heartless: {player_quirk_heartless}
Hedonist: {player_quirk_hedonist}
Magnanimous: {player_quirk_magnanimous}
Melancholy: {player_quirk_melancholy}
Ruthless: {player_quirk_ruthless}
Steadfast: {player_quirk_steadfast}
Subtle: {player_quirk_subtle}
}



=== function game_clock ===
~ action_count = action_count + 1
~ total_actions_count = total_actions_count + 1
~ player_smen_candle_lit = 0 // Reset SMEN Clock
The House of Chimes strikes a new hour. Time moves forward. # CLASS: italics
{action_count == actions_per_week:
You have exausted all your actions this week. Thus, you herald in a new week. # CLASS: italics
    ~ timer_week = timer_week + 1
    ~ action_count = 0
        {timer_week > 4:
            A month passes. {month_descriptor(timer_month)} passes into {month_descriptor(timer_month + 1)}. Time is fleeting. # CLASS: italics
            ~ timer_week = 1
            ~ timer_month = timer_month + 1
            {airs_shuffle()}
        - else:
            {airs_shuffle()}
        }
  - else:
    {airs_shuffle()}
}

=== function airs_shuffle ===
    ~ airs_of_london = RANDOM(1, 27)

=== function airs_text ===

{airs_of_london:
-   1:
        A bat zips past, not far overhead. # CLASS: italics
-   2:
        A small child meditatively pings stones off a butcher's shop-window. Eventually the butcher emerges, cleaver in hand. The child disappears with remarkable speed. # CLASS: italics
-   3:
        Shadows lie still, here where there is no sun to move them. Sometimes they shiver in candle-light. # CLASS: italics
-   4:
        Passers-by watch you with narrow eyes. What do they see? # CLASS: italics
-   5:
        On the roof-tops at day's end, urchins whistle a tune from Mahogany Hall. One improvises lyrics that would make a Master of the Bazaar blush. # CLASS: italics
-   6:
        A cat's eyes glint on a high window-ledge. "What a wretched day," it remarks. # CLASS: italics
-   7:
        A scuffle! A pool of blood! A wild-eyed girl with a knife in either hand! The cry goes up, "a Jack!" Is it a Jack? She's gone, regardless... # CLASS: italics
-   8:
        Someone speaks your name. But when you turn, there is only a mirror. # CLASS: italics
-   9:
        The light from the false-stars clings to every surface like oil. This is the kind of afternoon when Londoners run mad, shrieking "The sun! The sun!" # CLASS: italics
-   10:
        A church bell tolls. # CLASS: italics
-   11:
        All shall be well, and all manner of thing shall be well. # CLASS: italics
-   12:
        Aren't we all adrift on a Sea of Misery? # CLASS: italics
-   13:
        What would you give up for Love? # CLASS: italics
-   14:
        There is a cell in New Newgate, home to an inmate only known as the Rising Sun. # CLASS: italics
-   15:
        All stories are love stories in the end. # CLASS: italics
-   16:
        While philosphers might argue the weight of ones soul, Devils take a more analytical approach. # CLASS: italics
-   17:
        The Red-And-Gold Gala is the talk of the town. # CLASS: italics
-   18:
        A Tomb-Colonist steps outside a renknowned clothing venue, wearing the hottest new trend of Paisley. # CLASS: italics
-   19:
        Bulletin Boards are covered with posters of potential candidates for the next Mayoral election. Urchins articulates the views of each candidate. # CLASS: italics
-   20:
        Before the Fifth City; there was a Fourth; before that a Third; and a Second; and a First. But what came before? # CLASS: italics
-   21:
        Rumours of Monsters, Unfathomable Horrors, and Dragons in the Zee spill from the mouths of Zailors alongside globules of ale. # CLASS: italics
-   22:
        Out on the city's edge, zee-bats cry where black waves break on a black shore. # CLASS: italics
-   23:
    Two urchins run off, holding something and giggling. Shortly, a very angry and extremely hatless Society dame runs after them. # CLASS: italics
-   24:
    A Rubbery Man is dragged into an alley by some young-looking ruffians. Sometime later, only the ruffians egress the shadows, covered in boils and slime. # CLASS: italics
-   25:
    There is a large puddle on the ground. Within, you see a mirro image of yourself, and a darkness beyond. Is something stalking your reflection? # CLASS: italics
-   26:
    Candles burn in masses through a window to a dining room, a servant stumbles to the curtains to close them. Was that blood on her hand? # CLASS: italics
-   27:
    As you pass though a crowded street, a grinning devil bumps into you. as he apologizes and moves hurriedly away, you catch a glimpse of a bottled soul peering out of his satchel, looking regretful? # CLASS: italics
}

== function ordinal_descriptor(number) ===
{number}

<>
{number mod 100:
    - 11: th
        ~ return
    - 12: th
        ~ return
    - 13: th
        ~ return
}

{number mod 10:
    - 1: st
    - 2: nd
    - 3: rd
    - 4: th
    - 5: th
    - 6: th
    - 7: th
    - 8: th
    - 9: th
    - 0: th
}
    

=== function month_descriptor(month) ===
{month:
    - 1: January
    - 2: February
    - 3: March
    - 4: April
    - 5: May
    - 6: June
    - 7: July
    - 8: August
    - 9: September
    - 10: October
    - 11: November
    - 12: December
}

    


=== function information_display ===

‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾
{ordinal_descriptor(timer_week)}
<> Week of <> {month_descriptor(timer_month)}<>, 1899. <>

Actions Remaining: {actions_per_week - action_count}/{actions_per_week}

Dangerous: {player_dangerous}/{200 + (15 * event_gains_dangerous)} ; <>
Persuasive: {player_persuasive}/{200 + (15 * event_gains_persuasive)} ; <>
Shadowy: {player_shadowy}/{200 + (15 * event_gains_shadowy)} ; <>
Watchful: {player_watchful}/{200 + (15 * event_gains_watchful)}

{event_posi:
    {player_artisan > 0: Artisan of the Red Science: {player_artisan}/{5 + event_gains_artisan} ; <>}
    {player_glasswork > 0: Glasswork: {player_glasswork}/{5 + event_gains_glasswork} ; <>}
    {player_toxicology > 0: Kataleptic Toxicology: {player_toxicology}/{5 + event_gains_toxicology} ;}
    
    {player_mithridacy > 0: Mithridacy: {player_mithridacy}/{5 + event_gains_mithridacy} ; <>}
    {player_anatomy > 0: Monsterous Anatomy: {player_anatomy}/{5 + event_gains_anatomy} ; <>}
    {player_chess > 0: Player of Chess: {player_chess}/{5 + event_gains_chess} ;}
    
    {player_shapeling > 0: Shapeling Arts: {player_shapeling}/{5 + event_gains_shapeling} ; <>}
    {player_zeefaring > 0: Zeefaring: {player_zeefaring}/{5 + event_gains_zeefaring} ; <>}
    {player_discordance > 0: Steward of the Discordance: {player_discordance}/{5 + event_gains_discordance}}
    }

‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾

=== function stat_changer(stat_name, ref stat, delta) ===
~ stat = stat + delta
Your {stat_name} has {delta > 0: increased | decreased} to {stat}. # CLASS: italics

=== function css_class_room ===
normal
italics # CLASS: italics
bold # CLASS: bold

gold # CLASS: gold
silver # CLASS: silver
bronze # CLASS: bronze

requirement # CLASS: requirement

location # CLASS: location
event # CLASS: event
event_auto # CLASS: event_auto

irrigo # CLASS: irrigo
violant # CLASS: violant
cosmogone # CLASS: cosmogone
peligin # CLASS: peligin
apocyan # CLASS: apocyan
viric # CLASS: viric
gant # CLASS: gant

=== location_null ===
Null Location # CLASS: location
You have reached the edge of Reality.
Here there be Monsters.
-> your_lodgings

=== location_quest_exit ===
+ [Return to Your Lodgings]
# CLEAR
-> your_lodgings.lodging_dawn

=== function end_tale(ref tracker) ===
~ tracker = 777
~ pursuing_an_exceptional_tale = false
You can reset this story within the Tales Hub; if you would like to experience this Tale in new or unexpected ways! # CLASS: italics
And so one story ends. On to the next... # CLASS: italics