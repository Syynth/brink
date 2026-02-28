// ***************************************************************
// PREFACE
// ***************************************************************
VAR WorldSetting =      "This story takes place in \"Regio Scientiae: A Land of Understanding.\" Its a world of magic and invention. The story is a Choose Your Own Adventure (CYOA) which means You are an reader and active Player in story. The narratives can be done solo, but they are best experienced the first time as playful interaction with the Master Scoy (game master and teacher) and other real life Players (your classmates). NOTE: I do my best to make it functional and interesting, but if you encounter dead ends or have story suggestions - let me know!"
VAR CYOAPurpose =       "Purpose of the series..."
CONST HabitsOfMind  =   "way of thinking"
CONST HabitsOfMindEG =  "Curiosity, Openness to New Ideas, Critical Thinking, Perseverance, Creative Thinking, Adaptability, Self-direction, Integrity, and Growth Mindset."
CONST Curiosity =       ""
CONST Openness =        "" // Openness to New Ideas
CONST CriticalThinking = ""
CONST Perseverance =    ""
CONST CreativeThinking = ""
CONST Adaptability =    ""
CONST SelfDirection =   ""
CONST Integrity =       ""
CONST GrowthMindset =   ""

// ***************************************************************
// 5 Room Dungeon
// ***************************************************************
VAR RoomOne =   "ROOM ONE: Gain Entrance" // or Guardian
VAR RoomTwo =   "ROOM TWO: Puzzle or Role-play Challenge"
VAR RoomThree = "ROOM THREE: Setback" // or Trick
VAR RoomFour =  "ROOM FOUR: Big Battle" // or Climax or Conflict
VAR RoomFive =  "ROOM FIVE: Revelation" // or Reward or Plot Twist

// ***************************************************************
// MAIN CHARACTERS
// ***************************************************************
VAR Your = "Your"
VAR You = "You"
VAR Cleric =        "CLERIC"
    VAR TheCleric = "The cleric"
    VAR theCleric = "the cleric"
VAR Fighter =       "FIGHTER"
    VAR TheFighter =    "The fighter"
    VAR theFighter =    "the fighter"
VAR Rogue =         "ROGUE"
    VAR TheRogue =  "The rogue"
    VAR theRogue =  "the rogue"
VAR Wizard =        "WIZARD"
    VAR TheWizard = "The wizard"
    VAR theWizard = "the wizard"

// ***************************************************************
// Main NPCs
// ***************************************************************
VAR Innkeeper =         "Fin"
VAR Maid =              "Jane"
VAR MaidTwo =           "Suzie"
VAR DwarfSmith =        "Cedalion" // Say-dale-yon // a mostly friendly blacksmithing dwarf in Canis Villa or KABEIROI
VAR ForgeMaster =       "Hephaestus"
VAR EvilMage  =         "Malum Magus" // Ma-loom
VAR Queen =             "Queen Justina"
VAR NobleFamily =       "Noord Vlakte"
VAR Farmer1 =           "James"
VAR Farmer2 =           "Robert"
VAR Villager1 =         "Dan"
VAR Boy1    =           "Timothy"
VAR Girl1   =           "Becca"
VAR Castellan =         "Mr. Hillenar"
VAR FarmBoy =           "Timothy"
VAR ProfessorL =        "Professor Leyder" // a.k.a Leidenbrock
VAR Axel =              "Axel"


// ***************************************************************
// LOCATIONS
// ***************************************************************
// VILLAGES & CITIES
VAR village00 = "Canis Villa"
VAR village40 = "Yellow Wharf"
VAR village41 = "Dagger Town"
VAR village42 = "Verdant Bay"
VAR village5 = "Berwickshire"
VAR DwarfCity = "Iron Forge"
VAR CapitalCity = "Storm Crown"

VAR BlueMug = "The Blue Mug"
VAR SeaTavern = "Salty Carp"
VAR inn2 = "Inn 2"


// ***************************************************************
// CASTLES & TOWERS
VAR CastleOne =       "Clun Castle"
VAR CastleTwo =       "Muilder Slot"
VAR CelestialTower =  "Celestial Tower"
VAR MoonTower =       "Lunae Tower"
VAR SunTower =        "Helios Tower"
VAR MarsTower =       "Nergal Tower"

// ***************************************************************
// REGIONS
VAR region1 = "Calda Vale"
VAR region2 = "Noord Vlaktes" // North Plains
VAR region3 = ""
VAR region4 = ""
VAR region5 = ""
VAR region6 = ""
VAR region7 = ""
VAR region8 = "8"
VAR region9 = ""
VAR region10 = ""
VAR BearIslands = "Bear Islands" // Manitou Islands

// ***************************************************************
// MOUNTAINS
VAR mountains1 = "Misty Mountain"
VAR mountains2 = "Mountain 2"
VAR mountains3 = "Mountain 3"
VAR mountains4 = "Mountain 4"
VAR mountains5 = "Mountain 5"
VAR volcano1 = "Snarfell"

// ***************************************************************
// DESERT
VAR desert1 = "Arrakis"
VAR desert2 = "Sonoran"
VAR desert3 = ""

// FORESTS
VAR forest1 = ""
VAR forest2 = ""
VAR forest3 = ""

// ***************************************************************
// GRASSLANDS
VAR grassland1 = ""
VAR grassland2 = ""
VAR grassland3 = ""

// ***************************************************************
// TUNDRA
VAR tundra1 = ""
VAR tundra2 = ""
VAR tundra3 = ""

// ***************************************************************
// WATER: RIVERS & LAKES
VAR river1 = "River 1"
VAR river2 = "River 2"
VAR river3 = "River 3"
VAR river4 = "River 4"
VAR river5 = "River 5"

// ***************************************************************
VAR lake1 = "Hicks Lake"
VAR lake2 = "Big Lake"
VAR lake3 = "Lake Mishigami"
VAR lake4 = "Lake 4"
VAR lake5 = "Lake 5"

// ***************************************************************
// WATER: Sea
VAR NorthSea = "North Sea"
VAR SouthSea = "South Sea"
VAR WestSea = "West Sea"
VAR EastSea = "East Sea"
VAR Sea5 = "Sea of Falling Stars"

// ***************************************************************
// MONSTERS
// ***************************************************************
VAR BanditStats =       "BANDIT AC: 12 HP: 11"
  VAR BanditAction1 =     "SCIMITAR +3 to hit, reach 5 ft., one target. Hit: 4 (1d6 + 1) slashing damage."
  VAR BanditAction2 =     "LIGHT CROSSBOW +3 to hit, range 80 ft./320 ft., one target. Hit: 5 (1d8 + 1) piercing damage."
VAR Drow =              ""
  VAR DrowAction1 =              ""
  VAR DrowAction2 =              ""
VAR CaveBearStats = "AC: HP:"
  VAR CaveBearAction1 = "BITE"
  VAR CaveBearAction2 = "CLAW"
VAR AirElementalStats = ""
  VAR AirElementalAction1 = ""
  VAR AirElementalAction2 = ""
VAR FireElementalStats =""
  VAR FireElementalAction1 = ""
  VAR FireElementalAction2 = ""
VAR EarthElementalStats = ""
  VAR EarthElementalAction1 = ""
  VAR EarthElementalAction2 = ""
VAR WaterElementalStats = ""
  VAR WaterElementalAction1 = ""
  VAR WaterElementalAction2 = ""
VAR SkeletonStats = ""
  VAR SkeletonAction1 = ""
  VAR SkeletonAction2 = ""
VAR SpectreStats = ""
  VAR SpectreAction1 = ""
  VAR SpectreAction2 = ""

// ***************************************************************
// GAME MECHANICS
// ***************************************************************
CONST Initiative = "Everyone in combat or near combat, rolls a d20 to determine the order of each player's actions. For example, if PLAYER ONE rolls a 5 and PLAYER TWO rolls a 15, then PLAYER TWO performs his or her action before PLAYER TWO. Monsters also roll initiative."
CONST SavingThrow = ""
CONST DCcheck = "difficulty check"

VAR CombatStart = "An encounter has started. What do you do?"
VAR Combat = "A combat encounter is a fight between two sides. It might involve a flurry of weapon swings, punches, kicks, blocks (parries), spellcasting, or other actions. Combat is organized into a series of rounds and turns. During a round, each player takes a turn deciding what action(s) to take. The order of turns is determined when everyone rolls initiative (d20) at the very beginning of the combat encounter. Once everyone has taken a turn the round is over. Combat continues to the next round if neither side wins, flees, or ends the combat in some way."
VAR CombatTurns = "Combat Step by Step:  (1) Determine Surprise. The GM determines whether anyone involved in the combat encounter is surprised. (2) Establish positions: The GM decides where all the characters and Monsters are located. Given the adventurers’ Marching Order or their stated positions in the room or other location, the GM figures out where the adversaries are̶how far away and in what direction. (3) Roll Initiative: Everyone involved in the combat encounter rolls Initiative, determining the order of combatants’ turns. (4) Take turns. Each participant in the battle takes a turn in Initiative order. (5) Begin the next round. When everyone involved in the combat has had a turn, the round ends. Repeat step 4 until the fighting stops."
VAR CombatEnd = "The encounter is complete."

===CombatScene===
// TODO: copy and paste "CombatScene" template into a story as needed.
- (combat1)

// TODO: Add a specific monster here
- {d4()+1} MONSTERS come at you.
    // https://www.dndbeyond.com/
    // https://www.dndbeyond.com/

- {CombatStart}
- You:
    + "Fight!"
    + "Flee!"
        -- You attempt to run, but the the party isn't fast enough.
    + "Can we talk about this?"
        -- You attempt to talk.
//        TODO: Add a custom response
        -- <> Diplomacy doesn't seem to be the best option in this situation. Creature gains advantage on its first attack.
- Roll Initiative.
    + Proceed to fight.
    + Combat Encounter HOW-TO:
        -- {Combat}
            ++ [next]
        -- {CombatTurns}
            ++ Proceed to fight.

- \**************************
Combat actions by individuals and each party are determined LIVE in class and not scripted into the story.
\**************************
    + [next]
- The fight is over.
    + SUCCESS[!]
        ->postcombat
    + FAILURE[!]
        ->death
//        TODO: Add a specific death message
        -- (death) <>
        -- You have died.
        ++[next]
//        TODO: Change to a specific Start Menu

//            --> StartMenu
- (postcombat)
- Congratulations on your victory!
    + [next]
// TODO: Make this specific to the story
- Next part of story...

-> DONE
// ***************************************************************
// MOVEMENT
// ***************************************************************
VAR MoveStealth =       "move along trying be undetected"
VAR MoveWalk =          "walk"
VAR MoveHustle =        "move briskly"
VAR MoveRun =           "run"


// ***************************************************************
// DICE ROLLS
// ***************************************************************
//~ temp d4 = RANDOM (1,4)
//~ temp d6 = RANDOM (1,6)
//~ temp d8 = RANDOM (1,8)
//~ temp d10 = RANDOM (1,10)
//~ temp d12 = RANDOM (1,12)
//~ temp d20 = RANDOM (1,20)
//~ temp d100 = RANDOM (1,100)

=== function d4() ===
~ return RANDOM(1, 4)

=== function d6() ===
~ return RANDOM(1, 6)

=== function d8() ===
~ return RANDOM(1, 8)

=== function d10() ===
~ return RANDOM(1, 10)

=== function d12() ===
~ return RANDOM(1, 12)

=== function d20() ===
~ return RANDOM(1, 20)

=== function d100() ===
~ return RANDOM(1, 100)

// ***************************************************************
// INVENTORY VARIABLES
// ***************************************************************

LIST inventory = (none), flint_and_steel

LIST kits = (zero), ClimbingKit, HerbalistKit

LIST weapons = fists, dagger

LIST armor = (none), leather, chain_mail, plate_armor

LIST healing = (none), potion_of_minor_healing

LIST specialItems = (none), AirEssence, EarthEssence, FireEssence, WaterEssence

LIST ore = (none), CopperOre, IronOre, MeteoriteOre, ThoriumOre, PalladiumOre



/*
 Copper Ore
 Tin Ore
 Iron Ore
 Mithril Ore
 Thorium Ore
 Fel Iron Ore
 Adamantite Ore
 Cobalt Ore
 Saronite Ore
 Obsidium Ore
 Elementium Ore
 Ghost Iron Ore
 Blackrock Ore
 True Iron Ore
 Felslate
 Leystone Ore
 Empyrium Ore
 Monelite Ore
 Storm Silver Ore
*/


VAR ore30 = "meteorite ore"
VAR ore31 = "palladium ore"
VAR ore32 = "thorium ore"

/*
01 Acanthite (cooled polymorph of Argentite): Ag2S for production of silver
02 Barite: BaSO4
03 Bauxite Al(OH)3 and AlOOH, dried to Al2O3 for production of aluminum
04 Beryl: Be3Al2(SiO3)6
05 Bornite: Cu5FeS4
06 Cassiterite: SnO2
07 Chalcocite: Cu2S for production of copper
08 Chalcopyrite: asdCuFeS2
09 Chromite: (Fe, Mg)Cr2O4 for production of chromium
10 Cinnabar: HgS for production of mercury
11 Cobaltite: (Co, Fe)AsS
12 Columbite-Tantalite or Coltan: (Fe, Mn)(Nb, Ta)2O6
13 Dolomite: CaMg(CO3)2
14 Galena: PbS
15 Native gold: Au, typically associated with quartz or as placer deposits
16 Hematite: Fe2O3
17 Ilmenite: FeTiO3
18 Magnetite: Fe3O4
20 Malachite: Cu2CO3(OH)2
21 Molybdenite: MoS2
22 Pentlandite: (Fe, Ni)9S8
23 Pyrolusite: MnO2
24 Scheelite: CaWO4
25 Sperrylite: PtAs2 for production of platinum
26 Sphalerite: ZnS
27 Uraninite (pitchblende): UO2 for production of metallic uranium
28 Wolframite: (Fe, Mn)WO4
*/


// ***************************************************************
// SPECIAL ITEMS
// ***************************************************************

VAR EpicWeapon = "the Sickle of Cronus"
VAR EpicWeaponAbout = "The Sickle of Cronus is used to battle the beast in the Time Tombs. It can only be crafted at the forge of Hephaestus. The hearth must be crafted with special bricks. The hearth bricks are from a special type of rock only found deep in the planet's interior where players battle an earth elemental and fire elemental. The fire is started and maintained from the essence of a fire elemental. The bellows must be filled with the essence of an air elemental. The ore of the sickle comes from a meteorite. The sickle blade must be quenched in oil and water when complete though that water must come from the essence of a water elemental."

VAR GlacialRiverMelt = "It would stand to reason that glacial melt will be running lowest in the morning and highest in afternoon and early evening"

LIST DeathQuotes = (none)

VAR DeathQuote1 = "I have always depended on the kindness of strangers. ~ Tennessee Williams"
VAR DeathQuote2 = "Do or do not, there is no try. ~ Master Yoda"

VAR DeathQuote3 = "I get knocked down, but I get up again. I get knocked down, but I get up again. ~ Chumbawamba"

// ***************************************************************
// MAP COLORS and SETTINGS
// ***************************************************************
WONDER DRAFT SETTINGS
- Water Texture: Worn
- Ground Texture: Worn
- FX Blends: slider to the 'h'
- Coastline Style: Irregular Blend

OVERLAY
- Regional & World SIZE 32 (1 hex = 5 miles)
- Local & Interior SIZE (1 square = 5 feet)

COLORS
// Good for local green grass
Grass:
  COPIC Grass Green YG17
  RGB (114,193,86)
  HEX #72c156

  HEX #a9c79f

// Good for map overview
Forest:
  COPIC
  RGB (39,78,19, 255)
  HEX #274e13

  SYMBOL SCALES
  20% for Regional & World Scale
  50% for Regional Scale
  100% for Local Scale

GROUND LIGHT BROWN
  HEX #d8b48f

HUT
#43 for round huts (25% for regional)

// ***************************************************************
// SPELLS
// ***************************************************************
VAR ContinualFlame = "Continual Flame: LEVEL 2nd; CASTING TIME: 1 Action RANGE/AREA Touch COMPONENTS V, S, M * DURATION Until Dispelled SCHOOL Evocation ATTACK/SAVE None DAMAGE/EFFECT Creation A flame, equivalent in brightness to a torch, springs forth from an object that you touch. The effect looks like a regular flame, but it creates no heat and doesn't use oxygen. A continual flame can be covered or hidden but not smothered or quenched. * - (ruby dust worth 50 gp, which the spell consumes)"

// ***************************************************************
// WEAPONS
// ***************************************************************

// END OF GLOBAL VARIABLES

=== GlobalVariables ===
-> DONE
