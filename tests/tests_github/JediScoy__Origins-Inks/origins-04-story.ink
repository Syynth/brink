
#theme: dark
#title: JOURNEY to the CORE
#author: Master Scoy



\ ***************************************
SEASON 3: {Season04}
\ ***************************************
- EPISODE 1. {04EpisodeOne}
- You meet an uncle and his nephew at The Blue Mug with a cryptograph. You help them solve the cryptograph. They and your party both need to go to {DwarfCity} and meet the forge master.
+ [next]
- EPISODE 2. {04EpisodeTwo}
  You fight goblins just before entering {DwarfCity}. The gate guards and later the forge master give you curious looks when seeing {DwarfSmith}'s seal, but no further explanation is given. {DwarfSmith}'s seal does convince {ForgeMaster} to help you. He gives you access to special tunnel that takes you to {volcano1}.
+ [next]
- EPISODE 3. {04EpisodeThree}
  Journey deeper into the earth
+ [next]
- EPISODE 4. {04EpisodeFour}
  Discover and Trick OR Battle an Earth Elemental
+ [next]
- EPISODE 5. {04EpisodeFive}
  trapped deep in the earth but ride a spring of water upwards

  VAR Season04Hook = " \"Well ... \" The heavy wooden door of The Blue Mug bangs open. An elderly man dashes in and over to a young man seated a couple tables over. The old man is wearing a dusty old tweed jacket and round glasses. He appears to be quite excite."

=== 04Prologue ===
PROLOGUE
# IMAGE: images/blue-mug.jpg

- {Maid}: "Welcome weary adventurers!"
- Your party is greeted with a {~ warm| big| kind} smile from {Maid} at The Blue Mug inn.
    + You: "We grow {~bored|tired} of town life. What news do you have?"
- (season_hook) // SEASON HOOK
- {Maid}:  "Well..."
  + [next]
- The heavy wooden door of The Blue Mug bangs open. An elderly man dashes in and over to a young man seated a couple tables over. The old man is wearing a dusty old tweed jacket and round glasses. He appears to be quite excite.
    + [next]
- You:
    + "Who are those gentleman {Maid}?"
- {Maid}: "Ah, that would be {ProfessorL} and his young nephew, {Axel}. They've been in the last couple nights arguing, but they haven't caused any trouble with the other customers."
    + You start listening to them again.
- {ProfessorL}: "It is paramount (important) that we solve the cryptograph (code) {Axel}!"
- {Axel}: "That letter cannot be correct! Its speaks of impossible things. Why waste time on this puzzle?"
    + [next]
- {ProfessorL}: "Why? Why?! (getting louder and louder). Nothing is impossible my boy. How can you call yourself a learned man with such a mindset! Accept nothing - not even the "impossible." Whats impossible today maybe in fact be possible tomorrow or given new information turns out to be true!"
    + [next]
- At that moment another group comes in accompanied by a gust of wind. The paper {ProfessorL} was holding and waving is blown out of his hand toward your table.
    + (you_pick_up_letter) You lean down and pick it up.
    -- You take a quick glance before the old man comes over.
    -- You notice a collection of runes. There are also sketches of tunnels.
    -- \(Ask the Game Master for a copy of the paper)
    + You leave the paper alone.
-
    + {you_pick_up_letter } \ {ProfessorL} snatches it away from you.
    -- {ProfessorL}: gruffly "Beg your pardon sir; that belongs to me!"
+ { not you_pick_up_letter } \ {ProfessorL}: "Ah, my letter!"
    -- {ProfessorL} quickly comes over and snatches the paper off the floor
- You: "I mean no intrusion, but I couldn't help but over hear. My friends and I happen to be quite good at solving puzzles."
    + "Would you like a hand?"
- {ProfessorL}: "No thank you."
- He nods his head and turns back toward the young man.
- {Axel}: with a twinkle in his eyes "If you are as serious as you say uncle, we may need assistance."
    + [next]
- {ProfessorL}: "Absolutely not."
- {Axel}: "Uncle, you are the most brilliant man I know, but even the most intelligent cannot know all things and may in fact need help from time to time."
    + [next]

- (episode_end)
TODO: Change "04" to specific Season
OBJECTIVES for {Season04}
// Learning Objectives
\* Answer the Enduring Question: {Q04a} {Q04b}

// Phenomena
\* Explain the phenomena of {Ph04}.

// Game Objectives
\* Collect water essence from the water elementals.

    +[Ω]
        ->04StartMenu
    +[NEXT EPISODE]
        ->04Ep1
-
->DONE

=== 04Ep1 ===
// Room 1: Guardian OR Room 2: Puzzle / Role-play Challenge
{04EpisodeOne}
- {Axel}: turns to your party "Hello, I am {Axel} and this my uncle, {ProfessorL}. He is a savant (genius) and professor of geology. Please excuse his lack of social graces."
- {ProfessorL} humphs
- You:
    + "No worries!"
    + "{Axel}, you really should put him in check more."
- {TheCleric}: "Any who, perhaps we can help with that cryptograph now?"
- {ProfessorL}: {~reluctantly|grumpily} agrees and places it in the center of the table.
(See the Game Master for a copy of the cryptograph. Once is its solved, you may proceed to the next episode.)
    + [next]
- (end_episode)
    +[Ω]
        ->04StartMenu
    +[NEXT EPISODE]
        ->04Ep2
->DONE

=== 04Ep2 ===
// Room 2: Puzzle / Role-play Challenge OR Room 1: Guardian
// The cryptograph mentions an entrance into the center of planet.
// The entrance to the center is in {DwarfCity} in the hall of the forge master.
{04EpisodeTwo}

- You: "Interesting. We can go with you to {DwarfCity}."
- {ProfessorL}: "Why would you do that?"
    + You do not share your plans to visit {DwarfCity}.
    -- You: "It just seems like you might need some help"
    + You share your plans to visit {DwarfCity}.
    -- You: "We were actually going there ourselves.
    ++ \ {ProfessorL}: "Ah. May I ask your business there?"
    -- You: "We are going to see forge master {ForgeMaster}."
    ++ \ {ProfessorL}: "Interesting."
- You make your way towards {DwarfCity} in the {mountains1}. As you enter the mountainous region, the travel gets progressively more difficult. You are now within {~ 10 km | 20 km | 30 km} of the main gates.
    + [next]
- You enter into a mountain valley a little hesitantly. Surely it is safe being so close to such a formidable city like {DwarfCity}.
    + [next]

- (combat1)
- {d6()+3} GOBLINS starting shooting at you from the brush on both sides. A {~ hobgoblin|bugbear} also steps out in front of you.
# IMAGE: images/monster-goblin.jpeg
    // https://www.dndbeyond.com/
    // https://www.dndbeyond.com/


- {CombatStart}
- You:
    + "Fight!"
    + "Flee!"
        -- You attempt to run, but the the party isn't fast enough.
    + "Can we talk about this?"
        -- You attempt to talk.
        TODO: Add a custom response
        -- <> Diplomacy doesn't seem to be the best option in this situation. Creature gains advantage on its first attack.
- Roll Initiative.
    + Proceed to fight.
    + Combat Encounter HOW-TO:
        -- {Combat}
            ++ [next]
        -- {CombatTurns}
            ++ Proceed to fight.

- \**************************
Combat actions by individuals and each party are determined LIVE in class and not scripted into the story.
\**************************
    + [next]
- The fight is over.
    + SUCCESS[!]
        ->postcombat
    + FAILURE[!]
        ->death
        TODO: Add a specific death message
        -- (death) <>
        -- You have died.
        ++[next]
        TODO: Change to a specific Start Menu

//            --> StartMenu
- (postcombat)
- Congratulations on your victory!
    + [next]
- The rest of the trip is uneventful.
    + [next]
- You approach the guards of the Iron Gate.
- Guard: gruffly "What's your business?"
- You pull out the parchment with {DwarfSmith}'s clan seal. You don't know a lot of details about his personal life. Really the only thing you know is that he is an extremely talented smith from the dwarves' greatest city. Your not sure why he left or under what conditions.
+ You're actually starting to feel a little anxious.
- The first guard gate looks at his partner with {~surprise|astonishment|distrust} and back at your party. He hands the seal back to you and waves you through without a word.
+ [next]
- The party enters into the great city.
- {TheWizard}: "Well {DwarfSmith} and {ProfessorL}'s note say to see forge master {ForgeMaster}. He's in the Lemnos district."
- (CityShops)
    + Go to Potion shop.
        -- (potion_shop) You enter the potion shop and look around.
        -- {TheWizard}: "Let's purchase a POTION OF HEAT RESISTANCE for everyone. A POTION OF FIRE RESISTANCE has greater potency or effectiveness against heat, but it only last an hour. This potion will minimize the effects of intense heat but for many days."
        ++ [next] -> CityShops
    + Go to Weapons shop.
        -- {~Shop clerk: "I'm sorry, the Goblin War has emptied our inventory." | It appears to be closed.}
         ++ [next] -> CityShops
    + Go to Armor shop.
        -- {~Shop clerk: "I'm sorry, our inventory is being restocked." | It appears to be closed.}
         ++ [next] -> CityShops
    + Go to Explorers' League headquarters.
        -- {~Note on door: "Gone to lunch." | Note: "Out exploring"}
         ++ [next] -> CityShops
    + [Go to the Lemnos District.] <>
- You go to the tunnel that leads to Lemnos and walk the high ceiling tunnel through to another cavern. A signpost says "Lemnos District"
    + You look for the workshop. <>
- It isn't hard to find. Its size triumphs even the royal smithy in {CapitalCity}. Likewise, the exterior craftsmanship makes it stand out in this working class district of the city.
+ You enter the workshop of {ForgeMaster}.
- For all of its beauty on the outside there is no pretense on the inside. You walk immediately into a working smithy.
  + [next]
- A large dwarf hammers away in the middle of it all. He's tall enough to pass as human, but the proportions of thick muscles, large nose, braided red beard and fiery hair give away his dwarf heritage. The breastplate he's working on clearly isn't the average. It is exceptional in quality and for the owner's obviously impressive physique. A large golden chariot sits in the back of the shop. An exceptional looking spear is propped in one corner. A silver bow and quiver of arrows is attached to the wall behind him.
- {ForgeMaster}: gruffly and without looking up, "I'm not taking any commissions. I'm too busy as it is."
+ You show him the seal of {DwarfSmith}.
- {ForgeMaster} eyes you closely. He tightens his grip and tenses his arms even more.
- {ForgeMaster}: "Why would {DwarfSmith} give you his clan seal? Did he owe you gambling money?"
    + You: "Master {DwarfSmith} is helping us with a quest."
    + You: "Yes Master {DwarfSmith} owed us money from a game of chance."
    -- {ForgeMaster} shakes his head disapprovingly. Whether at your lie or {DwarfSmith} you cannot say for certain.
    -- You: "He said we could deliver this {ore32} and receive payment from you."
    -- You take some {ore32} out to show him.
    -- {ForgeMaster}: "I see."
    ++ [next]
    -- {ForgeMaster}: "{DwarfSmith} does not play at games of chance. Even if he did, his integrity would compel him to pay any and all debts immediately."
      +++ You open your mouth to explain.
      --- Forge Master {ForgeMaster} bashes your skull with single blow of his mighty hammer. He continues to crush ribs and other various body parts of your friends. Your apparent lack of integrity has earned you and the party a just reward from {ForgeMaster}.
      --- {ForgeMaster} didn't even break a sweat. You've experienced a crushing death.
        ++++ [Death] -> 04StartMenu
- {ForgeMaster}: "Okay. Tell me more"
- You: "We are from Canis Villa, same as him, so when we recently acquired some {ore30} we went to him to talk about it."
    + You tell him about the {CelestialTower}[...]<>
- , Kepler's notebook, the conversation with Hutton & Lyell, and the Air Temple.
- {ForgeMaster}'s intense blue eyes haven't left you.
- You: "We also have some {ore32} for you."
- {ForgeMaster}: "I am grateful for the {ore32}, but more importantly I'm impressed by your integrity.
    + You nod your head acknowledging the compliment.
- {ForgeMaster}:  "If someone is successful at triggering another Big Bang or a Cosmic Crunch event, our only counter option is with the {EpicWeapon}."
    + \ {TheWizard}: "I thought it was a legend and no one ever found it."
    + You: "Where would we find {EpicWeapon}?"
- {ForgeMaster}: "You won't find it anywhere."
    + \ {TheFighter}: "Um, I'm confused if we can't it anywhere..."
- {ForgeMaster}: "You won't find it anywhere because it hasn't been created yet. I know how. We are going to craft it!"
- (end_episode)
    +[Ω]
        ->04StartMenu
    +[NEXT EPISODE]
        ->04Ep3
->DONE

=== 04Ep3 ===
// Room 3: Trick or Setback
{04EpisodeThree}
- {TheCleric}: "Great! How long will it take you to craft {EpicWeapon}?"
- {ForgeMaster}: "That depends how long it takes you to retrieve the materials."
  + \ {TheWizard}: "What are the materials?"
- {ForgeMaster}: "The {ore30} is the main material. Until now no one has had enough or if they did, they didn't have the knowledge I possess. You've brought me plenty. It also requires Air Essence for the billows..."
  + \ {TheFighter}: interrupts "Great we already got those!"
- {ForgeMaster}: stares at him and continues "Earth Essence and Fire Essence to smelt the ore. Finally, {EpicWeapon} must be quenched in oil and Water Essence."
  + You: "Where would we find those?"
- {ForgeMaster}: "You can find Earth Essence through that tunnel."
- He moves his massive anvil aside and slides a false wall to the side.
- {ForgeMaster}: "You will need to travel to the core of the planet."
  + Jaws drop in the party except for {ProfessorL}.
- {ProfessorL}: "I knew it was possible! {Axel} and I shall also accompany you on this journey. We shall study the interior of the planet like no before!"
- {Axel} turns pale.
- {ForgeMaster}: "After you travel about 6,000 km (3,700 miles) you will battle the Earth Elementals that dwell there and collect their essence before returning."
  + \ {TheFighter}: sarcastically "Is that all? Why don't you do it then?"
- {ForgeMaster}: "I've not been to those depths in many decades. I'm not...welcome in those parts. There is also the risk of getting lost and I've not had a compelling reason to risk it."
  + \ {ProfessorL}: "Axel my boy! We have the map of these tunnels from our discovered manuscript!"
- {ForgeMaster}: Gives him a big grin. "Right then. Sounds like you need only prepare supplies and then be on your way."
  + You: "Aren't you coming?"
- {ForgeMaster}: "I must stay behind. I need to continue my work on a golden shield and guard the entrance."
- The party looks a little disappointed at this news.
  + You gather supplies and comeback to the forge of {ForgeMaster} and walk through the previously hidden tunnel.
- {ForgeMaster}: "Be on the lookout for duergar, fire beetles, driders - well basically any fire creatures and the Underdark creatures."
  + [next]
- {ForgeMaster}: "Take the tunnel going down, not the one going up. What you would find going up would seem more beautiful and handsome, but it is even more deadly. Good luck!"
- With that strange farewell,
  + <> you set off into the up-sloping tunnel.
    -- Only a few meters into the tunnel you feel a shimmering of your entire body. It would seem you've gone through a hidden portal.
    -- Looking around, your surroundings can only be described as heavenly-like.
    -- A muscular man approaches. He is handsome[...]
    ++ [next]
    -- <> and apparently deadly.
    -- The god: "You don't belong here mortal."
    ++ [next]
    -- With that he smites you all with a lightening bolt. You die shocking death.
    ++ [next] -> 04Ep3

  + <> you set off into the down-sloping tunnel.
- (end_episode)
    +[Ω]
        ->04StartMenu
    +[NEXT EPISODE]
        ->04Ep4
=== 04Ep4 ===
// Room 4
{04EpisodeFour}
- Initially the air feels cooler and more dank as you descend.
- You continue deeper and deeper. After some time (several kilometers?) it seems to be getting hotter and hotter
  + \ {ProfessorL}: "By my calculations[..."]<>
- , the temperature is increasing approximately 30 °C for every 1 kilometer (54 °F for every .67 mile) that we continue to descend."
  + Take a POTION OF HEAT RESISTANCE.
  + [You're tough, don't take a potion]
    -- Sweat pours off you. You're starting to become dizzy and nauseous with dehydration as you descend farther.
    ++ The party members force you to take the potion.
    -- They're also forced to listen your complaining.
- Without the potion, you'd surely would have died from the heat by now.
- {TheWizard}: "Take caution with any direct fire and remain hydrated. This potion will not protect you from falling into lava or other stupid behaviors."
- \ {TheCleric}: "Burn injuries are also extremely difficult to heal even with my skills."
- Most of the party nods in understanding.
    + \ {TheWizard}: To {theFighter} "Are you paying attention?!"
- {TheFighter}: "What? Oh right.
- <> Fireball to the face \= bad. I won't let that happen. Again."
    + [next]
- After a couple of days your water supply starts running low.
- {TheFighter}: "I'm so thirsty. I'm can practically hear the sound a creak bubbling through pebbles..." Trails off as he imagines the cool, refreshing water.
- {TheCleric}: "Seems {theFighter} is getting dehydrated."
    + \ {TheWizard}: "Did you know ground water [..."] <>
        -- is stored in large quantities in between the rocks of the ground?"
    + \ {TheWizard}: "Did you know dehydration [..."] <>
        -- side effects include poor judgement, delirium, and stomach distress?"
- You: "Um...cool?"
- The party walks a few miles more. Its clear you underestimated the amount of water you'd need.
    + Without warning[...] <>
- {theFighter} pulls out a war hammer and bashes it against the tunnel wall.
- Everyone jumps with surprise.
    + \ {Axel}: "What in the gods are you doing?!"
- {TheFighter}: "I'm just so thirsty. I guess I let my frustration get the better of me."
- You point.
    + You: "There's water trickling from the wall."
- Everyone turns to look where {theFighter} had crushed a portion of the rock. Sure enough water is seeping from the wall. Everyone starts filling up their water skins.
    + [next]
- A few more miles down the tunnel the party is still giddy from the water find in this assumed realm of earth and fire. Water stored or moving through rocks was interesting discovery.
+ Then things got even more interesting.
- An area of stone on the side of the tunnel starts to coalesce (come together) and bud outward. In a split second, an animated mound of rock and dirt stands before you. Its an Earth Elemental.

- (combat1)
- An EARTH ELEMENTAL stands before you.
# IMAGE: images/monster-earth-elemental.jpeg

    // https://www.dndbeyond.com/

- {CombatStart}
- You:
    + "Fight!"
    + "Flee!"
        -- You and the geologists turn to run.
        -- {TheCleric}: "Fools! We have no where to flee. We're thousands of miles underground by this point. Surely the elemental would catch us or reform ahead of us."
        -- The party turns and faces the creature. The party loses initiative.
    ++ You: "Fight?"    + "Can we talk about this?"
        -- You attempt to talk.
        TODO: Add a custom response
        -- <> Diplomacy doesn't seem to be the best option in this situation. Creature gains advantage on its first attack.
- Roll Initiative.
    + Proceed to fight.
    + Combat Encounter HOW-TO:
        -- {Combat}
            ++ [next]
        -- {CombatTurns}
            ++ Proceed to fight.

- \**************************
Combat actions by individuals and each party are determined LIVE in class and not scripted into the story.
\**************************
    + [next]
- The fight is over.
    + SUCCESS[!]
        ->postcombat
    + FAILURE[!]
        ->death
        -- (death) <>
        -- You are smothered and die.
        --There is the risk of being buried for millennia.
        ++ [next]
        -- Fortunately you aren't buried that long.
            +++ [next]
                -> 04StartMenu
        ++[next]
- (postcombat)
- Congratulations on your victory!
    + [next]
- <> - {CombatEnd}
      + [next]
  // END OF COMBAT
- As the elemental starts to fall into hunks of rock and sediments, you notice an especially fine mound of sediments that appears somewhat iridescent (shiny).
- {ProfessorL}: "Get the empty bottle and scoop that up!"
- {TheCleric} gets out the bottle and brushes in the sediments which everyone now realizes is the Earth Essence.
- {Axel}: "Well I guess that wasn't so bad. Could have been worse right?"
    + [next]
- A low rumble is coming from up the tunnel.
- {TheFighter}: "Is it just me or does everyone else hear a rush of moving water now?!"
    + You listen carefully. <>
        -- Apparently the crack {theFighter} created in the wall has turned into a large hole letting out a lot more water.
    + [Pay no attention to {theFighter}.]
        -- You: "I'm sure its nothing."
- The party hears a large boom. It sounds like a large chunk of the wall up the tunnel has been shot across to the far side.
- You:
    + "Run down the tunnel!"
        -- Everyone sprints down the tunnel as the sound of torrential water reaches you.
    + (grab_tight) "Grab something tight!"
        -- Everyone grabs little cracks and crevices.
- A moment later the rushing water reaches you.
    + {grab_tight} You try to hold on, but the current is too strong. <>
    + {not grab_tight} You're not fast enough. <>
- Everyone is swept down the tunnel and carried away.
    + [next]
- After the intial rush, the water lessens just enough for you to get your head above water and breathe. You see everyone else has done the same.
- Many hundreds of miles later the impromptu river starts to lose speed and power. The river's load starts sinking to the tunnel floor.
    + [next]
- The first thing to be deposited are the large boulders which until now, had been rolling along the bottom.
- As the river continues to lose force the medium size rocks and larger sediments settle to the bottom.
- The party is still floating and taking the free ride.
    + [next]
- Finally the river loses enough force - first along its edges - that even the finer sediments and silts are deposited.
- At this point, you're all walking along a new created shoreline on one side of the tunnel.
    + [next]
- The tunnel starts to widen out until eventually it opens to a cavern on a scale not seen in the lower depths of the planet. The river widens out. Small deltas form in the mouth of the river as it empties into an underground sea.
- The sea is large enough that you cannot view the far side.
    + \ {Axel}: "Look at the glow coming from the horizon to our right."
- {ProfessorL}: "Probably exposed lava."
- {TheFighter}: "Might a good place to dry off."
- You:
    + "I agree. Lets dry off our clothes."
        -- The party makes its way towards the glow {Axel} observed. {ProfessorL}'s inference was correct. It is an exposed lava pool. You're able to quickly dry out your clothes and gear.
        -- Everyone is dry and sleepy from the warmth of the lava.
        ++ You: "Let's turn in"
- Some time in the night you notice the heat getting more and more intense. You all awaken with a start.
    + [next]

- (combat2)
- A large FIRE ELEMENTAL, about 9 feet tall and 4 feet wide, stands before you.
# IMAGE: images/monster-fire-elemental.jpeg
    // https://www.dndbeyond.com/
- {CombatStart}
- You:
    + "Fight!"
    + "Flee!"
        -- You attempt to run, but the the party isn't fast enough.
    + "Can we talk about this?"
        -- You attempt to talk.
        TODO: Add a custom response
        -- <> Diplomacy doesn't seem to be the best option in this situation. Creature gains advantage on its first attack.
- Roll Initiative.
    + Proceed to fight.
    + Combat Encounter HOW-TO:
        -- {Combat}
            ++ [next]
        -- {CombatTurns}
            ++ Proceed to fight.

- \**************************
Combat actions by individuals and each party are determined LIVE in class and not scripted into the story.
\**************************
    + [next]
- The fight is over.
    + SUCCESS[!]
        ->postcombat2
    + FAILURE[!]
        -- (death2) <>
        -- You have died.
        -- You are first fried extra crispy then completely into ash.
          -- There is the very real danger of being buried ash for millennia.
          ++ [next]
          -- Fortunately you aren't buried that long.
          +++ [next]
            -> 04StartMenu
- (postcombat2)
- Congratulations on your victory!
    + [next]

TODO: Make this specific to the story
- You carefully gather its lingering essence into a bottle.

- (end_episode)
    +[Ω]
        ->04StartMenu
    +[NEXT EPISODE]
        ->04Ep5
->DONE
=== 04Ep5 ===
// Room 5
{04EpisodeFive}
- You go to edges of the great lake and cool off.
- {TheCleric}: "Look at the amount of steam where the lava meets the lake!"
- {ProfessorL}: "I bet there are some amazing updraft thermal currents are generated at that location!"
- {TheFighter}: Eyes glaze over in confusion.
    + [next]
- {Axel}: translates "There's a strong wind moving updwards."
- {TheWizard}: "Strong winds would make for good sailing."
- {TheCleric}: "I've seen gnomes craft hot air baloons that ride these - what did you call it, thermal currents?"
- {ProfessorL}: "It would seem we have a couple options for crossing the great lake."
- You:
  + "Let's make a raft"
        ->raft
    + "Let's make a hot air baloon."
        ->air_baloon
 == raft
 - Everyone agrees. You start looking for driftwood. It takes a considerable amount of time however as there aren't many trees thousands of miles below the surface that haven't become petrified. Eventually you get enough wood to craft a medium sized raft. You have enough rope and cloak material to fashion a makeshift sail.
    + [next]
- After a short rest and meal, you set out. Its a crude but functional vessel. You make good progress for {1+d4()} days.
- {ProfessorL}: "Look at the dark opening up above. That's either a storm or darkened tunnel leading up."
- {Axel}: "Perhaps we should head back and craft a baloon afterall."
    + [next]
- You have no time to consider the baloon option though. A pair of gigantic tentacles with suckers shoots out of the water and around the raft. {Axel} is knocked overboard immediately. Another pair of tentacles shoots out of the water and around the raft. The raft is easily crushed and pulled into the inky depths.
    + [next]
- The last thing you see is an open gigantic beak coming at you. The kraken has easily accomplish a total party kill (TPK). ->death3
- (death3)
- You have died
    + [next]
        ->04StartMenu

 == air_baloon
- You build an air baloon and ride the air currents. You are the mercy of the wind but at least you seem to be making progress.
- After a few days you notice a dark shaft leading up through the ceiling of the cavern.
    + [next]
- Gigantic suckered tentacles shoot up from the water towards you. They fall just short, but way too close for comfort.
- Everyone starts throwing over any extra weight to gain altitude. The kraken has now surfaced and throws its tentacles at you again and again.
    + [next]
- You continue to chuck heavy and sometimes valuable materials overboard. The baloon slowly makes its way up the shaft.
    + [next]
- More than once the sides of the baloon scrape sharp edges, but the structure holds.
- {ProfessorL}: I think we are in an old volcanic throat and that cavern was its lava chamber.
    + [next]
- You eventually make your way to the surface after many hours. You come out on the slopes of an extinct volcano named {volcano1} in the {mountains1}.
    + [next]
- You make your way back down the snowy slopes to the gates of Iron Forge.
    + [next]
- You meet with {ForgeMaster}.
- {ForgeMaster}: "I was excited about the possibility of crafting {EpicWeapon}, but I must say I never expected you to actually accomplish it!" He laughs loudly.
- No one from your party is laughing.
- {ForgeMaster}: "How are you at holding your breathe? The final material is water essence!" He laughs loudly again.
- Again, no one from your party is laughing.

- (end_episode)
    +[Ω]
        ->04StartMenu
    +[NEXT EPISODE]
        ->04Epilogue
->DONE
== 04Epilogue ==
EPILOGUE
Code for Season 4:
Z62kaepM

    +[Ω]
      ->04StartMenu

  ->END
// ***************************************************************
////
// ENDURING QUESTIONS
// AUDIO
// ***************************************************************
