// Simple Jungle Adventure - Ink Draft
// Based on GlitchCanary Map and Isometric Images

// Start the player at the crash site
-> Crash_Site_Clearing

=== Crash_Site_Clearing ===
IMAGE: crash_site_clearing.jpg
You stand amidst the wreckage of a small plane in a jungle clearing. Moss covers twisted metal. Paths lead East and South into the dense foliage.
+ [Go East] -> Overgrown_Path
+ [Go South] -> Dark_Cave

=== Overgrown_Path ===
IMAGE: overgrown_path_jungle.jpg
This path is narrow, crowded by large leaves and thick vines. You can barely see the way forward. The path continues East and South, or you can return West.
+ [Go West] -> Crash_Site_Clearing
+ [Go East] -> Snake_Pit
+ [Go South] -> Control_Room

=== Snake_Pit ===
IMAGE: snake_pit_chamber.jpg
You enter a low-walled area built from stone blocks. Several large, golden snakes slither across the floor. You see exits West and South.
+ [Go West] -> Overgrown_Path
+ [Go South] -> Ancient_Grove

=== Dark_Cave ===
IMAGE: dark_cave_entrance.jpg
The air grows cool and damp as you step into a dark cave entrance. Water drips somewhere within. You can go South deeper into the cave system, or North back to the crash site.
+ [Go North] -> Crash_Site_Clearing
+ [Go South] -> Waterfall_Base

=== Control_Room ===
IMAGE: control_room_altar.jpg
This stone chamber houses a strange, humming pedestal topped with a glowing orb. Ancient markings cover the walls. A heavy red door blocks the way South. The path North leads back outside.
// Add logic later for the button/pedestal and Red Door
+ [Go North] -> Overgrown_Path
+ [Go South] -> River_Ledge // For now, assume the door opens or is bypassed

=== Ancient_Grove ===
IMAGE: ancient_grove_tree.jpg
An ancient, gnarled tree dominates this quiet grove. Blue, shimmering locks block doorways to the sides. A large snake rests coiled near the tree's base. Paths lead North and South.
// Add logic later for blue locks
+ [Go North] -> Snake_Pit
+ [Go South] -> Hidden_Shrine

=== Waterfall_Base ===
IMAGE: waterfall_base_grotto.jpg
Water cascades down a mossy cliff face into a wide, shallow pool. The spray is refreshing. The only way forward seems to be North, back into the cave.
+ [Go North] -> Dark_Cave

=== River_Ledge ===
IMAGE: river_ledge_view.jpg
You emerge onto a narrow stone ledge overlooking a wide, flowing river below. It seems you came through the red door North. The ledge continues South.
+ [Go North] -> Control_Room
+ [Go South] -> River_Crossing

=== Hidden_Shrine ===
IMAGE: hidden_shrine_waterfall.jpg
Tucked away behind foliage is a small, weathered stone shrine beside a gentle cascade of water. Tiny lights like fireflies drift in the air. The path leads North.
+ [Go North] -> Ancient_Grove

=== River_Crossing ===
IMAGE: river_crossing_logs.jpg
Rough-hewn logs form a precarious bridge across the flowing river. They look slippery. You can attempt to cross South, or return North to the ledge.
+ [Go North] -> River_Ledge
+ [Cross the logs South] -> Far_Shore

=== Far_Shore ===
IMAGE: far_shore_ruins.jpg
You made it across the river! This shore is rocky and damp. You see an entranceway leading East into more ruins. You can head back North across the log bridge.
+ [Cross back North] -> River_Crossing
+ [Go East] -> Ancient_Vault

=== Ancient_Vault ===
IMAGE: ancient_vault_treasure.jpg
Success! This chamber glitters with heaps of gold coins and large, cut gems. Two imposing chests stand against the far wall, locked with red symbols. The only exit is West.
// Add logic later for chests
+ [Leave West] -> Far_Shore
+ [Admire the treasure]
    It's an incredible sight! -> Ancient_Vault // Stay in the room