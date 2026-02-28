#theme: dark
#title: EXPEDITION of ORIGINS
#author: Master Scoy


VAR Season03Hook = "our mutual friend in the village contacted me yesterday. His contacts say the two highlander alchemists are back around home and they are very interested in meeting your party and the...item that you have recently...acquired."



=== 03Prologue ===
PROLOGUE
#IMAGE: images/blue-mug.jpg

- {Maid}: "Welcome weary adventurers!"
- Your party is greeted with a {~ warm| big| kind} smile from {Maid} at The Blue Mug inn.
    + You: "We grow {~bored|tired} of town life. What news do you have?"
- (season_hook) // SEASON HOOK
- {Maid}: "{Season03Hook}"
    + [next]
- You:
- {TheCleric}: This is promising news. Thank you {Maid}!
- You: Sounds like we'll be venturing off to {village5}?
- The whole party agrees.
- You:
    + "We'll gather supplies and head out tomorrow morning.""

- (episode_end)
OBJECTIVES for {Season03}
// Learning Objectives
\* Answer the Enduring Question: {Q03a} {Q03b}

// Phenomena
\* Explain the phenomena of {Ph03}.

// Game Objectives
\* Meet with the scientists
\* Collect air essence from the air elementals.

   +[Ω]
       ->03StartMenu
    +[NEXT EPISODE]
        ->03Ep1
->DONE

=== 03Ep1 ===
// Room 1: Guardian OR Room 2: Puzzle
{03EpisodeOne}
- You travel to the highlands in search of {Hutton} and {Lyell}. You arrive without any significant events along the way.  A villager in {village5} tells you the pair spends a lot of time near the local exposed cliffs.
+ [next]
- You leave the village on a path that comes out at the top of the cliffs. Paths run along the cliff tops with numerous ragged ravines in between cliff tops. The mountain paths seem to be a more direct, but more dangerous route. There is also a switchback trail heading down to a beach along the base of the cliffs. The switchback looks like a longer and more difficult trip.
- {TheCleric}: We probably should've have asked for more specific directions.
+ [next]
- {TheWizard}: Where do you look for these men?
    + [along the high cliff tops]
        -- {TheFighter}: are we sure we want to cross numerous cliff top ravines without climbing gear?
        -- You:
            ++ We should take the shorter route on top. What could possibly go wrong up here?
            ++ We should take the longer switchback. A little walking never killed anyone right? ->switchback
        --- You cross {2+d4()} ravines. Then {~{theWizard}|{theFighter}|{theRogue}} slips and falls into you. The remaining party members try to grab you, but you partially pull and knock the rest of them off their feet in the process.
        --- You have just enough time to shout expletives and wish you had climbing gear or a FEATHER FALL spell. You and the party are greeted by hard rock two hundred feet below.
        --- If it  isn't obvious, you have died a painful splattering death. -> 03Ep1
    + [along the beach at the cliff base]
- (switchback) You make your way down the switchback from the cliff tops. Its a long, difficult route but you make your way safely to the beach.
+ [next]
- You walk {~a quarter mile|a half mile} along the beach noticing the wall of multicolored rock layers to your right and upwards
+ [next]
- Suddenly you are attacked by {1+d4()} SAND BEETLES attack.
- You:
    + Attack
    + Flee<>
    --? Your party has weapons, spell casters, armor...really, don't be a sissy.
    ++ [next]
- You defeat the SAND BEETLES.
+ [next]
- You go a short distance when Hutton & Lyell walk from behind an outcrop.
    +[Ω]
        ->03StartMenu
    +[NEXT EPISODE]
        ->03Ep2
->DONE

=== 03Ep2 ===
// Room 2: Puzzle OR Room 1: Guardian
{03EpisodeTwo}
- Hutton: Hello there. We heard a commotion and came to see what it was about.
- Sure enough, James Hutton and Charles Lyell are there studying the stratigraphy (layers).
- Lyell: The beach is not the best approach. Ascending or descending a crevasse is less hazardous because SAND BEETLES cannot climb. Assuming you have a CLIMBING KIT.
    + You face palm.
    + \ {TheFighter} cuffs you on the back of the head.
- Hutton: You must be the lads and lasses {DwarfSmith} mentioned. So you have a rock sample for us?
+ You get out the ore.
- They examine it more closely.
- Hutton: We'll run a few field tests such as a Moh's hardness test and examine its mineral crystal structure. Meanwhile, you lot need to figure out the relative dating and relative age of those bedrock rock layers for us. (See Game Master for a diagram)
    + \ {TheFighter}: What's relative dating and relative age? It sounds ... weird.
    + You: We can do that.
- Although some party members struggle, you eventually figure it out.
    + You: The rock layers at the bottom are the oldest and the rock layers near the top are the youngest
        -- Lyell: Nicely deduced on those sedimentary layers! Sedimentary rock layers on the bottom are older than the sedimentary rock layers above it.
    + You: The rock layers at the top are the oldest and the rock layers near the bottom are the youngest
        -- Lyell: Not quite. Sedimentary rock forms in layers. You need to start with layers at the bottom before you can pile on more layers. If the layer was created on the bottom - earlier - then is must be older.
- Lyell: What about that intrusion cutting across the first three layers we see?
    + \ {TheFighter}: What's an intrusion?
      -- Lyell: An intrusion a volcanic formation. For example, when magma from below the surface squeezes up through cracks or flows on top of a layer before being buried.
      ++ \ {TheFighter} nods his head in understanding or at least fakes Understanding
    + You: The intrusion must be younger than any layers it cuts across or rests upon.
    -- Lyell: Correct!
    + You: The intrusion must be older than any layers it cuts across or rests upon.
    -- Lyell: Incorrect (Intelligence -1)
- Lyell: Think of it this way. You can't cut across something if its not there. The sedimentary layer to be cut across must be their first. Then the igneous intrusion cuts across it. Therefore, intrusion came later and is younger than any layer it cuts across. Sedimentary layers that are laid on top of the intrusion are younger.
+ \ {TheCleric}: Wait a minute[...]
- <>, it sounds like you new about this already.
- Hutton: (laughs) Yes, we just came to this conclusion. We wanted to see if you could reason it out, too. You've proven yourselves as logical, rational thinkers.
+ [next]
- Hutton: Would you like to hear about our other ideas?
    + Yes please.
        -- Fabulous! We see similar features being formed today by processes that are very slow. We have concluded that processes occurring today have also occurred in the past. A concept some have since called uniformitarianism.
        ++ [other idea]
        --- These slow processes would need tens of thousands of years to form. Some features would need millions of years. A deep time, an incredibly long amount of time. The solar system and planet are not a few thousand years old; they are billions of years old.
        --- You: Its almost unbelievable. Its hard to wrap my head around how much time has passed.
        --- Hutton: Logically it makes sense, but its also difficult to fully grasp how much time has passed.
        +++ Perhaps we could hear your conclusion about the ore?
    + No thanks.
        -- Hutton: Well if you're sure...Read our published works coming out later this year.
        ++ Hutton: Are you wanting to hear about your ore now?
- Lyell: Keep in mind 'understanding the present is the key to past.'
- Hutton: One might also say, 'Understanding the present is the key to predicting the future.'
+ [next]
- Hutton: We agree the ore is a meteorite from the celestial plane.
+ [next]
- Hutton continues: Meteorites are the leftovers of the solar system's beginning. Kind of like  time capsule. They can tell us a lot about the planet's formation and the formation of the solar system. In many ways, they are the origins.
+ [Mind blown...]
- You: That's pretty incredible.
- Lyell smiles: We think so.
- Hutton: This particular sample has some unique properties as well. It bends nearby space-time ever so slightly. If it were purified and concentrated...(pauses) it could be extremely powerful.
+ You show them the notebook.
+ You decide not to share the notebook.
    -- Lyell: It'd be interesting to know how you came to possess this ore and who had it previously. It be troubling to think how it could be used.
    ++ Perhaps it would be better to show them the notebook.
    ++ You still don't trust them.
    -- Hutton: OK well if that's all you have...Right. Well, we wish you luck!
        ++ [next]
    -- You take the ore and travel back towards home. A few miles before Canis Villa you are ambushed by {5+d12()} ACOLYTES of the evil mage. You manage to kill a few of them, but they overrun your party. Its a TPK (Total Party Kill). The ore is taken back for nefarious purposes. ->03StartMenu
    -- ->END
- They take a minute to look through the notebook.
Lyell: We think large quantities of smelted ore (purified) when combined with ELEMENTAL ESSENCE and powerful magic would likely lead to a cataclysmic event. Perhaps even a second Big Bang which seems to be the goal of this evil mage.
+ [next]
- Hutton: I'm sure the dwarves of Iron Forge could tell you more. We recommend traveling to the Air Temple on the highest peak of this mountain chain. Gather rock from near the peak. Even small samples of the {ore32} are highly prized by the dwarves. The {ore32} would give you something to barter for information.
+ [next]
- You: We greatly appreciate your help. We shall tell every bard we meet about the genius of James Hutton and Charles Lyell!
  + { kits == zero } You: One more thing[...]
    -- <>, can we borrow a spare CLIMBING KIT?
    -- Absolutely! I'm surprised you didn't bring one to the mountains (both men laugh) and give you a kit.
    ~ kits = ClimbingKit
    ++ [next]
    -- You have added a {kits} to your "kits."
  + You: We must be off now!
    {kits == ClimbingKit:
    -else:
           {TheFighter}: (thinking to himself) we probably should have borrowed a Climbing Kit. Oh well.
    }
- The party sets out for the peaks of the Air Temple.

    +[Ω]
->03StartMenu
+[NEXT EPISODE]
        ->03Ep3
->DONE

=== 03Ep3 ===
// Room 3: Trick or Setback
{03EpisodeThree}
- You travel for a few hours into the heat of the afternoon. You come to a deep channeled mountain river.
+ [next]
- The water practically tumbles and crashes from glacial melt above and down the side of the mountain. It cuts through canyons and sprays outward in other areas.
+ [next]
- You do not see any bridges nearby even though you've definitely been following a traveled path up the mountain. As far as you can tell, people were crossing here but the bridge appears to have recently been washed out.
    + [Water details]
    -- Moving water is an amazing force in the moment and over time. Water and wind literally moves mountain over time. You notice wash out areas and the pattern of deposits in places where the river overran its banks.
    ++ [Water deposits]
    --- The heaviest load in the river such as boulders and rocks are deposited first.
    --- Afterwards, the medium sized sediments are deposited.
    --- The next to be deposited is the fine sands and silts.
    --- Lastly is the dissolved load which is not deposited until the water evaporates or becomes more saturated with dissolved sediments.
    + [Figure out how to cross]
- You need to figure out a way to cross the deep river. It looks like a narrow crossing here and there's a bend in the river.
    + [Scout around]
        -- You invest a little time in finding the best place to make your crossing.
        -- {TheFighter}: Avoiding bends in the river is best. Sure its slower on the one side of a bend, but on the other side water whips around the fastest.
        ++ You walk downstream[.]
        -- <> a few hundred feet to make sure there aren’t any hazards.
        -- You find out there's unknown pesky 30-foot waterfall around the bend.
    -- When you cross, you'll want to be down past the waterfall even if it adds an hour or two of hiking over and around boulders.
  ++ Try to cross here today. ->downriver
  ++ Camp and wait until morning.
    --- {TheWizard}: {GlacialRiverMelt}.
    --- {TheCleric}: Anyone else afraid of bandits or monsters attacking in the night if we camp?
    +++ You: True, lets cross here today.
        ->cross_here
    +++ You: We will take turns posting a watchman.
    --- Nothing happens in the night.
    ++++ { kits == ClimbingKit} [Cross]
        ---- You cross safely.
    -> end_episode
    ++++ { kits == zero} [Cross without rope]
    -- You almost make it across but slip on a slimy rock. You reach out for the person in front of you effectively pulling them into the water and knocking everyone behind you into the water. Too bad you didn't have a rope.
        ->river_death
  + [Cross here]
    --- (cross_here) Unfortunately, narrow crossings can be the most dangerous because they’re often the deepest part of the river which happens to be the case with this river spot.
    --- {GlacialRiverMelt}. You arrived in the midday heat.
    --- You ignore the whitecaps and plunge in.
    ++ [next]
    --- (downriver) You are swept downriver and off the 30-foot waterfall that was around the bend.
    --- (river_death) In a torrent of water, whitecaps, and rocks - you die.
        ->03Ep3
// Ditch Your Duds - If the water will reach your knees, strip down to your skivvies - your pride isn’t worth getting hypothermia from wet clothes. Even if the river is shallow, remove your socks and put on a second pair of shoes if you have them. If you’re backpacking, unbuckle your front straps so you can quickly slip out of your pack if you fall. 4. Shuffle Up - Face upstream, lean into the current, and move across the river with shuffling sidesteps. You’re less likely to fall while sidestepping since you don’t lift your feet as high. If you’re with a group, link arms. The technique creates more contact points with the stream bed and gives everyone a more solid footing. <a href="http://mentalfloss.com/article/53005/how-cross-river-without-bridge">link</a>
- (end_episode)
   +[Ω]
       ->03StartMenu
    +[NEXT EPISODE]
        ->03Ep4
abstract
->DONE

=== 03Ep4 ===
// Room 4: Climax
{03EpisodeFour}
- Your party reaches the Air Temple without further troubles. #audio_mountain_top
    + You: Let's look for the {ore32}.
- It only takes a moment before {~{theCleric}|{theWizard}|{theFighter}} says:
    + \(points) I think that's {ore32} to the west side of the temple.
- You all walk over to the spot.
- You: Yep, this seems to match the description Lyell gave me.
    + Everyone starts gathering ore.
    + (on_watch_during_ore)\ {TheWizard} keeps watch while the rest of you gather ore.
- Fortunately each of you has a BAG OF HOLDING.
~ ore = ThoriumOre // ore32
- You add {ore32} to your inventory of ore.
    + [next]
-
    + { not on_watch_during_ore } The wind suddenly picks up knocking everyone down.
        -- Each fallen player must roll a CONSTITUTION check
        ++ [next]
        -- Anyone that failed the check is stunned for {d4()} rounds of combat.
    + { on_watch_during_ore } The wind suddenly picks up knocking everyone down except {theWizard}.
        -- \ {TheWizard} quickly casts the spell FEATHER FALL on the party so you all land softly.
- A battle has started with a large AIR ELEMENTAL! #audio_battle
# IMAGE: images/monster-air-elemental.jpeg
    + [next]
- The battle rages on with the AIR ELEMENTAL
    + Failure
        -- You have been blown off the peak of the mountain. You die.
        -> 03Ep4
    + Victory!
- The AIR ELEMENTAL dissipates; elementals are never truly killed. {~{TheCleric}|{TheFighter}|{TheWizard}} is the first to notice a lingering wisp of shimmering air.
+ \ {TheWizard} quickly pulls out a large vial and scoops the wisp in before it disappears as well.

- {TheWizard} reminds you that Air Essence will be needed later in the quest.
- You: (to {theWizard}) Quick thinking!
- {TheFighter}: Now what?
    + (enter_air_temple) You enter the Air Temple. #audio_temple
        --
        ++ You look around briefly.
        -- There doesn't seem to be anything of interest.
            ->leave_mountain_for_home
    + (leave_mountain_for_home) You leave the mountain. #audio_mountain_top
- The party makes its way back to Canis Villa without further incident. #audio_none

   +[Ω]
       ->03StartMenu
    +[NEXT EPISODE]
        ->03Ep5

->DONE

=== 03Ep5 ===
// Room 5: Reward, Revelation, or Twist
{03EpisodeFive}
- You talk with {DwarfSmith} at the smithy in Canis Villa. #audio_canis_villa
- {DwarfSmith}: Hutton & Lyell gave you good advice. The dwarves in Iron Forge will definitely be interested in the {ore32}!
- He takes out a small metal seal on a small chain.
- {DwarfSmith}: I'm giving my clan seal to you. It should grant you passage into the dwarves' great city.
- You: Thank you sir.
- {DwarfSmith}: (nods in acknowledgement) Once you arrive in Iron Forge, head to my old district of Lemnos and talk with forge master {ForgeMaster}. He will know what to do. Perhaps he will craft you something epic if you impress him.
- MORE IN THE NEXT SEASON!
   +[Ω]
       ->03StartMenu
//    +[NEXT EPISODE]
//        ->03Epilogue

->DONE

=== 03Epilogue ===
EPILOGUE
Code for Season 3:
L4pZP56C

    +[Ω]
      ->03StartMenu

-> DONE

=== 03Preface ===
//{Season03}
SETTING
{WorldSetting}
PURPOSE
{CYOAPurpose}
->DONE

->END

// ***************************************************************
//
// History of Earth
//
// ENDURING QUESTIONS
// How do people reconstruct and date events in Earth’s planetary history?
// Why do the continents move?
//
// ***************************************************************
-> DONE
