#theme: dark
#title: DISCOVERY at CELESTIAL TOWERS
#author: Master Scoy

VAR Season02Hook = "a very large fireball recently streaked through the sky and at least a piece of it hit the ground nearby. It made a big hole in the ground and scorched a patch of land on farmer Jameson's property. Two mages in the Celestial Tower predicted when it would arrive and where it would hit. They even collected a piece of the fireball! Its been said they've predicted many great events in the ether over the past few decades. They can say when the moon will turn blood red or when nearby stars seem to brighten. The Queen is concerned they are using dark magic to make these events happen. The mages are notoriously reclusive and have offered little information to anyone - even the Queen herself. She can't openly accuse them of ill doing, but she's willing to make secret inquiries into their predictions. She will pay handsomely for valuable information. You must enter the Celestial Tower and figure out how the mages are able to predict events in the ether with such great accuracy."

=== 02Prologue ===
# IMAGE: images/blue-mug.jpg

PROLOGUE
- {Maid}: "Welcome weary adventurers!"
- Your party is greeted with a {~ warm| big| kind} smile from {Maid} at The Blue Mug inn.
    + You: "We grow {~bored|tired} of town life. What news do you have?"
- (season_hook) // SEASON HOOK
- {Maid}: "Well, a very large fireball recently streaked through the sky and at least a piece of it hit the ground nearby. It made a big hole in the ground and scorched a patch of land on farmer Jameson's property. Two mages in the Celestial Tower predicted when it would arrive and where it would hit."
# IMAGE: images/comet.jpg

  + [next]
- <> "They even collected a piece of the fireball! Its been said they've predicted many great events in the ether over the past few decades. They can say when the moon will turn blood red or when nearby stars seem to brighten. The Queen is concerned they are using dark magic to make these events happen. The mages are notoriously reclusive and have offered little information to anyone - even the Queen herself."
  + [next]
- <> "The Queen wants to make secret inquiries into their predictions. She will pay handsomely for valuable information. You must enter the Celestial Tower and figure out how the mages are able to predict events in the ether with such great accuracy."
  + [next]

//    + {TheWizard}: "Heading directly to the Celestial Tower makes the most sense."
//    + {TheCleric}: "I could check with other priests to find out their interpretation of the omen."
//    +
- {TheWizard}: We could use some coin.
- {TheFighter}: I just want to crack some skulls.
- You:
    + "Lets head to the {CelestialTower}."
- (episode_end)
OBJECTIVES for {Season02}
// Learning Objectives
//\* Answer the Enduring Question: {Q02a}
//\* Answer the Enduring Question: {Q02b}
//\* Answer the Enduring Question: {Q02c}

// Phenomena
//\* Explain the phenomena of {Ph02}.

// Game Objectives
\* Enter the {CelestialTower}.
\* Figure out what the mages are up to.
# IMAGE: images/celestial-tower.png


    +[Ω]
        ->02StartMenu
    +[NEXT EPISODE]
        ->02Ep1
->DONE

=== 02Ep1 ===
// Room 1: Guardian OR Room 2: Puzzle/Role play Challenge
# IMAGE: images/celestial-tower-outside.png
{02EpisodeOne}
- You arrive at the {CelestialTower} without incident. Many stones have fallen from the wall. Thick moss and ivy cover many of the stones.
- {TheCleric}: Mr. Kepler, one of the mages, wrote a book that I just so happen to have in my BAG OF HOLDING. I think we should read <i>Astronomia nova</i> before going inside.
# IMAGE: images/astronomia-nova.jpg
- You: <>
    + "Great idea! Its best to be prepared."
    -- (Astronomia_Nova) {TheCleric} opens the book and reads.
        Tycho Brahe is {Brahe}.
        {BraheAbout}
    ++ [next]
    -- Johannes Kepler is {Kepler}.
        {KeplerAbout}
    ++ You: "Lets go in now."
    + "I don't see the point."
- {TheFighter} knocks on the door.
- The castellan opens the door.
- The castellan: "Greetings. I am {Castellan}."
- With exasperation, {Castellan}: "Another party of adventurers hoping to earn easy gold from the royal family? Save yourself the trouble and go home."
- {TheFighter} turns to go, but {theCleric} stops the fighter with a hand on the shoulder.
- You: <>
    + "Yes, this was a bad idea."
    + "We are the new assistants to the mages."
- {Castellan}: "Really? <>
    + {not Astronomia_Nova} I don't believe you."
      ->enter_side
    + {Astronomia_Nova } Then you should have no problem providing evidence to your claim."
        -- You: Sure,
        ++ Brahe thinks our planet is at the center of the universe and our solar system
            --- Incorrect!
            ->enter_side
        ++ Brahe thinks the our planet is at the center of the solar system, but not the universe
        --- Ha!
        ->enter_side
        ++ Brahe thinks the nearby planets orbit the sun but the sun orbits our planet.
        -> enter_front
- (enter_front)
- {Castellan}: "Sorry for doubting you"
- {TheWizard}: "We weren't finished. We have reason and evidence to support the idea that the planets orbit the sun as Brahe stated, BUT our planet also orbits the sun."
+ [next]
- {Castellan}: Oh my, now that's an original! I'm sure Master Brahe and Kepler will have...thoughts on your idea. A word of advice; move cautiously up to the 13th floor to meet them. You are likely to encounter...less problems.
+ [next]
You glance at {theWizard} surprised by his additional idea. The entire party looks at each other questioningly at {Castellan}'s warning.  The party walks inside the main door.
  ->enter_celestial_towers

- (enter_side)
You need to leave. The castellan slams the door ending the conversation.
The party walks down the path a little aways from the tower and stops to talk.
- {TheFighter}: "Perhaps we could sneak in a side door?"
- You: "Its worth a shot."
- You:
    + Walk around to the right of the tower.
    + Walk around to the left of the tower.
- You find a small door for the serving staff. Its locked.
- (open_the_door)
    + Ask {TheFighter} to break down the door.
    -- ROLL a STRENGTH check
    // DC of 10
    ++ [Success]
    ++ [Fail] ->open_the_door
    + You attempt to pick the lock.
    -- ROLL a DEXTERITY check
    // DC of 15
    ++ [Success]
    ++ [Fail] ->open_the_door
    + Ask {TheWizard} to use a spell to unlock the door.
    -- ROLL a WISDOM check
    // DC 12
    ++ [Success]
    ++ [Fail] ->open_the_door
- (enter_celestial_towers)
- (end_episode)
    +[Ω]
        ->02StartMenu
    + {enter_side} You: "Probably not the best plan, but being adaptable is an important trait, too". [next]
        ->02Ep2.Side_Door
    + {enter_front} [next]
        ->02Ep2.Front_Door

=== 02Ep2 ===
+ [Entered the Front Door]
-- (Front_Door)
+ [Entered the Side door]
-- (Side_Door)

-{02EpisodeTwo} // Room 2: Puzzle/Role play Challenge OR Room 1: Guardian
-The tower interior is a little nicer than the exterior. You are in a small vestibule. Beyond the vestibule, there is a small dining room with a doorway on the back wall, probably to the kitchen. There are stairs up to your right.
    + [next]->vestibule
- (vestibule)
+ [Go through the door] -> into_the_kitchen
+ [Take stairs up] -> up_the_stairs
+ [Sit at the table] -> sit_in_dining

- (into_the_kitchen)
You head to the kitchen. No one is around except {~a couple rats chewing on moldy bread.|a few dozen spiders eating flies.}
+ You turn around and go back to the dining area. -> sit_in_dining

- (sit_in_dining) You decide to have a seat. After ten creepy minutes alone,  nothing happens.
    + Leave the castle -> leave_castle_early
    + Go up the stairs -> up_the_stairs
- (leave_castle_early)You walk back to the vestibule. A shadowy figure steps into the entrance front of you with a drawn dagger. You turn and bolt up the stairs .-> up_the_stairs
- (up_the_stairs)
    + {Front_Door} You walk up the stairs[.] <>
    + {Side_Door} You run up the stairs[.] <>
- for {2+d4()} floors.
    + [next]
- {d4()+1} SKELETONS come from the stairs above.
# IMAGE: images/skeleton.jpeg

- (combat1)
- {CombatStart}
- You:
    + "Fight!"
    + "Flee!"
        -- You attempt to run, but the the party isn't fast enough.
    + "Can we talk about this?"
        -- The skeletons: with a hollow "aaaghh!"
        -- <> Diplomacy doesn't seem to be the best option in this situation. The skeletons gain advantage on the first attack.
- Roll Initiative.
    + Proceed to fight.
    + Combat Encounter HOW-TO:
        -- {Combat}
            ++ [next]
            -- {CombatTurns}
            +++ Proceed to fight.
- \**************************
Combat actions by individuals and each party are determined LIVE in class and not scripted into the story.
\**************************
        + [next]
- The fight is over.
    + SUCCESS[!]
        ->postcombat
    + FAILURE[!]
        ->death
        -- (death) You die an inglorious death.
        ++ [next]
            --> 02StartMenu
- (postcombat)
- Congratulations on your victory!
    // + { not Side_Door } [next]
        ->landing
    + { Side_Door } The party continues running up the stairs.
        -- You face SKELETONS. Again.
        ++ SUCCESS[!]
        ++ FAILURE[!]
            ->death
        -- {TheWizard}: "I think we might try walking rather than running the rest of the way."
        -- The party agrees.
        + {Front_Door} The party continues walking up the stairs.
- (landing) You finally make it to the 13th floor landing. With a gentle nudge of the thick wooden door, you enter the top room of the tower.
- (end_episode)
    +[Ω]
       ->02StartMenu
    +[NEXT EPISODE]
        ->02Ep3
->DONE

=== 02Ep3 ===
# CLEAR
// Room 3: Trick or Setback
{02EpisodeThree}
- You enter the mage's study and discover his notebook on the desk. It is magically locked.
+ [next]
- A brief, magical message appears telling you to demonstrate your genius. You cannot open the notebook without first demonstrating an understanding of important principles of astronomy: (1) Kepler's Laws of Planetary Motion, (2) Nuclear Fusion and how elements are created by stars, (3) the life cycle of stars, and (4) the evidence for the big bang. Each correct concept grants you a key.
- (concept_keys)
{correct_concept} CORRECT \|\| {missed_concept} INCORRECT
+ {not yes1} {missed_concept < 2} [CONCEPT 1: Correct]
-- (yes1)->correct_concept
+ {not yes2} {missed_concept < 2} [CONCEPT 2: Correct]
-- (yes2)->correct_concept
+ {not yes3} {missed_concept < 2} [CONCEPT 3: Correct]
-- (yes3)->correct_concept
+ {not yes4} {missed_concept < 2} [CONCEPT 4: Correct]
-- (yes4)->correct_concept
--- (correct_concept) -> concept_keys
+ {missed_concept == 0} {correct_concept < 4} [ANY CONCEPT: Incorrect] ->missed_concept
+ {missed_concept == 1} [ANOTHER CONCEPT: Incorrect] ->missed_concept
+ {missed_concept == 2} THE POWER OF YET.
-- You haven't mastered the concepts...YET. You'll get the concepts soon enough by using retrevial practice and discussing the ideas with classmates and the local Master.
-- Unfortunately, a micro-black hole has formed near the notebook, stretching you from head to toe. Death by blackhole.
(reload the webpage to start again) ->END
-- (missed_concept) ->concept_keys
+ {correct_concept ==4} [UNLOCKED!]
- You unlock and open the notebook and notice a sketch
Unfortunately, the mage has also left a guardian as a trap and the trap has been triggered. A monster oozes from behind towards the party.
# IMAGE: images/NotebookSketch_1.png
# IMAGE: images/NotebookSketch_2.png
# IMAGE: images/NotebookSketch_3.png
# IMAGE: images/NotebookSketch_4.png
# IMAGE: images/NotebookSketch_5.png
# IMAGE: images/NotebookSketch_6.png


- (enter_tower_study)
    +[Ω]
        ->02StartMenu
    +[NEXT EPISODE]
        ->02Ep4
The room is filled with tombs of knowledge, thick stacks of parchment, and apparatus scattered throughout.

->DONE

=== 02Ep4 ===
// Room 4: Climax
{02EpisodeFour}
A GELATINOUS CUBE is slowly approaching from the hall and blocking the doorway that leads into and out of the study's.
- Battle the GELATINOUS CUBE
# IMAGE: images/gel-cube.jpeg

+ VICTORY
    ->cube_victory
+ DEFEAT
# IMAGE: images/gel-cube.jpeg
      ->cube_death
        -- (cube_death) A man in the shadows watches your party fall to the floor and the gelatinous cube slowly engulf your bodies. The acidic fluids quickly melt away your remains. You die an inglorious death.
        ++ [next]
            --> 02StartMenu
--(cube_victory)            
- With a shudder, the previously coagulated cube collapses and becomes an oozing puddle.
+ You inspect and loot the puddle with your hand.
-- You take acid damage on your hand every 5 seconds you are in contact with the ooze.
+ You visually inspect the puddle.
-- {TheWizard} casts "MAGE HAND" and loots the puddle without taking any damage.
+ Avoid the puddle entirely and leave the room.
-- {TheCleric}: I've got a bad feeling about this. Let's take a closer look at the puddle.
The party goes back in and {theFighter} carelessly swishes the ooze around with his hand and takes acid damage in the process. {RANDOM(1,4)} additional party members also takes damage. 
- <i>Roll table and seat number to see if anyone else gets acid splashed on them. Roll determines the seat location. A saving throw may be attempted.</i>
- You notice an unusual ore left on the floor from the CUBE. You stuff the ore and mage's notebook into a bag of holding. Later, you can always seek someone more knowledgeable about ore.
# IMAGE: images/thorium.png

    +[Ω]
        ->02StartMenu
    +[NEXT EPISODE]
        ->02Ep5

->DONE

=== 02Ep5 ===
// Room 5: Reward, Revelation, or Twist
{02EpisodeFive}
- You leave the castle tower without incident the same way you entered.
- You make your way back to the Blue Mug Inn & Tavern. You go through the notebook and find an encoded message code.
+ [next]
- (encoded_message) Ask the Game Master for a copy of the coded message if you're up to the challenge.

01010100 01101000 01100101 00100000 01100110 01101111 01110010 01100011 01100101 01110011 00100000 01101111 01100110 00100000 01100001 01101001 01110010 00101100 00100000 01100110 01101001 01110010 01100101 00101100 00100000 01100101 01100001 01110010 01110100 01101000 00100000 01100001 01101110 01100100 00100000 01110111 01100001 01110100 01100101 01110010 00100000 01110011 01101000 01100001 01101100 01101100 00100000 01100010 01100101 01100111 01101001 01101110 00100000 01100001 01101110 01100101 01110111 00100000 01110111 01101001 01110100 01101000 00100000 01100001 00100000 01000010 01101001 01100111 00100000 01000010 01100001 01101110 01100111
// The forces of air, fire, earth and water shall begin anew with a Big Bang

+ SUCCESS
+ FAIL (for now)
-- Comeback another time after you've figured it out.
-- END for now ->END
+ A hint please?
++ [The language of an automated apparatus] ->encoded_message
- The party deciphers the code. The encoded message code talks of an evil plot to remake the universe. This mage has found a way to somehow recreate the Big Bang using a combination of elemental forces - air, fire, earth, and water.  It is unclear if Brahe and Kepler are behind the plan or if their discoveries are being used for ill purposes without their consent.
+ [next]
- You take the ore to your blacksmithing friend, a dwarf.
{DwarfSmith}: it is an ore called a meteorite. It originates from the celestial plane. This rock possess unique properties. They are extremely rare and often used in enchanted crafts. How did you come by it?
YOU:
+ We'd rather not say for now
-- {DwarfSmith}: Fine I guess.
-- He shrugs his shoulders.
- {DwarfSmith}: I don't know how it would be used. Perhaps THE HIGHLANDER ALCHEMISTS, Hutton & Lyell, might be able to help? I'm not sure of their whereabouts right now. I'll get word out to my contacts and see if we can't setup a meeting.
- You: Let's keep this as secret as possible. I think the fewer people that know, the better. We would be interested in meeting these gentleman though. Thank you {DwarfSmith}!
- {DwarfSmith}: Good luck weary adventurers!

    +[Ω]
        ->02StartMenu
    +[NEXT EPISODE]
        ->02Epilogue

->DONE

=== 02Epilogue ===
EPILOGUE
Code for NEXT episode:
Q4ALjQhh

->END

// Essential Outcome
DCI - Big Bang Theory, Cosmic Background Radiation, Redshift & galaxies moving away, many light elements

// Synopsis
Players will travel to the wandering Wizard's Tower to look for him. Some of the locals suspect that the wandering wizard is the one who is causing changes and their weather. While at the tower players will learn some
// +[Ω]
//    ->02StartMenu
->DONE

=== 02Preface ===
// {Season02}
SETTING
{WorldSetting}
PURPOSE
{CYOAPurpose}
    +[Ω]
        ->02StartMenu
    +[NEXT EPISODE]
        ->02Epilogue

// ***************************************************************
// ORGINS an Earth & Space Science story
// ***************************************************************
//
// ENDURING QUESTIONS
// What is the universe?
// What goes on in stars?
// What are the predictable patterns caused by Earth’s movement in the solar system?
//
// ***************************************************************
->END

- You answer,
+ "Lets go to {CastleTwo}, home of the noble family" ->02Ep1
+ "Lets go to {MoonTower}" -> Moon_Tower
+ "Let's go to {SunTower}" -> Sun_Tower
+ "Let's go to {MarsTower}." -> Mars_Tower
- The trip to {CastleTwo} is uneventful. You are greeted by {Castellan} the Castellan, the custodian of the {NobleFamily}'s castle
  + "Thanks for nothing" {theFighter} says and turns away.
  + A firm hand from {theCleric} grabs {theFighter} and makes him wait.
- You quickly reply, "We are serious understanding how objects travel through the ether and why objects are falling out it."
You continue almost in a whisper, "We want to understand the universe and its glowing stars."
+ [next]
- {Castellan} is still skeptical.
- Roll PERSUASION and check with the game master.
  + [Succeeds]
  + [Fails]
  -- {Castellan} is not persuaded and turns to go. You slip him {~a gold piece|2 gold pieces|3 gold pieces|4 gold pieces}.]
- He stops to reconsider. He notices the scholarly robes of {theWizard} and the wholesome looking {theCleric}.
"Well if you are serious, I suggest you seek out Sir Tycho Brahe and his mage Kepler," and he turns to go.
    + You say, "Wait!"
        -> stayOrGo
    + No one says anything, and you let him leave.
        ->quick_leave_to_mars_tower
->DONE

= quick_leave_to_mars_tower
You decide there's nothing else to learn here, and make your way towards {MarsTower}.
    //-> MarsTower
->DONE

= stayOrGo
You say, "We should learn something about these two advisors before we just go knocking on their door."
{theWizard} nods approvingly.
+ \ {TheCleric}: "An excellent suggestion"
-
+ \ {TheFighter}: "There's nothing else to learn here. Let's go".
- {theCleric} says "Sorry. You're out numbered on this one."
- You turn to {Castellan},
  + "Do you have anything else that might aid us?"
- The Castellan turns back to you. {Castellan} says, "I would recommend reading Astronomia nova."
  + {theFighter} scrunches his eyebrows in confusion.
  -- {Castellan} says: "Its a book!" and rolls his eyes.
  + {theCleric} takes the book, "Thank you."

- You open the cover of the book and see a description of the astronomers on the first pages.
    + [Description of Brahe]
        -- {BraheAbout}
    + Description of Kepler
        -- {KeplerAbout}
    + [Skip the descriptions]
- {TheFighter}: says sarcastically, "Well this is going to be interesting."
- {TheWizard}: says without sarcasm, "Indeed it will"
+ [Go to {Mars_Tower}] // -> MarsTowers
->DONE

=== Moon_Tower ===
 {MoonTower}
You arrive at {MoonTower}, but don't learn anything helpful. For now, it's best to go to {CastleTwo}.
+ [next] ->02Ep1

-> DONE

=== Sun_Tower ===
 {SunTower}
You arrive at {SunTower}, but don't learn anything helpful. For now, it's best to go to {CastleTwo}.
+ [next] ->02Ep1
-> DONE

=== Mars_Tower ===
{MarsTower}
Your party travels towards {MarsTower} on a well maintained road with a canopy of trees overhead.
"I still think this is a bad idea," says {theFighter}.
{theCleric} says "Lighten up." to {theFighter}.
{TheFighter} bends down to look at something at the edge of the road.
A shadow suddenly passes over head.
+ You all look up. -> shadow_form

= shadow_form
The shadow is already gone.
Roll PERCEPTION and check with the game master
+ [PERCEPTION succeeds] -> shadow_observation
+ [PERCEPTION fail] -> shadow_observation

= shadow_observation
You ask, "Did anyone else think that shadow resembled the shape of a very large ape?"
{theWizard} says "Nonsense. They aren't native to these woods."
{theCleric} says "Besides, it would have to be massive in order to cast a shadow that size. We would have heard it approaching and leaving."
You all look at each other with uncertainty.
+ You ask, "Anyone else want to turn and go back to the Blue Mug?" -> coward
+ You state, "Well we're almost to the {MarsTower}. Let's keep going" ->arrive
=coward
{TheFighter}: "Don't be a coward" in surprising support of continuing.
+ [next] -> arrive
= arrive
You arrive at {MarsTower}.
{theWizard} says, "No one is here to welcome visitors. Rather odd for a nobleman."
{TheFighter}: "His roommate is a mage. Poor social skills aren't all that surprising."
{TheWizard}: looks confused. You and {theCleric} chuckled.
+ You proceed to the door and knock. -> tower_door_knock
= tower_door_knock
...continued in the next chapter...
-> DONE

= early
You arrive at {MarsTower}, but don't learn anything helpful. For now, it's best to go to {CastleTwo}.
+ [next] ->02Ep1


=== ChapterOne ===
-> DONE

=== Castle_One ===
->DONE
// Early Arrival
= early
Besides the occasional bushwhacking the walk isn't too difficult and soon you approach the dark tower’s main gate.
Stones have fallen around it.
+ [The place is clearly abandoned.]
- At least you thought it was abandoned. The Drow warrior slips from a shadowy side entrance. Blades move so fast they are a blur. This quickly ends your otherwise pleasant walk.
Upon entering the tower room you see stacks of papers and books
-> DONE

=== 02Death ===
You have died

->DONE

+[NEXT]
    ->02Ep1

->DONE

/*
    + [Look at your companions]
      -- You take a second to glance around, too. Sitting to your right is a male wizard more powerful than his young age would imply. The equally lovely and strong willed female healer sits across from him. The brawny fighter sits next to her. You are a green, but enthusiastic party of adventurers.
    + [Turn to {Maid}]
        - You respond, "On second thought, I think we'll pass on the extra meal. Any news of new quests?"
          + "We are interested in another meal," as your stomach gurgles. -> meal
            -- {Maid}  says, "I think we have some {~old mutton| brisket that has only slightly turned|muskrat stew} in the back."
*/
