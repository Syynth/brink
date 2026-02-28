// Kim Bar - Justice Encounters
// All Supreme Court Justice intro/victory/defeat dialogue knots
// Compiled with: npm run compile:ink
//
// Tag Reference:
//   speaker:Name           - Set speaker name in dialogue box
//   portrait:id            - Set portrait image
//   portrait:id:emotion    - Set portrait with emotion
//   sfx:sound_name         - Play sound effect
//   quest:set flag=value   - Set a story flag
//   encounter:deckTag=X count=N rewardId=Y  - Start encounter
//   give:item_id           - Give item/outfit

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
