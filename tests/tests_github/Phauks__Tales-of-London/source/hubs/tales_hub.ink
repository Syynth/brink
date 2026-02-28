=== tales_hub ===

{pursuing_an_exceptional_tale == false:

+   {debug_mode == true and not the_oath_of_st_eligius}
    Pursue A Story of Christmas Past # CLASS: silver
    Synopsis: A Museum Robbery has terrible ramifications on London.
    Author: Phauks # CLASS: italics
        ++ Pursue this Tale # CLASS: silver
            ~   pursuing_an_exceptional_tale = true
            ~   the_oath_of_st_eligius = 1
            ->  tales_routing_table
        ++ Some other time...
            -> tales_hub

+   {not a_christmas_venture}
    Tale: Coffee At Christmas # CLASS: silver
    Synopsis: 'Tis the Season! Gather your coffers, its time for some coffee. Work with different factions of London to set up your own coffee shop. Will you make it the height of high-fashion; or a haven for the underclass?
    Author: Phauks # CLASS: italics
        ++ Pursue this Tale # CLASS: silver
            ~   pursuing_an_exceptional_tale = true
            ~   a_christmas_venture = 1
            ->  tales_routing_table
        ++ Some other time...
            -> tales_hub

+   Reset A Tale
    Want to go another round? Choose a Tale and Reset it!
    Note: All qualities associated with the Tale will be lost. # CLASS: italics
        ++  {the_oath_of_st_eligius}
        Reset a Story of Christmas Past
        ~ the_oath_of_st_eligius = 0
            -> tales_hub
        ++  {a_christmas_venture}
        Reset Coffee At Christmas
        ~ a_christmas_venture = 0
            -> tales_hub
        ++  Do Not Reset Any Tales
            -> tales_hub

+   Return to Your Lodgings
        ->  your_lodgings
}

= tales_routing_table
{pursuing_an_exceptional_tale == true:
    # CLEAR

{the_oath_of_st_eligius > 0 and the_oath_of_st_eligius < 777:
    -> a_tale_of_christmas_past
    }
    
{a_christmas_venture > 0 and a_christmas_venture < 777:
    -> a_tale_of_christmas_present
    }
    
    Error: No match in Tale Routing Table
    ~   pursuing_an_exceptional_tale = false
    -> your_lodgings
}