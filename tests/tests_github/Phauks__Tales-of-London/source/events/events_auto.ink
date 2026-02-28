=== event_posi_gain
Special Event: POSI # CLASS: event_auto
    You have risen above the ranks of the lower peasants and unwashed masses. Arise, to your newfound status, and develop your skills in more unique and substantial ways.
    You have become 'A Person of Some Importance' # CLASS: italics
    ~ event_posi = true
    + [Accept]
    -> auto_fire_event_egress

=== event_hinterlands_access
Special Event: Opening of the Great Hellbound Railroad # CLASS: event_auto
    You read a bullitin board in the street. Construction has begun on a massive project in the Hinterlands.
    This could be of great use to you.
    The Great Hellbound Railroad is now open! # CLASS: italics
    ~ discovered_hinterlands = true
    + [Accept]
    -> auto_fire_event_egress

=== event_adulterine_castle_access
Special Event: Adulterine Castle Entrance # CLASS: event_auto
    Nothing changes. No path opens. No guardians await your presence in a place that does not exist.
    You cannot travel to the Adulterine Castle! # CLASS: italics
    ~ discovered_adulterine_castle = true
    + [Accept]
    -> auto_fire_event_egress

=== event_smen_well
Special Event: SMEN # CLASS: event_auto
    Event Description
    To partake of this path would be folly. Do you listen to the voice in the well?
    + Listen
        A Reckoning cannot be postponed...
        You light the first candle.
        ~ player_smen = 1
        ++ [Accept]
        -> auto_fire_event_egress
    + Forsake this foolishness
        What madness could drive you towards such destructive behaviour...
        ++ [Accept]
        -> auto_fire_event_egress

=== event_smen_weeping_scars
A Weeping Scars # CLASS: event_auto
    If there be a lesson to this madness, let it ring. If there be an answer to this hunger, let it be satiated. There is a darkness, creeping, like a twisted vine, upon your personage. What little you gain through this folly will not expunge your treason.
    + Scars Repeatable to 7
    ~ stat_changer("Weeping Scars", player_item_weeping_scars, 7)
    ++ [Accept]
    ->  auto_fire_event_egress

=== event_smen_stained_soul
A Stain Upon Your Soul # CLASS: event_auto
    If there be a lesson to this madness, let it ring. If there be an answer to this hunger, let it be satiated. There is a darkness, creeping, like a twisted vine, upon your personage. What little you gain through this folly will not expunge your treason.
    + Soul Repeatable to 7
    ~ stat_changer("Stained Soul", player_item_stained_soul, 7)
    ++ [Accept]
    ->  auto_fire_event_egress

=== event_smen_memory_of_chains
A Memory of Chains # CLASS: event_auto
    If there be a lesson to this madness, let it ring. If there be an answer to this hunger, let it be satiated. There is a darkness, creeping, like a twisted vine, upon your personage. What little you gain through this folly will not expunge your treason.
    + Chains Repeatable to 7
    ~ stat_changer("Memory of Chains", player_item_memory_of_chains, 7)
    ++ [Accept]
    ->  auto_fire_event_egress

=== event_smen_candle_arthur
St Arthur's Candle # CLASS: event_auto
    If there be a lesson to this madness, let it ring. If there be an answer to this hunger, let it be satiated. There is a darkness, creeping, like a twisted vine, upon your personage. What little you gain through this folly will not expunge your treason.
    ~ stat_changer("St. Arthurs Candle", player_item_candle_arthur, 1)
    + [Accept]
    ->  auto_fire_event_egress

=== event_smen_candle_beau
St Beau's Candle # CLASS: event_auto
    If there be a lesson to this madness, let it ring. If there be an answer to this hunger, let it be satiated. There is a darkness, creeping, like a twisted vine, upon your personage. What little you gain through this folly will not expunge your treason.
    ~ stat_changer("St. Beau's Candle", player_item_candle_beau, 1)
    + [Accept]
    ->  auto_fire_event_egress

=== event_smen_candle_cerise
St Cerise's Candle # CLASS: event_auto
    If there be a lesson to this madness, let it ring. If there be an answer to this hunger, let it be satiated. There is a darkness, creeping, like a twisted vine, upon your personage. What little you gain through this folly will not expunge your treason.
    ~ stat_changer("St. Cerise's Candle", player_item_candle_cerise, 1)
    + [Accept]
    ->  auto_fire_event_egress

=== event_smen_candle_destin
St Destin's Candle # CLASS: event_auto
    If there be a lesson to this madness, let it ring. If there be an answer to this hunger, let it be satiated. There is a darkness, creeping, like a twisted vine, upon your personage. What little you gain through this folly will not expunge your treason.
    ~ stat_changer("St. Destin's Candle", player_item_candle_destin, 1)
    + [Accept]
    ->  auto_fire_event_egress

=== event_smen_candle_erzulie
St. Erzulie's Candle # CLASS: event_auto
    If there be a lesson to this madness, let it ring. If there be an answer to this hunger, let it be satiated. There is a darkness, creeping, like a twisted vine, upon your personage. What little you gain through this folly will not expunge your treason.
    ~ stat_changer("St. Erzulie's Candle", player_item_candle_erzulie, 1)
    + [Accept]
    ->  auto_fire_event_egress

=== event_smen_candle_fortigan
St. Fortigan's Candle # CLASS: event_auto
    If there be a lesson to this madness, let it ring. If there be an answer to this hunger, let it be satiated. There is a darkness, creeping, like a twisted vine, upon your personage. What little you gain through this folly will not expunge your treason.
    ~ stat_changer("St. Fortigan's Candle", player_item_candle_fortigan, 1)
    + [Accept]
    ->  auto_fire_event_egress

=== event_smen_candle_gawain
St. Gawain's Candle # CLASS: event_auto
    If there be a lesson to this madness, let it ring. If there be an answer to this hunger, let it be satiated. There is a darkness, creeping, like a twisted vine, upon your personage. What little you gain through this folly will not expunge your treason.
    ~ stat_changer("St. Gawain's Candle", player_item_candle_gawain, 1)
    + [Accept]
    ->  auto_fire_event_egress

=== event_smen_indefinite_reckoning
A Reckoning Cannot Be Postponed Indefinitely # CLASS: event_auto
    The way is clear. There is nothing that stands between you and a destructive future. In fact, it may already be too late.
    Head to the place, where all things must end. Reap what you have sown, against all warnings.
    ~ stat_changer("An Inevitable Reckoning", player_smen_reckoning, 1)
    + [Accept]
    ->  auto_fire_event_egress
    
// Main Gains
=== event_gain_watchful_1
Watchful Gains 1 # CLASS: event_auto
    A Rip in the Void. A Tearing of the Flesh. The Searing of the Mind. The Awakening of the Spirit. You are not as you were. No, you are far greater, and achieve even higher heights.
    ~ event_gains_watchful = 1
    Your Max Watchful is now {200 + (15 * event_gains_watchful)} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

=== event_gain_watchful_2
Watchful Gains 2 # CLASS: event_auto
    A Rip in the Void. A Tearing of the Flesh. The Searing of the Mind. The Awakening of the Spirit. You are not as you were. No, you are far greater, and achieve even higher heights.
    ~ event_gains_watchful = 2
    Your Max Watchful is now {200 + (15 * event_gains_watchful)} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress


=== event_gain_dangerous_1
Dangerous Gains 1 # CLASS: event_auto
    A Rip in the Void. A Tearing of the Flesh. The Searing of the Mind. The Awakening of the Spirit. You are not as you were. No, you are far greater, and achieve even higher heights.
    ~ event_gains_dangerous = 1
    Your Max Dangerous is now {200 + (15 * event_gains_dangerous)} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

=== event_gain_dangerous_2
Dangerous Gains 2 # CLASS: event_auto
    A Rip in the Void. A Tearing of the Flesh. The Searing of the Mind. The Awakening of the Spirit. You are not as you were. No, you are far greater, and achieve even higher heights.
    ~ event_gains_dangerous = 2
    Your Max Dangerous is now {200 + (15 * event_gains_dangerous)} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress
    
=== event_gain_shadowy_1
Shadowy Gains 1 # CLASS: event_auto
    A Rip in the Void. A Tearing of the Flesh. The Searing of the Mind. The Awakening of the Spirit. You are not as you were. No, you are far greater, and achieve even higher heights.
    ~ event_gains_shadowy = 1
    Your Max Shadowy is now {200 + (15 * event_gains_shadowy)} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

=== event_gain_shadowy_2
Shadowy Gains 2 # CLASS: event_auto
    A Rip in the Void. A Tearing of the Flesh. The Searing of the Mind. The Awakening of the Spirit. You are not as you were. No, you are far greater, and achieve even higher heights.
    ~ event_gains_shadowy = 2
    Your Max Shadowy is now {200 + (15 * event_gains_shadowy)} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

=== event_gain_persuasive_1
Persuasive Gains 1 # CLASS: event_auto
    A Rip in the Void. A Tearing of the Flesh. The Searing of the Mind. The Awakening of the Spirit. You are not as you were. No, you are far greater, and achieve even higher heights.
    ~ event_gains_persuasive = 1
    Your Max Persuasive is now {200 + (15 * event_gains_persuasive)} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

=== event_gain_persuasive_2
Persuasive Gains 2 # CLASS: event_auto
    A Rip in the Void. A Tearing of the Flesh. The Searing of the Mind. The Awakening of the Spirit. You are not as you were. No, you are far greater, and achieve even higher heights.
    ~ event_gains_persuasive = 2
    Your Max Persuasive is now {200 + (15 * event_gains_persuasive)} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

// Advanced Gains
=== event_gain_artisan_1
Artisan of the Red Science Gains 1 # CLASS: event_auto
    Your knowledge of the Red Science has grown to extraordinary heights. Now, the laws of the world shall bend to your pleasure.
    ~ event_gains_artisan = 1
    Your Max Artisan of the Red Science is now {5 + event_gains_artisan} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

=== event_gain_artisan_2
Artisan of the Red Science Gains 2 # CLASS: event_auto
    Your knowledge of the Red Science has grown to extraordinary heights. Now, the laws of the world shall bend to your pleasure.
    ~ event_gains_artisan = 2
    Your Max Artisan of the Red Science is now {5 + event_gains_artisan} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

=== event_gain_anatomy_1
Monsterous Anatomy Gains 1 # CLASS: event_auto
    There is a fear that few men or monsters have achieved. Now, you join the ranks of those primordial entities.
    ~ event_gains_anatomy = 1
    Your Max Monsterous Anatomy is now {5 + event_gains_anatomy} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

=== event_gain_anatomy_2
Monsterous Anatomy Gains 2 # CLASS: event_auto
    There is a fear that few men or monsters have achieved. Now, you join the ranks of those primordial entities.
    ~ event_gains_anatomy = 2
    Your Max Monsterous Anatomy is now {5 + event_gains_anatomy} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

=== event_gain_chess_1
Player of Chess Gains 1 # CLASS: event_auto
    You shall wish it, and it shall be. The gates of castles and the crowns of kings shall be yours.
    ~ event_gains_chess = 1
    Your Max Player of Chess is now {5 + event_gains_chess} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

=== event_gain_chess_2
Player of Chess Gains 1 # CLASS: event_auto
    You shall wish it, and it shall be. The gates of castles and the crowns of kings shall be yours.
    ~ event_gains_chess = 1
    Your Max Player of Chess is now {5 + event_gains_chess} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

=== event_gain_glasswork_1
Glasswork Gains 1 # CLASS: event_auto
    There is a world beyond the glass; and though many will call it a stranger, you shall call upon it as a friend; and it shall be your home across the veil.
    ~ event_gains_glasswork = 1
    Your Max Glasswork is now {5 + event_gains_glasswork} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

=== event_gain_glasswork_2
Glasswork Gains 2 # CLASS: event_auto
    There is a world beyond the glass; and though many will call it a stranger, you shall call upon it as a friend; and it shall be your home across the veil.
    ~ event_gains_glasswork = 2
    Your Max Glasswork is now {5 + event_gains_glasswork} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

=== event_gain_mithridacy_1
Mithridacy Gains 1 # CLASS: event_auto
    Speak, and be heard. Your voice speaks no ills nor lies. It drips with honeyed silver and dances with truth.
    ~ event_gains_mithridacy = 1
    Your Max Mithridacy is now {5 + event_gains_mithridacy} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

=== event_gain_mithridacy_2
Mithridacy Gains 2 # CLASS: event_auto
    Speak, and be heard. Your voice speaks no ills nor lies. It drips with honeyed silver and dances with truth.
    ~ event_gains_mithridacy = 2
    Your Max Mithridacy is now {5 + event_gains_mithridacy} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

=== event_gain_shapeling_1
Shapeling Arts Gains 1 # CLASS: event_auto
    The world is changing, and so shall you with it. Behold, intangability, as a beauty, and behold greatness. You shall change, and be ever victorious.
    ~ event_gains_shapeling = 1
    Your Max Shapeling Arts is now {5 + event_gains_shapeling} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

=== event_gain_shapeling_2
Shapeling Arts Gains 2 # CLASS: event_auto
    The world is changing, and so shall you with it. Behold, intangability, as a beauty, and behold greatness. You shall change, and be ever victorious.
    ~ event_gains_shapeling = 2
    Your Max Shapeling Arts is now {5 + event_gains_shapeling} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

=== event_gain_toxicology_1
Kataleptic Toxicology Gains 1 # CLASS: event_auto
    Lies, deception, the destruction of the body. Now you have achieved a state where the toxins of reality will no longer hinder you. Rise up, champion, at top the pillars of your studies.
    ~ event_gains_toxicology = 1
    Your Max Kataleptic Toxicology is now {5 + event_gains_toxicology} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

=== event_gain_toxicology_2
Kataleptic Toxicology Gains 2 # CLASS: event_auto
    Lies, deception, the destruction of the body. Now you have achieved a state where the toxins of reality will no longer hinder you. Rise up, champion, at top the pillars of your studies.
    ~ event_gains_toxicology = 2
    Your Max Kataleptic Toxicology is now {5 + event_gains_toxicology} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

=== event_gain_zeefaring_1
Zeefaring Gains 1 # CLASS: event_auto
    The world is large, and vast. The waves, treacherous, and untameable. Yet you shall glide upon those lapping waves, and they shall yield to you. All the secret shallows, all the darkest depths; none shall be beyond your grasp.
    ~ event_gains_zeefaring = 1
    Your Max Zeefaring is now {5 + event_gains_zeefaring} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

=== event_gain_zeefaring_2
Zeefaring Gains 2 # CLASS: event_auto
    The world is large, and vast. The waves, treacherous, and untameable. Yet you shall glide upon those lapping waves, and they shall yield to you. All the secret shallows, all the darkest depths; none shall be beyond your grasp.
    ~ event_gains_zeefaring = 2
    Your Max Zeefaring is now {5 + event_gains_zeefaring} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

=== event_gain_discordance_1
Steward of the Discordance Gains 1 # CLASS: event_auto
    Only through loss do we realize what we have. Only through deception can we learn the value of truth. There is a price that had to be paid.
    You have paid the price, and gained nothing in return. No one applauds your efforts, no one beholds the heights you have achieved. This errand had no value. And there is nothing left to achieve.
    ~ event_gains_discordance = 1
    Your Max Steward of the Discordance is now {5 + event_gains_discordance} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

=== event_gain_discordance_2
Steward of the Discordance Gains 2 # CLASS: event_auto
    Only through loss do we realize what we have. Only through deception can we learn the value of truth. There is a price that had to be paid.
    You have paid the price, and gained nothing in return. No one applauds your efforts, no one beholds the heights you have achieved. This errand had no value. And there is nothing left to achieve.
    ~ event_gains_discordance = 2
    Your Max Steward of the Discordance is now {5 + event_gains_discordance} # CLASS: italics
    + [Accept]
    -> auto_fire_event_egress

// Festivals
=== event_festival_january
Turn of the Century 1899 # CLASS: event_auto
    A new year is upon us. A new year shall always be upon us. We shall circle the 20th century like vultures to a carcass; but will eternally await the lions to leave their feast.
    ~ stat_changer("Lodging Size", player_lodging_size, 1)
    + Celebrate
    -> auto_fire_event_egress
    
=== event_festival_february
Feast of the Rose # CLASS: event_auto
    Love is in the air. Grow a rose. Behold a garden. Perchance you shall find a bride or groom.
    ~ stat_changer("Lodging Size", player_lodging_size, 1)
    + Celebrate
    -> auto_fire_event_egress

=== event_festival_may
Whitsun # CLASS: event_auto
    You do not like green eggs and ham. And thankfully, we only have to deal with Monsterous Eggs in this festivity.
    ~ stat_changer("Lodging Size", player_lodging_size, 1)
    + Celebrate
    -> auto_fire_event_egress

=== event_festival_july
Estival # CLASS: event_auto
    One year it was an earth-shattering festivity. The next, a whole craze about Egyptology. After that, a spat with the Starved Men.
    Honestly, planning the banners and decorations for this is such a pain in the ass.
    ~ stat_changer("Lodging Size", player_lodging_size, 1)
    + Celebrate
    -> auto_fire_event_egress

=== event_festival_august
The Fruits of the Zee # CLASS: event_auto
    Fishing, fishing, and more fishing. Perhaps you shall dive, and meet the Queen of the Zee who resides in the depths beneath the waters.
    ~ stat_changer("Lodging Size", player_lodging_size, 1)
    + Celebrate
    -> auto_fire_event_egress

=== event_festival_october
Hallowmas # CLASS: event_auto
    This is Hallowmas, this is hallowmas, pumpkins scream in the dead of night.
    ~ stat_changer("Lodging Size", player_lodging_size, 1)
    + Celebrate
    -> auto_fire_event_egress

=== event_festival_december
Christmas in the Neath # CLASS: event_auto
    Time is fleeting, the year draws to a close. Mr. Sacks makes the rounds, and leaves gifts for good boys and girls.
    Behold the Lacre, and do not wake the pigs.
    ~ stat_changer("Lodging Size", player_lodging_size, 1)
    + Celebrate
    -> auto_fire_event_egress

// Other
=== event_demo_bypass
Special Event: That's All Folks! # CLASS: event_auto
    {information_display()}
    Thank you for trying the tutorial of Tales of London: The Loom of Fate!
    I hope that you will help flesh out this game!
    You can help in any small capacity.
    Add your own Airs of London!
    Add your own Opportunity Deck Events!
    Add your own multi-storylet questlines!
    Or, even help to build out the Main and Advanced Quality storylines!
    Recommendations are more than welcome!
    This isn't The Neath. # CLASS: italics
    This is Your Neath. # CLASS: italics
    ->DONE