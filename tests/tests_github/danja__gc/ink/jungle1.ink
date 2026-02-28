
-> gcstart

== gcstart ==

// Start the story at the Portal Chamber
* [Enter portal] -> Portal_Chamber

=== Portal_Chamber ===
// Image 1
You are in a dark room dominated by a large, rectangular portal humming with neon blue and pink light. Strange symbols pulse along its frame. The air crackles faintly.
+ [Step through the portal] -> Clearing_with_Doors

=== Clearing_with_Doors ===
// Image 2
You emerge into a grassy clearing surrounded by dense jungle foliage. Small, glowing blue plants dot the ground. Two simple arched doorways stand ahead.
+ [Enter the left doorway] -> Waterfall_Shrine
+ [Enter the right doorway] -> Grassy_Knoll_Entrance
+ [Step back towards the portal area] -> Portal_Chamber

=== Waterfall_Shrine ===
// Image 3
This small stone structure is built into a mossy embankment. A gentle waterfall cascades down one side into a clear pool. The air is damp and cool.
+ [Follow the water downstream (imagined path)] -> Lower_Waterfall_Cave
+ [Go back to the clearing] -> Clearing_with_Doors

=== Moonlit_Ledge ===
// Image 4
You stand on a stone platform built high on a cliff face. Below, water reflects the bright moonlight. It's quiet and somewhat eerie. A passage leads into the cliff.
+ [Enter the cliff passage] -> Ravine_Bridge
+ [Look for a way down the cliff] -> Red_Cliff_Ledge

=== Ravine_Bridge ===
// Image 5
Inside the cliff, a rickety wooden bridge spans a dark, deep ravine. Torches flicker, casting long shadows. The sound of dripping water echoes.
+ [Cross the bridge carefully] -> Serpent_Cave
+ [Go back to the moonlit ledge] -> Moonlit_Ledge

=== Crash_Site ===
// Image 6
In a gloomier part of the jungle, you find the wreckage of a small airplane half-buried in the earth. Glowing plants grow around it. A path leads up a rocky incline.
+ [Follow the path upwards] -> Red_Cliff_Ledge
+ [Head towards a grassy slope] -> Grassy_Knoll_Entrance

=== Serpent_Cave ===
// Image 7
This cave chamber glows with an orange light. Large, snake-like golden patterns are etched into the floor, shimmering faintly. Keys lie scattered among them.
+ [Follow a tunnel deeper into the caves] -> Golden_Hoard_Cave
+ [Go back across the bridge] -> Ravine_Bridge

=== Grassy_Knoll_Entrance ===
// Image 8
You are on a gentle, grassy slope. A single stone doorway is set into the side of the hill, similar to the ones in the clearing.
+ [Enter the hillside door] -> Crash_Site
+ [Go back towards the main clearing] -> Clearing_with_Doors

=== Red_Cliff_Ledge ===
// Image 9
You're on a ledge of reddish rock under a sunset sky. A glowing key symbol pulses near the edge. Paths lead along the cliff face and downwards.
+ [Follow the path towards a cave entrance] -> Blue_Gate_Cave
+ [Take the path leading down] -> Crash_Site
+ [Look for a way up towards the moonlight] -> Moonlit_Ledge

=== Golden_Hoard_Cave ===
// Image 10
A dazzling pile of golden coins and artifacts fills this part of the cave, illuminated by an unseen light source. An arched doorway stands at the back.
+ [Examine the arched doorway] -> Blue_Gate_Cave
+ [Return to the serpent room] -> Serpent_Cave

=== Blue_Gate_Cave ===
// Image 11
This section of the cave network has dark red rock walls. A distinct, glowing blue energy gate blocks a passage.
+ [Approach the Green Key area] -> Key_Platform
+ [Go back towards the Golden Hoard] -> Golden_Hoard_Cave
+ [Head towards the Red Cliff exit] -> Red_Cliff_Ledge

=== Lower_Waterfall_Cave ===
// Image 12
Water cascades brightly from an opening high above, falling into a pool in this cave. The light refracts through the spray.
+ [Look for a way up towards the shrine] -> Waterfall_Shrine
+ [Follow a passage towards a platform] -> Key_Platform

=== Key_Platform ===
// Image 13
You stand on a raised stone platform surrounded by greenery. A glowing green key symbol hovers above a pedestal. An archway leads out.
+ [Take the passage towards the Blue Gate] -> Blue_Gate_Cave
+ [Follow the sound of water] -> Lower_Waterfall_Cave

// No explicit END marker needed if the story is just exploration.
// If there was an end goal, you might add a condition or a final knot.