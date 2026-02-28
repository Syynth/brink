// Kim Bar - Main Story
// Compiled with: npm run compile:ink
//
// Tag Reference:
//   speaker:Name           - Set speaker name in dialogue box
//   portrait:id            - Set portrait image (e.g., portrait:npc.clerk_01)
//   portrait:id:emotion    - Set portrait with emotion (e.g., portrait:npc.clerk_01:happy)
//   sfx:sound_name         - Play sound effect
//   quest:set flag=value   - Set a story flag
//   quest:get flag         - Log flag value (debug)
//   encounter:deckTag=X count=N rewardId=Y  - Start encounter
//   give:item_id           - Give item/outfit
//   save                   - Save game

=== court_clerk_intro ===
# speaker:Court Clerk
# portrait:npc.clerk_01
# sfx:paper_shuffle
Welcome to the Supreme Court, counselor. Are you here to suffer… I mean, study?
*   [Yes. Hit me.] -> clerk_ready
*   [Not today.] -> clerk_later

=== clerk_ready ===
# speaker:Court Clerk
# portrait:npc.clerk_01
# quest:set court_intro=true
Excellent! The Justices await in their chambers. Each one guards a domain of legal knowledge.
# sfx:gavel_tap
Defeat them in flashcard combat to earn powerful outfits. Good luck, counselor.
-> END

=== clerk_later ===
# speaker:Court Clerk
# portrait:npc.clerk_01
Take your time. The law isn't going anywhere... unfortunately.
-> END

=== clerk_intro ===
// Alias for backwards compatibility
-> court_clerk_intro

=== evidence_challenge ===
# speaker:Court Clerk
# portrait:npc.clerk_01
Ready to test your knowledge of Evidence?
*   [Bring it on!]
    # encounter:deckTag=evidence count=3 rewardId=evidence_blazer
    -> END
*   [Not yet...]
    Come back when you're ready.
    -> END

// ============================================================
// JUSTICE ENCOUNTERS
// ============================================================

// Justice Encounters from justices.ink

// ============================================================
// JUSTICE THOMAS - Evidence
// ============================================================

=== justice_thomas_intro ===
# speaker:Justice Thomas
# portrait:npc.justice_thomas
# sfx:gavel_tap
Counselor. You dare challenge my command of the Rules of Evidence?
The hearsay exceptions alone have broken better lawyers than you.
*   [I'm ready, Your Honor.]
    # encounter:deckTag=evidence count=5 rewardId=evidence_blazer
    -> END
*   [Not yet...]
    Return when you've studied. The Federal Rules don't forgive ignorance.
    -> END

=== justice_thomas_victory ===
# speaker:Justice Thomas
# portrait:npc.justice_thomas:impressed
# give:evidence_blazer
# quest:set defeated_thomas=true
# sfx:victory_fanfare
Impressive. You know your hearsay from your admissions.
Wear this Evidence Blazer with pride, counselor.
-> END

=== justice_thomas_defeat ===
# speaker:Justice Thomas
# portrait:npc.justice_thomas:stern
# sfx:defeat_sound
Objection sustained. Study the Federal Rules and return.
-> END

// ============================================================
// CHIEF JUSTICE ROBERTS - Constitutional Law
// ============================================================

=== justice_roberts_intro ===
# speaker:Chief Justice Roberts
# portrait:npc.justice_roberts
# sfx:gavel_tap
Welcome to the highest court in the land, counselor.
Do you understand the Constitution well enough to argue before me?
*   [I'm prepared, Chief Justice.]
    # encounter:deckTag=constitutional_law count=7 rewardId=conlaw_robe
    -> END
*   [I need more preparation...]
    Wise. The Constitution rewards careful study.
    -> END

=== justice_roberts_victory ===
# speaker:Chief Justice Roberts
# portrait:npc.justice_roberts:impressed
# give:conlaw_robe
# quest:set defeated_roberts=true
# sfx:victory_fanfare
Outstanding constitutional analysis, counselor.
This Con Law Robe marks you as a true scholar of the founding document.
-> END

=== justice_roberts_defeat ===
# speaker:Chief Justice Roberts
# portrait:npc.justice_roberts:stern
# sfx:defeat_sound
The Constitution is not so easily mastered. Study the amendments and return.
-> END

// ============================================================
// JUSTICE SOTOMAYOR - Criminal Procedure
// ============================================================

=== justice_sotomayor_intro ===
# speaker:Justice Sotomayor
# portrait:npc.justice_sotomayor
# sfx:gavel_tap
Counselor, criminal procedure is the shield of liberty.
Do you know when a search becomes unreasonable?
*   [Test me, Your Honor.]
    # encounter:deckTag=criminal_procedure count=5 rewardId=crimpro_badge
    -> END
*   [Let me review Miranda first...]
    A prudent choice. The Fourth and Fifth Amendments await.
    -> END

=== justice_sotomayor_victory ===
# speaker:Justice Sotomayor
# portrait:npc.justice_sotomayor:impressed
# give:crimpro_badge
# quest:set defeated_sotomayor=true
# sfx:victory_fanfare
Excellent work, counselor. You understand the rights of the accused.
This Crim Pro Badge is yours.
-> END

=== justice_sotomayor_defeat ===
# speaker:Justice Sotomayor
# portrait:npc.justice_sotomayor:concerned
# sfx:defeat_sound
Remember: the Constitution protects the accused. Study harder.
-> END

// ============================================================
// JUSTICE KAGAN - Civil Procedure
// ============================================================

=== justice_kagan_intro ===
# speaker:Justice Kagan
# portrait:npc.justice_kagan
# sfx:paper_shuffle
Ah, civil procedure. The rules that make litigation possible.
Personal jurisdiction, venue, Erie doctrine... ready to test your knowledge?
*   [Bring it on!]
    # encounter:deckTag=civil_procedure count=5 rewardId=civpro_suit
    -> END
*   [I need to review the FRCP...]
    Fair enough. Those rules aren't going anywhere.
    -> END

=== justice_kagan_victory ===
# speaker:Justice Kagan
# portrait:npc.justice_kagan:impressed
# give:civpro_suit
# quest:set defeated_kagan=true
# sfx:victory_fanfare
Impressive procedural knowledge, counselor!
This Civ Pro Power Suit will serve you well in any courtroom.
-> END

=== justice_kagan_defeat ===
# speaker:Justice Kagan
# portrait:npc.justice_kagan:stern
# sfx:defeat_sound
The Federal Rules are complex. Review Erie and come back.
-> END

// ============================================================
// JUSTICE GORSUCH - Property
// ============================================================

=== justice_gorsuch_intro ===
# speaker:Justice Gorsuch
# portrait:npc.justice_gorsuch
# sfx:gavel_tap
Property law, counselor. The foundation of civilization itself.
Estates, future interests, the Rule Against Perpetuities...
*   [I've studied the estates.]
    # encounter:deckTag=property count=5 rewardId=property_vest
    -> END
*   [The Rule Against Perpetuities still confuses me...]
    It confuses everyone. But study it you must.
    -> END

=== justice_gorsuch_victory ===
# speaker:Justice Gorsuch
# portrait:npc.justice_gorsuch:impressed
# give:property_vest
# quest:set defeated_gorsuch=true
# sfx:victory_fanfare
Well done! You understand property rights as well as any.
This Property Vest is your reward.
-> END

=== justice_gorsuch_defeat ===
# speaker:Justice Gorsuch
# portrait:npc.justice_gorsuch:stern
# sfx:defeat_sound
Fee simple, life estate, remainder... study them all again.
-> END

// ============================================================
// JUSTICE KAVANAUGH - Contracts
// ============================================================

=== justice_kavanaugh_intro ===
# speaker:Justice Kavanaugh
# portrait:npc.justice_kavanaugh
# sfx:paper_shuffle
Contracts, counselor. Offer, acceptance, consideration.
The UCC and common law dance together. Ready?
*   [I accept the challenge.]
    # encounter:deckTag=contracts count=5 rewardId=contracts_tie
    -> END
*   [Let me review formation first...]
    Consideration is key. Come back when you've thought it through.
    -> END

=== justice_kavanaugh_victory ===
# speaker:Justice Kavanaugh
# portrait:npc.justice_kavanaugh:impressed
# give:contracts_tie
# quest:set defeated_kavanaugh=true
# sfx:victory_fanfare
A binding performance, counselor!
This Contracts Tie symbolizes your mastery.
-> END

=== justice_kavanaugh_defeat ===
# speaker:Justice Kavanaugh
# portrait:npc.justice_kavanaugh:stern
# sfx:defeat_sound
Breach of preparation. Review the Restatement and return.
-> END

// ============================================================
// JUSTICE BARRETT - Family Law
// ============================================================

=== justice_barrett_intro ===
# speaker:Justice Barrett
# portrait:npc.justice_barrett
# sfx:gavel_tap
Family law touches the most personal aspects of life, counselor.
Marriage, divorce, custody, support. Are you ready?
*   [I'm ready, Your Honor.]
    # encounter:deckTag=family_law count=5 rewardId=family_cardigan
    -> END
*   [This area requires more study...]
    Family matters deserve careful attention. Take your time.
    -> END

=== justice_barrett_victory ===
# speaker:Justice Barrett
# portrait:npc.justice_barrett:impressed
# give:family_cardigan
# quest:set defeated_barrett=true
# sfx:victory_fanfare
Compassionate and competent, counselor.
This Family Law Cardigan is yours.
-> END

=== justice_barrett_defeat ===
# speaker:Justice Barrett
# portrait:npc.justice_barrett:concerned
# sfx:defeat_sound
Family law requires sensitivity and knowledge. Study more.
-> END

// ============================================================
// JUSTICE ALITO - Criminal Law
// ============================================================

=== justice_alito_intro ===
# speaker:Justice Alito
# portrait:npc.justice_alito
# sfx:gavel_tap
Criminal law, counselor. The definition of crimes and defenses.
Mens rea, actus reus, inchoate offenses. Let's see what you know.
*   [Challenge accepted.]
    # encounter:deckTag=criminal_law count=5 rewardId=criminal_jacket
    -> END
*   [I need to review the MPC...]
    The Model Penal Code awaits. Return when ready.
    -> END

=== justice_alito_victory ===
# speaker:Justice Alito
# portrait:npc.justice_alito:impressed
# give:criminal_jacket
# quest:set defeated_alito=true
# sfx:victory_fanfare
You know the elements of crime well, counselor.
This Criminal Law Jacket is your reward.
-> END

=== justice_alito_defeat ===
# speaker:Justice Alito
# portrait:npc.justice_alito:stern
# sfx:defeat_sound
Guilty of insufficient preparation. Study the elements again.
-> END

// ============================================================
// JUSTICE JACKSON - Torts
// ============================================================

=== justice_jackson_intro ===
# speaker:Justice Jackson
# portrait:npc.justice_jackson
# sfx:gavel_tap
Torts, counselor. Negligence, strict liability, intentional wrongs.
The duty of care awaits your analysis.
*   [I understand proximate cause.]
    # encounter:deckTag=torts count=5 rewardId=torts_blazer
    -> END
*   [Let me review negligence first...]
    Breach, causation, damages. Master them and return.
    -> END

=== justice_jackson_victory ===
# speaker:Justice Jackson
# portrait:npc.justice_jackson:impressed
# give:torts_blazer
# quest:set defeated_jackson=true
# sfx:victory_fanfare
Excellent tort analysis, counselor!
This Torts Blazer suits you perfectly.
-> END

=== justice_jackson_defeat ===
# speaker:Justice Jackson
# portrait:npc.justice_jackson:concerned
# sfx:defeat_sound
You've breached the duty of preparation. Study proximate cause and return.
-> END

// ============================================================
// TUTORIAL DIALOGUES
// ============================================================

// Tutorial Dialogues from tutorial.ink

// ============================================================
// LIBRARIAN - Tutorial NPC
// ============================================================

=== librarian_intro ===
# speaker:Law Librarian
# portrait:npc.librarian
# sfx:book_open
Welcome to the Supreme Court Library, counselor.
I'm here to help you prepare for the challenges ahead.
*   [How does this place work?]
    -> librarian_tutorial
*   [Tell me about the Justices.]
    -> librarian_justices
*   [Just browsing, thanks.]
    Take your time. Knowledge is power in these halls.
    -> END

=== librarian_tutorial ===
# speaker:Law Librarian
# portrait:npc.librarian
Each Justice guards a domain of legal knowledge.
To earn their respect, you must defeat them in flashcard combat.
# quest:set tutorial_started=true
*   [How do I win?]
    -> librarian_combat
*   [What do I get for winning?]
    -> librarian_rewards
*   [Thanks, I understand.]
    -> END

=== librarian_combat ===
# speaker:Law Librarian
# portrait:npc.librarian
In each encounter, you'll answer questions from a specific subject.
Get enough correct to defeat the Justice.
The more difficult the question, the more points you earn.
*   [What about the outfits?]
    -> librarian_rewards
*   [Got it, thanks!]
    -> END

=== librarian_rewards ===
# speaker:Law Librarian
# portrait:npc.librarian
Each Justice rewards their challenger with a powerful outfit.
These outfits grant special abilities in future encounters.
Hints, extra time, bonus points... collect them all!
*   [How do I unlock all areas?]
    -> librarian_progression
*   [I'm ready to challenge them!]
    -> END

=== librarian_progression ===
# speaker:Law Librarian
# portrait:npc.librarian
Some areas are locked until you've proven yourself.
Defeat all nine Justices to access the final challenge.
The Supreme Robe awaits the worthy.
*   [I'll do my best!]
    Good luck, counselor. The law is on your side.
    -> END

=== librarian_justices ===
# speaker:Law Librarian
# portrait:npc.librarian
There are nine Justices, each master of a different subject:
- Justice Thomas: Evidence
- Chief Justice Roberts: Constitutional Law
- Justice Sotomayor: Criminal Procedure
- Justice Kagan: Civil Procedure
- Justice Gorsuch: Property
*   [Continue...]
    -> librarian_justices_2
*   [Thanks, that's enough.]
    -> END

=== librarian_justices_2 ===
# speaker:Law Librarian
# portrait:npc.librarian
- Justice Kavanaugh: Contracts
- Justice Barrett: Family Law
- Justice Alito: Criminal Law
- Justice Jackson: Torts
Find them in their chambers throughout the courthouse.
*   [Where should I start?]
    Start with the subject you know best. Build confidence first.
    -> END

// ============================================================
// BAILIFF - Gatekeeper NPC
// ============================================================

=== bailiff_intro ===
# speaker:Bailiff
# portrait:npc.bailiff
# sfx:footstep_heavy
Halt. The main courtroom is off-limits.
Only those who have proven themselves may enter.
*   [What do I need to enter?]
    -> bailiff_requirements
*   [I'll come back later.]
    See that you do. With proper credentials.
    -> END

=== bailiff_requirements ===
# speaker:Bailiff
# portrait:npc.bailiff
You must defeat all nine Justices in their chambers first.
Only then may you face the Chief Justice in the main courtroom.
*   [How many have I defeated?]
    -> bailiff_progress
*   [I understand.]
    -> END

=== bailiff_progress ===
# speaker:Bailiff
# portrait:npc.bailiff
// This should be dynamically updated based on quest flags
Check your progress in the menu.
Each defeated Justice is marked with their reward.
*   [Thanks.]
    -> END

// ============================================================
// COURT REPORTER - Hints NPC
// ============================================================

=== reporter_intro ===
# speaker:Court Reporter
# portrait:npc.reporter
# sfx:typing
Hello, counselor. I document everything that happens here.
I might have some useful information for you...
*   [Any tips for the challenges?]
    -> reporter_tips
*   [What's the latest news?]
    -> reporter_news
*   [Just passing through.]
    -> END

=== reporter_tips ===
# speaker:Court Reporter
# portrait:npc.reporter
Each subject has its own patterns and tricks.
Pay attention to the answer explanations—they often hint at related questions.
Also, the outfits you earn can help a lot in tough encounters.
*   [Thanks for the tip!]
    -> END

=== reporter_news ===
# speaker:Court Reporter
# portrait:npc.reporter
Word is, the Records Vault contains the ultimate challenge.
But it's only accessible to those who've mastered all subjects.
The Supreme Robe awaits the worthy...
*   [Interesting...]
    -> END

// ============================================================
// REWARD DIALOGUES
// ============================================================

// Reward Dialogues from rewards.ink

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