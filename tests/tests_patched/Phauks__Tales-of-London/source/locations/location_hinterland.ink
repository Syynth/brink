VAR player_anatomy = false
VAR player_chess = false
VAR player_dangerous = false
VAR player_mithridacy = false
VAR player_persuasive = false
VAR player_shadowy = false
VAR player_shapeling = false
VAR player_toxicology = false
VAR player_watchful = false
VAR player_zeefaring = false
=== location_hinterlands ===

= ealing_gardens
Ealing Gardens # CLASS: location
Home to an eccentric mix of Rubbery Men, the poor, and the outcasts of London. It has seen a recent revival with the creation of the Great Hellbound Railway.
Nestled hidden within the bosom of this slum, one might find Helicon House, a sanctuary for Rubbery Men, and those who associate with them.
    + {player_shapeling == 5}
        Visit Helicon House # CLASS: gold
        -> adv_shapeling
    + Return to your Lodgings
        -> your_lodgings
= jericho_locks
Jericho Locks # CLASS: location
The place where the bargemen cool their heels between voyages, where certain trade between London and Hell flows. A crossroad of canals traverse this station; under the watchful eyes of the Guild of Gondoliers. No music is allowed, except for the viol.
You can pay members of the Guild to take you to The Fiddler's Scarlet, The Persephone, The Eversmoulder, The Octagonal tomb, or the Cedar-Woods. However, access to the Sere Palace is strictly regulated.
    + {player_zeefaring == 5}
        Explore the Locks of Jericho # CLASS: gold
        -> adv_zeefaring
    + Return to your Lodgings
        -> your_lodgings
= evenlode
The Magistry of Evenlode # CLASS: location
A Legal Institution located on the banks of the Evenlode River. An ancient stone courthouse looms over these hills. This is a place of law, ancient, and powerful. 
The Constabulary of the Evenlode make this their base of operations for policing matters in the Hinterlands.
Grease enough wheels and fill enough pockets, and you may find yourself in possession of a Special Dispensation.
There are secrets hidden in these depths, in the bowels only the foolish dare tread.
    + {player_watchful == 200}
        Practice the Application of the Law in Court # CLASS: gold
        -> main_watchful
    + {player_anatomy == 6}
        Descend into the Depths of the Magistry # CLASS: gold
        -> adv_anatomy
    + Return to your Lodgings
        -> your_lodgings
= balmoral
Balmoral # CLASS: location
A Castle that looms over its citizens. Surrounded by a thick forest. Befriend the Castellon of Balmoral, and become as a Vengeful Ruler...or a Merciful one.
Some rumors suggest that the Dumbwaiter still connects to the Surface, but only those of a criminal element would know the details.
    + {player_shadowy == 215}
        Investigate the woods outside Balmoral # CLASS: gold
        -> main_shadowy
    + {player_chess == 6}
        Create an Elaborate Network of Spies and Secrets # CLASS: gold
        -> adv_chess
    + {player_mithridacy == 5}
        Help the Castellon of Balmoral rule their kingdom # CLASS: gold
        -> adv_mithridacy
    + Return to your Lodgings
        -> your_lodgings
= station_viii
Station VIII # CLASS: location
Factory VIII, which is the fifth station of the Great Hellbound Railway, is home to Factory VIII. Producer of chemicals and emotion-inducing substances; the factories here are closely tied to the Masters, who can be seen coming and going on their Dirigibles.
Doesn't the crippling working conditions of the masses just make you want to open a restaurant?
    + {player_shadowy == 200}
        Sneak into Factory VIII # CLASS: gold
        -> main_shadowy
    + {player_toxicology == 5}
        Engage in Practical Applications of Toxicity Testings # CLASS: gold
        -> adv_toxicology
    + Return to your Lodgings
        -> your_lodgings
= burrow_infra_mump
Burrow-Infra-Mump # CLASS: location
A lone hill in a long and unbroken plain. the ruins of a Saxon church. Empty arches and broken towers.
It can be repaired, if one so wished.
There is a drumming coming from below.
    + {player_persuasive == 200}
        Found a Church # CLASS: gold
        -> main_persuasive
    + Return to your Lodgings
        -> your_lodgings
= moulin
Moulin # CLASS: location
The threshold of Hell, beyond which lies wild territory full of great and terrible things. This is a desolate place, pitted with craters, relics, and remnants of war.
Gather a crew, head off into the wastelands, and perhaps you will find something beyond beliefs.
    + {player_dangerous == 200}
        Head deep into the Moulin-Wastes # CLASS: gold
        -> main_dangerous
    + {player_anatomy == 5}
        Track Prey in the Wasteland # CLASS: gold
        -> adv_anatomy
    + {player_mithridacy == 6}
        Seek out the Wellspring # CLASS: gold
        -> adv_mithridacy
    + Return to your Lodgings
        -> your_lodgings
= hurlers
The Hurlers # CLASS: location
Nothing is native to this desolate wasteland. Candles struggle to burn: flames have been known to freeze on their wicks. Walk a little, and you'll find a lone encampment where an old goat-demon tends a dying fire. Walk any further than that, and you may not come back.
Besides two circles of standing stones, there is nothing here.
This is a place of Desolation.
    + {player_dangerous == 215}
        Recapture a runaway Stove # CLASS: gold
        -> main_dangerous
    + Return to your Lodgings
        -> your_lodgings
= marigold
Marigold Station # CLASS: location
Behold the very walls of Hell. Sit beneath the shadow of the white walls of the great city. Here, laws are made, broken, and made again.
Petition the Devils for entry into the great city, and perhaps you shall come out changed...
    + {player_persuasive == 215}
        Enter Hell # CLASS: gold
        -> main_persuasive
    + Return to your Lodgings
        -> your_lodgings
        
= tracklayers_city
The Silvered City # CLASS: location
A sprawling city, forged anew. This is a place of hope for the Tracklayers Union. This shall be a utopia, or a prison.
The formation of this city came at a great cost.
    + {player_watchful == 215}
        Behold a New Day # CLASS: gold
        -> main_watchful
    + Return to your Lodgings
        -> your_lodgings
        

=== adv_anatomy ===
-> END

=== adv_chess ===
-> END

=== adv_mithridacy ===
-> END

=== adv_shapeling ===
-> END

=== adv_toxicology ===
-> END

=== adv_zeefaring ===
-> END

=== main_dangerous ===
-> END

=== main_persuasive ===
-> END

=== main_shadowy ===
-> END

=== main_watchful ===
-> END

=== your_lodgings ===
-> END
