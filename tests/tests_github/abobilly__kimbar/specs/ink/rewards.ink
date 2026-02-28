// Kim Bar - Reward Dialogues
// Outfit acquisition and reward celebration dialogues
// Compiled with: npm run compile:ink
//
// Tag Reference:
//   speaker:Name           - Set speaker name in dialogue box
//   portrait:id            - Set portrait image
//   give:item_id           - Give item/outfit
//   quest:set flag=value   - Set a story flag

// ============================================================
// OUTFIT ACQUISITION - Generic reward dialogues
// ============================================================

=== outfit_acquired ===
# speaker:System
# sfx:reward_fanfare
You've earned a new outfit!
Check your wardrobe in the Robing Room to equip it.
-> END

=== all_justices_defeated ===
# speaker:Court Clerk
# portrait:npc.clerk_01
# sfx:achievement_unlock
Counselor! You've defeated all nine Supreme Court Justices!
The path to the Records Vault is now open.
# quest:set all_justices_complete=true
The ultimate challenge—and the Supreme Robe—await you there.
*   [I'm ready!]
    -> END
*   [I need to prepare first.]
    -> END

=== supreme_robe_acquired ===
# speaker:Chief Justice Roberts
# portrait:npc.justice_roberts:impressed
# give:supreme_robe
# quest:set supreme_champion=true
# sfx:ultimate_victory
Counselor, you have demonstrated mastery of all legal domains.
This Supreme Robe is the highest honor we can bestow.
You are now a true champion of the law.
*   [Thank you, Chief Justice.]
    The honor is ours, counselor. Now go forth and practice law with excellence.
    -> END

// ============================================================
// INDIVIDUAL OUTFIT REWARDS - Called after justice victories
// ============================================================

=== reward_evidence_blazer ===
# speaker:Justice Thomas
# portrait:npc.justice_thomas
# give:evidence_blazer
# quest:set has_evidence_blazer=true
This Evidence Blazer grants +1 hint in all encounters.
Use it wisely, counselor.
-> END

=== reward_conlaw_robe ===
# speaker:Chief Justice Roberts
# portrait:npc.justice_roberts
# give:conlaw_robe
# quest:set has_conlaw_robe=true
The Con Law Robe grants +5 seconds of extra time.
The Constitution rewards patience.
-> END

=== reward_civpro_suit ===
# speaker:Justice Kagan
# portrait:npc.justice_kagan
# give:civpro_suit
# quest:set has_civpro_suit=true
This Civ Pro Power Suit grants +1 strike.
Procedural armor for any litigation.
-> END

=== reward_property_vest ===
# speaker:Justice Gorsuch
# portrait:npc.justice_gorsuch
# give:property_vest
# quest:set has_property_vest=true
The Property Vest grants +10% citation bonus.
Real property, real rewards.
-> END

=== reward_contracts_tie ===
# speaker:Justice Kavanaugh
# portrait:npc.justice_kavanaugh
# give:contracts_tie
# quest:set has_contracts_tie=true
This Contracts Tie grants +1 hint.
A binding commitment to your success.
-> END

=== reward_family_cardigan ===
# speaker:Justice Barrett
# portrait:npc.justice_barrett
# give:family_cardigan
# quest:set has_family_cardigan=true
The Family Law Cardigan grants +5 seconds extra time.
Warm comfort for complex matters.
-> END

=== reward_criminal_jacket ===
# speaker:Justice Alito
# portrait:npc.justice_alito
# give:criminal_jacket
# quest:set has_criminal_jacket=true
This Criminal Law Jacket grants +1 strike.
Protection against wrongful answers.
-> END

=== reward_torts_blazer ===
# speaker:Justice Jackson
# portrait:npc.justice_jackson
# give:torts_blazer
# quest:set has_torts_blazer=true
The Torts Blazer grants +10% citation bonus.
Damages well calculated.
-> END

=== reward_crimpro_badge ===
# speaker:Justice Sotomayor
# portrait:npc.justice_sotomayor
# give:crimpro_badge
# quest:set has_crimpro_badge=true
This Crim Pro Badge grants +1 hint.
The rights of the prepared are protected.
-> END
