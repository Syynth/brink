=== event_001 ===
Constables at Your Door # CLASS: event
-   Disguised Constables lurk outside your lodgings.
    + You hide your contraband[]. Now it is entirely unfair to have *that* be illegal. Everyone and their mum uses it!
    + You have nothing to hide[]. You are a proper citizen of the law!
-           -> random_event_egress

=== event_002 ===
Rumours of the High Wilderness # CLASS: event
-   A ravid and rabid man scrambles, struggles, and screams through the Bazaar Sidestreets. He screams of stars and suns. Those who stand in Judgement. Of Reckonings.
    + You approach the man[]. He speaks of a gate to the North. Where Horizons meet, sealed, guarded.
    + You ignore the man[].
-   He sights the Special Constables, and slinks away.
         -> random_event_egress

=== event_003 ===
Drinking with Zailors # CLASS: event
-   They speak of places beyond the Zee.
    + [Ask about the Khanate] The Zailor looks at you with beady eyes, "Head North-east, and try to avoid pirates".
    + [Ask about the Carnelion Canal] The Zailor laughs, "Thinking of leaving this hell-hole. Only death lies at the end of that path."
-   You finish your drink and move out of the way as a single-toothed zailor defenestrates himself out a window.
        -> random_event_egress

=== event_004 ===
Attending Sunday Mass # CLASS: event
-   A priest stands upon an altar
    + [Pretend to Pray] You are here for other reasons. Perhaps it is to pickpocket, or tail a target.
    + [Pray] There must be a power somewhere that stands in Judgement over this world. Perhaps they are merciful. Praying couldn't hurt.
-   With all the craziness in this place, it brings up a significant theological debate if this increases or decreases your religious fervor.
        -> random_event_egress

=== event_005 ===
Intercept a Message # CLASS: event
-   For one reason or another, you find yourself in possession of some rather compromising documents.
    + Use them for profit.
    + Use them for good.
-   Whatever purpose these documents once served, it will serve a better purpose now. 
        -> random_event_egress

=== event_006 ===
Slippery Fingers # CLASS: event
-   An Urchin passes by, and suddenly you find your wallet much lighter!
    + [Game Respects Game] What little money you lost, you far make up the difference with some knowledge you could use yourself.
    + [Confront the Urchin] How dare he?! You turn around to confront the vagrant, but they have vanished in the mist.
-   You will remember their face, and the next time you see them, they shall reap your reward.
        -> random_event_egress

=== event_007 ===
A Crate Commotion # CLASS: event
-   Crates Everywhere!
    + [Ehh! Sounds like work] You already have enough on your plate.
    + [To the Hinterlands!] Well...someone will probably pay a handsome price for them.
-   -> random_event_egress

=== event_008 ===
Lost in Shifting Streets # CLASS: event
-   You find yourself beneath a flickering streetlamp. This is the fifth time you've been down this street!
    + [Try the same path again] You try the same path again <>
    + [Cross your eyes and see where it leads you] Well besides nearly twisting your ankle in the cobblestones. You find yourself in a dark alley <>
-   and it leads back to the streetlamp.
    Bugger.
        -> random_event_egress

=== event_009 ===
Lifeburg Sighted! # CLASS: event
-   Off to Zee in search of glory!
    + [Head North]  You find no sight of the Lifeburg.
    + [Head South]  You find no sight of the Lifeburg.
-   Turns out it was East!
        -> random_event_egress

=== event_010 ===
A Devil Wants Your Soul # CLASS: event
-   Yes you can sell your soul!
    + [Sell it]
    + [No thank you I like my soul]
-   Hopefully they will stop bugging you after this.
        -> random_event_egress

=== event_the_infinite_void ===
The Infinite Void # CLASS: event
-   Only in dreams do can we sometimes find understanding.
Time is a void, and darkness is a dream.
You find yourself on the edge of creation.
    + Breath In.
-   This Standard Event will occur in perpetuity # CLASS: italics
~   random_event_standard += Event_The_Infinite_Void
        -> random_event_egress

=== event_null_rare
Event Null Rare # CLASS: event
    + Breath In.
-   This Rare Event will occur in perpetuity # CLASS: italics
~   random_event_rare += Event_Null_Rare
        -> random_event_egress

=== event_null_unusual
Event Null Unusual # CLASS: event
    + Breath In.
-   This Unusual Event will occur in perpetuity # CLASS: italics
~   random_event_unusual += Event_Null_Unusual
        -> random_event_egress

=== event_null_frequent
Event Null Frequent # CLASS: event
    + Breath In.
-   This Frequent Event will occur in perpetuity # CLASS: italics
~   random_event_frequent += Event_Null_Frequent
        -> random_event_egress

=== event_disable_opportunity_deck
Disable Opportunity Deck # CLASS: Event
For one reason or another, you have emptied the Standard Event LIST; likely signifying that you have finished most if not all available events.
This Event can be used to disable all further loading of the Opportunity Deck, so you don't have to continue to playing recurring events.
    + Disable Opportunity Deck
    ~ debug_events_enabled = false
    -> random_event_egress
    + Return to Opportunity Deck
    -> opportunity_deck_hub.event_decision_tree
