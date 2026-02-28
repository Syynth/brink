// Dream: Bright and glitzy stand with way too many colors and a characterized pretzel for the logo. There’s a pretzel-person behind the counter.
// Real:  A traditional, Annies-style pretzel stand.
VAR currentPartyChar = "PARTY"


//REAL NOT VISITED
//here, we check if real has been visited, and skip to realvisited if so.
NOUR: Man, this looks like a great place to try some cinnamon sugar bites! I was never allowed to get junk food as a kid...

:NOUR:
* [Talk to the guy]NOUR: Uh... Hello? Do you guys have cinnamon sugar bites?

No response.

:NOUR:
* [Huh? Why the hello isn't he responding?]NOUR: Hello? Hey, are you deaf or something?

Still, no response.

    -> END

==realvisited==

NOUR: Looks like now's my chance to try some junk food!

:NOUR:
* [Talk to the guy]NOUR: Uh... Hello? Do you guys have cinnamon sugar bites?

No response.

:NOUR:
* [Huh? Why the hell isn't he responding?]NOUR: Hello? Hey, are you deaf or something?

Still, no response.

    -> END

