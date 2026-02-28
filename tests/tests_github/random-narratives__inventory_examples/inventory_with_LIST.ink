// Example of an inventory system using a single list called "Inventory"

-> room


 // In Ink everything defined with the keyword LIST is a multi-valued list, which is actually a set of booleans. The line below defines two things: firstly three new booleans - lamp, sword and skull - and secondly, an immutable set of booleans, called Inventory.
LIST Inventory = lamp, sword, skull

=== function get_item(item)
    // Change the value of the boolean item in the list Inventory to True, without changing the value of other booleans.
    ~ Inventory += item
    // Note : The line below would have set the boolean item to True and set all other booleans in the list Inventory to False, making the player only able to carry one item at a time.
    // ~ Inventory = item

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
You enter a dim room, lit only by an old oil lamp. The light flickers, casting shadows on the stone walls. In the center, a shiny sword rests on a pedestal, its blade reflecting the lamp’s glow.

To the right, a skull sits on a small table, its empty eye sockets staring at you. The skull adds a creepy feeling to the room.

-(opt)

* [Take lamp]
    You take the lamp.
    ~ get_item(lamp)
* [Take sword]
    You take the sword.
    ~ get_item(sword)
* [Take skull]
    You take the skull.
    ~ get_item(skull)
+ [Check inventory]
    ~ display_inventory()
* -> end
-
-> opt

=== end ===
-> END