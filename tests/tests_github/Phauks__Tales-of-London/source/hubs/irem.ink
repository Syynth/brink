=== irem ===
= opening_act
    - 
    Irem, The Pillared City # CLASS: location
    You gaze up Irem, the Pillared City. <>
    *   She will rise from the zee[...] and the ice like dawn. She will be garlanded with red and decked with gold. The Seven-Serpent will watch you longingly from its high pedestal. You will always arrive as a stranger, but when you leave, some part of you will always remain.
    Riddlefishers will greet you upon those stony docks; boots clacking upon the Sphinxstone blocks.
    You will lay your head at the House of the Amber Sky; and dream bitter-sweet dreams of forgotten tomorrows and inevitable yesterdays.
    -
    *   You will know where to go. You have always known.
    You find yourself before the Seven-Serpent. Resplendent, Imposing, Spectacular, Luminous. The Hydra of Irem. Adversary of Iolaus and Ninurta. Shadow of Mushmahhu and Bashmu.
    These are lesser names to lesser beings. The stars may wink out, the world may breath its last breath, and yet they shall remain. For they always wait for thee.
    -
    *   The Seven-Serpent will coil atop its pedestal[]. Its ruby eyes will reflect the future. Fate's thread will dangle from its jaws like a mouse's tail – and you will seize it.
    You stand before the Loom of Fate. Weft and Warp, threads of Destiny spreading across your fingertips.
    -
    *   All that remains[...] is to act.
    You gaze upon a kaleidoscope of thread; multitudes of pasts overlapping with potentialities of futures.
    -
    *   You must find you find yourself.[] You must find your past. 
    Who are you?
    How did you get here?
    What future will be so great to be worth the price it took to arrive?
    Ponder.
    Weave.
    ******Remember London
    -> your_lodgings

= returning
You blink. Your lodgings surround you.
You blink. Colour begins to seep.
Memory impeading upon Reality. Reality impeading upon Truth.
You blink.
London fades away.
Irrigo swirls with Violant, Cosmogone battles with Peligin, Apocyan and Viric share tea, Gant swallows all.
A kaleidoscope of colours cascades until a new Reality is formed.
+ [Behold Irem]
    -> loom_of_fate

- (loom_of_fate)
# CLEAR
Irem, The Pillared City # CLASS: location
You stand before the Loom of Fate.
+ Consider Your Future[].
    -> considering_futures
    
+ {player_smen > 0} Consider a Most Destructive Future[].
    {player_smen < 77: To continue down this path would be most folly. To Seek the Name of Eaten is a foolish endeavour. If you do this, all other futures will be closed to you. You will forsake your every action. You will find no quarter in any land. You shall destroy this place.}
    {player_smen == 77: All that remains is to walk the path. There is no alternative.}
    ++ Begin the Rites
        You begin the rituals.
    -> rites_of_smen
    ++ For now, you will go no further
    -> loom_of_fate
    
+ {player_smen != 77} Guide: Consider Your Quest[].
    Your aim is to recall your past. Explore London, the Underzee, the Hinterlands, the hidden corners of the Neath. Grow, make decisions, partake of festivities, and through your existence discover this wonders of this underworld. Away from the prying of eyes of those who stand in Judgement; grow, adapt, morph. The actions of even the smallest beings on the Chain of Being can transmogrify the cosmos. # CLASS: italics
    But take heed, some futures are too terrible to pursue.
    -> loom_of_fate
    
+ Guide: Tales of London - The Loom of Fate
    Actions - You get 2 actions per week.
    'Tales of London' - These are Community-Made Questlines. Interacting with these will not cost you an action.
    Gold - These Choices will cost you 1 action, and increase one of your stats. # CLASS: gold
    Silver - These will appear on Choices related to 'Tales of London'. # CLASS: silver
    Bronze - Nothing yet! But maybe one day. # CLASS: bronze
    Auto-fire Events - Will Activate at anytime that the firing conditions are met.
    Opportunity Deck - Will Activate only at the end of the week, when all your actions for the week have been consumed.
    -> loom_of_fate
    
+ Debug Mode Console
    Note: Activating Debug Mode may cause events to transpire against even the Treachery of Clocks. Things might break. There is no special content beyond this gate. Enter at your own risk # CLASS: italics
    ++  I accept my fate... (Enable Debug Mode)
        ~ debug_mode = true
        -> loom_of_fate
    ++  I will remain on the path... (Disable Debug Mode)
        ~ debug_mode = false
        -> loom_of_fate
        
+ It isn't time yet. Return to London
    You breath deeply, and dive once again into the past.
    -> your_lodgings

= rites_of_smen
- (the_rites_of_smen)
    + {player_item_weeping_scars == 7}
    You present your Weeping Scars.
        -> smen_rite_1
    + {player_item_weeping_scars != 7}
    You present your Weeping Scars. # UNCLICKABLE
        -> smen_rite_1
    + Turn Back
        -> loom_of_fate
- (smen_rite_1)
    + {player_item_stained_soul == 7}
    You present your Stained Soul.
        -> smen_rite_2
    + {player_item_stained_soul != 7}
    You present your Stained Soul. # UNCLICKABLE
        -> smen_rite_2
    + Turn Back
        -> loom_of_fate
- (smen_rite_2)
    + {player_item_memory_of_chains == 7}
    You remember your Memory of Chains.
        -> smen_rite_3
    + {player_item_memory_of_chains != 7}
    You remember your Memory of Chains. # UNCLICKABLE
        -> smen_rite_3
    + Turn Back
        -> loom_of_fate
- (smen_rite_3)
    + {player_item_candle_arthur == 1}
    You light the candle.
        -> smen_rite_4
    + {player_item_candle_arthur != 1}
    You light the candle. # UNCLICKABLE
        -> smen_rite_4
    + Turn Back
        -> loom_of_fate
- (smen_rite_4)
    + {player_item_candle_beau == 1}
    You light the candle.
        -> smen_rite_5
    + {player_item_candle_beau != 1}
    You light the candle. # UNCLICKABLE
        -> smen_rite_5
    + Turn Back
        -> loom_of_fate
- (smen_rite_5)
    + {player_item_candle_cerise == 1}
    You light the candle.
        -> smen_rite_6
    + {player_item_candle_cerise != 1}
    You light the candle. # UNCLICKABLE
        -> smen_rite_6
    + Turn Back
        -> loom_of_fate
- (smen_rite_6)
    + {player_item_candle_destin == 1}
    You light the candle.
        -> smen_rite_7
    + {player_item_candle_destin != 1}
    You light the candle. # UNCLICKABLE
        -> smen_rite_7
    + Turn Back
        -> loom_of_fate
- (smen_rite_7)
    + {player_item_candle_erzulie == 1}
    You light the candle.
        -> smen_rite_8
    + {player_item_candle_erzulie != 1}
    You light the candle. # UNCLICKABLE
        -> smen_rite_8
    + Turn Back
        -> loom_of_fate
- (smen_rite_8)
    + {player_item_candle_fortigan == 1}
    You light the candle.
        -> smen_rite_9
    + {player_item_candle_fortigan != 1}
    You light the candle. # UNCLICKABLE
        -> smen_rite_9
    + Turn Back
        -> loom_of_fate
- (smen_rite_9)
    + {player_item_candle_gawain == 1}
    You light the candle.
        -> smen_rite_10
    + {player_item_candle_gawain != 1}
    You light the candle. # UNCLICKABLE
        -> smen_rite_10
    + Turn Back
        -> loom_of_fate
- (smen_rite_10)
    + {player_smen_reckoning == 1}
    Seek the Name
    This is the height of foolishness.
    ~ the_loom_set = "smen"
        -> a_chosen_future
    + {player_smen_reckoning != 1}
    Seek the Name # UNCLICKABLE
    This is the height of foolishness. 
    ~ the_loom_set = "smen"
        -> a_chosen_future
    + Turn Back
        -> loom_of_fate

= considering_futures
You hold the thread in your hand.
Was this fate yours? Will this fate become your future? Become your Destiny?

// Main Stat Futures
+ {player_watchful == 230} Consider A Watchful Future
    ++  Future_1
        ~ the_loom_set = "watchful_future_1"
    ->  a_chosen_future
    ++  Future_2
        ~ the_loom_set = "watchful_future_2"
    ->  a_chosen_future
+ {player_shadowy == 230} Consider A Shadowy Future
    ++  Future_1
        ~ the_loom_set = "shadowy_future_1"
    ->  a_chosen_future
    ++  Future_2
        ~ the_loom_set = "shadowy_future_2"
    ->  a_chosen_future
+ {player_dangerous == 230} Consider A Dangerous Future
    ++  Future_1
        ~ the_loom_set = "dangerous_future_1"
    ->  a_chosen_future
    ++  Future_2
        ~ the_loom_set = "dangerous_future_2"
    ->  a_chosen_future
+ {player_persuasive == 230} Consider A Persuasive Future
    ++  Future_1
        ~ the_loom_set = "persuasive_future_1"
    ->  a_chosen_future
    ++  Future_2
        ~ the_loom_set = "persuasive_future_2"
    ->  a_chosen_future
    
// Advanced Stat Futures
+ {player_artisan == 7} Consider An Artisan of the Red Science Future
    ++  Future_1
        ~ the_loom_set = "artisan_future_1"
    ->  a_chosen_future
    ++  Future_2
        ~ the_loom_set = "artisan_future_2"
    ->  a_chosen_future
+ {player_chess == 7} Consider A Player of Chess Future
    ++  Future_1
        ~ the_loom_set = "chess_future_1"
    ->  a_chosen_future
    ++  Future_2
        ~ the_loom_set = "chess_future_2"
    ->  a_chosen_future
+ {player_mithridacy == 7} Consider A Mithradic Future
    ++  Future_1
        ~ the_loom_set = "mithradic_future_1"
    ->  a_chosen_future
    ++  Future_2
        ~ the_loom_set = "mithradic_future_2"
    ->  a_chosen_future
+ {player_anatomy == 7} Consider A Monsterous Anatomy Future
    ++  Future_1
        ~ the_loom_set = "anatomy_future_1"
    ->  a_chosen_future
    ++  Future_2
        ~ the_loom_set = "anatomy_future_2"
    ->  a_chosen_future
+ {player_shapeling == 7} Consider A Shapeling Arts Future
    ++  Future_1
        ~ the_loom_set = "shapeling_future_1"
    ->  a_chosen_future
    ++  Future_2
        ~ the_loom_set = "shapeling_future_2"
    ->  a_chosen_future
+ {player_toxicology == 7} Consider A Kataleptic Toxicology Future
    ++  Future_1
        ~ the_loom_set = "toxicology_future_1"
    ->  a_chosen_future
    ++  Future_2
        ~ the_loom_set = "toxicology_future_2"
    ->  a_chosen_future
+ {player_glasswork == 7} Consider A Glasswork Future
    ++  Future_1
        ~ the_loom_set = "glasswork_future_1"
    ->  a_chosen_future
    ++  Future_2
        ~ the_loom_set = "glasswork_future_2"
    ->  a_chosen_future
+ {player_zeefaring == 7} Consider A Zeefaring Future
    ++  Future_1
        ~ the_loom_set = "zeefaring_future_1"
    ->  a_chosen_future
    ++  Future_2
        ~ the_loom_set = "zeefaring_future_2"
    ->  a_chosen_future
+ {player_discordance == 7} Consider A Discordant Future
    ++  Future_1
        ~ the_loom_set = "discordant_future_1"
    ->  a_chosen_future
    ++  Future_2
        ~ the_loom_set = "discordant_future_2"
    ->  a_chosen_future

// Special Futures
+ One Day, we shall weave our fate together. Today is not that day.
    -> loom_of_fate
+ Now is not the time for choosing.
    -> loom_of_fate

= a_chosen_future
You stand before the Loom of Fate.
You hold the thread in your hand.
Out of all the weaves; out of all the colors; out of all the potentialities - you have chosen this one.
{debug_mode == true: Chosen Future: {the_loom_set}} # CLASS: italics
This is your last warning... # CLASS: italics
    + {the_loom_set} All things must end
    -> cop_out
    + Consider other futures once again
    ~ the_loom_set = 0
    ->  considering_futures
    
= cop_out
Thank you for playing the Tales of London - The Loom of Fate!
If you are seeing this, that means that you have decided to take a future from the Loom of Fate.
There is a lot of content that went into making this game, and I didn't want to half-ass the endings. I think they could be especially interesting if a Quirk system was also built into it to enable more variety over choices.
I will come back to Endings after more of the game as been ironed out.
But I am incredibly thankful you made it this far!
If you want to help out in any capacity reach out to any of the creators of this game. We are always looking for more help and inspiration. 
Maybe take a crack at making your own Exceptional Tale!
+[Return to Your Lodgings]
-> your_lodgings