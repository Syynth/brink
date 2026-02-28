=== location_parabola ===

= adulterine_castle
The Adulterine Castle # CLASS: location
When you remove space and time, soul and mind, power and reality; this is what remains.
You are surrounded by nothing. You don't see a serene castle, towering over infinity.
You do not meet the Stewards, who walk empty halls.
You are alone; no one walks besides you.
{player_item_breath_void == 0 and player_item_masters_blood == 0 and player_item_reported_location == 0 and player_item_impossible_theorem == 0 and player_item_veils_velvet == 0 and player_item_rumourmongers_network == 0 and player_item_fluke_core == 0 and player_item_tasting_flight == 0: One must achieve great heights to gain great riches. One must stand at the peak of the mountain to fall the farthest. Only when gains great treasures will the path open.} # CLASS: italics

    + {player_item_breath_void > 0}
        Let out your Breath of the Void
        -> adv_discordance.discordance_breath_void
    + {player_item_masters_blood > 0}
        Pour out your Vial of Masters Blood
        -> adv_discordance.discordance_masters_blood
    + {player_item_reported_location > 0}
        Forget your Reported Location of a One-Time Prince of Hell
        -> adv_discordance.discordance_reported_location
    + {player_item_impossible_theorem > 0}
        Release your Impossible Theorem
        -> adv_discordance.discordance_impossible_theorem
    + {player_item_veils_velvet > 0}
        Shred your Veils Velvet Scrap
        -> adv_discordance.discordance_veils_velvet
    + {player_item_rumourmongers_network > 0}
        Deconstruct your Rumourmongers Network
        -> adv_discordance.discordance_rumourmongers_network
    + {player_item_fluke_core > 0}
        Shatter your Fluke Core
        -> adv_discordance.discordance_fluke_core
    + {player_item_tasting_flight > 0}
        Pour out your Tasting Flight of Targeted Toxins
        -> adv_discordance.discordance_tasting_flight
    + Return to your Lodgings
        -> your_lodgings

= arbor
Arbor - The City of Roses # CLASS: location
 A long road lies before you, paved with white stones. They bake beneath a dual sunlight: a queasy orange glow in the sky, and a brighter more insistent light from the South. This is a secret place, hidden between the Parabola and the Elder Continent. It both Is, and Is-Not, and is ruled by her royal eminence the Roseate Queen.
    + {player_toxicology == 2}
        Learn the Secrets of the Arborians # CLASS: gold
        -> adv_toxicology
    + Return to your Lodgings
        -> your_lodgings

= base_camp
Your Parabolan Base Camp # CLASS: location
From here, one can reach all manner of dreams and nightmares. This is your camp, your home, in the land of dreams.
    + {player_dangerous == 125}
        Hunt a Parabolan Beast 1 # CLASS: gold
        -> main_dangerous
    + {player_dangerous == 150}
        Hunt a Parabolan Beast 2 # CLASS: gold
        -> main_dangerous
    + {player_glasswork == 3}
        Build a Base-Camp in Parabola  # CLASS: gold
        -> adv_glasswork
    + {player_anatomy == 2}
        Pursue a Nightmare # CLASS: gold
        -> adv_anatomy
    + {player_toxicology == 3}
        Eat a Parabolan Orange-Apple # CLASS: gold
        -> adv_toxicology
    + {player_artisan == 2}
        Consider the Treachery of Glass # CLASS: gold
        -> adv_artisan
    + Return to your Lodgings
        -> your_lodgings

= dome_of_scales
The Dome of Scales # CLASS: location
A vast, broken dome soars over the jungle. 
Directly overhead, the Skin of the Sun sears Cosmogone light upon the land below
    + {player_glasswork == 4}
        Engage Powers at the Dome of Scales # CLASS: gold
        -> adv_glasswork
    + Return to your Lodgings
        -> your_lodgings

= moonlit_chessboard
The Moonlit Chessboard # CLASS: location
Moves and Counter-Moves
The Dreams of Spies and Politicians.
If one can influence the Surface by talking to the Dreamer, what manner of influence would one have by influencing their dreams?
    + {player_chess == 4}
        Play the Game at the Moonlit Chessboard # CLASS: gold
        -> adv_chess
    + Return to your Lodgings
        -> your_lodgings

= sea_of_spines
The Sea of Spines # CLASS: location
A hidden place, a secret place.
The water is full of chemical thoughts, of memories of ancient times.
This is where the Rubbery Man dream. Where one can remember Axile.
    + Return to your Lodgings
        -> your_lodgings

= skin_of_the_sun
The Skin of the Sun # CLASS: location
Cosmogone - The color of Remember Sunlight.
The Skin of Sun bathes all in her glory; before her glory, this land was a much darker place.
    + {player_glasswork == 6}
        Approach the Skin of the Sun # CLASS: gold
        -> adv_glasswork
    + Return to your Lodgings
        -> your_lodgings

= viric_jungle
The Viric Jungle # CLASS: location
The land of shallow sleep, the edge of Parabola. The land is saturated in Viric.
    + Return to your Lodgings
        -> your_lodgings

= war_camp
Your Parabolan War Camp # CLASS: location
War, war never changes. The only dream of war is one that ends in peace, war makes its home in the land of nightmares.
    + {player_watchful == 175}
        Engage in a Parabolan War # CLASS: gold
        -> main_watchful
    + Return to your Lodgings
        -> your_lodgings

= waswood
The Waswood # CLASS: location
Things that Were are the same as things that are Is-Not. Because now that the present has become the past, the Is has transitioned into the Is-Not.
This is a place of history, where one might remember long forgotten days.
    + {player_mithridacy == 3}
        Visit the Things that Were in the Waswood # CLASS: gold
        -> adv_mithridacy
    + Return to your Lodgings
        -> your_lodgings
