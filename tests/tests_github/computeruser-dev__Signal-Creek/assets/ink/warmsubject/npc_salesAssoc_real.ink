VAR currentPartyChar = "THE PARTY"

{currentPartyChar == "NOUR":
The clerk looks Nour up and down, seemingly bored, as if her presence neither surprises nor interests him. 

CLERK: "Can I help you?" 
        -> SARealWorldConvStart
}

{currentPartyChar == "NICK":
When Nick approaches, he visibly tries to hide a cringe, but does an awful job at it. Still, he addresses him politely, if in a slightly strained tone.
    
CLERK: "Can I help you?"
        -> SARealWorldConvStart
-else:
When he glances up at Ms. Suwan, after a brief moment of almost practiced boredom, he does a double take, suddenly far more interested in the conversation.
    
CLERK: "C...Can I help you, Ma'am? Are you sure you're in the right store?"
    -> SARealWorldConvStart
}

===SARealWorldConvStart===
:THE PARTY:
* NOUR: "Hey, have you been having weird... plane shifts recently?"
    -> PlaneShifts
* NICK: "Hey! Do you have any cool T-Shirts?"
    -> TShirts
* MS SUWAN: "Yes, you can. Do you know anything about this mall and it's history?"
    -> MallHistory

// if there's no more options left 
// He doesn't seem interested in talking to anyone in the party anymore. It seems they've all used up their good favors with him. ->END

===PlaneShifts===
He turns to Nour with a skeptical look. 

CLERK: "Are you insinuating I'm on drugs? What are you, a Narc?" 

:THE PARTY:
* NOUR: "No, I didn't say that!"
    -> WhateverMan
* MS SUWAN: "Well are you on drugs?"
    ->ReallyOnDrugs

===WhateverMan===
CLERK: "Whatever, man. I don't know what you're talking about, and if you want to go off telling on me to my manager, you'd have a problem doing it. I am the manager."
    -> SARealWorldConvStart

===ReallyOnDrugs===
He looks incredulously at Ms.Suwan. 

CLERK: "And what if I am? What's that going to change? I can still do my job perfectly fine, get off my ass. You'd be smoking too if you were about to lose your job." 

    -> MsSuwanSAPlaneShift

===MsSuwanSAPlaneShift===
&His words seem to hit close to home for Ms.Suwan. The world starts to waver in front of her, the mall closing in, almost crashing down even though it's still in place. In the span of a few moments, she drags everyone else into the other world along with her.
    -> END

===TShirts===
This seems to be a normal question for him, and there's almost an instant switch in his demeanor as he parrots off a practiced sentence. 

CLERK: "The T-Shirt rack is over there, you can browse through it and pick out what you want. DON'T, and I repeat DON'T steal anything. Upper management is more on my ass than usual because the mall is closing.

:NICK:
* [Thank him.] "Thank you!"
    ->NickThankSA
* [Walk away.] Nick walks away without saying thank you.
    ->SARealWorldConvStart

===NickThankSA===
CLERK: Yeah yeah... you're welcome, kid. (There's the faintest trace of a smile on his face.) 
    ->SARealWorldConvStart

=== MallHistory ===
He blinks a few times at Ms. Suwan before looking around Warm Subject. Then, after a brief moment, shrugs.

CLERK: "I just work here, Ma'am. For however longer that'll be. I don't know anything."
-> END