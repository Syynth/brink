=== your_lodgings ===
-> your_lodgings.lodging_main_menu


= lodging_dawn

{game_clock()}

-> auto_fire_events -> // Auto-fire any events before checking if end of week.

{debug_events_enabled == true and action_count != 1:
// Route through Opportunity Deck only if end of week and opportunity deck has not been disabled.
-> opportunity_deck_hub ->
}

-> your_lodgings.lodging_main_menu

= lodging_main_menu
# CLEAR
{information_display()}  # CLASS: left_align
{airs_text()}
{debug_mode == true: <> Airs of London: {airs_of_london}}
{player_smen_candle_lit == 1:
    Smoke rises from your newly lit candle. # CLASS: italics
}
Your Lodgings # CLASS: location



+   {debug_mode} Debug Menu # CLASS: navigational_menu
    -> debug_menu
    
+   Pursue Main Stats # CLASS: navigational_menu
    ++ {player_dangerous != 230}
        Pursue Your Dangerous Activities # CLASS: navigational_menu
        -> lodging_dangerous
    ++ {player_dangerous == 230}
        Pursue Your Dangerous Activities # CLASS: navigational_menu # UNCLICKABLE
        -> lodging_dangerous

    ++ {player_persuasive != 230}
        Pursue Your Persuasive Activities # CLASS: navigational_menu
        -> lodging_persuasive
    ++ {player_persuasive == 230}
        Pursue Your Persuasive Activities # CLASS: navigational_menu # UNCLICKABLE
        -> lodging_persuasive

    ++ {player_shadowy != 230}
        Pursue Your Shadowy Activities # CLASS: navigational_menu
        -> lodging_shadowy
    ++ {player_shadowy == 230}
        Pursue Your Shadowy Activities # CLASS: navigational_menu # UNCLICKABLE
        -> lodging_shadowy

    ++ {player_watchful != 230}
        Pursue Your Watchful Activities # CLASS: navigational_menu
        -> lodging_watchful
    ++ {player_watchful == 230}
        Pursue Your Watchful Activities # CLASS: navigational_menu # UNCLICKABLE
        -> lodging_watchful
    ++ [Return to Your Lodgings]
        -> your_lodgings

+   {event_posi} Pursue Your Esoteric Studies # CLASS: navigational_menu
    ++ {player_artisan != 7}
        Pursue Artisan of the Red Science Activities # CLASS: navigational_menu
        -> lodging_artisan
    ++ {player_artisan == 7}
        Pursue Artisan of the Red Science Activities # CLASS: navigational_menu # UNCLICKABLE
        -> lodging_artisan
        
    ++ {player_anatomy != 7}
        Pursue Monsterous Anatomy Activities # CLASS: navigational_menu
        -> lodging_anatomy
    ++ {player_anatomy == 7}
        Pursue Monsterous Anatomy Activities # CLASS: navigational_menu # UNCLICKABLE
        -> lodging_anatomy

    ++ {player_chess != 7}
        Pursue Player of Chess Activities # CLASS: navigational_menu
        -> lodging_chess
    ++ {player_chess == 7}
        Pursue Player of Chess Activities # CLASS: navigational_menu # UNCLICKABLE
        -> lodging_chess
        
    ++ {player_glasswork != 7}
        Pursue Glasswork Activities # CLASS: navigational_menu
        -> lodging_glasswork
    ++ {player_glasswork == 7}
        Pursue Glasswork Activities # CLASS: navigational_menu # UNCLICKABLE
        -> lodging_glasswork
        
    ++ {player_mithridacy != 7}
        Pursue Mithridacy Activities # CLASS: navigational_menu
        -> lodging_mithridacy
    ++ {player_mithridacy == 7}
        Pursue Mithridacy Activities # CLASS: navigational_menu # UNCLICKABLE
        -> lodging_mithridacy
        
    ++ {player_shapeling != 7}
        Pursue Shapeling Arts Activities # CLASS: navigational_menu
        -> lodging_shapeling
    ++ {player_shapeling == 7}
        Pursue Shapeling Arts Activities # CLASS: navigational_menu # UNCLICKABLE
        -> lodging_shapeling
        
    ++ {player_toxicology != 7}
        Pursue Kataleptic Toxicology Activities # CLASS: navigational_menu
        -> lodging_toxicology
    ++ {player_toxicology == 7}
        Pursue Kataleptic Toxicology Activities # CLASS: navigational_menu # UNCLICKABLE
        -> lodging_toxicology
        
    ++ {player_zeefaring != 7}
        Pursue Zeefaring Activities # CLASS: navigational_menu
        -> lodging_zeefaring
    ++ {player_zeefaring == 7}
        Pursue Zeefaring Activities # CLASS: navigational_menu # UNCLICKABLE
        -> lodging_zeefaring
        
    ++ [Return to Your Lodgings]
        -> your_lodgings

+   {player_smen and player_smen_candle_lit == 0}
    Attend to Your Candles # CLASS: navigational_menu # CLASS: violant
        -> special_smen
+   {player_smen and player_smen_candle_lit == 1}
    Attend to Your Candles # CLASS: navigational_menu # UNCLICKABLE
        -> special_smen

+   {discovered_adulterine_castle == true}
    Cross the Threshold, Return to the Adulterine Castle # CLASS: navigational_menu
        -> location_parabola.adulterine_castle

+   {debug_mode} Explore The Neath (Free Roam) # CLASS: navigational_menu
        -> free_roam_hub

+   {pursuing_an_exceptional_tale}
    Continue Your Exceptional Tale # CLASS: navigational_menu
        -> tales_hub
+   {not pursuing_an_exceptional_tale} 
    Begin An Exceptional Tale # CLASS: navigational_menu
        -> tales_hub

+   Attend to Other Matters # CLASS: navigational_menu
        -> lodging_other_matters


= lodging_dangerous
{player_dangerous == 100 and event_posi != true:
    Achieve 'A Person of Some Importance' by raising all main stats to 100 unlock new opportunities. # CLASS: italics
    }
{player_dangerous == 200 and discovered_hinterlands != true:
    Achieve 'Discovered: Hinterlands' by raising all main stats to 200 unlock new opportunities. # CLASS: italics
    }
{player_dangerous == 230:
    You have achieved Max Dangerous. # CLASS: italics
    }

    {player_dangerous == 0:
        + [Go to Watchmaker's Hill]
            -> location_london.watchmakers_hill
        }
    {player_dangerous == 25:
        + [Meander to Wolfstack Docks]
            -> location_london.wolfstack_docks
        }
    {player_dangerous == 50:
        + [Visit the Labyrinth of Tigers]
            -> location_london.labyrinth_of_tigers
        }
    {player_dangerous == 75:
        + [Die. (One-Way)]
            -> location_london.a_boat_trip
        }
    {player_dangerous == 100 and event_posi == true:
        + [Brawl at The Blind Helmsman]
            -> location_london.blind_helmsman
        }
    {player_dangerous == 125:
        + [Hunt Beasts In Parabola]
            -> location_parabola.base_camp
        }
    {player_dangerous == 150:
        + [Capture the Storm-bird in Parabloa]
            -> location_parabola.base_camp
        }
    {player_dangerous == 175:
        + [Zail, quite foolishly, to the Corsairs Forest]
            -> location_zee.corsairs_forest
        }
    {player_dangerous == 200 and discovered_hinterlands == true and event_gains_dangerous == 1:
        + [Prepare an Expedition to the Moulin Wastelands]
            -> location_hinterlands.moulin
        }
    {player_dangerous == 215 and event_gains_dangerous == 2:
        + [Capture a Stove at the Hurlers]
            -> location_hinterlands.hurlers
        }
    + [Return to Your Lodgings]
        -> your_lodgings

= lodging_persuasive
{player_persuasive == 100 and event_posi != true:
    Achieve 'A Person of Some Importance' by raising all main stats to 100 unlock new opportunities. # CLASS: italics
    }
{player_persuasive == 200 and discovered_hinterlands != true:
    Achieve 'Discovered: Hinterlands' by raising all main stats to 200 unlock new opportunities. # CLASS: italics
    }
{player_persuasive == 230:
    You have achieved Max Persuasive. # CLASS: italics
    }

    {player_persuasive == 0:
        + [Visit Veilgarden]
            -> location_london.veilgarden
        }
    {player_persuasive == 25:
        + [Seek entry to the Shuttered Palace]
            -> location_london.shuttered_palace
        }
    {player_persuasive == 50:
        + [Petition the Empresses Court]
            -> location_london.empress_court
        }
    {player_persuasive == 75:
        + [Go into Disgraced Exile in the Tomb Colonies (One-Way)]
            -> location_london.disgraced_exile
        }
    {player_persuasive == 100 and event_posi == true:
        + [Slink to the Foreign Office]
            -> location_london.foreign_office
        }
    {player_persuasive == 125:
        + [Zail to Port Carnelion]
            -> location_zee.port_carnelian
        }
    {player_persuasive == 150:
        + [Zail to Port Cecil]
            -> location_zee.port_cecil
        }
    {player_persuasive == 175:
        + [Zail to the Khanate]
            -> location_zee.khanate
        }
    {player_persuasive == 200  and discovered_hinterlands == true and event_gains_persuasive == 1:
        + [Found a Church in Burrow-Infra-Mump]
            -> location_hinterlands.burrow_infra_mump
        }
    {player_persuasive == 215 and event_gains_persuasive == 2:
        + [Behold the Walls of Hell]
            -> location_hinterlands.marigold
        }
    + [Return to Your Lodgings]
        -> your_lodgings

= lodging_shadowy
{player_shadowy == 100 and event_posi != true:
    Achieve 'A Person of Some Importance' by raising all main stats to 100 unlock new opportunities. # CLASS: italics
    }
{player_shadowy == 200 and discovered_hinterlands != true:
    Achieve 'Discovered: Hinterlands' by raising all main stats to 200 unlock new opportunities. # CLASS: italics
    }
{player_shadowy == 230:
    You have achieved Max Shadowy. # CLASS: italics
    }

    {player_shadowy == 0:
        + [Go to Spite]
            -> location_london.spite
        }
    {player_shadowy == 25:
        + [Rise to Flit]
            -> location_london.flit
        }
    {player_shadowy == 50:
        + [Purchase tickets to Mahogany Hall]
            -> location_london.mahogany_hall
        }
    {player_shadowy == 75:
        + [Return (willingly) to New Newgate Prison (One-Way)]
            -> location_london.new_newgate_prison
        }
    {player_shadowy == 100 and event_posi == true:
        + [Peruse ratty wares at the Rat Market]
            -> location_london.rat_market
        }
    {player_shadowy == 125:
        + [Build 'legitimate' pieces of art at the Bone Market]
            -> location_london.bone_market
        }
    {player_shadowy == 150:
        + [Join a round of Knife-and-Candle, wait, scratch that, Hearts Game, at Watchmakers Hill]
            -> location_london.watchmakers_hill
        }
    {player_shadowy == 175:
        + [Visit Visage]
            -> location_zee.visage
        }
    {player_shadowy == 200  and discovered_hinterlands == true and event_gains_shadowy == 1:
        + [Sneak into the factories of Station VIII]
            -> location_hinterlands.station_viii
        }
    {player_shadowy == 215 and event_gains_shadowy == 2:
        + [Visit a bandit camp outside Balmoral]
            -> location_hinterlands.balmoral
        }
    + [Return to Your Lodgings]
        -> your_lodgings

= lodging_watchful
{player_watchful == 100 and event_posi != true:
    Achieve 'A Person of Some Importance' by raising all main stats to 100 unlock new opportunities. # CLASS: italics
    }
{player_watchful == 200 and discovered_hinterlands != true:
    Achieve 'Discovered: Hinterlands' by raising all main stats to 200 unlock new opportunities. # CLASS: italics
    }
{player_watchful == 230:
    You have achieved Max Watchful. # CLASS: italics
    }

    {player_watchful == 0:
        + [Head to Ladybones Road]
            -> location_london.ladybones_road
        }
    {player_watchful == 25:
        + [Explore the Forgotten Quarter]
            -> location_london.forgotten_quarter
        }
    {player_watchful == 50:
        + [Pursue Scholarly Wisdom to the University]
            -> location_london.university
        }
    {player_watchful == 75:
        + [Enter the Mirror Marches (One-Way)]
            -> location_london.mirror_marches
        }
    {player_watchful == 100 and event_posi == true:
        + [Set to Zee in search of Research Opportunities]
            -> location_zee.bullbone_island
        }
    {player_watchful == 125:
        + [Discover the location of the Cave of Nadir]
            -> location_london.cave_of_nadir
        }
    {player_watchful == 150:
        + [Generate a formula for Railway Steel at your Laboratory]
            -> location_london.university
        }
    {player_watchful == 175:
        + [Engage in Parabolan Warfare]
            -> location_parabola.war_camp
        }
    {player_watchful == 200 and discovered_hinterlands == true and event_gains_watchful == 1:
        + [Engage in court proceedings in the Magistry of Evenlode]
            -> location_hinterlands.evenlode
        }
    {player_watchful == 215 and event_gains_watchful == 2:
        + [Witness the Birth of a New Power in the Hinterlands]
            -> location_hinterlands.tracklayers_city
        }
    + [Return to Your Lodgings]
        -> your_lodgings

= lodging_artisan
{player_artisan == 5 and discovered_hinterlands != true:
    Achieve 'Discovered: Hinterlands' by raising all main stats to 200 unlock new opportunities. # CLASS: italics
    }
{player_artisan == 7:
    You have achieved Max Artisan of the Red Science. # CLASS: italics
    }

    {player_artisan == 0:
        + [Consider the Treachery of Breath]
            -> location_london.a_boat_trip
        }
    {player_artisan == 1:
        + [Consider the Treachery of Clocks]
            -> location_london.university
        }
    {player_artisan == 2:
        + [Consider the Treachery of Glass]
            -> location_parabola.base_camp
        }
    {player_artisan == 3:
        + [Consider the Treachery of Distances]
            -> location_london.university
        }
    {player_artisan == 4:
        + [Consider the Treachery of Measures]
            -> location_london.university
        }
    {player_artisan == 5 and event_gains_artisan == 1:
        + [Consider the Treachery of Shapes]
            -> location_london.university
        }
    {player_artisan == 6 and event_gains_artisan == 2:
        + [Consider the Treachery of Maps]
            -> location_zee.high_zee
        }
    + Return to Your Lodgings
        -> your_lodgings

= lodging_anatomy
{player_anatomy == 5 and discovered_hinterlands != true:
    Achieve 'Discovered: Hinterlands' by raising all main stats to 200 unlock new opportunities. # CLASS: italics
    }
{player_anatomy == 7:
    You have achieved Max Monsterous Anatomy. # CLASS: italics
    }

    {player_anatomy == 0:
        + [Hunt Prey at the Department of Menace Eradication]
            -> location_london.department_of_menace_eradication
        }
    {player_anatomy == 1:
        + [Ascertain the form of a Spider Senate]
            -> location_london.singing_mandrake
        }
    {player_anatomy == 2:
        + [Pursue a Nightmare]
            -> location_parabola.base_camp
        }
    {player_anatomy == 3:
        + [Hunt a Beast at Zee 1]
            -> location_zee.high_zee
        }
    {player_anatomy == 4:
        + [Hunt a Beast at Zee 2]
            -> location_zee.high_zee
        }
    {player_anatomy == 5 and event_gains_anatomy == 1:
        + [Track Prey in the Moulin Wastes]
            -> location_hinterlands.moulin
        }
    {player_anatomy == 6 and event_gains_anatomy == 2:
        + [Meet the Light-In-Exile Beneath Evenlode]
            -> location_hinterlands.evenlode
        }
    + Return to Your Lodgings
        -> your_lodgings

= lodging_chess
{player_chess == 5 and discovered_hinterlands != true:
    Achieve 'Discovered: Hinterlands' by raising all main stats to 200 unlock new opportunities. # CLASS: italics
    }
{player_chess == 7:
    You have achieved Max Player of Chess. # CLASS: italics
    }

    {player_chess == 0:
        + [Engage in the Great Game with the Cheesemonger]
            -> location_london.veilgarden
        }
    {player_chess == 1:
        + [Engage in Khaganian Subterfuge]
            -> location_zee.khanate
        }
    {player_chess == 2:
        + [Appreciate a the movement of pieces in Port Cecil]
            -> location_zee.port_cecil
        }
    {player_chess == 3:
        + [Seek Advice from an Experienced Player]
            -> location_london.a_boat_trip
        }
    {player_chess == 4:
        + [Play the Game at the Moonlit Chessboard]
            -> location_parabola.moonlit_chessboard
        }
    {player_chess == 5 and event_gains_chess == 1:
        + [Play Interests Against One-Another to Further Your Railway]
            -> location_london.moloch_street
        }
    {player_chess == 6 and event_gains_chess == 2:
        + [Cement Your Vast Network of Spies & Alternative Identities]
            -> location_hinterlands.balmoral
        }
    + Return to Your Lodgings
        -> your_lodgings
= lodging_glasswork
{player_glasswork == 5 and discovered_hinterlands != true:
    Achieve 'Discovered: Hinterlands' by raising all main stats to 200 unlock new opportunities. # CLASS: italics
    }
{player_glasswork == 7:
    You have achieved Max Glasswork. # CLASS: italics
    }

    {player_glasswork == 0:
        + [Perceive the Mysticism of Mahogany Hall]
            -> location_london.mahogany_hall
        }
    {player_glasswork == 1:
        + [Consider the Mirror-Marshes]
            -> location_london.mirror_marches
        }
    {player_glasswork == 2:
        + [Forge a Path to Parabola]
            -> location_london.university
        }
    {player_glasswork == 3:
        + [Build a Base-Camp in Parabola]
            -> location_parabola.base_camp
        }
    {player_glasswork == 4:
        + [Engage Powers at the Dome of Scales]
            -> location_parabola.dome_of_scales
        }
    {player_glasswork == 5 and event_gains_glasswork == 1:
        + [Cross the Zee to learn more in Varchas]
            -> location_zee.varchas
        }
    {player_glasswork == 6 and event_gains_glasswork == 2:
        + [Approach the Skin of the Sun]
            -> location_parabola.skin_of_the_sun
        }
    + Return to Your Lodgings
        -> your_lodgings

= lodging_mithridacy
{player_mithridacy == 5 and discovered_hinterlands != true:
    Achieve 'Discovered: Hinterlands' by raising all main stats to 200 unlock new opportunities. # CLASS: italics
    }
{player_mithridacy == 7:
    You have achieved Max Mithridacy. # CLASS: italics
    }

    {player_mithridacy == 0:
        + [Learn to Speak Lies in Truth in Doubt Street]
            -> location_london.doubt_street
        }
    {player_mithridacy == 1:
        + [Aid in the Campaign of the North-Bound Parlamentarian]
            -> location_london.foreign_office
        }
    {player_mithridacy == 2:
        + [Speak No Falsehoods and Spread No Lies in the Shuttered Palace]
            -> location_london.shuttered_palace
        }
    {player_mithridacy == 3:
        + [Visit the Things that Were in the Waswood]
            -> location_parabola.waswood
        }
    {player_mithridacy == 4:
        + [Practice Manipulation in the Khanate]
            -> location_zee.khanate
        }
    {player_mithridacy == 5 and event_gains_mithridacy == 1:
        + [Help the Castelon Govern Balmoral]
            -> location_hinterlands.balmoral
        }
    {player_mithridacy == 6 and event_gains_mithridacy == 2:
        + [Find a source of Mis-Truth and Half-Lies in the Moulin Wastelands]
            -> location_hinterlands.moulin
        }
    + Return to Your Lodgings
        -> your_lodgings

= lodging_shapeling
{player_shapeling == 5 and discovered_hinterlands != true:
    Achieve 'Discovered: Hinterlands' by raising all main stats to 200 unlock new opportunities. # CLASS: italics
    }
{player_shapeling == 7:
    You have achieved Max Shapeling Arts. # CLASS: italics
    }

    {player_shapeling == 0:
        + [Study the Mechanisms of Change Present in Rubbery Men]
            -> location_london.mrs_plentys_carnival
        }
    {player_shapeling == 1:
        + [Attend a Starved Embassy]
            -> location_london.watchmakers_hill
        }
    {player_shapeling == 2:
        + [Visit the Roof in search of the secrets of Malleability]
            -> location_london.roof
        }
    {player_shapeling == 3:
        + [Decend to Flute Street]
            -> location_london.mrs_plentys_carnival
        }
    {player_shapeling == 4:
        + [Visit Godfall, and seek aid of the Abbot-Commander]
            -> location_zee.godfall
        }
    {player_shapeling == 5 and event_gains_shapeling == 1:
        + [Practice your Shapeling Arts in Helicon House]
            -> location_hinterlands.ealing_gardens
        }
    {player_shapeling == 6 and event_gains_shapeling == 2:
        + [Behold that which came from the Stars]
            -> location_zee.high_zee
        }
    + Return to Your Lodgings
        -> your_lodgings

= lodging_toxicology
{player_toxicology == 5 and discovered_hinterlands != true:
    Achieve 'Discovered: Hinterlands' by raising all main stats to 200 unlock new opportunities. # CLASS: italics
    }
{player_toxicology == 7:
    You have achieved Max Kataleptic Toxicology. # CLASS: italics
    }

    {player_toxicology == 0:
        + [Visit the Museum of Prelapsarian History]
            -> location_london.veilgarden
        }
    {player_toxicology == 1:
        + [Engage in Hearts Game]
            -> location_london.watchmakers_hill
        }
    {player_toxicology == 2:
        + [Visit Arbor - Land of the Roses]
            -> location_parabola.arbor
        }
    {player_toxicology == 3:
        +  [Partake of Parabolan Orange-Apples]
            -> location_parabola.base_camp
        }
    {player_toxicology == 4:
        + [Carefully study Cantigaster Venom]
            -> location_london.shuttered_palace
        }
    {player_toxicology == 5 and event_gains_toxicology == 1:
        + [Cook for your Patrons at Station VIII]
            -> location_hinterlands.station_viii
        }
    {player_toxicology == 6 and event_gains_toxicology == 2:
        + [Commune with the Drownies]
            -> location_zee.high_zee
        }
    + Return to Your Lodgings
        -> your_lodgings

= lodging_zeefaring
{player_zeefaring == 5 and discovered_hinterlands != true:
    Achieve 'Discovered: Hinterlands' by raising all main stats to 200 unlock new opportunities. # CLASS: italics
    }
{player_zeefaring == 7:
    You have achieved Max Zeefaring. # CLASS: italics
    }

    {player_zeefaring == 0:
        + [Sail West]
            -> location_zee.hunters_keep
        }
    {player_zeefaring == 1:
        + [Sail South]
            -> location_zee.elder_continent
        }
    {player_zeefaring == 2:
        + [Visit the Heart of the Zee, the Corsair's Forest]
            -> location_zee.corsairs_forest
        }
    {player_zeefaring == 3:
        + [Sail North]
            -> location_zee.avid_horizon
        }
    {player_zeefaring == 4:
        + [Behold the Fathomking's Court]
            -> location_zee.fathomkings_court
        }
    {player_zeefaring == 5 and event_gains_zeefaring == 1:
        + [Sail the Jericho Locks]
            -> location_hinterlands.jericho_locks
        }
    {player_zeefaring == 6 and event_gains_zeefaring == 2:
        + [Sail East]
            -> location_zee.east
        }
    + Return to Your Lodgings
        -> your_lodgings
        
= lodging_other_matters
    + [Return to Irem]
        -> irem.returning
    + [Check Inventory]
        {player_checker_inventory()}
        -> your_lodgings.lodging_other_matters
    + [Check Your Qualities]
        {player_checker_quality()}
        -> your_lodgings.lodging_other_matters
    + [Return to Your Lodgings]
        -> your_lodgings




= debug_menu
    + Quick Changes
    ++ Toggle POSI & All Main Stats 100
        You're a cheater!
            ~ event_posi = true
            ~ player_dangerous = 100
            ~ player_persuasive = 100
            ~ player_shadowy = 100
            ~ player_watchful = 100
            {information_display()}
            -> your_lodgings.debug_menu
    ++ Toggle POSI, Hinterlands & All Main Stats 230
        You're an even bigger cheater!
            ~ event_posi = true
            ~ discovered_hinterlands = true
            ~ player_dangerous = 230
            ~ player_persuasive = 230
            ~ player_shadowy = 230
            ~ player_watchful = 230
            {information_display()}
            -> your_lodgings.debug_menu
    ++ All locations and SMEN Enabled
        Do you even want to play?
            ~ event_posi = true
            ~ discovered_hinterlands = true
            ~ discovered_adulterine_castle = true
            ~ player_smen = 1
            {information_display()}
            -> your_lodgings.debug_menu
    ++ All Advanced Stats to 5
        Tbf this would take a bit of time to get to.
            ~ player_anatomy = 5
            ~ player_artisan = 5
            ~ player_chess = 5
            ~ player_glasswork = 5
            ~ player_mithridacy = 5
            ~ player_shapeling = 5
            ~ player_toxicology = 5
            ~ player_zeefaring = 5
            ~ player_discordance = 5
            {information_display()}
            -> your_lodgings.debug_menu
    ++ Cancel
        -> your_lodgings.debug_menu
    + Open CSS Class Room
        {css_class_room()}
        -> your_lodgings.debug_menu
    + Change Stat Value
        -> precise_stat_changer ->
        -> your_lodgings.debug_menu
    + Force HUD
        {information_display()}
        -> your_lodgings.debug_menu
    + Open Auto-Fire Event Operator
        -> auto_fire_events ->
        All Auto-Fire Events have been checked for launch parameters. # CLASS: italics
        -> your_lodgings.debug_menu
    + Open Opportunity Deck
        {player_lodging_size == 0:
            ~ player_lodging_size = 1
            Player Lodgings Size Was 0, Increased to 1 to prevent potential errors (occurs when opp deck is activated before auto-fire events activates that gives 1st Lodging Upgrade) # CLASS: italics
        }
        -> opportunity_deck_hub ->
        -> your_lodgings.debug_menu
    + Shuffle Airs
        Current Airs: {airs_of_london}
        Current Airs Text: {airs_text()}
        {airs_shuffle()}
        New Airs: {airs_of_london}    
        New Airs Text: {airs_text()}
        -> your_lodgings.debug_menu
    + Credits
        ++ Phauks' Fallen London Profile # LINKOPEN: https:\/\/www.fallenlondon.com/profile/Phauks
        Don't be a stranger!
            -> your_lodgings.debug_menu 
        ++ Go back
            -> your_lodgings.debug_menu
    + Return to Your Lodgings
        -> your_lodgings

= precise_stat_changer
~ temp stat = 0
~ temp new_value = 0

- Stat to Change
    + Main
        ++ Dangerous
            ~ stat = "Dangerous"
        ++ Persuasive
            ~ stat = "Persuasive"
        ++ Shadowy
            ~ stat = "Shadowy"
        ++ Watchful
            ~ stat = "Watchful"
    + Advanced
        ++ Artisan of the Red Science
            ~ stat = "Artisan of the Red Science"
        ++ Monsterous Anatomy
            ~ stat = "Monsterous Anatomy"
        ++ Player of Chess
            ~ stat = "Player of Chess"
        ++ Glasswork
            ~ stat = "Glasswork"
        ++ Mithridacy
            ~ stat = "Mithridacy"
        ++ Shapeling Arts
            ~ stat = "Shapeling Arts"
        ++ Kataleptic Toxicology
            ~ stat = "Kataleptic Toxicology"
        ++ Zeefaring
            ~ stat = "Zeefaring"
        ++ Steward of the Discordance
            ~ stat = "Steward of the Discordance"
    + Other
        ++ Lodging Size
            ~ stat = "Lodging Size"
    + Cancel
        -> your_lodgings.debug_menu

- (stat_value) New Value of {stat}.
Note: You must pick valid New Value, or who knows what will happen. # CLASS: italics
    + 0 to 230 (Main)
        ++ 0
            ~ new_value = 0
        ++ 25
            ~ new_value = 25
        ++ 50
            ~ new_value = 50
        ++ 75
            ~ new_value = 75
        ++ 100
            ~ new_value = 100
        ++ 125
            ~ new_value = 125
        ++ 150
            ~ new_value = 150
        ++ 175
            ~ new_value = 175
        ++ 200
            ~ new_value = 200
        ++ 215
            ~ new_value = 215
        ++ 230
            ~ new_value = 230
        ++ Go Back...
            -> stat_value
    + 0 to 7 (Advanced)
        ++ 0
            ~ new_value = 0
        ++ 1
            ~ new_value = 1
        ++ 2
            ~ new_value = 2
        ++ 3
            ~ new_value = 3
        ++ 4
            ~ new_value = 4
        ++ 5
            ~ new_value = 5
        ++ 6
            ~ new_value = 6
        ++ 7
            ~ new_value = 7
        ++ Go Back...
            -> stat_value
    + True / False
        ++ True
            ~ new_value = true
        ++ False
            ~ new_value = false
    + Cancel
        ->->
- Confirm Stat Change
    Your {stat} will change to {new_value}. # CLASS: cosmogone
    + Confirm
        {stat:
        -   "Dangerous":
            ~ player_dangerous = new_value
        -   "Persuasive":
            ~ player_persuasive = new_value
        -   "Shadowy":
            ~ player_shadowy = new_value
        -   "Watchful":
            ~ player_watchful = new_value
        
        -   "Artisan of the Red Science":    
            ~ player_artisan = new_value    
        -   "Monsterous Anatomy":
            ~ player_anatomy = new_value    
        -   "Player of Chess":
            ~ player_chess = new_value
        -   "Glasswork":
            ~ player_glasswork = new_value
        -   "Mithridacy":
            ~ player_mithridacy = new_value
        -   "Shapeling Arts":
            ~ player_shapeling = new_value
        -   "Kataleptic Toxicology":
            ~ player_toxicology = new_value
        -   "Zeefaring":
            ~ player_zeefaring = new_value
        -   "Steward of the Discordance":
            ~ player_discordance = new_value
            
        -   "Lodging Size":
            ~ player_lodging_size = new_value
        }
        {information_display()}
        ->->
    + Cancel
        ->->