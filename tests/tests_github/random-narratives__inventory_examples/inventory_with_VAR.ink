// Example of an inventory system using an Inventory multi-list and a Room multi-list

-> room

// In Ink everything defined with the keyword LIST is a multi-valued list, which is actually a set of booleans. The line below defines two things: firstly three new booleans - lamp, sword and skull - and secondly, an immutable set of booleans, called Objects that we're not going to use in this example.
LIST Objects = lamp, sword, skull

// The lines below define a mutable set called Inventory, and a mutable set called Room. It's possible to append or remove booleans from those.
VAR Inventory = () // Empty set
VAR Room = (lamp, sword, skull) // Set containing the booleans lamp, sword and skull. Those booleans have to be defined in a list, that's why we defined the list Objects in this example.

=== function get_item(item)
    // Append the boolean item to the set Inventory, and remove it form the set Room.
    ~ Inventory += item
    ~ Room -= item

=== function put_back_item(item)
    // Append the boolean item to the set Room, and remove it form the set Inventory.
    ~ Room += item
    ~ Inventory -= item

=== function display_inventory()
// Note: a list can be tested "as it is", and will return true, unless it's empty.
{Inventory:
    You're currently carrying : <>
    ~ display_item(lamp, "an old oil lamp")
    ~ display_item(sword, "a shiny sword")
    ~ display_item(skull, "a creepy skull")
    <>.
- else:
    Your bag is empty.
}

=== function display_item(item, item_description)
{Inventory ? item:
    {LIST_MIN(Inventory)==item:
        // if it's first item in the inventory
        {item_description}<>
    -else:
        // if not the first item we add a comma before the description
        , {item_description}<>
    }
}
=== room === 
// Because we track what objects are in the room, we can add in this example a Look around choice. We can also make all choices sticky, and add the options to put objects back.
You are in a dim room, lit only by {Room ? lamp : an old oil lamp}{Inventory ? lamp : the lamp you're holding}. The light flickers, casting shadows on the stone walls. {Room ? sword: In the center, a shiny sword rests on a pedestal, its blade reflecting the lamp’s glow.}

{Room ? skull: To the right, a skull sits on a small table, its empty eye sockets staring at you. The skull adds a creepy feeling to the room.}

-(opt)

+ [Look around]
    -> room
+ {Room ? lamp} [Take lamp]
    You take the lamp.
    ~ get_item(lamp)
+ {Inventory ? lamp} [Put back lamp]
    You put the lamp back where you've found it.
    ~ put_back_item(lamp)
+ {Room ? sword} [Take sword]
    You take the sword.
    ~ get_item(sword)
+ {Inventory ? sword} [Put back sword]
    You put the sword back.
    ~ put_back_item(sword)
+ {Room ? skull}[Take skull]
    You take the skull.
    ~ get_item(skull)
+ {Inventory ? skull}[Put back skull]
    You put the skull back on the table.
    ~ put_back_item(skull)
+ [Check inventory]
    ~ display_inventory()
* -> end
-
-> opt

=== end ===
-> END
