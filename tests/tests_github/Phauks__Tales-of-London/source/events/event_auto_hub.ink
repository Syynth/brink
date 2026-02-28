=== auto_fire_events ===

// Person of Some Importance Gain
+   {not event_posi and player_watchful >= 100 and player_shadowy >= 100 and player_dangerous >= 100 and player_persuasive >= 100}
        ->  event_posi_gain
    
// Discovered Hinterlands
+   {not discovered_hinterlands and player_watchful >= 200 and player_shadowy >= 200 and player_dangerous >= 200 and player_persuasive >= 200}
        ->  event_hinterlands_access

// Discovered Adulterine Castle
+   {not discovered_adulterine_castle and (player_item_breath_void == 1 or player_item_masters_blood == 1 or player_item_reported_location == 1 or player_item_impossible_theorem == 1 or player_item_veils_velvet == 1 or player_item_rumourmongers_network == 1 or player_item_fluke_core == 1 or player_item_tasting_flight == 1)}
        -> event_adulterine_castle_access
    
// Main Stat Gains
+   {player_watchful >= 200 and event_gains_watchful < 1 and discovered_hinterlands == true}
        ->  event_gain_watchful_1
+   {player_watchful >= 215 and event_gains_watchful < 2}
        ->  event_gain_watchful_2
    
+   {player_persuasive >= 200 and event_gains_persuasive < 1 and discovered_hinterlands == true}
        ->  event_gain_persuasive_1
+   {player_persuasive >= 215 and event_gains_persuasive < 2}
        ->  event_gain_persuasive_2
    
+   {player_dangerous >= 200 and event_gains_dangerous < 1 and discovered_hinterlands == true}
        ->  event_gain_dangerous_1
+   {player_dangerous >= 215 and event_gains_dangerous < 2}
        ->  event_gain_dangerous_2

+   {player_shadowy >= 200 and event_gains_shadowy < 1 and discovered_hinterlands == true}
        ->  event_gain_shadowy_1
+   {player_shadowy >= 215 and event_gains_shadowy < 2}
        ->  event_gain_shadowy_2

// Advanced Stat Gains
+   {player_artisan >= 5 and event_gains_artisan < 1 and discovered_hinterlands == true}
        ->  event_gain_artisan_1
+   {player_artisan >= 6 and event_gains_artisan < 2}
        ->  event_gain_artisan_2

+   {player_anatomy >= 5 and event_gains_anatomy < 1 and discovered_hinterlands == true}
        ->  event_gain_anatomy_1
+   {player_anatomy >= 6 and event_gains_anatomy < 2}
        ->  event_gain_anatomy_2

+   {player_chess >= 5 and event_gains_chess < 1 and discovered_hinterlands == true}
        ->  event_gain_chess_1
+   {player_chess >= 6 and event_gains_chess < 2}
        ->  event_gain_chess_2

+   {player_glasswork >= 5 and event_gains_glasswork < 1 and discovered_hinterlands == true}
        ->  event_gain_glasswork_1
+   {player_glasswork >= 6 and event_gains_glasswork < 2}
        ->  event_gain_glasswork_2

+   {player_mithridacy >= 5 and event_gains_mithridacy < 1 and discovered_hinterlands == true}
        ->  event_gain_mithridacy_1
+   {player_mithridacy >= 6 and event_gains_mithridacy < 2}
        ->  event_gain_mithridacy_2

+   {player_shapeling >= 5 and event_gains_shapeling < 1 and discovered_hinterlands == true}
        ->  event_gain_shapeling_1
+   {player_shapeling >= 6 and event_gains_shapeling < 2}
        ->  event_gain_shapeling_2

+   {player_toxicology >= 5 and event_gains_toxicology < 1 and discovered_hinterlands == true}
        ->  event_gain_toxicology_1
+   {player_toxicology >= 6 and event_gains_toxicology < 2}
        ->  event_gain_toxicology_2

+   {player_zeefaring >= 5 and event_gains_zeefaring < 1 and discovered_hinterlands == true}
        ->  event_gain_zeefaring_1
+   {player_zeefaring >= 6 and event_gains_zeefaring < 2}
        ->  event_gain_zeefaring_2
    
+   {player_discordance >= 5 and event_gains_discordance < 1}
        ->  event_gain_discordance_1
+   {player_discordance >= 6 and event_gains_discordance < 2}
        ->  event_gain_discordance_2

// Quest Events


// Festivals. Always trigger on the 2nd of the Month
+   {timer_month >= 1 and timer_week >= 2 and not event_festival_january}
        ->  event_festival_january
+   {timer_month >= 2 and timer_week >= 2 and not event_festival_february}
        ->  event_festival_february
+   {timer_month >= 5 and timer_week >= 2 and not event_festival_may}
        ->  event_festival_may
+   {timer_month >= 7 and timer_week >= 2 and not event_festival_july}
        ->  event_festival_july
+   {timer_month >= 8 and timer_week >= 2 and not event_festival_august}
        ->  event_festival_august
+   {timer_month >= 10 and timer_week >= 2 and not event_festival_october}
        ->  event_festival_october
+   {timer_month >= 12 and timer_week >= 2 and not event_festival_december}
        ->  event_festival_december

// SMEN
+   {timer_month >= 2 and timer_week >= 3 and not event_smen_well}
        ->  event_smen_well
+   {player_smen >= 7 and not event_smen_weeping_scars}
        ->  event_smen_weeping_scars
+   {player_smen >= 14 and not event_smen_stained_soul}
        ->  event_smen_stained_soul
+   {player_smen >= 21 and not event_smen_memory_of_chains}
        ->  event_smen_memory_of_chains
+   {player_smen >= 28 and not event_smen_candle_arthur}
        ->  event_smen_candle_arthur
+   {player_smen >= 35 and not event_smen_candle_beau}
        ->  event_smen_candle_beau
+   {player_smen >= 42 and not event_smen_candle_cerise}
        ->  event_smen_candle_cerise
+   {player_smen >= 49 and not event_smen_candle_destin}
        ->  event_smen_candle_destin
+   {player_smen >= 56 and not event_smen_candle_erzulie}
        ->  event_smen_candle_erzulie
+   {player_smen >= 63 and not event_smen_candle_fortigan}
        ->  event_smen_candle_fortigan
+   {player_smen >= 70 and not event_smen_candle_gawain}
        ->  event_smen_candle_gawain
+   {player_smen == 77 and not event_smen_indefinite_reckoning}
        ->  event_smen_indefinite_reckoning

// Fallback back to call point
+ ->->


=== auto_fire_event_egress ===
+   [Finish this Event]
    # CLEAR
-> auto_fire_events
