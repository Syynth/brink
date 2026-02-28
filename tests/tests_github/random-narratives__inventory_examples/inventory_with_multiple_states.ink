// Example of an inventory system using a list of states for each object

-> room
// In this example, we define a list for each property we want to track: where is the object, and is the object lit or unlit. The values of each list below are the states that each property can take.
LIST Locations = in_room, in_inventory, in_chest
LIST LitUnlit = lit, unlit

// Each object can have several properties, but we assume only one state is active per property.
// In this example, the lamp cannot be simultaneously both lit and unlit, as this would disrupt some of the script’s logic.
// The values of the lists below are the initial active states for each object.
VAR LampStates = (in_room, lit)
VAR SwordStates = (in_room)
VAR SkullStates = (in_room)


=== function get_item(ref item_states)
    ~ item_states -= LIST_ALL(in_inventory) // remove all location states
    ~ item_states += in_inventory // add state in_inventory to item

=== function put_back_item(ref item_states)
    ~ item_states -= LIST_ALL(in_inventory) // remove all location states
    ~ item_states += in_room // add state in_room to item

=== function display_inventory()
~ temp nb_objects_in_inventory = 0
{LampStates ? in_inventory:
~ nb_objects_in_inventory++
}
{SwordStates ? in_inventory:
~ nb_objects_in_inventory++
}
{SkullStates ? in_inventory:
~ nb_objects_in_inventory++
}

{nb_objects_in_inventory>0:
    You're currently carrying :
    ~ display_item(LampStates, "an old oil lamp")
    ~ display_item(SwordStates, "a shiny sword")
    ~ display_item(SkullStates, "a creepy skull")
- else:
    Your bag is empty.
}

=== function display_item(item_states, item_description)
{item_states ? in_inventory:
    {item_description}
}

=== function set_state(ref item_states, state)
    ~ item_states -= LIST_ALL(state)
    ~ item_states += state

=== room === 
{LampStates ? lit:
You are in a dim room, lit only by {LampStates ? in_room : an old oil lamp}{LampStates ? in_inventory : the lamp you're holding}. The light flickers, casting shadows on the stone walls. {SwordStates ? in_room: In the center, a shiny sword rests on a pedestal, its blade reflecting the lamp’s glow.}

{SkullStates ? in_room: To the right, a skull sits on a small table, its empty eye sockets staring at you. The skull adds a creepy feeling to the room.}
- else:
It's dark. You can't see anything.
}
-(opt)

+ [Look around]
    -> room
+ {LampStates ? in_room} [Take lamp]
    You take the lamp.
    ~ get_item(LampStates)
+ {LampStates ? in_inventory} [Put back lamp]
    You put the lamp back where you've found it.
    ~ put_back_item(LampStates)
+ {LampStates ? (in_inventory, lit)} [Turn off lamp]
    You turn the brass knob, and the flame of the old lamp dwindle to a soft glow before going completely dark.
    ~ set_state(LampStates, unlit)
+ {LampStates ? (in_inventory, unlit)} [Turn on lamp]
    You strike a match and manage to light the lamp again.
    ~ set_state(LampStates, lit)
+ {SwordStates ? in_room} [Take sword]
    You take the sword.
    ~ get_item(SwordStates)
+ {SwordStates ? in_inventory} [Put back sword]
    You put the sword back.
    ~ put_back_item(SwordStates)
+ {SkullStates ? in_room}[Take skull]
    You take the skull.
    ~ get_item(SkullStates)
+ {SkullStates ? in_inventory}[Put back skull]
    You put the skull back on the table.
    ~ put_back_item(SkullStates)
+ [Check inventory]
    ~ display_inventory()
* -> end
-
-> opt

=== end ===
-> END
