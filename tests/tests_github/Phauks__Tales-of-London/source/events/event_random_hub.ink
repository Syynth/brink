=== opportunity_deck_hub ===
Opportunity Deck # CLASS: location
What Fortune Falls Upon You? # CLASS: italics

// Randomly pull events and remove them from the event list
~ chosen_01_rarity = opportunity_deck_rarity()
~ chosen_01 = rarity_list_to_pull(chosen_01_rarity)

~ chosen_02_rarity = opportunity_deck_rarity()
~ chosen_02 = rarity_list_to_pull(chosen_02_rarity)

~ chosen_03_rarity = opportunity_deck_rarity()
~ chosen_03 = rarity_list_to_pull(chosen_03_rarity)

~ chosen_04_rarity = opportunity_deck_rarity()
~ chosen_04 = rarity_list_to_pull(chosen_04_rarity)

~ chosen_05_rarity = opportunity_deck_rarity()
~ chosen_05 = rarity_list_to_pull(chosen_05_rarity)

~ chosen_06_rarity = opportunity_deck_rarity()
~ chosen_06 = rarity_list_to_pull(chosen_06_rarity)

~ chosen_07_rarity = opportunity_deck_rarity()
~ chosen_07 = rarity_list_to_pull(chosen_07_rarity)

/*
{debug_mode:
Event Log # CLASS: italics
Event 1: {chosen_01_rarity} ({chosen_01})
Event 2: {chosen_02_rarity} ({chosen_02})
Event 3: {chosen_03_rarity} ({chosen_03})
Event 4: {chosen_04_rarity} ({chosen_04})
Event 5: {chosen_05_rarity} ({chosen_05})
Event 6: {chosen_06_rarity} ({chosen_06})
Event 7: {chosen_07_rarity} ({chosen_07})

Unselected Events # CLASS: italics
Frequent: {random_event_frequent}
Standard: {random_event_standard}
Unusual: {random_event_unusual}
Rare: {random_event_rare}
}
*/


- (event_decision_tree) //Necessary for Opportunities that route back to hub (ex. Disable Opportunity Deck). Prevents reroll of Opportunities.

// List # events equal to lodging size
    + {player_lodging_size == 0} // Only when lodging is 0, send back to Your Lodgings.
        -> your_lodgings
    
    + {player_lodging_size >= 1}
    {chosen_01}
        <> [{event_title(chosen_01)}]
        {event_add_unused_events(chosen_01)}
        -> event_router(chosen_01)

    + {player_lodging_size >= 2}
    {chosen_02}
        <> [{event_title(chosen_02)}]
        {event_add_unused_events(chosen_02)}
        -> event_router(chosen_02)

    + {player_lodging_size >= 3}
    {chosen_03}
        <> [{event_title(chosen_03)}]
        {event_add_unused_events(chosen_03)}
        -> event_router(chosen_03)

    + {player_lodging_size >= 4}
    {chosen_04}
        <> [{event_title(chosen_04)}]
        {event_add_unused_events(chosen_04)}
        -> event_router(chosen_04)

    + {player_lodging_size >= 5}
    {chosen_05}
        <> [{event_title(chosen_05)}]
        {event_add_unused_events(chosen_05)}
        -> event_router(chosen_05)

    + {player_lodging_size >= 6}
    {chosen_06}
        <> [{event_title(chosen_06)}]
        {event_add_unused_events(chosen_06)}
        -> event_router(chosen_06)

    + {player_lodging_size >= 7}
    {chosen_07}
        <> [{event_title(chosen_07)}]
        {event_add_unused_events(chosen_07)}
        -> event_router(chosen_07)
    
    + { (not chosen_01) or (not chosen_02) or (not chosen_03) or (not chosen_04) or (not chosen_05) or (not chosen_06) or (not chosen_07)} // Event activates when Bypass List(Standard) is empty; signifying no new events in current iteration. 
        [Disable Opportunity Deck]
        -> event_disable_opportunity_deck

=== random_event_egress ===
    + [Return to Your Lodgings]
    # CLEAR
    ->->

=== function opportunity_deck_rarity ===
~ temp rarity_of_event = RANDOM(1, 7)
// Frequent Events disabled.

{
- rarity_of_event >= 1 and rarity_of_event < 2:
    ~ return "Rare"
    
- rarity_of_event >= 2 and rarity_of_event < 4:
    ~ return "Unusual"
    
- rarity_of_event >= 4 and rarity_of_event < 8:
    ~ return "Standard"
    
- rarity_of_event >= 8 and rarity_of_event < 16:
    ~ return "Frequent"
- else: Error in Opportunity Deck Rarity Roll. Value outside of bounds. Rolled Value: {opportunity_deck_rarity()}. # CLASS: italics
}

=== function rarity_list_to_pull(ref rarity) ===
~ temp chosen_event = 0
~ temp bypass_destination = "Standard"
{rarity:
- "Rare":
    ~ chosen_event = LIST_RANDOM(random_event_rare)
    ~ random_event_rare -= chosen_event
        
        {not chosen_event: 
        ~ chosen_event = rarity_list_to_pull(bypass_destination)
        ~ rarity = bypass_destination
        } //Bypass if Chosen Event List is Empty
        
    ~ return chosen_event
    
- "Unusual":
    ~ chosen_event = LIST_RANDOM(random_event_unusual)
    ~ random_event_unusual -= chosen_event
    
        {not chosen_event:
        ~ chosen_event = rarity_list_to_pull(bypass_destination)
        ~ rarity = bypass_destination
        } //Bypass if Chosen Event List is Empty
    
    ~ return chosen_event 
    
- "Standard":
    ~ chosen_event = LIST_RANDOM(random_event_standard)
    ~ random_event_standard -= chosen_event
    ~ return chosen_event
    
- "Frequent":
    ~ chosen_event = LIST_RANDOM(random_event_frequent)
    ~ random_event_frequent -= chosen_event

        {not chosen_event:
        ~ chosen_event = rarity_list_to_pull(bypass_destination)
        ~ rarity = bypass_destination
        } //Bypass if Chosen Event List is Empty

    ~ return chosen_event
}

=== function event_addition_from_list(rarity, event) ===
{rarity:
    - "Standard":
        ~   random_event_standard += event

    - "Unusual":
        ~   random_event_unusual += event

    - "Rare":
        ~   random_event_rare += event

    - "Frequent":
        ~   random_event_frequent += event
}

=== function event_add_unused_events(chosen_event) ===
/*
{debug_mode:
Function Debug: Add Unused Events Back to Event Lists
Chosen Event: {chosen_event}
Event 1: {chosen_01_rarity} ({chosen_01})
Event 2: {chosen_02_rarity} ({chosen_02})
Event 3: {chosen_03_rarity} ({chosen_03})
Event 4: {chosen_04_rarity} ({chosen_04})
Event 5: {chosen_05_rarity} ({chosen_05})
Event 6: {chosen_06_rarity} ({chosen_06})
Event 7: {chosen_07_rarity} ({chosen_07})
}
*/

{chosen_event != chosen_01:
    ~ event_addition_from_list(chosen_01_rarity, chosen_01)
}

{chosen_event != chosen_02:
    ~ event_addition_from_list(chosen_02_rarity, chosen_02)
}

{chosen_event != chosen_03:
    ~ event_addition_from_list(chosen_03_rarity, chosen_03)
}

{chosen_event != chosen_04:
    ~ event_addition_from_list(chosen_04_rarity, chosen_04)
}

{chosen_event != chosen_05:
    ~ event_addition_from_list(chosen_05_rarity, chosen_05)
}

{chosen_event != chosen_06:
    ~ event_addition_from_list(chosen_06_rarity, chosen_06)
}

{chosen_event != chosen_07:
    ~ event_addition_from_list(chosen_07_rarity, chosen_07)
}

=== function event_title(event) ===
{event:
    - Event_001:
            Constables at Your Door
    - Event_002:
            Rumours of the High Wilderness
    - Event_003:
            Drinking with Zailors
    - Event_004:
            Attending Sunday Mass
    - Event_005:
            Intercept a Message
    - Event_006:
            Slippery Fingers
    - Event_007:
            A Crate Commotion
    - Event_008:
            Lost in Shifting Streets
    - Event_009:
            Lifeburg Sighted!
    - Event_010:
            A Devil Wants Your Soul
    - Event_The_Infinite_Void:
            The Infinite Void
    - Event_Null_Rare:
            Event Null Rare
    - Event_Null_Unusual:
            Event Null Unusual
    - Event_Null_Frequent:
            Event Null Frequent
    - else:
            Error in Event Title, Event is Unlisted.
}

=== event_router(selected_event) ===
{selected_event:
    - Event_001:
        -> event_001
    - Event_002:
        -> event_002
    - Event_003:
        -> event_003
    - Event_004:
        -> event_004
    - Event_005:
        -> event_005
    - Event_006:
        -> event_006
    - Event_007:
        -> event_007
    - Event_008:
        -> event_008
    - Event_009:
        -> event_009
    - Event_010:
        -> event_010
    - Event_The_Infinite_Void:
        -> event_the_infinite_void
    - Event_Null_Rare:
        -> event_null_rare
    - Event_Null_Unusual:
        -> event_null_unusual
    - Event_Null_Frequent:
        -> event_null_frequent
    - else:
        Error in Event Router, Event has not been routed.
        -> DONE
}