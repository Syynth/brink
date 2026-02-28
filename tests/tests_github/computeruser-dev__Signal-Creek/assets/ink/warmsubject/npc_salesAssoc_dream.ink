VAR currentPartyChar = "THE PARTY"
He seems to waver in and out of reality, but still notices the party arrive and offers them a friendly wave.

CLERK: "Hey there ya'll, can I help you?"
-> SADreamWorldConvStart

===SADreamWorldConvStart===
* MS SUWAN: "..."
    ->MsSuwanNoComment
* NOUR: "Are you alright?"
    ->NourIsSAALright
* NICK: "I got a T-Shirt..."
    ->NickGotHisDreamTShirt
-> END

===MsSuwanNoComment===
He notices Ms.Suwan's lack of enthusiasm, cocking his head in an almost animal-like manner. 

CLERK: "Are you alright?"

:MS SUWAN:
* [Say nothing.] Ms.Suwan doesn't say anything, pursing her lips
    ->SAShrugging
* ["I'm fine."]
    ->MsSuwanIsFine


===SAShrugging===
He shrugs his shoulders at her lack of comment, looking over at the others, awaiting their questions.
    ->SADreamWorldConvStart

===MsSuwanIsFine===
He doesn't seem to believe her when she replies she's fine, but he doesn't pry, shrugging and turning to the others, awaiting their questions.
    -> SADreamWorldConvStart

===NourIsSAALright===
He blinks, seemingly confused at the question. His form seems to waver briefly, but after a moment of thought he slowly nods.

CLERK: "Yes... I don't see why I wouldn't be? What's with the weird question?

:NOUR:
* [Squint] Nour squints at the CLERK, suspicious. Something isn't right here.
    ->NourIsSuspicious
* ["No reason."] Despite her suspicion, she doesn't pry and steps away.
    ->SADreamWorldConvStart
->END

===NourIsSuspicious===
He seems uncomfortable underneath her prying gaze, form wavering. 

CLERK: "... is there a reason you're squinting at me? Do I have something stuck in my teeth?" 

:NOUR:
* "How do you not realize you're... like this?
    ->SAObliviousToDreamWorld
* "You're hiding something from me."
    ->IsSAHidingSomething
* "... You know what, yes. You do have something stuck in your teeth."
    -> SAFoodStuckInTeeth

===SAObliviousToDreamWorld===
CLERK: "Like... what? I'm like I've always been. Are you alright, do you need help?"

:NOUR:
* [Continue prying.] He doesn't seem to realize anything is wrong, but Nour doesn't trust it.
    ->NourContinuesPrying
* [Stop prying.] With a frustrated exhale of breath, Nour lets it go.
    ->SADreamWorldConvStart

===IsSAHidingSomething===
CLERK: "Hiding... what are you talking about? I'm not hiding anything, all the deals here are as transparent as they've always been!"

:NOUR:
* [Continue prying.] He doesn't seem to realize anything is wrong, but Nour doesn't trust it.
    ->NourContinuesPrying
* [Stop prying.] With a frustrated exhale of breath, Nour lets it go.
    ->SADreamWorldConvStart

===NourContinuesPrying===
Nour prepares to continue digging, feeling frustration well up within her. He's hiding something, she's sure of it, she can see it underneath the facade of his innocent face.
&Her mouth opens, and instead of the loud yell she was expecting, the entire world seems to envelop her, dragging her back into reality.

-> END

===SAFoodStuckInTeeth===
He blushes, entire body going hot. 

CLERK: "Ah... how embarassing of me. Give me a moment."
-> SADreamWorldConvStart

==NickGotHisDreamTShirt===
He frowns, looking Nick up and down. 

CLERK: "Are you alright? You seem sad."

:NICK:
* "No it's alright."
    ->NickPretendsToBeAlright
* "I... I don't have the money to buy this." ->NickDoesntHaveMoney

===NickPretendsToBeAlright===
Though the clerkdoesn't seem convinced, he gives a slow nod. 

CLERK: "Well, if you need anything else feel free to peruse or ask me."
-> SADreamWorldConvStart

===NickDoesntHaveMoney===
The clerkfrowns, before looking at the rest of the party. 

CLERK: "I'm sorry... as much as I'd like to give this to you for free, I can't. You'll have to put it back on the rack and come back later when you have the money for it." 

&Nick's used to this, unfortunately. He feels a familiar, overwhelming sadness and embarassment, feeling the eyes of others both real and imaginary on him, the pity they shower him with. He wants to run away from it, retreat back into the real world, anything to escape this feeling.

-> END
