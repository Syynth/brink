//the real book stack file... boring ones Nick has to read for class

VAR currentPartyChar = "PARTY"

//DREAM NOT VISITED
//Checking if dream has been visited, and skip to dreamvisited if so.
NICK: Oh, that book... Didn't I have to read that for class once?

:NICK:

* [Open the book on the top of the stack.] -> readBook

== readBook ==
NICK: Huh? I don't remember this part... oh, it's probably because I only read the SparkNotes. Ugh... it's just so BORING, though. I kept falling asleep when I tried to really read it...

-> END

//DREAM VISITED
==dreamvisited==
NICK: Wait, where did the manga go? Darn it, now it's this boring thing again.

-> END



