=== location_zee ===
// https://thefifthcity.fandom.com/wiki/Category:The_Unterzee

= aestival
Aestival # CLASS: location
Descriptive Text
    + Return to your Lodgings
        -> your_lodgings

= avid_horizon
The Avid Horizon # CLASS: location
This is a place where journeys end.
Two vast winged shapes guard a gate to the High Wilderness.
    + {player_zeefaring == 3}
    Sail North # CLASS: gold
        -> adv_zeefaring
    + Return to your Lodgings
        -> your_lodgings

= bullbone_island
Bullbone Island # CLASS: location
They say that not all the bones are from bulls, and the trees dance in the pale glim-light.
    + {player_watchful == 100}
        Investigate Bullbone Island # CLASS: gold
        -> main_watchful
    + Return to your Lodgings
        -> your_lodgings

= corsairs_forest
Gaider's Mourn # CLASS: location
The Mourn is a stalagmite vast as a crag, and its foot has no safe harbors. The corsair's citadel nestles halfway up. An intricate system of winches takes the strain... and your ship rises slowly from the zee. Her hull creaks in protest. Grizzled zailors groan and cling to stanchions."
If one would wish to pursue illicit activities on the high zee, this would be the place to start
    + {player_dangerous == 175}
        Engage in Acts of Piracy # CLASS: gold
        -> main_dangerous
    + {player_zeefaring == 2}
        Summit the Mourn # CLASS: gold
        -> adv_zeefaring
    + Return to your Lodgings
        -> your_lodgings

= dawn_machine
The Dawn Machine # CLASS: location
Descriptive Text
    + Return to your Lodgings
        -> your_lodgings

= east
East # CLASS: location
One Day, you will go East.
    + {player_zeefaring == 6}
        Sail East # CLASS: gold
        -> adv_zeefaring

= elder_continent
The Elder Continent # CLASS: location
One Day, you will go South.
    + {player_zeefaring == 1}
        Sail South # CLASS: gold
        -> adv_zeefaring
    + Return to your Lodgings
        -> your_lodgings

= empire_of_hands
The Empire of Hands # CLASS: location
Descriptive Text
    + Return to your Lodgings
        -> your_lodgings

= fathomkings_court
The Fathomkings Court # CLASS: location
This is the fate of all who sail the Zee Recklessly.
Present your offering, and seal your fate.
    + {player_zeefaring == 4}
        Descend to the Depths of this Sunless Sea # CLASS: gold
        -> adv_zeefaring

= frostfound
Frostfound # CLASS: location
Descriptive Text
    + Return to your Lodgings
        -> your_lodgings

= gaiders_mourn
Gaiders Mourn # CLASS: location
Descriptive Text
    + Return to your Lodgings
        -> your_lodgings

= gant_pole
The Gant Pole # CLASS: location
Descriptive Text
    + Return to your Lodgings
        -> your_lodgings

= grand_geode
The Grand Geode # CLASS: location
Descriptive Text
    + Return to your Lodgings
        -> your_lodgings

= godfall
Godfall # CLASS: location
A great stalactite that fell from the Roof.
Those who live here are great practitioners of the Shapeling Arts.
    + {player_shapeling == 4}
        Learn from the Clergymen of Godfall # CLASS: gold
        -> adv_shapeling
    + Return to your Lodgings
        -> your_lodgings

= high_zee
The High Zee # CLASS: location
The Zee can be dangerous and rough, or calm and still.
This is the space that lies between lands.
    + {player_artisan == 6}
        Consider the Treachery of Maps # CLASS: gold
        -> adv_artisan
    + {player_anatomy == 3}
        Hunt a Beast at Zee 1 # CLASS: gold
        -> adv_anatomy
    + {player_anatomy == 4}
        Hunt a Beast at Zee 2 # CLASS: gold
        -> adv_anatomy
    + {player_shapeling == 6}
        Behold a Fluke # CLASS: gold
        -> adv_shapeling
    + {player_toxicology == 6}
        Commune with the Drownies # CLASS: gold
        -> adv_toxicology
    + Return to your Lodgings
        -> your_lodgings

= hunters_keep
Hunters Keep # CLASS: location
A safe harbor near London.
Home of three sisters, who may be willing to share a cup of tea.
    + {event_posi == 1 and player_zeefaring == 0}
        Sail West # CLASS: gold
        -> adv_zeefaring
    + Return to your Lodgings
        -> your_lodgings

= khanate
The Khanate # CLASS: location
Those of the Fourth City have made their kingdom out here.
Become entangled in the machinations of their Khaganian Intrigues.
    + {player_persuasive == 175}
        Learn the Machinations of Zee-Related Affairs # CLASS: gold
        -> main_persuasive
    + {player_chess == 1}
        Engage in Khaganian Subterfuge # CLASS: gold
        -> adv_chess
    + {player_mithridacy == 4}
        Practice Manipulation in the Khanate # CLASS: gold
        -> adv_mithridacy
    + Return to your Lodgings
        -> your_lodgings

= kingeaters_castle
Kingeaters Castle # CLASS: location
Descriptive Text
    + Return to your Lodgings
        -> your_lodgings

= mount_palmerson
Mount Palmerson # CLASS: location
Descriptive Text
    + Return to your Lodgings
        -> your_lodgings

= mt_nomad
Mt. Nomad # CLASS: location
Descriptive Text
    + Return to your Lodgings
        -> your_lodgings

= mutton_island
Mutton Island # CLASS: location
Descriptive Text
    + Return to your Lodgings
        -> your_lodgings

= pigmote_isle
Pigmote Island # CLASS: location
Descriptive Text
    + Return to your Lodgings
        -> your_lodgings

= polythreme
Polythreme # CLASS: location
Descriptive Text
    + Return to your Lodgings
        -> your_lodgings

= port_carnelian
Port Carnelian # CLASS: location
An outpost, a London colony, an experiment.
Regardless of what they say of this place, this place is a nexus of Tigers, Khaganians, and Londoners.
    + {player_persuasive == 125}
        Act as Governor of Port Carnelian # CLASS: gold
        -> main_persuasive
    + Return to your Lodgings
        -> your_lodgings

= port_cecil
Port Cecil # CLASS: location
The Principles of Coral. A massive coral reef. Rumpled convolutions of coral fill the water, glimmering with silvery light.
Games of Chess are of great importance here.
    + {player_persuasive == 150}
        Visit the Poisoned Pawn # CLASS: gold
        -> main_persuasive
    + {player_chess == 2}
        Engage in a game of Chess # CLASS: gold
        -> adv_chess
    + Return to your Lodgings
        -> your_lodgings

= salt_lions
The Salt Lions # CLASS: location
Descriptive Text
    + Return to your Lodgings
        -> your_lodgings

= uttershroom
The Uttershroom # CLASS: location
Descriptive Text
    + Return to your Lodgings
        -> your_lodgings

= varchas
Varchas # CLASS: location
Also known as the Mirrored City. Those here worship the sun, and banish the darkness. A beacon that burns brighter than any lighthouse. This is a city of towers and mirrors.
    + {player_glasswork == 5}
        Behold a City in the Is, closely bordering the Is-Not # CLASS: gold
        -> adv_glasswork
    + Return to your Lodgings
        -> your_lodgings

= visage
Visage # CLASS: location
No face may be naked here, save the large state that gazes upon the land. Take up a new identity, partake of new rituals, and learn terrible secrets.
    + {player_shadowy == 175}
        Take up a mask, and partake of the local customs # CLASS: gold
        -> main_shadowy
    + Return to your Lodgings
        -> your_lodgings

= winking_isle
Winking Isle # CLASS: location
Descriptive Text
    + Return to your Lodgings
        -> your_lodgings

= wisdom
Wisdom # CLASS: location
Descriptive Text
    + Return to your Lodgings
        -> your_lodgings




