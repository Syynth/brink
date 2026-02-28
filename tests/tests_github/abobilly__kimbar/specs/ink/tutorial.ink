// Kim Bar - Tutorial Dialogues
// Librarian and other tutorial/help NPCs
// Compiled with: npm run compile:ink
//
// Tag Reference:
//   speaker:Name           - Set speaker name in dialogue box
//   portrait:id            - Set portrait image
//   quest:set flag=value   - Set a story flag

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
