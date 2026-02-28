VAR Event_001 = false
VAR Event_002 = false
VAR Event_003 = false
VAR Event_004 = false
VAR Event_005 = false
VAR Event_006 = false
VAR Event_007 = false
VAR Event_008 = false
VAR Event_009 = false
VAR Event_010 = false
VAR Event_Null_Frequent = false
VAR Event_Null_Rare = false
VAR Event_Null_Unusual = false
VAR Event_The_Infinite_Void = false
VAR action_count = false
VAR player_lodging_size = false
VAR random_event_frequent = false
VAR random_event_rare = false
VAR random_event_standard = false
VAR random_event_unusual = false
=== opportunity_deck_hub ===

-> auto_fire_events -> //Auto-fire any events before checking if end of week.


    // Unseen Events: {random_event_list}
    {action_count == 1: ->->}

Opportunity Deck # CLASS: location
What Fortune Falls Upon You? # CLASS: italics

    // Randomly pull events and remove them from the event list
    ~ temp chosen_01_rarity = opportunity_deck_rarity()
    ~ temp chosen_01 = LIST_RANDOM(chosen_01_rarity)

    ~ temp chosen_02_rarity = opportunity_deck_rarity()    
    ~ temp chosen_02 = LIST_RANDOM(chosen_02_rarity)

    ~ temp chosen_03_rarity = opportunity_deck_rarity()
    ~ temp chosen_03 = LIST_RANDOM(chosen_03_rarity)

    ~ temp chosen_04_rarity = opportunity_deck_rarity()
    ~ temp chosen_04 = LIST_RANDOM(chosen_04_rarity)

    ~ temp chosen_05_rarity = opportunity_deck_rarity()
    ~ temp chosen_05 = LIST_RANDOM(chosen_05_rarity)

    ~ temp chosen_06_rarity = opportunity_deck_rarity()
    ~ temp chosen_06 = LIST_RANDOM(chosen_06_rarity)

    ~ temp chosen_07_rarity = opportunity_deck_rarity()
    ~ temp chosen_07 = LIST_RANDOM(chosen_07_rarity)

// The current iteration of the multi-list rarity-based opportunity deck is thus: rarity is randomly decided, rarity GLOBAL LIST is cloned, one event is randomly selected from cloned list. Repeat until all available choice slots have been filled. Upon selection of choice; remove selection from GLOBAL LIST.
// This current iteration causes duplication of events; but no matter what, there shall be an event for each choice slot.

/*
{debug_mode:
Event 1 Rarity: {chosen_01_rarity}
Event 1: {chosen_01}
Event 2 Rarity: {chosen_02_rarity}
Event 2: {chosen_02}
Event 3 Rarity: {chosen_03_rarity}
Event 3: {chosen_03}
Event 4 Rarity: {chosen_04_rarity}
Event 4: {chosen_04}
Event 5 Rarity: {chosen_05_rarity}
Event 5: {chosen_05}
Event 6 Rarity: {chosen_06_rarity}
Event 6: {chosen_06}
Event 7 Rarity: {chosen_07_rarity}
Event 7: {chosen_07}
}
*/

// List # events equal to lodging size
    + {player_lodging_size == 0} // Only when lodging is 0, send back to Your Lodgings.
        -> your_lodgings
    
    + {player_lodging_size >= 1}
    {chosen_01}
        <> {event_title(chosen_01)}
        {event_deletion_from_list(chosen_01_rarity, chosen_01)}
        -> event_router(chosen_01)

    + {player_lodging_size >= 2}
    {chosen_02}
        <> {event_title(chosen_02)}
        {event_deletion_from_list(chosen_02_rarity, chosen_02)}
        -> event_router(chosen_02)

    + {player_lodging_size >= 3}
    {chosen_03}
        <> {event_title(chosen_03)}
        {event_deletion_from_list(chosen_03_rarity, chosen_03)}
        -> event_router(chosen_03)

    + {player_lodging_size >= 4}
    {chosen_04}
        <> {event_title(chosen_04)}
        {event_deletion_from_list(chosen_04_rarity, chosen_04)}
        -> event_router(chosen_04)

    + {player_lodging_size >= 5}
    {chosen_05}
        <> {event_title(chosen_05)}
        {event_deletion_from_list(chosen_05_rarity, chosen_05)}
        -> event_router(chosen_05)

    + {player_lodging_size >= 6}
    {chosen_06}
        <> {event_title(chosen_06)}
        {event_deletion_from_list(chosen_06_rarity, chosen_06)}
        -> event_router(chosen_06)

    + {player_lodging_size >= 7}
    {chosen_07}
        <> {event_title(chosen_07)}
        {event_deletion_from_list(chosen_07_rarity, chosen_07)}
        -> event_router(chosen_07)

=== event_router(selected_event) ===
    {selected_event:
        - Event_001 :
            -> event_001
        - Event_002 :
            -> event_002
        - Event_003 :
            -> event_003
        - Event_004 :
            -> event_004
        - Event_005 :
            -> event_005
        - Event_006 :
            -> event_006
        - Event_007 :
            -> event_007
        - Event_008 :
            -> event_008
        - Event_009 :
            -> event_009
        - Event_010 :
            -> event_010
        - Event_The_Infinite_Void :
            -> event_the_infinite_void
        - Event_Null_Rare :
            -> event_null_rare
        - Event_Null_Unusual :
            -> event_null_unusual
        - Event_Null_Frequent :
            -> event_null_frequent
        - else: Error in Event Router, no event available.
        -> DONE
    }

    -> your_lodgings
    

=== random_event_egress ===
    + [Return to Your Lodgings]
    ->->

=== function opportunity_deck_rarity ===
~ temp rarity_of_event = RANDOM(1, 8)
// Frequent events disabled. Otherwise with current iteration enabling duplication, there would be a spam of frequent events.

{
- rarity_of_event >= 1 and rarity_of_event < 2:
    ~ return random_event_rare
    
- rarity_of_event >= 2 and rarity_of_event < 4:
    ~ return random_event_unusual
    
- rarity_of_event >= 4 and rarity_of_event <= 8:
    ~ return random_event_standard
    
- rarity_of_event >= 9 and rarity_of_event <= 16:
    ~ return random_event_frequent
}

=== function event_deletion_from_list(ref rarity, ref event) ===
{
- rarity == random_event_standard:
~   random_event_standard -= event
- rarity == random_event_unusual:
~   random_event_unusual -= event
- rarity == random_event_rare:
~   random_event_rare -= event
- rarity == random_event_frequent:
~   random_event_frequent -= event
}


//Take selected event and call it

=== function event_title(event) ===
{event == Event_001:
    Constables at Your Door
    }
{event == Event_002:
    Rumours of the High Wilderness
    }
{event == Event_003:
    Drinking with Zailors
    }
{event == Event_004:
    Attending Sunday Mass
    }
{event == Event_005:
    Intercept a Message
    }
{event == Event_006:
    Slippery Fingers
    }
{event == Event_007:
    A Crate Commotion
    }
{event == Event_008:
    Lost in Shifting Streets
    }
{event == Event_009:
    Lifeburg Sighted!
    }
{event == Event_010:
    A Devil Wants Your Soul
    }
{event == Event_The_Infinite_Void:
    The Infinite Void
    }
{event == Event_Null_Rare:
    Event Null Rare
    }
{event == Event_Null_Unusual:
    Event Null Unusual
    }
{event == Event_Null_Frequent:
    Event Null Frequent
    }

=== auto_fire_events ===
-> END

=== event_001 ===
-> END

=== event_002 ===
-> END

=== event_003 ===
-> END

=== event_004 ===
-> END

=== event_005 ===
-> END

=== event_006 ===
-> END

=== event_007 ===
-> END

=== event_008 ===
-> END

=== event_009 ===
-> END

=== event_010 ===
-> END

=== event_null_frequent ===
-> END

=== event_null_rare ===
-> END

=== event_null_unusual ===
-> END

=== event_the_infinite_void ===
-> END

=== your_lodgings ===
-> END
