VAR event_posi = false
VAR player_anatomy = false
VAR player_artisan = false
VAR player_chess = false
VAR player_dangerous = false
VAR player_glasswork = false
VAR player_mithridacy = false
VAR player_persuasive = false
VAR player_shadowy = false
VAR player_shapeling = false
VAR player_toxicology = false
VAR player_watchful = false
=== location_london ===

= a_boat_trip
A Slow Boat Trip # CLASS: location
You made a terrible mistake. Maybe you wronged the wrong foe, or slipped on the wet cobblestones, or simply forgot to tend to your wounds. Now you find yourself in a place of Death.
Placid black water. Barren trees. A boat filled with pale and shivering passengers. Across the calm waters is must be the place of the dead, over there on the far bank. Maybe stay clear of it?
    + {player_dangerous == 75}
        Consider the living, the dead, and the space between  # CLASS: gold
        -> main_dangerous
    + {player_chess == 3}
        Learn strategy from the Boatman # CLASS: gold
        -> adv_chess
    + {event_posi == 1 and player_artisan == 0}
        Consider the Treachery of Breaths # CLASS: gold
        -> adv_artisan

= blind_helmsman
The Blind Helmsman # CLASS: location
Zailors find relaxation here; where the drinks are cold and they can finally take off their barnacle-toed boots. You see a Black Ribbon Duelist, or, what remains of one. Awful business, isn't it? The backrooms are right for the gamblers of all ilk’s.
Rats scamper around the basement
    + {player_dangerous == 100}
        Brawl with the local drunks # CLASS: gold
        -> main_dangerous
    + Return to your Lodgings
        -> your_lodgings

= bone_market
The Bone Market # CLASS: location
For those in the know, the Department of Menace Eradication has secretive backroom. Maybe you heard about it from your University friends. Or maybe you attended a party and noticed an outrageous skeletal display.
Buying, selling, trading. For those who can gather legitimate materials, and even so for illegitimate ones, this can be a place of great wealth.
    + {player_shadowy == 125}
        Build skeletons, find buyers, profit # CLASS: gold
        -> main_shadowy
    + Return to your Lodgings
        -> your_lodgings
        
= cave_of_nadir
Cave of Nadir # CLASS: location
You come to a place that no one will know. That you will soon forget. Is this your first time? Or have you been here before?
Listen to the voices. Do not go too deep into the miasma.
This place may be of great interest to higher powers.
    + {player_watchful == 125}
        Enter the Cave of Nadir # CLASS: gold
        -> main_watchful
    + Return to your Lodgings
        -> your_lodgings

= department_of_menace_eradication
The Department of Menace Eradication # CLASS: location
I guess you could say that London falling into the Neath helped with the rat problem, as now you can talk to the bloody menaces. But, on the other side, Londoners have much more to worry about now.
    + {event_posi == 1 and player_anatomy == 0}
        Learn of a more Monsterous form. # CLASS: gold
        -> adv_anatomy
    + Return to your Lodgings
        -> your_lodgings
        
= disgraced_exile
Disgraced Exile in the Tomb Colonies # CLASS: location
Egad! You said what?! To whom?! Maybe it was a blunder, or perhaps you used your anatomical knowledge to show someone  you shouldn't have where they can stick their opinions. Either way, you probably deserve to be here.
Few truly die in London. Most make their way over here to wither or become something truly different. Home to outcasts, the disgraced, the bored, the forgotten.
Enjoy your (probably temporary) stay in Venderbight!
    + {player_persuasive == 75}
        Learn secrets from the living dead # CLASS: gold
        -> main_persuasive

= doubt_street
Doubt Street # CLASS: location
Let the presses roll.
    + {event_posi == 1 and player_mithridacy == 0}
        Learn how to manipulate fact and fiction # CLASS: gold
        -> adv_mithridacy

= empress_court
The Empress' Court # CLASS: location
You enter the Court of Her Endearing Majesty, the Queen, high power of London. This is a place of machinations, subtlety, and particularly tasty meat pies.
You see a particularly bored fellow in a side-room working on a literary project. They seem quite bored with the monotony of the work. Truly, should art be so boring?
    + {player_persuasive == 50}
        Appreciate the finer points of politics # CLASS: gold
        -> main_persuasive
    + Return to your Lodgings
        -> your_lodgings

= flit
The Flit # CLASS: location
High in on the rooftops! As close to the sky as you can get in London unless you feel like scrambling like a monkey up the Spires of the Bazaar.
Enjoy breakfast-dinner with the Topsy King: the notorious beggar-king of the Flit. Although he's far from royalty, you should probably present yourself. Oh yes: he's supposed to be incomprehensibly insane.
Whittle your time with Fisher-Kings, Noughts, Crosses, Bats, Cats, and engage in all manner of Chicanery!
    + {player_shadowy == 25}
        Engage in a Heist # CLASS: gold
        -> main_shadowy
    + Return to your Lodgings
        -> your_lodgings

= foreign_office
The Foreign Office # CLASS: location
London must protect her interests, and her interests are your interests! Engage with Agents of the Crown, and those who seek to influence the balance of power in the Neath.
Wear a smile on your face and hold a dagger behind your back. Or perhaps poison is more your forte? In this line of work, you really should pull out life-insurance.
    + {player_persuasive == 100}
        Push papers for the foreign office # CLASS: gold
        -> main_persuasive
    + {player_mithridacy == 1}
        Appreciate the nuances of diplomacy # CLASS: gold
        -> adv_mithridacy
    + Return to your Lodgings
        -> your_lodgings

= forgotten_quarter
The Forgotten Quarter # CLASS: location
They call London the Fifth City for a reason. Walk the avenues of a forgotten land, crushed under the footprint of London like workers under late-stage capitalism.
Few remain from this city. They have vanished or taken new names. Even less are still around from the cities before this one.
You see an image of a Silver Tree carved in a rock nearby.
    + {player_watchful == 25}
        Pick your way through the ruins # CLASS: gold
        -> main_watchful
    + Return to your Lodgings
        -> your_lodgings
        
= labyrinth_of_tigers
The Labyrinth of Tigers # CLASS: location
How exciting! It's like Mrs. Plenty's Carnival, but more dangerous! Seven delicious coils of spiraling delights. Are you the prey, or the predator? Whom is viewing whom in the exhibits? What enigmatic secrets does the Tiger Keeper keep?
Uzumaki. Uzumaki. Uzumaki. What lies beneath? Who knows what they awoke in the darkness... shadow and flame...
And when, oh when, will the finally open up the next coil?
    + {player_dangerous == 50}
        Enjoy the exhibits, and try not to become one yourself # CLASS: gold
        -> main_dangerous
    + Return to your Lodgings
        -> your_lodgings

= ladybones_road
Ladybones Road # CLASS: location
A charming enough place. Home to the Brass Embassy and those who engage in the other goods and exchanges. For many of those who find themselves in London, this is the first place they visit as they try and learn the secrets of the city.
Truly a soulless place.
    + {player_watchful == 0}
        Get a grasp for the city streets of London # CLASS: gold
        -> main_watchful
    + Return to your Lodgings
        -> your_lodgings

= mahogany_hall
Mahogany Hall # CLASS: location
Lights, Camera, Action! You were made for the stage! Behold the weekly variety bill, perform every day of the week! Tout tickets, provide master-classes in Etiquette. And perhaps form a relationship with some particularly well-connected Unfinished Men?
    + {player_shadowy == 50}
        Lurk in the Shadows of a Theatrical Production # CLASS: gold
        -> main_shadowy
    + {event_posi == 1 and player_glasswork == 0}
        Consider the Conflict of the Glass and the Shroud # CLASS: gold
        -> adv_glasswork
    + Return to your Lodgings
        -> your_lodgings

= mirror_marches
The Mirror Marches # CLASS: location
A Cosmogone sun hovers low in a real sky. Tangles of green foliage and the sounds of nature; little tickings and chirpings. Here and there, though, are man-made shapes and shiny surfaces; straight lines and right angles – empty frames.
You find yourself in a place of Dreams. A vast jungle of mirrors, snakes, temples. Glimpse shadows of what lies in the Is, for this is the Is-Not.
Perhaps you will find your way to the Royal Bethlehem Hotel?
    + {player_watchful == 75}
        Walk the Realm of Mania and Madness # CLASS: gold
        -> main_watchful
    + {player_glasswork == 1}
        Consider the laws that bind this place # CLASS: gold
        -> adv_glasswork
    
= mrs_plentys_carnival
Mrs. Plenty's Carnival # CLASS: location
A great place to make a first impression. Enjoy the sights, the sounds, the smells. What amusements lie under her tarped amusements?
    + {event_posi == 1 and player_shapeling == 0}
        Converse with the Rubbery Men. Appreciate their shapes # CLASS: gold
        -> adv_shapeling
    + {player_shapeling == 3}
        Descend to Fluke Street # CLASS: gold
        -> adv_shapeling
    + Return to your Lodgings
        -> your_lodgings

= moloch_street
Moloch Street # CLASS: location
Choo Choo! It's time for Trains! Trains! Trains!
Yes, yes, at one point you did work with a famous detective, but let’s be honest; isn't it more fun to go to... Board Meetings!
Poor Rubbery Entrepreneur, we just wanted you for your money.
    + {player_chess == 5}
        Manipulate the Board of the Great Hellbound Railway # CLASS: gold
        -> adv_chess
    + Return to your Lodgings
        -> your_lodgings

= new_newgate_prison
New Newgate Prison # CLASS: location
This is a place of Birth. It is strange, isn't it...to think of a prison as a place of creation? But it's true. This is where it begins, for all of us.
A prison in the roof of the Neath. Truly I'm surprised that the Starved Men have never attacked this place. Well... they are known for their malleable opinions.
    + {player_shadowy == 75}
        Meet with nefarious elements...on their home turf, naturally # CLASS: gold
        -> main_shadowy

= singing_mandrake
The Singing Mandrake # CLASS: location
Drinking, raucous and brilliant; artists, drunk and brilliant. If one needs a place for the ale to flow and Bohemian company, this is the right place.
    + {player_anatomy == 1}
        Appreciate the form of a Spider-Senate # CLASS: gold
        -> adv_anatomy
    + Return to your Lodgings
        -> your_lodgings

= rat_market
The Rat Market # CLASS: location
Although many would say that basing your opinions on the whims of the wind or Moon-phases is obscene, or ridiculous, you have learned better.
Timed right, this is a place of enormous wealth. If obsessed over too much, perhaps soon you will scry your future in your tea-leaves.
    + {player_shadowy == 100}
        Puruse the stalls of the Ratmarket # CLASS: gold
        -> main_shadowy
    + Return to your Lodgings
        -> your_lodgings

= roof
The Roof # CLASS: location
The only comparable view would be from New Newgate Prison, and let’s be honest, it is much more pleasurable to look on the Neath 'not' from behind metal bars. The city is smaller, from here, like a beating heart. Lights glow and permeate through the darkness. To the Zee, to Hell, from here, one can view a fraction of the Neath, and appreciate(or realize the cosmic horror) of how small we all are.
    + {player_shapeling == 2}
        Carefully navigate the lands of the Starved Men # CLASS: gold
        -> adv_shapeling

= shuttered_palace
The Shuttered Palace # CLASS: location
The Traitor Empress hasn't left the palace in thirty years. Her consort still arranges concerts and banquets in the darkly glittering rooms and dripping gardens. You may be invited. But go carefully. She dislikes sudden movements.
    + {player_persuasive == 25}
        Learn the high art of Fashion # CLASS: gold
        -> main_persuasive
    + {player_mithridacy == 2}
        Manipulate the Manipulators # CLASS: gold
        -> adv_mithridacy
    + {player_toxicology == 4}
        Requisition some Cantigaster Venom # CLASS: gold
        -> adv_toxicology
    + Return to your Lodgings
        -> your_lodgings

= spite
Spite # CLASS: location
Mrs. Chapman has a lovely abode here for the lost, depraved, and those with nowhere else to go. A place of shadowy and sly characters.
The Gracious Widow has a strong presence here. Best to stay on her good side, else your body will be smuggled out with the in her entirely legitimate shipping operation.
    + {player_shadowy == 0}
        Try a rather dark alleyway # CLASS: gold
        -> main_shadowy
    + Return to your Lodgings
        -> your_lodgings

= university
The University # CLASS: location
Fallen London's two colleges, Benthic and Summerset, enjoy a healthy rivalry. They play team sports with each other. They play pranks on each other. On certain days of the year, they play trumpets and French horns at each other." (Like the Oxford-Cambridge boat race. But with trumpets.)
Play games of Badminton, battle in games of the mind. Perhaps someone will be up for a game of Cricket?
The Roguish Semiotician and the Infamous Mathematician are giving you very seductive looks. Well, nothing wrong with some good old-fashioned polygamy!
    + {player_watchful == 50}
        Engage with the Summerset and Benthic Colleges # CLASS: gold
        -> main_watchful
    + {player_watchful == 150}
        Generate a formula for Railway Steel at your Laboratory # CLASS: gold
        -> main_watchful
    + {player_artisan == 1}
        Consider the Treachery of Clocks # CLASS: gold
        -> adv_artisan
    + {player_artisan == 3}
        Consider the Treachery of Distances # CLASS: gold
        -> adv_artisan 
    + {player_artisan == 4}
        Consider the Treachery of Measures # CLASS: gold
        -> adv_artisan
    + {player_artisan == 5}
        Consider the Treachery of Shapes # CLASS: gold
        -> adv_artisan
    + {player_glasswork == 2}
        Open a path to Parabola # CLASS: gold
        -> adv_glasswork
    + Return to your Lodgings
        -> your_lodgings

= veilgarden
Veilgarden # CLASS: location
A haunt of poets, prostitutes and other low types, and location of the notorious Singing Mandrake. Partake of wine, song, pleasure, and love affairs.
Quite Recently a most excellent Museum on Prelapsarian History opened, although it really was quite a theatrical production!
    + {player_persuasive == 0}
        Engage with individuals of greater importance than your own # CLASS: gold
        -> main_persuasive
    + {event_posi == 1 and player_chess == 0}
        Enter the game as a pawn of the Cheesemonger # CLASS: gold
        -> adv_chess
    + {event_posi == 1 and player_toxicology == 0}
        Visit the Museum of Prelapsarian History # CLASS: gold
        -> adv_toxicology
    + Return to your Lodgings
        -> your_lodgings

= watchmakers_hill
Watchmaker Hill # CLASS: location
A sinister fungal wilderness by the river. The Department of Menace Eradication subcontracts the adventurous to deal with the things that slither out of Bugsby's Marshes. An observatory atop the hill employs only blind men.
Mr. Iron used to run a game of Knife-and-Candle was once practiced on this very hill. Although that time has long since passed, Mr. Hearts has recently taken residence.
    + {player_dangerous == 0}
        Visit the Medusas Head # CLASS: gold
        -> main_dangerous
    + {player_shadowy == 150}
        Engage in a round of Hearts Game (Shadowy)# CLASS: gold
        -> main_shadowy
    + {player_shapeling == 1}
        Visit the Starved Embassy # CLASS: gold
        -> adv_shapeling
    + {player_toxicology == 1}
        Engage in a round of Hearts Game (Kataleptic Toxicology)# CLASS: gold
        -> adv_toxicology
    + Return to your Lodgings
        -> your_lodgings

= wolfstack_docks
Wolfstack Docks # CLASS: location
This is where the trading steamer fleets come in from the lands across the Unterzee, the sunless sea of the Bazaar. Mr Fires, who deals with trade in coal, keeps his office here among the warehouses and rowdy dockside pubs.
    + {player_dangerous == 25}
        Visit the Docks of London # CLASS: gold
        -> main_dangerous
    + Return to your Lodgings
        -> your_lodgings

=== adv_anatomy ===
-> END

=== adv_artisan ===
-> END

=== adv_chess ===
-> END

=== adv_glasswork ===
-> END

=== adv_mithridacy ===
-> END

=== adv_shapeling ===
-> END

=== adv_toxicology ===
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
