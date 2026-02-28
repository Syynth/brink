VAR currentPartyChar = "THE PARTY"

It's one of those chattering teeth that kids used to play with back in the day.
{currentPartyChar == "MS SUWAN":
Ms. Suwan gazes on them longingly, struck by their dated appearance.
    -> suwanTeeth
}
    -> END

== suwanTeeth == 
:MS SUWAN:
** [Pick up the Teeth] -> TakeTeeth
** [Pull Away] -> PullAway

== PullAway==
They call out to her, longing for her to play with them.

:MS SUWAN:
** [Pick up the Teeth] -> TakeTeeth
** [Walk Away] -> WalkAway
==TakeTeeth==
&In a blink of an eye, the once yellowed children's toy is now vibrant and shiny as if it was recently made.

Almost instinctually, she places her fingers on the handle attempting to wind it up, curious to see if it still works. 

:MS SUWAN:
** [Play the toy] -> PlayToy
** [Put it back] -> PutBack

== PlayToy == 
Suddenly, the toy starts shaking in her palm. The teeth begin to jounce as the clattering noise fills her ears.

As she gazes down upon the chattering toy, her muscles slowly relax and her straightened posture begins to slouch. Her brow unfurrows and her grin begins to stretch wide, like a rubber band, in seeming mystic revelation.

:MS SUWAN:
** [Put back] -> PutBack

==PutBack==

&She slowly places the toy back onto the dusty shelf, exactly in the place she found it as if it was never touched. -> WalkAway

==WalkAway==
Taking a step back, she gazes at the rusted toy one final time before stepping away from the shelf she found it.
-> END




