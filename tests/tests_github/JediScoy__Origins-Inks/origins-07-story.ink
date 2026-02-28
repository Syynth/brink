#theme: dark
#title: MINING the CAVERNS of TIME
#author: Master Scoy



VAR Season07Hook = ""



===07Prologue ===
PROLOGUE
\*****************************************
This story is still in active development
\*****************************************
- {Maid}: "Welcome weary adventurers!"
- Your party is greeted with a {~ warm| big| kind} smile from {Maid} at The Blue Mug inn.
    + You: "We grow {~bored|tired} of town life. What news do you have?"
- (season_hook) // SEASON HOOK
    + [next]
- (episode_end)
TODO: Change "07" to specific Season
OBJECTIVES for {Season07}
// Learning Objectives
\* Answer the Enduring Question: {Q07}

// Phenomena
\* Explain the phenomena of {Ph07}.

// Game Objectives
\* Collect water essence from the water elementals.

    +[NEXT EPISODE]
            ->07Ep2

    ->DONE
=== 07Ep1 ===
// Room 1: Guardian OR Room 2: Puzzle / Roleplay Challenge
{07EpisodeOne}


TODO: copy and paste "CombatScene" template into a story as needed.
- (combat1)

TODO: Add a specific monster here
- {d4()+1} MONSTERS come at you.
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

TODO: Make this specific to the story
- Next part of story...

- (end_episode)
     +[Ω]
         ->07StartMenu
    +[NEXT EPISODE]
        ->07Ep2

->DONE

=== 07Ep2 ===
// Room 2: Puzzle / Roleplay Challenge OR Room 1: Guardian
{07EpisodeTwo}
     +[Ω]
         ->07StartMenu
    +[NEXT EPISODE]
        ->07Ep3

->DONE

=== 07Ep3 ===
// Room 3: Trick or Setback
{07EpisodeThree}
     +[Ω]
         ->07StartMenu
    +[NEXT EPISODE]
        ->07Ep4

->DONE

=== 07Ep4 ===
// Room 4: Climax
{07EpisodeFour}

TODO: copy and paste "CombatScene" template into a story as needed.
- (combat1)

TODO: Add a specific monster here
- {d4()+1} MONSTERS come at you.
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

TODO: Make this specific to the story
- Next part of story...

     +[Ω]
         ->07StartMenu
    +[NEXT EPISODE]
        ->07Ep5

->DONE

=== 07Ep5 ===
// Room 5: Reward, Revelation, or Twist
{07EpisodeFive}

     +[Ω]
         ->07StartMenu
    +[NEXT EPISODE]
        ->07Epilogue

->DONE

=== 07Epilogue ===
    EPILOGUE
     +[Ω]
         ->07StartMenu
->DONE

=== 07Preface ===
SETTING
{WorldSetting}
PURPOSE
{CYOAPurpose}
->DONE
     +[Ω]
         ->07StartMenu

->DONE
->END
<b>Chapter 2: AN INVITATION</b> // An Invitation to The Arrakis Sandworms Raid”</b>
Recently an expedition party in the southeast has disturbed a previously unknown colony of Arrakis Sandworms.
+ [next]
- Naturalists have determined that these are not the same worms from the Arrakis Wasteland. They are in fact a new species resulting from an evolutionary process when this colony became physically separated. They are still plenty big and a potential threat to the entire region.
+ [next]
- Storm Crown has declared an emergency, and is calling upon members of the “League of Heroes.”
+[next]
- The battle will demand a force of epic proportions including the use of siege engines to drive the worms back down. Many parties of heroes from across multiple factions will be needed to win the day.
+ [next]
- If a party is filled with all <i>League of Heroes</i> members, Storm Crown will send a steam-powered dirigible to pick you up and take you to the battlefield. Whereupon, you will need to build and operate a siege weapon.
+ <i>Storm Crown and the entire realm are counting on you!</i>
- You must be a member in the <i>League of Heroes</i> in order to join this raid. You can become a member of the league by totally mastering all Quests to this point (i.e. green check on the folder in Schoology).
+[next]
- Make plans to prepare and determine when & where any Elite Bosses might respawn.
-> OriginsEND

=== OriginsEND ===
-
+ [Ω]
- Party members should help one another with preparations including studying notes together or playing Quizlet games. We're in this together!

-> DONE

-> END
