// rina will be divided up differently than most other objects, into 'interactions' instead of dreamvisited/realvisited (irst interaction, second, third, etc.)
VAR currentPartyChar = "PARTY"

Before the party stands a lady in her early 20s, a brunette with blonde highlights and an almost girly look to her despite her age. Before anyone in the party can speak, Nour steps forward. 

NOUR: "Rina?"

Rina turns towards Nour, a bright smile on her face.

RINA: "Nour! Long time no see, how are you doing? I didn't expect you to be in the mall, too."

:NOUR:
* "I didn't expect you to be here, either, Rina..."
    -> Rina1
* "Really? That's all you have to say to me?"
    -> Rina2

===Rina1===
RINA: "... of course you didn't. You aren't here to find me on purpose, are you?"

NOUR grits her teeth in reponse. 

:NOUR:
* "You know the answer to that. Why would I try and look for you after what you did?" -> Rina3
* "No I'm not here to find you on purpose. But I think it's still good I'm here." -> Rina4

===Rina2===
Rina looks taken aback, before her eyes turn angry.

RINA: "Well what do you want me to say? Sorry for my ~grevious sins~ against you?" 

NOUR: "YES! Yes you could fucking apologize for leaving me alone when I needed you most instead of acting like nothing happened!"

Rina looks like she's about to say something, but before she can, Nour interupts her.

NOUR: "You could at least say 'hey, Nour, good old friend, turns out I don't care about you and don't want you here and it would be best to leave me alone so you don't get *fucking hurt again*-"

// this is where the world shifts mid sentence :)
-> END

===Rina3===
RINA: "After what *I* did?"

NOUR: "Yes, after what *you* did! Why did you even try to pretend you cared, if you were going to abandon me anyways?!" 
-> END

===Rina4===
RINA: "... is it? I would have thought you wanted to avoid me at all costs, like you always have."

Nour doesn't seem to know what to say to that for a moment before biting her lip, looking away.

NOUR: "you didn't seem like... I didn't think... look, right now is different. I can talk to you now. Do you want that or not?"

Rina looks apprehensive, before shaking her head.

RINA: "fine. What do you want to talk about, then? How do you want this to go?"

Nour doesn't hesitate to speak once Rina is finished asking the question, her voice toying on just the side of too loud, just below a shout. 

NOUR: "Why did you abandon me when I needed you most?" 

This seems to catch Rina off guard. She stops and stares at Nour before looking away, offering no response. Her eyes look sad.

NOUR: "No answer. I should have expected as much. Fucking coward."

RINA: "Nour, please, it wasn't easy for m-"

NOUR: "AND YOU THINK IT WAS EASY FOR ME? Fuck, I thought I at least had *someone*, but *no*, even you decided at the last moment to just... to just let it happen!"

// this is where a plane shift occurs o7
-> END

// Here we check if Rina has already been interacted with once in the dream world. If so, go to secondinteraction. Is there some way in godot/ink to check what choices above haven't been chosen and move them down to here? If not it will be a lot of checks and balances lol.
===SecondInteraction===
Rina is still there. Despite the heated argument that Nour just had with her, it seems she doesn't remember any of it. She smiles when Nour approaches ammicably. 

RINA: "Nour! Long time no see, how are you doing? I didn't expect you to be in the mall, too."

Nour seems to be taken aback at her mood.

NOUR: "... Rina?" 

RINA: "... yes? Is something the matter?"

:NOUR:
* "Do you... not remember what just happened?" -> Rina5

===Rina5===
RINA: "... should I remember? Are you still talking about that whole... situation? I thought you'd be over it by now, it's been *years*, Nour."

The confusion in Nour's eyes is very quickly replaced by anger. 

NOUR: "bold of you to bring it up, then. I wasn't even talking about that! It must be on *your* head instead." 

Rina lets out a brief scoff of a laugh. 

RINA: "Yeah, right. I wouldn't bring it up if you didn't make it so obvious. Fine, lets do this. Let's pretend like you're not thinking what you're thinking."

NOUR: "Like you love to pretend like nothing ever happened? Like you love to pretend that everything was my fault and you're completely innocent in every single fucking thing you do?"

RINA: "That's rich, coming from the person who split up our friend group."

Nour's fists clench, and before the party could try and stop her, she yells. 

NOUR: "IT WASN'T MY FUCKING FAULT!"
// plane shift two electric boogaloo
-> END

// and then we add as many ===interaction=== parts as we need for the future based on other objects that the player interacts with! 